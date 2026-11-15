//! Generates `manifest.json` + `actions.json` content from a `Plugin`.
//! Emitted by the plugin binary itself when launched with `KNOBDECK_CODEGEN=1`
//! (the `knobdeck-plugin` CLI does this on `dev` / `build`).

use crate::plugin::{Action, Kind, Plugin};
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

/// Globally unique action id: `<pluginId>-<actionId>-<sha1(pluginId:actionId)[..8]>`.
/// Deterministic, so placed slices survive regeneration.
pub fn make_full_id(plugin_id: &str, action_id: &str) -> String {
    if plugin_id.is_empty() {
        return action_id.into();
    }
    let digest = Sha1::digest(format!("{plugin_id}:{action_id}").as_bytes());
    let hash: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{plugin_id}-{action_id}-{hash}")
}

#[derive(Clone, Copy, PartialEq)]
pub enum Stage {
    Dev,
    Build,
}

fn exe_name(base: &str) -> String {
    format!("{base}{}", std::env::consts::EXE_SUFFIX)
}

pub fn generate_manifest(p: &Plugin, stage: Stage) -> Value {
    let m = &p.meta;
    let cur = std::env::current_exe().unwrap_or_default();
    let setup = m.setup_bin.as_deref().map(exe_name);
    let (plugin_exe, setup_exe) = match stage {
        // Dev runs the binaries straight out of target/: absolute paths.
        Stage::Dev => (
            cur.to_string_lossy().into_owned(),
            setup.map(|n| cur.with_file_name(n).to_string_lossy().into_owned()),
        ),
        Stage::Build => (cur.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), setup),
    };

    // No `language` key: the host spawns it as a native executable.
    let mut runtime = Map::new();
    if stage == Stage::Build {
        // Per-OS entry, so packages built on Windows and macOS can be merged
        // (the CLI keeps the other OS's entries); `executable` is the fallback.
        runtime.insert("executables".into(), json!({ crate::platform(): plugin_exe }));
    }
    runtime.insert("executable".into(), plugin_exe.into());
    if let Some(v) = m.ack_timeout_ms {
        runtime.insert("ack_timeout_ms".into(), v.into());
    }
    if let Some(v) = m.query_timeout_ms {
        runtime.insert("query_timeout_ms".into(), v.into());
    }

    let mut manifest = Map::new();
    manifest.insert("id".into(), m.id.clone().into());
    manifest.insert("name".into(), m.name.clone().into());
    manifest.insert("protocol".into(), crate::runtime::PROTOCOL.into());
    manifest.insert("runtime".into(), Value::Object(runtime));
    for (k, v) in [("icon", &m.icon), ("version", &m.version), ("author", &m.author), ("description", &m.description)] {
        if let Some(v) = v {
            manifest.insert(k.into(), v.clone().into());
        }
    }
    if !m.platforms.is_empty() {
        manifest.insert("platforms".into(), json!(m.platforms));
    }
    // The OS context this binary can read (its SDK `os-*` features), for the plugins page.
    let uses = crate::context::uses();
    if !uses.is_empty() {
        manifest.insert("uses".into(), json!(uses));
    }

    if let Some(exe) = setup_exe {
        let mut setup = Map::new();
        if stage == Stage::Build {
            setup.insert("executables".into(), json!({ crate::platform(): exe }));
        }
        setup.insert("executable".into(), exe.into());
        if let Some(page) = &m.setup_page {
            setup.insert("page".into(), page.clone().into());
        }
        manifest.insert("setup".into(), Value::Object(setup));
        manifest.insert("setup_required".into(), true.into());
        if !m.setup_inputs.is_empty() {
            manifest.insert("setup_inputs".into(), Value::Array(m.setup_inputs.clone()));
        }
    }
    Value::Object(manifest)
}

fn strip_style(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("color");
        o.remove("style");
    }
    v
}

