//! Plugin runtime: protocol v2 over stdio. The host writes one JSON object per
//! line to our stdin (`hello`, `sync`, `invoke`, `request`, `event`, `result`,
//! `shutdown`); we answer on stdout (`ready`, `ack`, `state`, `error`, `result`,
//! `host_call`, `subscribe`, `setup_required`). Logs go to stderr, which the
//! host writes to the plugin's log file.
//!
//! Handlers for the same instance run in order on that instance's worker
//! thread (a slider never applies 30 after 50); different instances, queries
//! and events run in parallel, so a slow handler never blocks the reader.

use crate::codegen::make_full_id;
use crate::ctx::{Ctx, Event, InitialCtx, OpenCtx, QueryCtx};
use crate::error::{Error, Result};
use crate::log::log;
use crate::plugin::{Action, Kind, LiveFn, Plugin};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Protocol version this SDK speaks.
pub const PROTOCOL: u32 = 2;
/// Host feature (listed in `hello.features`): a `state`/`error` frame sent to an
/// `action_id` may carry `match`, and only instances whose inputs hold those
/// values get it. Older hosts lack it; the SDK then sends per-instance frames.
pub const FEATURE_STATE_MATCH: &str = "state_match";
const MAX_WHEEL_BUTTONS: usize = 16;
/// An idle per-instance worker thread exits after this long.
const WORKER_IDLE: Duration = Duration::from_secs(30);

/// Partial visual update for one instance. Fields left `None` are not sent; a
/// patch without `state` leaves the instance's state/error untouched.
#[derive(Default, Clone, Debug)]
pub struct Visual {
    pub state: Option<String>,
    pub label: Option<String>,
    pub icon: Option<String>,
}

impl Visual {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn state(mut self, v: impl ToString) -> Self {
        self.state = Some(v.to_string());
        self
    }
    pub fn label(mut self, v: impl ToString) -> Self {
        self.label = Some(v.to_string());
        self
    }
    pub fn icon(mut self, v: impl ToString) -> Self {
        self.icon = Some(v.to_string());
        self
    }

    fn frame(&self, target: (&str, &str)) -> Value {
        let mut m = Map::new();
        m.insert("type".into(), "state".into());
        m.insert(target.0.into(), target.1.into());
        self.fill(&mut m);
        Value::Object(m)
    }

    fn fill(&self, m: &mut Map<String, Value>) {
        if let Some(v) = &self.state {
            m.insert("state".into(), v.clone().into());
        }
        if let Some(v) = &self.label {
            m.insert("label".into(), v.clone().into());
        }
        if let Some(v) = &self.icon {
            m.insert("icon".into(), v.clone().into());
        }
    }
}

/// Does an instance's `inputs` hold every value in `filter`? Values compare as
/// strings (`5` matches `"5"`), and a missing input counts as `""`. The host
/// applies the same rule to a `match` frame.
pub fn inputs_match(inputs: &Value, filter: &Map<String, Value>) -> bool {
    filter.iter().all(|(k, want)| {
        let have = match &inputs[k] {
            Value::Null => String::new(),
            v => value_str(v),
        };
        let want = match want {
            Value::Null => String::new(),
            v => value_str(v),
        };
        have == want
    })
}

/// A `match` filter made from `keys` of `inputs` (the targeted fan-out of `Ctx::fan_out_by`).
pub(crate) fn filter_from(inputs: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter().map(|k| (k.to_string(), inputs[*k].clone())).collect()
}

type Job = Box<dyn FnOnce() + Send>;

pub(crate) struct Inner {
    out: Mutex<Box<dyn Write + Send>>,
    data_dir: PathBuf,
    state: Mutex<HashMap<String, String>>,
    action_of: Mutex<HashMap<String, String>>,
    inputs_of: Mutex<HashMap<String, Value>>,
    short_to_full: HashMap<String, String>,
    full_to_short: HashMap<String, String>,
    pending: Mutex<HashMap<String, Sender<std::result::Result<Value, String>>>>,
    next_id: AtomicU64,
    /// instance id → its ordered worker's queue.
    workers: Mutex<HashMap<String, Sender<Job>>>,
    /// The host takes `match` on fan-out frames (`FEATURE_STATE_MATCH`).
    state_match: bool,
}

