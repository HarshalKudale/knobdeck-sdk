//! The Rust and TypeScript SDKs give the same plugin the same behaviour: `examples/parity.rs` and
//! `typescript/sdk/examples/parity.ts` are driven through one host script, and every frame they send
//! must match (numbers compared as numbers), as must their manifest and actions. The TypeScript one
//! must also pass `knobdeck-plugin conform`. Skipped when Node (22.6+) isn't installed.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

fn rust_parity() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["build", "--example", "parity", "--message-format=json-render-diagnostics"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "parity")
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .unwrap()
}

fn ts_parity() -> Vec<String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("typescript/sdk/examples/parity.ts");
    vec!["node".into(), "--experimental-strip-types".into(), "--no-warnings".into(), script.to_string_lossy().into_owned()]
}

fn have_node() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Numbers as f64, so `0.0` (Rust) equals `0` (JavaScript).
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

/// Run `cmd` through the host script; the frames it sends, in order. The script waits for each
/// invoke's `done` (and each request's `result`) before the next, so the order is deterministic.
fn transcript(cmd: &[String]) -> Vec<Value> {
    let full = |a: &str| knobdeck_sdk::make_full_id("parity", a);
    let mut child = Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let (tx, rx) = channel::<Value>();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(serde_json::from_str(&line).unwrap_or(json!({"stray": line})));
        }
    });
    let mut frames = vec![];
    let mut send = |v: Value| writeln!(stdin, "{v}").unwrap();
    let wait = |frames: &mut Vec<Value>, stop: &dyn Fn(&Value) -> bool| loop {
        let f = rx.recv_timeout(Duration::from_secs(10)).expect("plugin went quiet");
        let done = stop(&f);
        frames.push(f);
        if done {
            break;
        }
    };
    send(json!({"type": "hello", "protocol": 2, "plugin_id": "parity", "data_dir": std::env::temp_dir(), "host_version": "parity", "features": ["state_match", "notify"], "host_pid": 1}));
    wait(&mut frames, &|f| f["type"] == "ready");
    send(json!({"type": "sync", "instances": [
        {"instance_id": "b", "action_id": full("ping"), "inputs": {}},
        {"instance_id": "t", "action_id": full("mute"), "inputs": {}},
        {"instance_id": "h", "action_id": full("ptt"), "inputs": {}},
        {"instance_id": "s", "action_id": full("volume"), "inputs": {}},
        {"instance_id": "f", "action_id": full("fail"), "inputs": {}},
        {"instance_id": "n", "action_id": full("toast"), "inputs": {}},
    ]}));
    for _ in 0..6 {
        wait(&mut frames, &|f| f["type"] == "state");
    }
    let steps = [
        json!({"type": "invoke", "id": "1", "instance_id": "b", "action_id": full("ping"), "trigger": "press", "inputs": {}}),
        json!({"type": "invoke", "id": "2", "instance_id": "t", "action_id": full("mute"), "trigger": "press", "inputs": {}}),
        json!({"type": "invoke", "id": "3", "instance_id": "t", "action_id": full("mute"), "trigger": "press", "inputs": {}}),
        json!({"type": "invoke", "id": "4", "instance_id": "h", "action_id": full("ptt"), "trigger": "hold_start", "inputs": {}}),
        json!({"type": "invoke", "id": "5", "instance_id": "h", "action_id": full("ptt"), "trigger": "hold_end", "inputs": {}}),
        json!({"type": "invoke", "id": "6", "instance_id": "s", "action_id": full("volume"), "trigger": "adjust", "inputs": {"value": 40}}),
        json!({"type": "invoke", "id": "7", "instance_id": "s", "action_id": full("volume"), "trigger": "adjust", "inputs": {"value": "12.5"}}),
        json!({"type": "invoke", "id": "8", "instance_id": "f", "action_id": full("fail"), "trigger": "press", "inputs": {}}),
    ];
    for s in steps {
        let id = s["id"].clone();
        send(s);
        wait(&mut frames, &|f| f["type"] == "done" && f["id"] == id);
    }
    // The toast's host call needs an answer before `done`.
    send(json!({"type": "invoke", "id": "9", "instance_id": "n", "action_id": full("toast"), "trigger": "press", "inputs": {}}));
    loop {
        let f = rx.recv_timeout(Duration::from_secs(10)).expect("plugin went quiet");
        if f["type"] == "host_call" {
            send(json!({"type": "result", "id": f["id"], "ok": true, "value": null}));
        }
        let done = f["type"] == "done";
        frames.push(f);
        if done {
            break;
        }
    }
    for (id, req) in [
        ("q1", json!({"query_id": "items", "inputs": {"group": "fruit"}})),
        ("q2", json!({"query_id": "nope", "inputs": {}})),
    ] {
        send(json!({"type": "request", "id": id, "method": "query", "params": req}));
        wait(&mut frames, &|f| f["type"] == "result" && f["id"] == id);
    }
    send(json!({"type": "request", "id": "q3", "method": "mystery", "params": {}}));
    wait(&mut frames, &|f| f["type"] == "result" && f["id"] == "q3");
    send(json!({"type": "shutdown"}));
    let _ = child.wait();
    frames.iter().map(norm).collect()
}

