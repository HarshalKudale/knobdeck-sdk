//! The contract tests: every pinned frame parses into the typed messages and is written back byte
//! for byte, receivers ignore what they don't know, and the generated files are current.

use knobdeck_protocol::{fixtures, HostMsg, PluginMsg};
use serde_json::json;

fn raw_frames(dir: &str) -> Vec<String> {
    fixtures::V2
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let e: serde_json::Value = serde_json::from_str(l).unwrap();
            // Re-serialize the frame alone, keeping the fixture's key order.
            (e["dir"] == dir).then(|| e["frame"].to_string())
        })
        .collect()
}

#[test]
fn every_host_frame_round_trips() {
    let frames = raw_frames("to_plugin");
    assert!(!frames.is_empty());
    for f in frames {
        let msg: HostMsg = serde_json::from_str(&f).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_ne!(msg, HostMsg::Unknown, "{f} parsed as Unknown");
        assert_eq!(serde_json::to_string(&msg).unwrap(), f, "written back differently");
    }
}

#[test]
fn every_plugin_frame_round_trips() {
    let frames = raw_frames("to_host");
    assert!(!frames.is_empty());
    for f in frames {
        let msg: PluginMsg = serde_json::from_str(&f).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_ne!(msg, PluginMsg::Unknown, "{f} parsed as Unknown");
        assert_eq!(serde_json::to_string(&msg).unwrap(), f, "written back differently");
    }
}

#[test]
fn every_message_type_has_a_fixture() {
    let types = |dir| fixtures::frames(fixtures::V2, dir).iter().map(|f| f["type"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let host = types("to_plugin");
    for t in ["hello", "sync", "invoke", "request", "event", "result", "shutdown"] {
        assert!(host.iter().any(|x| x == t), "no to_plugin fixture for {t:?}");
    }
    let plugin = types("to_host");
    for t in ["ready", "ack", "done", "state", "error", "result", "host_call", "subscribe", "setup_required"] {
        assert!(plugin.iter().any(|x| x == t), "no to_host fixture for {t:?}");
    }
}

#[test]
fn unknown_types_and_fields_are_ignored() {
    let p = |v: serde_json::Value| serde_json::from_value::<PluginMsg>(v).unwrap();
    let h = |v: serde_json::Value| serde_json::from_value::<HostMsg>(v).unwrap();
    assert_eq!(p(json!({"type": "from_the_future", "x": 1})), PluginMsg::Unknown);
    assert_eq!(h(json!({"type": "from_the_future"})), HostMsg::Unknown);
    assert_eq!(p(json!({"type": "ack", "id": "1", "later": true})), PluginMsg::Ack { id: "1".into() });
    assert!(matches!(h(json!({"type": "invoke", "id": "1", "instance_id": "i", "action_id": "a", "trigger": "press", "new": 2})), HostMsg::Invoke(_)));
    // Fields added after v2 shipped default to the old behaviour.
    assert!(matches!(p(json!({"type": "ready", "protocol": 2})), PluginMsg::Ready { done: false, ref subscribe, .. } if subscribe.is_empty()));
    let HostMsg::Hello(hello) = h(json!({"type": "hello", "protocol": 2, "plugin_id": "p", "data_dir": "/d", "host_version": "1"})) else { panic!() };
    assert!(hello.features.is_empty() && hello.host_pid.is_none());
}

#[test]
fn a_null_result_value_is_not_the_same_as_none() {
    let r: HostMsg = serde_json::from_str(r#"{"type":"result","id":"1","ok":true,"value":null}"#).unwrap();
    let HostMsg::Result(r) = r else { panic!() };
    assert_eq!(r.value, Some(serde_json::Value::Null));
    assert_eq!(r.into_result(), Ok(serde_json::Value::Null));
}

#[test]
fn protocol_range() {
    use knobdeck_protocol::*;
    assert!(protocol_supported(PROTOCOL) && protocol_supported(MIN_PROTOCOL));
    assert!(!protocol_supported(MIN_PROTOCOL - 1) && !protocol_supported(PROTOCOL + 1));
    assert_eq!(fixtures::frames(fixtures::V2, "to_plugin")[0]["protocol"], PROTOCOL);
}

/// `schema/`, `typescript/` and `REFERENCE.md` are generated. Rewrite them with
/// `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.
#[cfg(feature = "schema")]
#[test]
fn generated_files_are_current() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let bless = std::env::var_os("KNOBDECK_BLESS").is_some();
    let mut stale = vec![];
    for (rel, text) in knobdeck_protocol::generate::all() {
        let path = root.join(&rel);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(rel);
        }
    }
    assert!(stale.is_empty(), "{stale:?} out of date: run KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema");
}
