//! `drive`: run the built plugin over the real stdio protocol, the way KnobDeck does, without
//! KnobDeck. hello → ready → sync (one instance per action) → steps → shutdown. Every message the
//! plugin sends is printed; host calls are answered with harmless stubs.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// The plugin under `dist/` and the executable this OS runs.
fn find_plugin() -> Result<(PathBuf, Value, PathBuf), String> {
    let dirs: Vec<PathBuf> = std::fs::read_dir("dist")
        .map_err(|_| "no dist/ here: run `knobdeck-plugin dev` first".to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    let dir = match dirs.as_slice() {
        [d] => d.clone(),
        [] => return Err("no dist/<id>/manifest.json: run `knobdeck-plugin dev` first".into()),
        _ => return Err("several plugins under dist/: remove the stale ones".into()),
    };
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).map_err(|e| e.to_string())?)
        .map_err(|e| format!("manifest.json: {e}"))?;
    let rt = &manifest["runtime"];
    let exe = rt["executables"][std::env::consts::OS]
        .as_str()
        .or_else(|| rt["executable"].as_str())
        .ok_or_else(|| format!("the manifest has no executable for {}", std::env::consts::OS))?;
    let exe = dir.join(exe);
    if !exe.is_file() {
        return Err(format!("{} is missing: run `knobdeck-plugin dev`", exe.display()));
    }
    Ok((dir, manifest, exe))
}

/// Action ids from actions.json: (short id, full id, kind). Full ids are
/// `<plugin id>-<action id>-<8 hex>` (`make_full_id`), so the short id is the middle part.
fn actions(dir: &Path, plugin_id: &str) -> Vec<(String, String, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("actions.json")) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    let list = v["actions"].as_array().cloned().unwrap_or_default();
    list.iter()
        .filter_map(|a| {
            let full = a["id"].as_str()?.to_string();
            let short = full
                .strip_prefix(&format!("{plugin_id}-"))
                .and_then(|rest| rest.rsplit_once('-').map(|(s, _)| s.to_string()))
                .unwrap_or_else(|| full.clone());
            Some((short, full, a["type"].as_str().unwrap_or("button").to_string()))
        })
        .collect()
}

struct Host {
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    bad_lines: usize,
    errors: usize,
}

impl Host {
    fn send(&mut self, v: Value) {
        println!("  → {v}");
        let _ = writeln!(self.stdin, "{v}");
        let _ = self.stdin.flush();
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    /// Print and answer everything the plugin sends for `ms`, or until `until` matches.
    fn pump(&mut self, ms: u64, until: impl Fn(&Value) -> bool) -> Option<Value> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                        println!("  ✗ not a protocol message on stdout: {line}");
                        self.bad_lines += 1;
                        continue;
                    };
                    println!("  ← {msg}");
                    match msg["type"].as_str() {
                        Some("host_call") => self.answer(&msg),
                        Some("error") => self.errors += 1,
                        Some("result") if msg["ok"] == false => self.errors += 1,
                        _ => {}
                    }
                    if until(&msg) {
                        return Some(msg);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return None,
                Err(RecvTimeoutError::Disconnected) => {
                    println!("  · the plugin exited");
                    return None;
                }
            }
        }
    }

    /// `send_keys` is the only host call; it gets a stub answer so the plugin keeps going, and is
    /// worth noticing: the action fell back to keystrokes. Anything else is refused as KnobDeck does.
    fn answer(&mut self, msg: &Value) {
        let method = msg["method"].as_str().unwrap_or("");
        let id = msg["id"].clone();
        if method == "send_keys" {
            println!("  ! send_keys {}: this action types a shortcut; prefer an API (see the skill)", msg["params"]);
            self.send(json!({"type": "result", "id": id, "ok": true, "value": null}));
        } else {
            println!("  ! unknown host call {method:?} (KnobDeck only takes send_keys)");
            self.send(json!({"type": "result", "id": id, "ok": false, "error": format!("unknown host call method \"{method}\"")}));
        }
    }
}

