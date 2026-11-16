//! A plugin that breaks the protocol on purpose, written without the SDK: `knobdeck-plugin
//! conform` must fail it. It prints a stray line, never answers requests and ignores `shutdown`.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        match v["type"].as_str() {
            Some("hello") => {
                let _ = writeln!(out, "{}", serde_json::json!({"type": "ready", "protocol": 2, "subscribe": []}));
                let _ = writeln!(out, "starting up..."); // not a frame
            }
            Some("invoke") => {
                let _ = writeln!(out, "{}", serde_json::json!({"type": "ack", "id": v["id"]}));
            }
            _ => {} // requests unanswered, shutdown ignored
        }
        let _ = out.flush();
    }
    // stdin closed: linger, as a stuck plugin would.
    std::thread::sleep(std::time::Duration::from_secs(10));
}
