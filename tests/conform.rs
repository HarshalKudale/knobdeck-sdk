//! `knobdeck-plugin conform` against a plugin that follows the protocol (the demo) and one that
//! doesn't (`examples/conform_bad.rs`), each run as a plain command, as a non-Rust plugin would be.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Build an example and return its executable (cargo says where).
fn example(name: &str) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["build", "--example", name, "--message-format=json-render-diagnostics"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == name)
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .unwrap()
}

/// A plugin folder with the demo's manifest and actions (from its codegen), for `--dir`.
fn plugin_dir(tag: &str) -> PathBuf {
    let demo = example("demo");
    let cg = Command::new(&demo).env("KNOBDECK_CODEGEN", "1").output().unwrap();
    let v: Value = serde_json::from_slice(&cg.stdout).unwrap();
    let dir = std::env::temp_dir().join(format!("knobdeck-conform-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), v["manifest"].to_string()).unwrap();
    std::fs::write(dir.join("actions.json"), v["actions"].to_string()).unwrap();
    dir
}

fn conform(dir: &PathBuf, exe: &PathBuf) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_knobdeck-plugin"))
        .arg("conform")
        .arg("--dir")
        .arg(dir)
        .arg("--")
        .arg(exe)
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

#[test]
fn the_demo_conforms() {
    let dir = plugin_dir("good");
    let (ok, text) = conform(&dir, &example("demo"));
    assert!(ok, "{text}");
    assert!(text.contains("✓ conforms to protocol 2"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_broken_plugin_fails_the_right_checks() {
    let dir = plugin_dir("bad");
    let (ok, text) = conform(&dir, &example("conform_bad"));
    assert!(!ok, "{text}");
    let line = |name: &str| text.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("no {name} line:\n{text}")).to_string();
    assert!(line("ready ").contains('✓'), "{text}");
    assert!(line("ready.done").contains('✗'), "it doesn't promise done");
    assert!(line("ack").contains('✓'));
    assert!(line("answers requests").contains('✗'));
    assert!(line("shutdown").contains('✗'));
    assert!(line("stdout is frames only").contains('✗') && text.contains("starting up..."), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}
