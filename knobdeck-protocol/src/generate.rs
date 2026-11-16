//! Everything generated from the types: JSON Schema, TypeScript and the Markdown reference.
//! The outputs are checked in; `tests/contract.rs` fails when one is stale
//! (`KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema` rewrites them).

use crate::{
    companion, events, features, fixtures, host_calls, requests, triggers, AppState, DialOpened, HostMsg, Notify, OpenDial, PluginMsg, MIN_PROTOCOL,
    PROTOCOL,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

const HEADER: &str = "Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust\ntypes and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.";

/// One direction of one layer.
struct Dir {
    /// Schema file under `schema/`.
    file: &'static str,
    /// Prefix of the TypeScript frame interfaces.
    prefix: &'static str,
    /// The TypeScript union (and Rust enum) name.
    union: &'static str,
    /// Section title in `REFERENCE.md`.
    title: &'static str,
    /// The fixture file and its `dir` value.
    fixture: &'static str,
    dir: &'static str,
    /// The array name in `fixtures.check.ts`.
    var: &'static str,
    schema: Value,
}

fn directions() -> [Dir; 4] {
    let s = |v: schemars::Schema| serde_json::to_value(v).unwrap();
    [
        Dir { file: "host-to-plugin.json", prefix: "Host", union: "HostMsg", title: "Host → plugin", fixture: fixtures::V2, dir: "to_plugin", var: "toPlugin", schema: s(schemars::schema_for!(HostMsg)) },
        Dir { file: "plugin-to-host.json", prefix: "Plugin", union: "PluginMsg", title: "Plugin → host", fixture: fixtures::V2, dir: "to_host", var: "toHost", schema: s(schemars::schema_for!(PluginMsg)) },
        Dir { file: "companion-to-hub.json", prefix: "Companion", union: "CompanionMsg", title: "Companion → hub (layer 2)", fixture: companion::FIXTURES_V1, dir: "to_hub", var: "toHub", schema: s(schemars::schema_for!(companion::CompanionMsg)) },
        Dir { file: "hub-to-companion.json", prefix: "Hub", union: "HubMsg", title: "Hub → companion (layer 2)", fixture: companion::FIXTURES_V1, dir: "to_companion", var: "toCompanion", schema: s(schemars::schema_for!(companion::HubMsg)) },
    ]
}

/// `(file name, schema)` for each direction of both layers.
pub fn schemas() -> Vec<(&'static str, Value)> {
    directions().into_iter().map(|d| (d.file, d.schema)).collect()
}

/// Typed payloads that travel inside `event.payload`, `host_call.params` and `result.value`.
pub fn payload_defs() -> BTreeMap<String, Value> {
    let mut g = schemars::SchemaGenerator::default();
    g.subschema_for::<DialOpened>();
    g.subschema_for::<Notify>();
    g.subschema_for::<AppState>();
    g.subschema_for::<OpenDial>();
    g.subschema_for::<companion::Link>();
    g.subschema_for::<companion::LinkGone>();
    g.subschema_for::<companion::LinkEvent>();
    g.subschema_for::<companion::LinkRequest>();
    g.subschema_for::<companion::ActionResult>();
    g.subschema_for::<companion::Discovery>();
    g.take_definitions(true).into_iter().collect()
}

/// `schema/payloads.json`: the payload types as `$defs`.
fn payload_schema() -> Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Payloads",
        "description": "Typed payloads carried in event.payload, host_call.params and result.value.",
        "$defs": payload_defs(),
    })
}

