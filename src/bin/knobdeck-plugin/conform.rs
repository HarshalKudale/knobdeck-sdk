//! `conform`: check that a plugin follows the protocol, whatever language it is written in.
//!
//! `knobdeck-plugin conform` checks `dist/<id>/`; `knobdeck-plugin conform --dir <plugin dir> --
//! <command…>` runs any command (`node plugin.js`, `python plugin.py`) with that folder's
//! `manifest.json` and `actions.json`. The same checks run for every plugin, so a plugin in a
//! language without a KnobDeck SDK is supported by passing them.

use knobdeck_sdk::protocol::{self, features, host_calls, CallResult, Hello, HostMsg, Instance, PluginMsg};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// One check's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// Everything conform needs to start the plugin.
pub struct Target {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub plugin_id: String,
    /// (full action id, kind) from actions.json.
    pub actions: Vec<(String, String)>,
}

impl Target {
    /// `dist/<id>/` (built by `knobdeck-plugin dev`), or `dir` + an explicit command.
    pub fn from_args(dir: Option<&Path>, command: &[String]) -> Result<Target, String> {
        let dir = match dir {
            Some(d) => d.to_path_buf(),
            None => crate::drive::find_plugin()?.0,
        };
        // The plugin runs inside its folder, so every path must stand on its own.
        let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("manifest.json")).map_err(|e| format!("{}: {e}", dir.join("manifest.json").display()))?,
        )
        .map_err(|e| format!("manifest.json: {e}"))?;
        let plugin_id = manifest["id"].as_str().ok_or("manifest.json has no id")?.to_string();
        let command = if command.is_empty() {
            let rt = &manifest["runtime"];
            let exe = rt["executables"][std::env::consts::OS]
                .as_str()
                .or_else(|| rt["executable"].as_str())
                .ok_or_else(|| format!("the manifest has no executable for {}; pass one after --", std::env::consts::OS))?;
            vec![dir.join(exe).to_string_lossy().into_owned()]
        } else {
            command.to_vec()
        };
        let actions = std::fs::read_to_string(dir.join("actions.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["actions"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|a| Some((a["id"].as_str()?.to_string(), a["type"].as_str().unwrap_or("button").to_string())))
            .collect();
        Ok(Target { command, cwd: dir, plugin_id, actions })
    }
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    /// Every stdout line, in order.
    seen: Vec<String>,
    /// Lines that were not a known protocol frame.
    bad: Vec<String>,
    /// Host calls with a method this protocol doesn't define.
    unknown_calls: Vec<String>,
}