/// Handle for pushing state/visuals/errors outside of a handler (timers, pollers,
/// external listeners). Cheap to clone and `Send`; hand it to your own threads.
#[derive(Clone)]
pub struct Runtime(pub(crate) Arc<Inner>);

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(crate) fn global() -> Option<&'static Runtime> {
    RUNTIME.get()
}

impl Runtime {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }

    /// This plugin's persistent data folder (`%APPDATA%\KnobDeck\plugin-data\<id>`).
    /// Survives plugin updates and reinstalls: keep tokens and settings here.
    pub fn data_dir(&self) -> &Path {
        &self.0.data_dir
    }

    pub(crate) fn send(&self, obj: Value) {
        {
            let mut out = self.0.out.lock().unwrap();
            if let Err(e) = writeln!(out, "{obj}").and_then(|_| out.flush()) {
                log(format!("send error: {e}"));
            }
        }
        // Only frames that carry a `state` update the local mirror.
        if obj["type"] == "state" {
            if let Some(s) = obj["state"].as_str() {
                let mut st = self.0.state.lock().unwrap();
                if let Some(iid) = obj["instance_id"].as_str() {
                    st.insert(iid.into(), s.into());
                } else if let Some(aid) = obj["action_id"].as_str() {
                    let short = self.0.full_to_short.get(aid).map(String::as_str).unwrap_or(aid);
                    let filter = obj["match"].as_object();
                    let inputs = self.0.inputs_of.lock().unwrap();
                    for (iid, a) in self.0.action_of.lock().unwrap().iter() {
                        let hit = filter.map_or(true, |f| inputs.get(iid).is_some_and(|i| inputs_match(i, f)));
                        if a == short && hit {
                            st.insert(iid.clone(), s.into());
                        }
                    }
                }
            }
        }
    }

    fn full_id(&self, short: &str) -> String {
        self.0.short_to_full.get(short).cloned().unwrap_or_else(|| short.into())
    }

    /// Set the state of one instance.
    pub fn set_state(&self, instance_id: &str, value: impl ToString) {
        self.send(json!({"type": "state", "instance_id": instance_id, "state": value.to_string()}));
    }
    /// Fan-out: set the state of ALL instances of an action (short action id).
    pub fn notify(&self, action_id: &str, value: impl ToString) {
        self.send(json!({"type": "state", "action_id": self.full_id(action_id), "state": value.to_string()}));
    }
    pub fn set_visual(&self, instance_id: &str, patch: Visual) {
        self.send(patch.frame(("instance_id", instance_id)));
    }
    pub fn notify_visual(&self, action_id: &str, patch: Visual) {
        self.send(patch.frame(("action_id", &self.full_id(action_id))));
    }
    /// Targeted fan-out: set the state of every instance of an action (short id)
    /// whose inputs hold the values in `filter`, e.g. `json!({"device": id})`.
    /// For state that belongs to one target (a device, a user, a light) rather
    /// than to the whole action. Values compare as strings; see [`inputs_match`].
    pub fn notify_matching(&self, action_id: &str, filter: &Value, value: impl ToString) {
        self.notify_visual_matching(action_id, filter, Visual::new().state(value));
    }
    /// Targeted fan-out of visuals; see [`Runtime::notify_matching`].
    pub fn notify_visual_matching(&self, action_id: &str, filter: &Value, patch: Visual) {
        let filter = filter.as_object().cloned().unwrap_or_default();
        if self.0.state_match {
            let mut m = Map::new();
            m.insert("type".into(), "state".into());
            m.insert("action_id".into(), self.full_id(action_id).into());
            m.insert("match".into(), Value::Object(filter));
            patch.fill(&mut m);
            self.send(Value::Object(m));
        } else {
            // An older host would send an unknown `match` to every instance: address each one.
            for iid in self.instances_matching(action_id, &filter) {
                self.set_visual(&iid, patch.clone());
            }
        }
    }
    pub fn set_error(&self, instance_id: &str, message: impl ToString) {
        self.send(json!({"type": "error", "instance_id": instance_id, "message": message.to_string()}));
    }
    /// Instance ids of the placed instances of `action_id` (short id) whose inputs hold `filter`.
    pub fn instances_matching(&self, action_id: &str, filter: &Map<String, Value>) -> Vec<String> {
        let inputs = self.0.inputs_of.lock().unwrap();
        self.instances_of(action_id)
            .into_iter()
            .filter(|iid| inputs.get(iid).is_some_and(|i| inputs_match(i, filter)))
            .collect()
    }
    /// Subscribe to host events (e.g. `"focus_changed"`).
    pub fn subscribe<S: AsRef<str>>(&self, events: &[S]) {
        let names = normalize(events.iter().map(|s| s.as_ref().to_string()));
        if !names.is_empty() {
            self.send(json!({"type": "subscribe", "events": names}));
        }
    }
    /// Tell the host this plugin needs setup again (expired token, revoked
    /// auth…). The host remembers it, stops the plugin and asks the user to run
    /// setup; `reason` is shown to them.
    pub fn mark_setup_required(&self, reason: impl ToString) {
        self.send(json!({"type": "setup_required", "reason": reason.to_string()}));
    }
    /// Instance ids of every placed instance of `action_id` (short id) learned from sync.
    pub fn instances_of(&self, action_id: &str) -> Vec<String> {
        self.0
            .action_of
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| a.as_str() == action_id)
            .map(|(i, _)| i.clone())
            .collect()
    }
    /// Whether any action of this plugin is currently placed. Pollers can
    /// skip work while this is false.
    pub fn has_instances(&self) -> bool {
        !self.0.action_of.lock().unwrap().is_empty()
    }
    /// Configured inputs of a placed instance.
    pub fn inputs_of(&self, instance_id: &str) -> Value {
        self.0.inputs_of.lock().unwrap().get(instance_id).cloned().unwrap_or(json!({}))
    }

    pub(crate) fn host_call(&self, method: &str, params: Value) -> Result<Value> {
        let id = format!("host-{}", self.0.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = channel();
        self.0.pending.lock().unwrap().insert(id.clone(), tx);
        self.send(json!({"type": "host_call", "id": id, "method": method, "params": params}));
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(Error::Other(e)),
            Err(_) => {
                self.0.pending.lock().unwrap().remove(&id);
                Err(Error::msg("KnobDeck host call timed out"))
            }
        }
    }
}