/// Every `$defs` entry of both schemas, by name.
fn shared_defs() -> BTreeMap<String, Value> {
    let mut defs = payload_defs();
    for d in directions() {
        if let Some(d) = d.schema["$defs"].as_object() {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
    }
    defs
}

/// Rustdoc links (`[`x`]`, `[x](y)`) read as plain code in other languages.
fn plain(doc: &str) -> String {
    let mut out = String::new();
    let mut rest = doc;
    while let Some(i) = rest.find("[`") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find("`]") {
            Some(j) => {
                out.push_str(&after[..j + 1]);
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn description(s: &Value) -> Option<String> {
    s["description"].as_str().map(plain)
}

fn ref_name(s: &Value) -> Option<&str> {
    s["$ref"].as_str().and_then(|r| r.strip_prefix("#/$defs/"))
}

fn variant_tag(v: &Value) -> &str {
    v["properties"]["type"]["const"].as_str().expect("every variant has a `type` const")
}

fn pascal(tag: &str) -> String {
    tag.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

/// The fields of an object schema in order, without `type`: (name, schema, required).
fn fields(s: &Value) -> Vec<(String, Value, bool)> {
    let required: Vec<&str> = s["required"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    s["properties"]
        .as_object()
        .map(|p| {
            p.iter()
                .filter(|(k, _)| k.as_str() != "type")
                .map(|(k, v)| (k.clone(), v.clone(), required.contains(&k.as_str())))
                .collect()
        })
        .unwrap_or_default()
}

// ── TypeScript ───────────────────────────────────────────────────────────────

fn ts_type(s: &Value, optional: bool) -> String {
    if let Some(r) = ref_name(s) {
        return r.into();
    }
    if let Some(c) = s.get("const") {
        return c.to_string();
    }
    if let Some(any) = s["anyOf"].as_array().or(s["oneOf"].as_array()) {
        let parts: Vec<String> = any.iter().map(|a| ts_type(a, optional)).filter(|t| !(optional && t == "null")).collect();
        return parts.join(" | ");
    }
    let types: Vec<&str> = match &s["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => return "unknown".into(),
    };
    let parts: Vec<String> = types
        .iter()
        .filter(|t| !(optional && **t == "null"))
        .map(|t| match *t {
            "string" => "string".into(),
            "integer" | "number" => "number".into(),
            "boolean" => "boolean".into(),
            "null" => "null".into(),
            "array" => {
                let item = ts_type(&s["items"], false);
                if item.contains(' ') { format!("({item})[]") } else { format!("{item}[]") }
            }
            "object" if s.get("properties").is_some() => inline_object(s),
            "object" => "Record<string, unknown>".into(),
            other => panic!("schema type {other} has no TypeScript mapping"),
        })
        .collect();
    parts.join(" | ")
}

fn inline_object(s: &Value) -> String {
    let body: Vec<String> = fields(s).iter().map(|(k, v, req)| format!("{k}{}: {}", if *req { "" } else { "?" }, ts_type(v, !req))).collect();
    format!("{{ {} }}", body.join("; "))
}

fn ts_doc(out: &mut String, indent: &str, doc: Option<String>) {
    let Some(doc) = doc else { return };
    let lines: Vec<&str> = doc.lines().collect();
    if lines.len() == 1 {
        let _ = writeln!(out, "{indent}/** {} */", lines[0]);
    } else {
        let _ = writeln!(out, "{indent}/**");
        for l in lines {
            let _ = writeln!(out, "{indent} * {l}");
        }
        let _ = writeln!(out, "{indent} */");
    }
}

fn ts_fields(out: &mut String, s: &Value) {
    for (k, v, req) in fields(s) {
        ts_doc(out, "  ", description(&v));
        let _ = writeln!(out, "  {k}{}: {};", if req { "" } else { "?" }, ts_type(&v, !req));
    }
}

fn ts_list(out: &mut String, name: &str, ty: &str, doc: &str, values: &[&str]) {
    let _ = writeln!(out, "/** {doc} */");
    let list: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
    let _ = writeln!(out, "export const {name} = [{}] as const;", list.join(", "));
    let _ = writeln!(out, "export type {ty} = (typeof {name})[number];\n");
}

/// `typescript/protocol.ts`: every frame as a TypeScript type, plus the protocol constants.
pub fn typescript() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "// {}\n", HEADER.replace('\n', "\n// "));
    let _ = writeln!(out, "/** The newest protocol number. It moves only for a break (COMPAT.md). */");
    let _ = writeln!(out, "export const PROTOCOL = {PROTOCOL};");
    let _ = writeln!(out, "/** The oldest protocol number a host still loads. */");
    let _ = writeln!(out, "export const MIN_PROTOCOL = {MIN_PROTOCOL};\n");
    let _ = writeln!(out, "/** The companion protocol number (layer 2). */");
    let _ = writeln!(out, "export const COMPANION_PROTOCOL = {};", companion::COMPANION_PROTOCOL);
    let _ = writeln!(out, "/** The hub's discovery file, in KnobDeck's data folder (`Discovery`). */");
    let _ = writeln!(out, "export const COMPANION_DISCOVERY_FILE = {:?};", companion::DISCOVERY_FILE);
    let _ = writeln!(out, "/** The WebSocket path on the hub's port. */");
    let _ = writeln!(out, "export const COMPANION_WS_PATH = {:?};", companion::WS_PATH);
    let _ = writeln!(out, "/** The hub takes the first free port of `COMPANION_PORTS`. */");
    let ports: Vec<String> = (0..companion::PORT_RANGE).map(|i| (companion::DEFAULT_PORT + i).to_string()).collect();
    let _ = writeln!(out, "export const COMPANION_PORTS = [{}] as const;", ports.join(", "));
    let _ = writeln!(out, "/** Ping every this many seconds; the hub drops a link silent for `COMPANION_SILENT_DROP_SECS`. */");
    let _ = writeln!(out, "export const COMPANION_PING_EVERY_SECS = {};", companion::PING_EVERY_SECS);
    let _ = writeln!(out, "export const COMPANION_SILENT_DROP_SECS = {};\n", companion::SILENT_DROP_SECS);
    ts_list(&mut out, "FEATURES", "Feature", "Optional host behaviour, listed in `hello.features`.", features::ALL);
    ts_list(&mut out, "EVENTS", "EventName", "Host events a plugin can subscribe to.", events::ALL);
    ts_list(&mut out, "TRIGGERS", "Trigger", "Values of `invoke.trigger`.", triggers::ALL);
    ts_list(&mut out, "REQUESTS", "RequestMethod", "Values of `request.method`.", requests::ALL);
    ts_list(&mut out, "HOST_CALLS", "HostCallMethod", "Values of `host_call.method`.", host_calls::ALL);

    for (name, def) in shared_defs() {
        ts_doc(&mut out, "", description(&def));
        let _ = writeln!(out, "export interface {name} {{");
        ts_fields(&mut out, &def);
        let _ = writeln!(out, "}}\n");
    }

    for Dir { prefix, union, schema, .. } in directions() {
        let mut names = vec![];
        for v in schema["oneOf"].as_array().unwrap() {
            let tag = variant_tag(v);
            let name = format!("{prefix}{}", pascal(tag));
            ts_doc(&mut out, "", description(v));
            match ref_name(v) {
                Some(base) => {
                    let _ = writeln!(out, "export interface {name} extends {base} {{");
                }
                None => {
                    let _ = writeln!(out, "export interface {name} {{");
                }
            }
            let _ = writeln!(out, "  type: {tag:?};");
            ts_fields(&mut out, v);
            let _ = writeln!(out, "}}\n");
            names.push(name);
        }
        ts_doc(&mut out, "", description(&schema));
        let _ = writeln!(out, "export type {union} =\n  | {};\n", names.join("\n  | "));
    }
    out.trim_end().to_string() + "\n"
}

/// `typescript/fixtures.check.ts`: every fixture frame assigned to the generated union, so
/// `tsc --strict --noEmit` proves the TypeScript types accept real traffic (excess-property checks
/// included).
pub fn typescript_fixture_check() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "// {}
", HEADER.replace('\n', "\n// "));
    let unions: Vec<&str> = directions().iter().map(|d| d.union).collect();
    let _ = writeln!(out, "import type {{ {} }} from \"./protocol\";\n", unions.join(", "));
    for Dir { union, var, fixture, dir, .. } in directions() {
        let _ = writeln!(out, "export const {var}: {union}[] = [");
        for f in fixtures::frames(fixture, dir) {
            let _ = writeln!(out, "  {f},");
        }
        let _ = writeln!(out, "];\n");
    }
    out.trim_end().to_string() + "\n"
}

// ── Markdown reference ───────────────────────────────────────────────────────

fn md_type(s: &Value, optional: bool) -> String {
    ts_type(s, optional).replace('|', "\\|")
}

fn md_cell(doc: Option<String>) -> String {
    doc.unwrap_or_default().replace('\n', " ").replace('|', "\\|")
}

fn md_fields(out: &mut String, s: &Value, defs: &BTreeMap<String, Value>) {
    let mut rows = fields(s);
    if let Some(base) = ref_name(s) {
        rows.extend(fields(&defs[base]));
    }
    if rows.is_empty() {
        let _ = writeln!(out, "No fields besides `type`.\n");
        return;
    }
    let _ = writeln!(out, "| Field | Type | Required | Meaning |\n|---|---|---|---|");
    for (k, v, req) in rows {
        let _ = writeln!(out, "| `{k}` | `{}` | {} | {} |", md_type(&v, !req), if req { "yes" } else { "no" }, md_cell(description(&v)));
    }
    out.push('\n');
}

/// `REFERENCE.md`: the written protocol spec.
pub fn reference() -> String {
    let defs = shared_defs();
    let mut out = String::new();
    let _ = writeln!(out, "# KnobDeck plugin protocol reference (v{PROTOCOL})\n");
    let _ = writeln!(out, "<!-- {} -->\n", HEADER.replace('\n', " "));
    let _ = writeln!(
        out,
        "A plugin is a process KnobDeck starts. The two talk over the plugin's stdin and stdout, one JSON \
object per line (UTF-8, `\\n`-terminated). Logs go to stderr; stdout carries frames only. Every frame \
has a `type`. Unknown types and unknown fields must be ignored, so new optional fields and new frame \
types can appear without a new protocol number; what may change is in `COMPAT.md`.\n"
    );
    let _ = writeln!(
        out,
        "A session: the host sends `hello`; the plugin answers `ready`; the host sends `sync` with the placed \
instances, then `invoke`, `request` and `event` frames as the user works; `shutdown` ends it. A plugin \
built for protocol `p` runs on hosts that support `{MIN_PROTOCOL}..={PROTOCOL}` today.\n"
    );
    let _ = writeln!(out, "Machine-readable forms: `schema/*.json` (JSON Schema for each direction of both layers, and `payloads.json`) and `typescript/protocol.ts`.\n");
    let lists: [(&str, &[&str]); 5] = [
        ("Features (`hello.features`)", features::ALL),
        ("Events (`ready.subscribe`, `subscribe`, `event.event`)", events::ALL),
        ("Triggers (`invoke.trigger`)", triggers::ALL),
        ("Request methods (`request.method`)", requests::ALL),
        ("Host calls (`host_call.method`)", host_calls::ALL),
    ];
    let _ = writeln!(out, "## Names\n");
    for (title, values) in lists {
        let v: Vec<String> = values.iter().map(|x| format!("`{x}`")).collect();
        let _ = writeln!(out, "- **{title}:** {}", v.join(", "));
    }
    out.push('\n');
    for Dir { title, schema, fixture, dir, .. } in directions() {
        let _ = writeln!(out, "## {title}\n");
        if dir == "to_hub" {
            let _ = writeln!(
                out,
                "A companion (an add-on inside another app) dials KnobDeck's hub over a WebSocket on loopback: \
`ws://127.0.0.1:<port>{}`, the port and a token in `{}` in KnobDeck's data folder (ports {}–{}). \
Its `hello` names the plugin it belongs to; the plugin sees it through the `companion_*` events and host \
calls. Companion protocol {}; same compatibility rules as the plugin protocol.\n",
                companion::WS_PATH,
                companion::DISCOVERY_FILE,
                companion::DEFAULT_PORT,
                companion::DEFAULT_PORT + companion::PORT_RANGE - 1,
                companion::COMPANION_PROTOCOL
            );
        }
        let examples = fixtures::frames(fixture, dir);
        for v in schema["oneOf"].as_array().unwrap() {
            let tag = variant_tag(v);
            let _ = writeln!(out, "### `{tag}`\n");
            if let Some(d) = description(v) {
                let _ = writeln!(out, "{d}\n");
            }
            if let Some(base) = ref_name(v) {
                if let Some(d) = description(&defs[base]) {
                    let _ = writeln!(out, "{d}\n");
                }
            }
            md_fields(&mut out, v, &defs);
            let ex: Vec<String> = examples.iter().filter(|f| f["type"] == tag).map(Value::to_string).collect();
            if !ex.is_empty() {
                let _ = writeln!(out, "```json\n{}\n```\n", ex.join("\n"));
            }
        }
    }
    let _ = writeln!(out, "## Payloads\n");
    let _ = writeln!(out, "Typed contents of `event.payload`, `host_call.params` and `result.value`; see the event and host call names above.\n");
    for (name, def) in payload_defs() {
        let _ = writeln!(out, "### `{name}`\n");
        if let Some(d) = description(&def) {
            let _ = writeln!(out, "{d}\n");
        }
        md_fields(&mut out, &def, &defs);
    }
    out.trim_end().to_string() + "\n"
}

/// Every generated file, by path relative to the crate root.
pub fn all() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = schemas()
        .into_iter()
        .map(|(name, s)| (format!("schema/{name}"), serde_json::to_string_pretty(&s).unwrap() + "\n"))
        .collect();
    files.push(("schema/payloads.json".into(), serde_json::to_string_pretty(&payload_schema()).unwrap() + "\n"));
    files.push(("typescript/protocol.ts".into(), typescript()));
    files.push(("typescript/fixtures.check.ts".into(), typescript_fixture_check()));
    files.push(("REFERENCE.md".into(), reference()));
    files
}