impl Session {
    fn start(t: &Target, data: &Path) -> Result<Session, String> {
        let (prog, args) = t.command.split_first().ok_or("no command to run")?;
        let mut child = Command::new(prog)
            .args(args)
            .current_dir(&t.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("KNOBDECK_PLUGIN_DATA", data)
            .env("KNOBDECK_PLATFORM", std::env::consts::OS)
            .spawn()
            .map_err(|e| format!("{prog}: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Session { child, stdin, lines, seen: vec![], bad: vec![], unknown_calls: vec![] })
    }

    fn send(&mut self, msg: &Value) {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = writeln!(stdin, "{msg}");
            let _ = stdin.flush();
        }
    }

    fn send_typed(&mut self, msg: HostMsg) {
        self.send(&serde_json::to_value(msg).expect("serializes"));
    }

    /// Read frames for up to `ms`, answering host calls, until `want` matches one.
    fn until(&mut self, ms: u64, want: impl Fn(&PluginMsg) -> bool) -> Option<PluginMsg> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            match self.lines.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(line) => {
                    self.seen.push(line.clone());
                    let msg = match serde_json::from_str::<PluginMsg>(&line) {
                        Ok(PluginMsg::Unknown) | Err(_) => {
                            self.bad.push(line);
                            continue;
                        }
                        Ok(m) => m,
                    };
                    if let PluginMsg::HostCall { id, method, .. } = &msg {
                        if !host_calls::ALL.contains(&method.as_str()) {
                            self.unknown_calls.push(method.clone());
                        }
                        let reply = match method.as_str() {
                            host_calls::APP_STATE => CallResult::ok(id, json!({"os": std::env::consts::OS, "host_version": "conform", "dial_open": false})),
                            host_calls::COMPANION_LINKS => CallResult::ok(id, json!([])),
                            host_calls::COMPANION_REQUEST => CallResult::err(id, "no companion is connected"),
                            host_calls::OPEN_DIAL => CallResult::ok(id, Value::Bool(true)),
                            m if host_calls::ALL.contains(&m) => CallResult::ok(id, Value::Null),
                            m => CallResult::err(id, format!("unknown host call method \"{m}\"")),
                        };
                        self.send_typed(HostMsg::Result(reply));
                    }
                    if want(&msg) {
                        return Some(msg);
                    }
                }
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn exited_within(&mut self, ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn check(name: &'static str, ok: bool, detail: impl Into<String>) -> Check {
    Check { name, ok, detail: detail.into() }
}

/// Run every check against the plugin. The order matters: later checks need `ready`.
pub fn run(t: &Target) -> Result<Vec<Check>, String> {
    let data = std::env::temp_dir().join(format!("knobdeck-conform-{}-{}", t.plugin_id, std::process::id()));
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let mut s = Session::start(t, &data)?;
    let mut out = vec![];

    // 1. hello → ready.
    s.send_typed(HostMsg::Hello(Hello {
        protocol: protocol::PROTOCOL,
        plugin_id: t.plugin_id.clone(),
        data_dir: data.to_string_lossy().into_owned(),
        host_version: "knobdeck-plugin conform".into(),
        features: features::ALL.iter().map(|f| f.to_string()).collect(),
        host_pid: Some(std::process::id()),
    }));
    let ready = s.until(10_000, |m| matches!(m, PluginMsg::Ready { .. }));
    let Some(PluginMsg::Ready { protocol: p, done: sends_done, .. }) = ready else {
        out.push(check("ready", false, "no `ready` within 10 s of `hello`"));
        let _ = std::fs::remove_dir_all(&data);
        return Ok(out);
    };
    out.push(check(
        "ready",
        protocol::protocol_supported(p),
        if protocol::protocol_supported(p) { format!("protocol {p}") } else { format!("protocol {p} is outside {}..={}", protocol::MIN_PROTOCOL, protocol::PROTOCOL) },
    ));
    out.push(check(
        "ready.done",
        sends_done,
        if sends_done { "promises `done` after each invoke" } else { "no `done`: workflows can't wait for this plugin's actions" },
    ));

    // 2. sync: one instance per action.
    let instances: Vec<Instance> = t
        .actions
        .iter()
        .enumerate()
        .map(|(i, (id, _))| Instance { instance_id: format!("conform-{i}"), action_id: id.clone(), inputs: json!({}) })
        .collect();
    s.send_typed(HostMsg::Sync { instances: instances.clone() });
    s.until(800, |_| false);

    // 3. invoke each button/toggle once: ack, and done when promised.
    let mut missing_ack = vec![];
    let mut missing_done = vec![];
    for (i, inst) in instances.iter().enumerate() {
        if !matches!(t.actions[i].1.as_str(), "button" | "toggle") {
            continue;
        }
        let id = format!("conform-r{i}");
        s.send_typed(HostMsg::Invoke(protocol::Invoke {
            id: id.clone(),
            instance_id: inst.instance_id.clone(),
            action_id: inst.action_id.clone(),
            trigger: protocol::triggers::PRESS.into(),
            inputs: json!({}),
            child_id: None,
        }));
        if s.until(2_000, |m| matches!(m, PluginMsg::Ack { id: a } if *a == id)).is_none() {
            missing_ack.push(inst.action_id.clone());
        }
        if sends_done && s.until(10_000, |m| matches!(m, PluginMsg::Done { id: d } if *d == id)).is_none() {
            missing_done.push(inst.action_id.clone());
        }
    }
    out.push(check("ack", missing_ack.is_empty(), if missing_ack.is_empty() { "every invoke acknowledged within 2 s".into() } else { format!("no ack for {}", missing_ack.join(", ")) }));
    if sends_done {
        out.push(check("done", missing_done.is_empty(), if missing_done.is_empty() { "every invoke reported done".into() } else { format!("no done for {}", missing_done.join(", ")) }));
    }

    // 4. Every request is answered, even ones the plugin can't serve.
    s.send_typed(HostMsg::Request { id: "conform-q1".into(), method: protocol::requests::QUERY.into(), params: json!({"query_id": "conform-no-such-query", "inputs": {}}) });
    let q1 = s.until(5_000, |m| matches!(m, PluginMsg::Result(r) if r.id == "conform-q1"));
    s.send_typed(HostMsg::Request { id: "conform-q2".into(), method: "conform_no_such_method".into(), params: json!({}) });
    let q2 = s.until(5_000, |m| matches!(m, PluginMsg::Result(r) if r.id == "conform-q2"));
    let q2_refused = matches!(&q2, Some(PluginMsg::Result(r)) if !r.ok);
    out.push(check(
        "answers requests",
        q1.is_some() && q2_refused,
        match (q1.is_some(), q2.is_some(), q2_refused) {
            (true, true, true) => "an unknown query and an unknown method both got a `result`".to_string(),
            (false, _, _) => "no `result` for a query it doesn't know (the host waits for one)".into(),
            (_, false, _) => "no `result` for an unknown request method".into(),
            _ => "an unknown request method was answered ok: true".into(),
        },
    ));

    // 5. Unknown frame types and fields are ignored.
    s.send(&json!({"type": "conform_from_the_future", "x": 1}));
    s.send(&json!({"type": "event", "event": "conform_unknown_event", "payload": {}, "extra": true}));
    s.send(&json!({"type": "request", "id": "conform-q3", "method": "query", "params": {"query_id": "x", "inputs": {}}, "future_field": [1]}));
    let alive = s.until(5_000, |m| matches!(m, PluginMsg::Result(r) if r.id == "conform-q3")).is_some();
    out.push(check("ignores unknown", alive, if alive { "still answering after unknown frames and fields" } else { "stopped answering after an unknown frame or field" }));

    // 6. shutdown → exit.
    s.send_typed(HostMsg::Shutdown);
    s.stdin.take();
    let exited = s.exited_within(3_000);
    s.until(200, |_| false);
    out.push(check("shutdown", exited, if exited { "exited within 3 s" } else { "still running 3 s after `shutdown` (the host kills it after 2 s)" }));

    // Across the whole session.
    out.push(check(
        "stdout is frames only",
        s.bad.is_empty(),
        if s.bad.is_empty() { format!("{} lines, all protocol frames", s.seen.len()) } else { format!("{} stray line(s), first: {}", s.bad.len(), s.bad[0].chars().take(120).collect::<String>()) },
    ));
    out.push(check(
        "host calls",
        s.unknown_calls.is_empty(),
        if s.unknown_calls.is_empty() { "only defined methods".into() } else { format!("unknown method(s): {}", s.unknown_calls.join(", ")) },
    ));
    drop(s);
    let _ = std::fs::remove_dir_all(&data);
    Ok(out)
}

/// The CLI: print the checks, exit 1 on any failure.
pub fn conform(args: &[String]) -> Result<(), String> {
    let split = args.iter().position(|a| a == "--");
    let (opts, command) = match split {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };
    let dir = opts.iter().position(|a| a == "--dir").and_then(|i| opts.get(i + 1)).map(PathBuf::from);
    let t = Target::from_args(dir.as_deref(), command)?;
    println!("\n  KnobDeck Plugin — conform {} ({})\n", t.plugin_id, t.command.join(" "));
    let checks = run(&t)?;
    for c in &checks {
        println!("  {} {:<22} {}", if c.ok { "✓" } else { "✗" }, c.name, c.detail);
    }
    let failed = checks.iter().filter(|c| !c.ok).count();
    println!();
    if failed > 0 {
        return Err(format!("{failed} of {} checks failed", checks.len()));
    }
    println!("  ✓ conforms to protocol {}\n", protocol::PROTOCOL);
    Ok(())
}
