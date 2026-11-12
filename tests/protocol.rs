//! End-to-end: run the demo example as the host would (piped stdio) and drive
//! protocol v2.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn demo_exe() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // deps/
    p.pop();
    p.join("examples").join(format!("demo{}", std::env::consts::EXE_SUFFIX))
}

struct Host {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Host {
    fn start(data_dir: &std::path::Path) -> Host {
        Self::start_with(data_dir, json!([]))
    }

    /// `features`: what this fake host advertises in `hello`.
    fn start_with(data_dir: &std::path::Path, features: Value) -> Host {
        let mut child = Command::new(demo_exe())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn demo (run via `cargo test`, which builds examples)");
        let stdin = child.stdin.take().unwrap();
        let stdout: ChildStdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut host = Host { child, stdin, lines };
        host.send(json!({"type": "hello", "protocol": 2, "plugin_id": "example-rust", "data_dir": data_dir, "host_version": "test", "features": features}));
        host
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Every stdout line must be a protocol message; return the next one.
    fn next(&self) -> Value {
        let line = self.lines.recv_timeout(Duration::from_secs(10)).expect("plugin went quiet");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("non-protocol stdout line {line:?}: {e}"))
    }

    /// Read until a message of type `ty` (skipping unrelated pushes like the ticker).
    fn expect(&self, ty: &str) -> Value {
        self.expect_where(ty, |_| true)
    }