fn normalize(events: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in events {
        let t = e.trim().to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn compute_initial(def: Option<&Action>, ic: &InitialCtx) -> String {
    let Some(def) = def else { return "off".into() };
    if def.kind == Kind::Hold {
        return "released".into();
    }
    if let Some(init) = &def.initial {
        if let Some(v) = init.resolve(ic) {
            return v;
        }
    }
    if let Some(v) = def.states.first().and_then(|s| s.get("value")) {
        return value_str(v);
    }
    if def.kind == Kind::Slider {
        return value_str(&json!(def.min));
    }
    "off".into()
}

pub(crate) fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

struct Core {
    plugin: Plugin,
    rt: Runtime,
}

impl Core {
    fn action(&self, short: &str) -> Option<&Action> {
        self.plugin.actions.get(short)
    }
    fn short(&self, full: &str) -> String {
        self.rt.0.full_to_short.get(full).cloned().unwrap_or_else(|| full.into())
    }
}

/// Take the real stdout for the protocol and point the process's stdout at
/// stderr, so a stray `println!` in plugin code lands in the log instead of
/// corrupting the message stream.
#[cfg(windows)]
fn take_protocol_stdout() -> Box<dyn Write + Send> {
    use std::os::windows::io::FromRawHandle;
    type Handle = *mut std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn SetStdHandle(which: u32, handle: Handle) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out as isize == -1 {
            return Box::new(std::io::stdout());
        }
        // std's stdout looks the handle up on every write, so this takes effect at once.
        SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
        Box::new(std::fs::File::from_raw_handle(out))
    }
}

/// Unix (macOS): keep a duplicate of fd 1 for the protocol, then point fd 1
/// at stderr so `println!` lands in the log.
#[cfg(unix)]
fn take_protocol_stdout() -> Box<dyn Write + Send> {
    use std::os::unix::io::FromRawFd;
    extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(src: i32, dst: i32) -> i32;
    }
    let _ = std::io::stdout().flush();
    unsafe {
        let proto = dup(1);
        if proto < 0 {
            return Box::new(std::io::stdout());
        }
        if dup2(2, 1) < 0 {
            // Couldn't redirect: write the protocol through the duplicate anyway.
            log("stdout redirect failed; stray prints may corrupt the protocol");
        }
        Box::new(std::fs::File::from_raw_fd(proto))
    }
}