fn validate_inputs(a: &Action, p: &Plugin) -> Result<(), String> {
    // "combobox" is rendered by the host (SliceConfig) though the JS SDK validator omits it.
    const VALID: [&str; 6] = ["text", "textarea", "checkbox", "select", "combobox", "keybind"];
    for inp in &a.inputs {
        let key = inp["key"].as_str().filter(|k| !k.is_empty());
        let key = key.ok_or_else(|| format!("Input in action \"{}\" is missing required \"key\" field", a.id))?;
        if let Some(t) = inp["type"].as_str() {
            if !VALID.contains(&t) {
                return Err(format!("Input \"{key}\" in action \"{}\" has unknown type \"{t}\"", a.id));
            }
        }
        let q = &inp["query"];
        if q.is_null() {
            continue;
        }
        let qid = q.as_str().or_else(|| q["id"].as_str()).filter(|s| !s.is_empty());
        let qid = qid.ok_or_else(|| format!("Input \"{key}\" in action \"{}\" has a \"query\" with no id", a.id))?;
        if !p.queries.contains_key(qid) {
            return Err(format!(
                "Input \"{key}\" in action \"{}\" references query \"{qid}\", but no such provider is registered (use plugin.query(\"{qid}\", ..)).",
                a.id
            ));
        }
        for dep in q["args"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !a.inputs.iter().any(|o| o["key"] == dep) {
                return Err(format!(
                    "Input \"{key}\" in action \"{}\" query depends on \"{dep}\", but no sibling input has that key.",
                    a.id
                ));
            }
        }
    }
    Ok(())
}

pub fn generate_actions(p: &Plugin) -> Result<Value, String> {
    let mut list = Vec::new();
    for id in &p.order {
        let a = &p.actions[id];
        validate_inputs(a, p)?;
        match a.kind {
            Kind::Toggle if a.states.is_empty() => {
                return Err(format!("Toggle action \"{id}\" must declare a non-empty \"states\" list"))
            }
            Kind::Hold if a.released.is_none() || a.held.is_none() => {
                return Err(format!("Hold action \"{id}\" must declare both \"released\" and \"held\""))
            }
            Kind::Slider if a.step.map_or(false, |s| s <= 0.0) => {
                return Err(format!("Slider action \"{id}\" \"step\" must be a positive number"))
            }
            _ => {}
        }

        let mut e = Map::new();
        e.insert("id".into(), make_full_id(&p.meta.id, id).into());
        e.insert("label".into(), a.label.clone().into());
        e.insert("type".into(), a.kind.as_str().into());
        if let Some(icon) = &a.icon {
            e.insert("icon".into(), icon.clone().into());
        }
        e.insert("inputs".into(), Value::Array(a.inputs.clone()));
        // An action without its own platforms inherits the plugin's.
        let platforms = if a.platforms.is_empty() { &p.meta.platforms } else { &a.platforms };
        if !platforms.is_empty() {
            e.insert("platforms".into(), json!(platforms));
        }
        if !a.needs.is_empty() {
            e.insert("needs".into(), a.needs.iter().map(|n| n.as_str()).collect());
        }
        match a.kind {
            Kind::Toggle => {
                e.insert("states".into(), a.states.iter().map(strip_style).collect());
            }
            Kind::Hold => {
                e.insert("released".into(), strip_style(a.released.as_ref().unwrap()));
                e.insert("held".into(), strip_style(a.held.as_ref().unwrap()));
            }
            Kind::Slider => {
                e.insert("min".into(), json!(a.min));
                e.insert("max".into(), json!(a.max));
                e.insert("step".into(), json!(a.step.unwrap_or(1.0)));
                if let Some(u) = &a.unit {
                    e.insert("unit".into(), u.clone().into());
                }
            }
            _ => {}
        }
        list.push(Value::Object(e));
    }
    Ok(json!({ "actions": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_id_matches_js_sdk() {
        // Value computed with the JS SDK's makeFullId('discord', 'mute').
        assert_eq!(make_full_id("discord", "mute"), "discord-mute-b094f5e1");
        assert_eq!(make_full_id("", "mute"), "mute");
    }

    /// Placed slices store these ids; they must never change.
    #[test]
    fn full_ids_are_pinned() {
        assert_eq!(make_full_id("example-rust", "mute"), "example-rust-mute-b1c2189c");
        assert_eq!(make_full_id("win-tools", "always-on-top"), "win-tools-always-on-top-7ee2f192");
    }

    #[test]
    fn platforms_and_per_os_executables() {
        let p = Plugin::new("mac-tools", "Mac Tools")
            .platforms(&["macos"])
            .action(Action::button("a", "A").on_press(|_| Ok(())))
            .action(Action::button("b", "B").platforms(&["windows"]).on_press(|_| Ok(())));
        let m = generate_manifest(&p, Stage::Build);
        assert_eq!(m["platforms"], json!(["macos"]));
        let exe = m["runtime"]["executable"].clone();
        assert_eq!(m["runtime"]["executables"][crate::platform()], exe);
        assert!(generate_manifest(&p, Stage::Dev)["runtime"]["executables"].is_null());
        let a = generate_actions(&p).unwrap();
        assert_eq!(a["actions"][0]["platforms"], json!(["macos"])); // inherited from the plugin
        assert_eq!(a["actions"][1]["platforms"], json!(["windows"]));
    }
}