#[test]
fn rust_and_typescript_plugins_behave_the_same() {
    if !have_node() {
        eprintln!("skipped: node not installed");
        return;
    }
    let rust = transcript(&[rust_parity().to_string_lossy().into_owned()]);
    let ts = transcript(&ts_parity());
    for (i, (r, t)) in rust.iter().zip(&ts).enumerate() {
        assert_eq!(r, t, "frame {i} differs:\n  rust: {r}\n  ts:   {t}");
    }
    assert_eq!(rust.len(), ts.len(), "frame counts differ:\nrust: {rust:#?}\nts: {ts:#?}");
    // Something real was compared.
    assert!(rust.iter().any(|f| f["type"] == "error" && f["message"] == "nope"));
    assert!(rust.iter().any(|f| f["state"] == "40"));
    assert!(rust.iter().any(|f| f["type"] == "host_call" && f["method"] == "notify"));
}

#[test]
fn rust_and_typescript_generate_the_same_manifest_and_actions() {
    if !have_node() {
        eprintln!("skipped: node not installed");
        return;
    }
    let cg = |cmd: &[String]| -> Value {
        let out = Command::new(&cmd[0]).args(&cmd[1..]).env("KNOBDECK_CODEGEN", "1").output().unwrap();
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let rust = cg(&[rust_parity().to_string_lossy().into_owned()]);
    let ts = cg(&ts_parity());
    assert_eq!(norm(&rust["actions"]), norm(&ts["actions"]));
    // The manifests differ only in how the plugin is run.
    let strip = |v: &Value| {
        let mut m = v["manifest"].clone();
        m.as_object_mut().unwrap().remove("runtime");
        m
    };
    assert_eq!(strip(&rust), strip(&ts));
    assert_eq!(ts["manifest"]["runtime"], json!({"language": "node", "entry": "parity.ts"}));
}

#[test]
fn the_typescript_plugin_conforms() {
    if !have_node() {
        eprintln!("skipped: node not installed");
        return;
    }
    let out = Command::new(&ts_parity()[0]).args(&ts_parity()[1..]).env("KNOBDECK_CODEGEN", "1").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let dir = std::env::temp_dir().join(format!("knobdeck-parity-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), v["manifest"].to_string()).unwrap();
    std::fs::write(dir.join("actions.json"), v["actions"].to_string()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_knobdeck-plugin")).arg("conform").arg("--dir").arg(&dir).arg("--").args(ts_parity()).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).ok();
}

/// TypeScript plugins in KnobDeck share one Node process (`typescript/runner`), each in a worker.
/// Through the runner (and the shim the host starts per plugin), the parity plugin must still give
/// the Rust transcript, with two plugins running in the same runner at once.
#[test]
fn the_shared_runner_gives_the_same_transcript() {
    if !have_node() {
        eprintln!("skipped: node not installed");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("typescript");
    let address = if cfg!(windows) {
        format!(r"\\.\pipe\knobdeck-runner-test-{}", std::process::id())
    } else {
        std::env::temp_dir().join(format!("knobdeck-runner-test-{}.sock", std::process::id())).to_string_lossy().into_owned()
    };
    let token = format!("t{}", std::process::id());
    // The worker inherits the runner's flags, so it can load the .ts example.
    let mut runner = Command::new("node")
        .args(["--experimental-strip-types", "--no-warnings"])
        .arg(root.join("runner/runner.mjs"))
        .arg(&address)
        .env("KNOBDECK_RUNNER_TOKEN", &token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Wait for "listening on", then keep draining the runner's log.
    let mut log = BufReader::new(runner.stderr.take().unwrap());
    let mut first = String::new();
    log.read_line(&mut first).unwrap();
    assert!(first.contains("listening on"), "{first}");
    let loaded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = loaded.clone();
    std::thread::spawn(move || {
        for line in log.lines().map_while(Result::ok) {
            sink.lock().unwrap().push(line);
        }
    });

    let shim: Vec<String> = vec![
        "node".into(),
        "--experimental-strip-types".into(),
        "--no-warnings".into(),
        root.join("runner/shim.ts").to_string_lossy().into_owned(),
        address.clone(),
        root.join("sdk/examples/parity.ts").to_string_lossy().into_owned(),
    ];
    // A wrong token: the runner closes the connection before loading anything.
    let refused = Command::new(&shim[0]).args(&shim[1..]).env("KNOBDECK_RUNNER_TOKEN", "wrong").stdin(Stdio::null()).output().unwrap();
    assert!(refused.stdout.is_empty(), "no frames without the token");
    std::env::set_var("KNOBDECK_RUNNER_TOKEN", &token); // inherited by the shims below
    let rust = transcript(&[rust_parity().to_string_lossy().into_owned()]);
    let runs: Vec<_> = (0..2).map(|_| {
        let shim = shim.clone();
        std::thread::spawn(move || transcript(&shim))
    }).collect();
    for run in runs {
        let ts = run.join().unwrap();
        for (i, (r, t)) in rust.iter().zip(&ts).enumerate() {
            assert_eq!(r, t, "frame {i} differs through the runner:\n  rust: {r}\n  ts:   {t}");
        }
        assert_eq!(rust.len(), ts.len());
    }
    let _ = runner.kill();
    let _ = runner.wait();
    let log = loaded.lock().unwrap().join("\n");
    assert_eq!(log.matches("loading ").count(), 2, "both plugins ran in the one runner:\n{log}");
    assert!(log.contains("refused a connection: wrong token"), "{log}");
    if !cfg!(windows) {
        let _ = std::fs::remove_file(&address);
    }
}