#[cfg(not(any(windows, unix)))]
fn take_protocol_stdout() -> Box<dyn Write + Send> {
    Box::new(std::io::stdout())
}

pub(crate) fn run(plugin: Plugin) -> ! {
    let out = take_protocol_stdout();
    // Panics inside handler threads are caught per-thread; log them.
    std::panic::set_hook(Box::new(|info| log(format!("panic: {info}"))));

    let mut lines = std::io::stdin().lock().lines();
    let hello = loop {
        match lines.next() {
            Some(Ok(line)) => match serde_json::from_str::<Value>(&line) {
                Ok(v) if v["type"] == "hello" => break v,
                _ => log(format!("expected hello, got: {line}")),
            },
            _ => {
                log("stdin closed before hello (run plugins from KnobDeck, not directly)");
                std::process::exit(1)
            }
        }
    };
    if hello["protocol"] != PROTOCOL {
        log(format!("host speaks protocol {}, this SDK speaks {PROTOCOL}", hello["protocol"]));
    }
    let plugin_id = hello["plugin_id"].as_str().filter(|s| !s.is_empty()).unwrap_or(&plugin.meta.id).to_string();
    let data_dir = hello["data_dir"].as_str().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    let mut short_to_full = HashMap::new();
    let mut full_to_short = HashMap::new();
    for short in plugin.actions.keys() {
        let full = make_full_id(&plugin_id, short);
        short_to_full.insert(short.clone(), full.clone());
        full_to_short.insert(full, short.clone());
    }

    let rt = Runtime(Arc::new(Inner {
        out: Mutex::new(out),
        data_dir,
        state: Default::default(),
        action_of: Default::default(),
        inputs_of: Default::default(),
        short_to_full,
        full_to_short,
        pending: Default::default(),
        next_id: AtomicU64::new(1),
        workers: Default::default(),
        state_match: hello["features"]
            .as_array()
            .is_some_and(|f| f.iter().any(|x| x == FEATURE_STATE_MATCH)),
    }));
    let _ = RUNTIME.set(rt.clone());

    let mut wanted = plugin.subscriptions.clone();
    if plugin.actions.values().any(|a| a.volatile) {
        wanted.push("focus_changed".into());
    }
    // `done`: this runtime reports each invoke's end, so the host's workflows
    // run the next step only after this one's keys/windows have landed.
    rt.send(json!({"type": "ready", "protocol": PROTOCOL, "subscribe": normalize(wanted.into_iter()), "done": true}));
    log("ready");

    for (short, def) in &plugin.actions {
        if let Some((every, f)) = &def.live {
            start_live(rt.clone(), short.clone(), *every, f.clone());
        }
    }
    let core = Arc::new(Core { plugin, rt });
    for line in lines {
        let Ok(line) = line else { break };
        match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg["type"] == "shutdown" => break,
            Ok(msg) => dispatch(&core, msg),
            Err(e) => log(format!("bad line from host ({e}): {line}")),
        }
    }
    finish(&core)
}