    fn expect_where(&self, ty: &str, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..50 {
            let m = self.next();
            if m["type"] == ty && pred(&m) {
                return m;
            }
        }
        panic!("never saw a matching {ty} message");
    }
}

#[test]
fn hello_ready_sync_invoke_request_shutdown() {
    let dir = tempfile_dir("proto");
    let mut h = Host::start(&dir);

    let ready = h.expect("ready");
    assert_eq!(ready["protocol"], 2);
    assert_eq!(ready["done"], true);

    let mute = knobdeck_sdk::make_full_id("example-rust", "mute");
    let ping = knobdeck_sdk::make_full_id("example-rust", "ping");
    let volume = knobdeck_sdk::make_full_id("example-rust", "volume");
    h.send(json!({"type": "sync", "instances": [{"instance_id": "i1", "action_id": mute, "inputs": {}}]}));
    let st = h.expect_where("state", |m| m["instance_id"] == "i1");
    assert_eq!((st["instance_id"].as_str(), st["state"].as_str()), (Some("i1"), Some("unmuted")));

    h.send(json!({"type": "invoke", "id": "r1", "instance_id": "i1", "action_id": mute, "trigger": "press", "inputs": {}}));
    assert_eq!(h.expect("ack")["id"], "r1");
    let fan = h.expect_where("state", |m| m["action_id"] == mute.as_str());
    assert_eq!((fan["action_id"].as_str(), fan["state"].as_str()), (Some(mute.as_str()), Some("muted")));

    // `done` follows once the handler has returned.
    assert_eq!(h.expect("done")["id"], "r1");

    // ping's handler println!s; that must not reach the protocol stream.
    h.send(json!({"type": "invoke", "id": "r2", "instance_id": "p1", "action_id": ping, "trigger": "press", "inputs": {}}));
    assert_eq!(h.expect("ack")["id"], "r2");

    // Same-instance invokes are handled in order.
    for (i, v) in [10, 20, 30, 40, 50].iter().enumerate() {
        h.send(json!({"type": "invoke", "id": format!("s{i}"), "instance_id": "v1", "action_id": volume, "trigger": "adjust", "inputs": {"value": v}}));
    }
    let mut seen = Vec::new();
    while seen.len() < 5 {
        let m = h.expect_where("state", |m| m["instance_id"] == "v1");
        seen.push(m["state"].as_str().unwrap().to_string());
    }
    assert_eq!(seen, ["10", "20", "30", "40", "50"]);

    h.send(json!({"type": "request", "id": "q1", "method": "query", "params": {"query_id": "items", "inputs": {"group": "fruit"}}}));
    let res = h.expect("result");
    assert_eq!((res["id"].as_str(), res["ok"].as_bool()), (Some("q1"), Some(true)));
    assert_eq!(res["value"], json!(["apple", "pear"]));

    h.send(json!({"type": "request", "id": "q2", "method": "query", "params": {"query_id": "nope"}}));
    let res = h.expect("result");
    assert_eq!((res["ok"].as_bool(), res["error"].as_str()), (Some(false), Some("unknown query \"nope\"")));

    // A failing action reports an error message and no panic.
    let pick = knobdeck_sdk::make_full_id("example-rust", "pick");
    h.send(json!({"type": "invoke", "id": "r3", "instance_id": "i2", "action_id": pick, "trigger": "press", "inputs": {}}));
    assert_eq!(h.expect("error")["message"], "demo error");

    // A sync is the full instance set: i1 is gone, so a fan-out reaches nobody
    // and only the new instance gets a state.
    h.send(json!({"type": "sync", "instances": [{"instance_id": "i9", "action_id": mute, "inputs": {}}]}));
    let st = h.expect_where("state", |m| m["instance_id"] == "i9");
    assert_eq!(st["state"], "unmuted");

    // Unknown message types are ignored.
    h.send(json!({"type": "from_the_future"}));

    h.send(json!({"type": "shutdown"}));
    let status = wait_exit(&mut h.child);
    assert!(status.success());
    assert!(dir.join("shutdown.txt").exists(), "on_shutdown ran");
    std::fs::remove_dir_all(&dir).ok();
}

/// Three device-volume sliders: two on device "a" (different steps), one on "b".
fn sync_device_sliders(h: &mut Host) -> String {
    let dv = knobdeck_sdk::make_full_id("example-rust", "device-volume");
    h.send(json!({"type": "sync", "instances": [
        {"instance_id": "a1", "action_id": dv, "inputs": {"device": "a", "step": "5"}},
        {"instance_id": "a2", "action_id": dv, "inputs": {"device": "a", "step": "10"}},
        {"instance_id": "b1", "action_id": dv, "inputs": {"device": "b", "step": "5"}},
    ]}));
    dv
}

#[test]
fn targeted_fan_out_sends_one_match_frame_to_a_new_host() {
    let dir = tempfile_dir("match-new");
    let mut h = Host::start_with(&dir, json!(["state_match"]));
    h.expect("ready");
    let dv = sync_device_sliders(&mut h);
    h.send(json!({"type": "invoke", "id": "r1", "instance_id": "a1", "action_id": dv, "trigger": "adjust",
                  "inputs": {"device": "a", "step": "5", "value": 40}}));
    let m = h.expect_where("state", |m| m["action_id"] == dv.as_str());
    assert_eq!(m["match"], json!({"device": "a"}));
    assert_eq!(m["state"], "40");
    assert!(m.get("instance_id").is_none());
    h.send(json!({"type": "shutdown"}));
    wait_exit(&mut h.child);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn targeted_fan_out_addresses_each_match_on_an_old_host() {
    let dir = tempfile_dir("match-old");
    let mut h = Host::start(&dir);
    h.expect("ready");
    let dv = sync_device_sliders(&mut h);
    h.send(json!({"type": "invoke", "id": "r1", "instance_id": "a1", "action_id": dv, "trigger": "adjust",
                  "inputs": {"device": "a", "step": "5", "value": 40}}));
    // Until `done`, collect the frames carrying "40": exactly a1 and a2, never b1 or an action-wide one.
    let mut got = Vec::new();
    loop {
        let m = h.next();
        if m["type"] == "done" {
            break;
        }
        if m["type"] == "state" && m["state"] == "40" {
            assert!(m.get("action_id").is_none(), "no action-wide frame to an old host: {m}");
            got.push(m["instance_id"].as_str().unwrap().to_string());
        }
    }
    got.sort();
    assert_eq!(got, ["a1", "a2"]);
    h.send(json!({"type": "shutdown"}));
    wait_exit(&mut h.child);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn setup_required_carries_reason() {
    let dir = tempfile_dir("setup");
    let mut h = Host::start(&dir);
    h.expect("ready");
    let expire = knobdeck_sdk::make_full_id("example-rust", "expire");
    h.send(json!({"type": "invoke", "id": "r1", "instance_id": "e1", "action_id": expire, "trigger": "press", "inputs": {}}));
    let m = h.expect("setup_required");
    assert_eq!(m["reason"], "demo token expired");
    drop(h.stdin);
    assert!(wait_exit(&mut h.child).success(), "exits cleanly on stdin EOF");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn codegen_emits_manifest_and_actions() {
    let out = Command::new(demo_exe()).env("KNOBDECK_CODEGEN", "1").env("KNOBDECK_STAGE", "build").output().unwrap();
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["manifest"]["id"], "example-rust");
    assert_eq!(v["manifest"]["protocol"], 2);
    assert!(v["manifest"]["runtime"].get("language").is_none());
    let actions = v["actions"]["actions"].as_array().unwrap();
    assert_eq!(actions[0]["id"], knobdeck_sdk::make_full_id("example-rust", "ping"));
    assert_eq!(actions.iter().find(|a| a["type"] == "slider").unwrap()["max"], 100.0);
}

fn tempfile_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("knobdeck-sdk-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_exit(child: &mut Child) -> std::process::ExitStatus {
    for _ in 0..100 {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().ok();
    panic!("plugin did not exit");
}