/// Expand a shorthand step into protocol messages, given the action table.
fn expand(step: &Value, table: &[(String, String, String)], host: &mut Host) -> Result<Vec<Value>, String> {
    let full = |short: &str| -> Result<String, String> {
        table
            .iter()
            .find(|(s, f, _)| s == short || f == short)
            .map(|(_, f, _)| f.clone())
            .ok_or_else(|| format!("no action \"{short}\" (known: {})", table.iter().map(|t| t.0.as_str()).collect::<Vec<_>>().join(", ")))
    };
    let inputs = step.get("inputs").cloned().unwrap_or(json!({}));
    let instance = |a: &str| step["instance"].as_str().map(str::to_string).unwrap_or_else(|| format!("drive-{a}"));
    let invoke = |host: &mut Host, a: &str, trigger: &str, inputs: Value| -> Result<Value, String> {
        let mut m = json!({"type": "invoke", "id": host.id("r"), "instance_id": instance(a), "action_id": full(a)?, "trigger": trigger, "inputs": inputs});
        if let Some(c) = step.get("child") {
            m["child_id"] = c.clone();
        }
        Ok(m)
    };
    if let Some(a) = step["press"].as_str() {
        return Ok(vec![invoke(host, a, "press", inputs)?]);
    }
    if let Some(a) = step["adjust"].as_str() {
        let mut i = inputs;
        for k in ["value", "delta"] {
            if let Some(v) = step.get(k) {
                i[k] = v.clone();
            }
        }
        return Ok(vec![invoke(host, a, "adjust", i)?]);
    }
    if let Some(a) = step["hold"].as_str() {
        let ms = step["ms"].as_u64().unwrap_or(300);
        return Ok(vec![invoke(host, a, "hold_start", inputs.clone())?, json!({"wait": ms}), invoke(host, a, "hold_end", inputs)?]);
    }
    if let Some(q) = step["query"].as_str() {
        return Ok(vec![json!({"type": "request", "id": host.id("q"), "method": "query", "params": {"query_id": q, "inputs": inputs}})]);
    }
    if let Some(a) = step["open"].as_str() {
        return Ok(vec![json!({"type": "request", "id": host.id("w"), "method": "wheel", "params": {"action_id": full(a)?, "instance_id": instance(a), "inputs": inputs}})]);
    }
    if step.get("wait").is_some() || step.get("type").is_some() {
        return Ok(vec![step.clone()]);
    }
    Err(format!("unknown step {step}: use press, adjust, hold, query, open, wait or a raw protocol message"))
}

pub fn drive(steps_file: Option<&str>) -> Result<(), String> {
    let steps: Vec<Value> = match steps_file {
        Some(f) => serde_json::from_str(&std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?).map_err(|e| format!("{f}: {e}"))?,
        None => vec![],
    };
    let (dir, manifest, exe) = find_plugin()?;
    let id = manifest["id"].as_str().unwrap_or("plugin").to_string();
    let table = actions(&dir, &id);
    let data = std::env::temp_dir().join(format!("knobdeck-drive-{id}"));
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    println!("\n  KnobDeck Plugin — drive {}\n  data folder: {}\n", exe.display(), data.display());

    let mut child = Command::new(&exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("KNOBDECK_PLUGIN_DATA", &data)
        .env("KNOBDECK_PLATFORM", std::env::consts::OS)
        .spawn()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut host = Host { stdin, lines, next_id: 0, bad_lines: 0, errors: 0 };

    host.send(json!({"type": "hello", "protocol": 2, "plugin_id": id, "data_dir": data, "host_version": "knobdeck-plugin drive", "host_pid": std::process::id()}));
    if host.pump(10_000, |m| m["type"] == "ready").is_none() {
        let _ = child.kill();
        return Err("the plugin never sent `ready` (does main() end with .run()?)".into());
    }
    let instances: Vec<Value> = table.iter().map(|(s, f, _)| json!({"instance_id": format!("drive-{s}"), "action_id": f, "inputs": {}})).collect();
    host.send(json!({"type": "sync", "instances": instances}));
    host.pump(1500, |_| false);

    let mut bad_steps = 0;
    for step in &steps {
        let msgs = match expand(step, &table, &mut host) {
            Ok(m) => m,
            Err(e) => {
                println!("  ✗ {e}");
                bad_steps += 1;
                continue;
            }
        };
        for m in msgs {
            if let Some(ms) = m.get("wait").and_then(Value::as_u64) {
                host.pump(ms, |_| false);
                continue;
            }
            let rid = m["id"].clone();
            let is_request = m["type"] == "request";
            host.send(m);
            // Wait for the answer to this message, then let pushes that follow it arrive.
            if is_request {
                host.pump(10_000, |r| r["type"] == "result" && r["id"] == rid);
            } else {
                host.pump(10_000, |r| r["type"] == "ack" && r["id"] == rid);
            }
            host.pump(400, |_| false);
        }
    }

    host.send(json!({"type": "shutdown"}));
    drop(host.stdin);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut exited = false;
    while Instant::now() < deadline {
        if host.lines.recv_timeout(Duration::from_millis(100)).is_err() {
            if let Ok(Some(_)) = child.try_wait() {
                exited = true;
                break;
            }
        }
    }
    if !exited {
        let _ = child.kill();
        println!("  ! the plugin didn't exit within 3 s of shutdown (the host would kill it)");
    }
    println!(
        "\n  {} {} error message(s), {} non-protocol stdout line(s). Logs: {}",
        if host.bad_lines == 0 { "✓" } else { "✗" },
        host.errors,
        host.bad_lines,
        data.join("log.txt").display()
    );
    if host.bad_lines > 0 {
        return Err("the plugin wrote non-protocol lines to stdout".into());
    }
    if bad_steps > 0 {
        return Err(format!("{bad_steps} step(s) in the steps file could not run"));
    }
    Ok(())
}