fn finish(core: &Core) -> ! {
    for (_, tx) in core.rt.0.pending.lock().unwrap().drain() {
        let _ = tx.send(Err("KnobDeck host disconnected".into()));
    }
    if let Some(f) = &core.plugin.on_shutdown {
        if catch_unwind(AssertUnwindSafe(|| f(&core.rt))).is_err() {
            log("on_shutdown panicked");
        }
    }
    log("shutdown");
    std::process::exit(0)
}

/// Run `f` on its own thread, converting panics into a logged message.
fn spawn(what: &'static str, f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || run_caught(what, f));
}

fn run_caught(what: &str, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        log(format!("{what}: handler panicked"));
    }
}

/// Run `job` after every job already queued for `key`, on that key's worker
/// thread (started on demand, exits after `WORKER_IDLE`).
fn run_ordered(rt: &Runtime, key: String, job: Job) {
    let mut workers = rt.0.workers.lock().unwrap();
    let job = match workers.get(&key) {
        Some(tx) => match tx.send(job) {
            Ok(()) => return,
            Err(e) => e.0,
        },
        None => job,
    };
    let (tx, rx) = channel::<Job>();
    let _ = tx.send(job);
    workers.insert(key.clone(), tx);
    drop(workers);
    let rt = rt.clone();
    std::thread::spawn(move || loop {
        match rx.recv_timeout(WORKER_IDLE) {
            Ok(job) => run_caught("invoke", job),
            Err(RecvTimeoutError::Timeout) => {
                // Senders enqueue under this lock, so nothing can slip in after the check.
                let mut workers = rt.0.workers.lock().unwrap();
                match rx.try_recv() {
                    Ok(job) => {
                        drop(workers);
                        run_caught("invoke", job);
                    }
                    Err(_) => {
                        workers.remove(&key);
                        return;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    });
}

/// Poll `f` for every placed instance of `action` and push state/label/icon changes.
fn start_live(rt: Runtime, action: String, every: Duration, f: LiveFn) {
    spawn("live", move || {
        let mut sent: HashMap<String, (Option<String>, Option<String>, Option<String>)> = HashMap::new();
        let mut failed: HashMap<String, String> = HashMap::new();
        // Each instance runs every `every`, but a newly placed one runs within a tick, not a whole interval.
        let mut due: HashMap<String, Instant> = HashMap::new();
        let tick = every.min(Duration::from_secs(1));
        loop {
            let ids = rt.instances_of(&action);
            sent.retain(|k, _| ids.contains(k));
            failed.retain(|k, _| ids.contains(k));
            due.retain(|k, _| ids.contains(k));
            for iid in ids {
                let now = Instant::now();
                if due.get(&iid).is_some_and(|t| now < *t) {
                    continue;
                }
                due.insert(iid.clone(), now + every);
                let ic = InitialCtx {
                    instance_id: iid.clone(),
                    action_id: action.clone(),
                    inputs: rt.inputs_of(&iid),
                    fg_process: None,
                    fg_title: None,
                };
                match catch_unwind(AssertUnwindSafe(|| f(&ic))) {
                    Ok(Ok(v)) => {
                        failed.remove(&iid);
                        let key = (v.state.clone(), v.label.clone(), v.icon.clone());
                        if sent.get(&iid) != Some(&key) {
                            rt.set_visual(&iid, v);
                            sent.insert(iid, key);
                        }
                    }
                    // Log each distinct failure once, not every tick.
                    Ok(Err(e)) => {
                        if failed.get(&iid).map(String::as_str) != Some(e.message()) {
                            log(format!("live ({action}): {}", e.message()));
                            failed.insert(iid, e.message().to_string());
                        }
                    }
                    Err(_) => log(format!("live ({action}): panicked")),
                }
            }
            std::thread::sleep(tick);
        }
    });
}

fn dispatch(core: &Arc<Core>, msg: Value) {
    let c = core.clone();
    match msg["type"].as_str().unwrap_or("") {
        "sync" => handle_sync(core, &msg),
        "invoke" => {
            // Ack immediately on the reader thread, before any handler work.
            core.rt.send(json!({"type": "ack", "id": msg["id"]}));
            let key = msg["instance_id"].as_str().unwrap_or("").to_string();
            run_ordered(&core.rt, key, Box::new(move || {
                // Sent on drop, so a handler that panics still reports its end.
                let _done = Done(c.rt.clone(), msg["id"].clone());
                handle_invoke(&c, msg)
            }));
        }
        "request" => match msg["method"].as_str().unwrap_or("") {
            "query" => spawn("query", move || handle_query(&c, &msg)),
            "wheel" => spawn("wheel", move || handle_wheel(&c, &msg)),
            other => core.rt.send(json!({"type": "result", "id": msg["id"], "ok": false, "error": format!("unknown request method \"{other}\"")})),
        },
        "event" => handle_event(core, msg),
        "result" => {
            let Some(id) = msg["id"].as_str() else { return };
            if let Some(tx) = core.rt.0.pending.lock().unwrap().remove(id) {
                let _ = tx.send(if msg["ok"] == true {
                    Ok(msg.get("value").cloned().unwrap_or(Value::Null))
                } else {
                    Err(msg["error"].as_str().unwrap_or("host call failed").into())
                });
            }
        }
        _ => {} // forward compatibility
    }
}

/// Sends `done` for an invoke when dropped: its handler has returned.
struct Done(Runtime, Value);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.send(json!({"type": "done", "id": self.1}));
    }
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(String::from)
}

fn obj_or_empty(v: &Value) -> Value {
    if v.is_object() {
        v.clone()
    } else {
        json!({})
    }
}

fn handle_sync(core: &Arc<Core>, msg: &Value) {
    let rt = &core.rt;
    let fg_process = opt_str(&msg["fg_process"]);
    let fg_title = opt_str(&msg["fg_title"]);
    let instances = msg["instances"].as_array().cloned().unwrap_or_default();
    log(format!("sync: {} instance(s)", instances.len()));

    // `sync` is the full set: forget instances that were removed.
    let keep: std::collections::HashSet<&str> = instances.iter().filter_map(|i| i["instance_id"].as_str()).collect();
    rt.0.action_of.lock().unwrap().retain(|iid, _| keep.contains(iid.as_str()));
    rt.0.inputs_of.lock().unwrap().retain(|iid, _| keep.contains(iid.as_str()));
    rt.0.state.lock().unwrap().retain(|iid, _| keep.contains(iid.as_str()));

    for inst in &instances {
        let Some(iid) = inst["instance_id"].as_str() else { continue };
        let action_id = core.short(inst["action_id"].as_str().unwrap_or(""));
        let inputs = obj_or_empty(&inst["inputs"]);
        rt.0.action_of.lock().unwrap().insert(iid.into(), action_id.clone());
        rt.0.inputs_of.lock().unwrap().insert(iid.into(), inputs.clone());
        let def = core.action(&action_id);
        // Volatile actions re-query initial() on every sync (per-window state).
        let volatile = def.map_or(false, |d| d.volatile);
        let known = rt.0.state.lock().unwrap().contains_key(iid);
        if !known || volatile {
            let ic = InitialCtx {
                instance_id: iid.into(),
                action_id,
                inputs,
                fg_process: fg_process.clone(),
                fg_title: fg_title.clone(),
            };
            let s = compute_initial(def, &ic);
            rt.0.state.lock().unwrap().insert(iid.into(), s);
        }
        let s = rt.0.state.lock().unwrap().get(iid).cloned().unwrap_or_default();
        rt.send(json!({"type": "state", "instance_id": iid, "state": s}));
    }

    // on_ready fires once after the first sync: all instances are known.
    if let Some(f) = &core.plugin.on_ready {
        if !core.plugin.ready_fired.swap(true, Ordering::SeqCst) {
            let (f, rt) = (f.clone(), rt.clone());
            spawn("on_ready", move || {
                if let Err(e) = f(&rt) {
                    log(format!("on_ready error: {e}"));
                }
            });
        }
    }
}

fn handle_event(core: &Arc<Core>, msg: Value) {
    let name = msg["event"].as_str().unwrap_or("").to_string();
    let payload = obj_or_empty(&msg["payload"]);
    let fg_process = opt_str(&payload["fg_process"]);
    let fg_title = opt_str(&payload["fg_title"]);

    if name == "focus_changed" {
        let snapshot = core.rt.0.action_of.lock().unwrap().clone();
        for (iid, aid) in snapshot {
            let Some(def) = core.action(&aid).filter(|d| d.volatile) else { continue };
            let inputs = core.rt.0.inputs_of.lock().unwrap().get(&iid).cloned().unwrap_or(json!({}));
            let ic = InitialCtx {
                instance_id: iid.clone(),
                action_id: aid.clone(),
                inputs,
                fg_process: fg_process.clone(),
                fg_title: fg_title.clone(),
            };
            let next = compute_initial(Some(def), &ic);
            if core.rt.0.state.lock().unwrap().get(&iid) == Some(&next) {
                continue;
            }
            core.rt.send(json!({"type": "state", "instance_id": iid, "state": next}));
        }
    }

    if let Some(f) = core.plugin.on_event.clone() {
        let rt = core.rt.clone();
        spawn("on_event", move || {
            let evt = Event { name: name.clone(), payload, fg_process, fg_title };
            if let Err(e) = f(&evt, &rt) {
                log(format!("on_event error ({name}): {e}"));
            }
        });
    }
}

fn report(core: &Core, what: &str, instance_id: &str, r: Result) {
    if let Err(Error::Other(m)) = r {
        log(format!("{what}: {m}"));
        core.rt.set_error(instance_id, m);
    }
}

fn handle_invoke(core: &Arc<Core>, msg: Value) {
    let rt = &core.rt;
    let instance_id = msg["instance_id"].as_str().unwrap_or("").to_string();
    let full = msg["action_id"].as_str().unwrap_or("").to_string();
    let action_id = core.short(&full);
    let trigger = msg["trigger"].as_str().unwrap_or("press").to_string();
    let inputs = obj_or_empty(&msg["inputs"]);
    rt.0.action_of.lock().unwrap().insert(instance_id.clone(), action_id.clone());

    let Some(def) = core.action(&action_id) else {
        log(format!("unknown action: {action_id}"));
        return;
    };

    let child_id = match &msg["child_id"] {
        Value::Null => None,
        v => Some(value_str(v)),
    };
    let is_adjust = def.kind == Kind::Slider && trigger == "adjust";
    let ctx = Ctx {
        instance_id: instance_id.clone(),
        action_id: action_id.clone(),
        full_action_id: full,
        trigger: trigger.clone(),
        state: rt.0.state.lock().unwrap().get(&instance_id).cloned(),
        fg_process: opt_str(&msg["fg_process"]),
        fg_title: opt_str(&msg["fg_title"]),
        child_id: child_id.clone(),
        value: if is_adjust { inputs.get("value").and_then(num) } else { None },
        delta: if is_adjust { inputs.get("delta").and_then(num) } else { None },
        inputs,
        rt: rt.clone(),
    };

    if child_id.is_some() {
        match &def.on_child_press {
            Some(h) => report(core, &format!("wheel child ({action_id})"), &instance_id, h(&ctx)),
            None => log(format!("wheel action \"{action_id}\" has no on_child_press handler")),
        }
        return;
    }
    let handler = if is_adjust && def.on_adjust.is_some() {
        &def.on_adjust
    } else if trigger == "hold_start" && def.on_hold_start.is_some() {
        &def.on_hold_start
    } else if trigger == "hold_end" && def.on_hold_end.is_some() {
        &def.on_hold_end
    } else {
        &def.on_press
    };
    if let Some(h) = handler {
        report(core, &format!("handler ({action_id})"), &instance_id, h(&ctx));
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn reply(core: &Core, id: &Value, outcome: Result<Value>, what: &str) {
    core.rt.send(match outcome {
        Ok(v) => json!({"type": "result", "id": id, "ok": true, "value": v}),
        Err(e) => {
            if let Error::Other(m) = &e {
                log(format!("{what} error: {m}"));
            }
            json!({"type": "result", "id": id, "ok": false, "error": e.message()})
        }
    });
}

fn handle_query(core: &Arc<Core>, msg: &Value) {
    let p = &msg["params"];
    let qid = p["query_id"].as_str().unwrap_or("");
    let outcome = match core.plugin.queries.get(qid) {
        None => Err(Error::msg(format!("unknown query \"{qid}\""))),
        Some(provider) => provider(&QueryCtx {
            inputs: obj_or_empty(&p["inputs"]),
            fg_process: opt_str(&p["fg_process"]),
            fg_title: opt_str(&p["fg_title"]),
        }),
    };
    reply(core, &msg["id"], outcome, &format!("query ({qid})"));
}

fn handle_wheel(core: &Arc<Core>, msg: &Value) {
    let p = &msg["params"];
    let action_id = core.short(p["action_id"].as_str().unwrap_or(""));
    let Some(handler) = core.action(&action_id).and_then(|d| d.on_open.as_ref()) else {
        reply(core, &msg["id"], Err(Error::msg(format!("unknown wheel action \"{action_id}\""))), "wheel");
        return;
    };
    if let Some(iid) = p["instance_id"].as_str() {
        core.rt.0.action_of.lock().unwrap().insert(iid.into(), action_id.clone());
    }
    let outcome = handler(&OpenCtx {
        inputs: obj_or_empty(&p["inputs"]),
        fg_process: opt_str(&p["fg_process"]),
        fg_title: opt_str(&p["fg_title"]),
    })
    .map(|mut slices| {
        if slices.len() > MAX_WHEEL_BUTTONS {
            log(format!(
                "on_open() for wheel \"{action_id}\" returned {} slices — truncated to {MAX_WHEEL_BUTTONS}",
                slices.len()
            ));
            slices.truncate(MAX_WHEEL_BUTTONS);
        }
        json!({"mode": "buttons", "slices": slices})
    });
    reply(core, &msg["id"], outcome, &format!("on_open ({action_id})"));
}

#[cfg(test)]
mod match_tests {
    use super::*;

    #[test]
    fn inputs_match_compares_as_strings_and_treats_missing_as_empty() {
        let f = |v: Value| v.as_object().unwrap().clone();
        let inputs = json!({"device": "a", "index": 2, "step": "5"});
        assert!(inputs_match(&inputs, &f(json!({"device": "a"}))));
        assert!(inputs_match(&inputs, &f(json!({"index": "2"}))));
        assert!(!inputs_match(&inputs, &f(json!({"device": "b"}))));
        assert!(inputs_match(&inputs, &f(json!({"missing": null}))));
        assert!(inputs_match(&inputs, &f(json!({"missing": ""}))));
        assert!(!inputs_match(&inputs, &f(json!({"missing": "x"}))));
        assert!(inputs_match(&inputs, &f(json!({}))));
    }
}
