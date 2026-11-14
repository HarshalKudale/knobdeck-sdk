//! `Plugin` + `Action` builders — the entry point of the SDK.
//!
//! ```no_run
//! use knobdeck_sdk::{Action, Plugin};
//!
//! fn main() {
//!     Plugin::new("demo", "Demo")
//!         .action(Action::button("ping", "Ping").on_press(|ctx| {
//!             ctx.log("pong");
//!             Ok(())
//!         }))
//!         .run();
//! }
//! ```

use crate::codegen::{generate_actions, generate_manifest, Stage};
use crate::ctx::{Ctx, Event, InitialCtx, OpenCtx, QueryCtx};
use crate::error::Result;
use crate::runtime::{Runtime, Visual};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub type Handler = Arc<dyn Fn(&Ctx) -> Result + Send + Sync>;
pub(crate) type OpenFn = Arc<dyn Fn(&OpenCtx) -> Result<Vec<Value>> + Send + Sync>;
pub(crate) type QueryFn = Arc<dyn Fn(&QueryCtx) -> Result<Value> + Send + Sync>;
pub(crate) type ReadyFn = Arc<dyn Fn(&Runtime) -> Result + Send + Sync>;
pub(crate) type EventFn = Arc<dyn Fn(&Event, &Runtime) -> Result + Send + Sync>;
pub(crate) type LiveFn = Arc<dyn Fn(&InitialCtx) -> Result<Visual> + Send + Sync>;
pub(crate) type ShutdownFn = Arc<dyn Fn(&Runtime) + Send + Sync>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Button,
    Toggle,
    Hold,
    Slider,
    Wheel,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Button => "button",
            Kind::Toggle => "toggle",
            Kind::Hold => "hold",
            Kind::Slider => "slider",
            Kind::Wheel => "wheel",
        }
    }
}

/// Initial state of a placed instance.
pub(crate) enum Initial {
    Static(String),
    Dynamic(Arc<dyn Fn(&InitialCtx) -> Option<String> + Send + Sync>),
}

impl Initial {
    pub(crate) fn resolve(&self, ic: &InitialCtx) -> Option<String> {
        match self {
            Initial::Static(s) => Some(s.clone()),
            Initial::Dynamic(f) => f(ic),
        }
    }
}

/// One action definition. Build with `Action::button/toggle/hold/slider/wheel`.
pub struct Action {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: Kind,
    pub(crate) icon: Option<String>,
    pub(crate) inputs: Vec<Value>,
    pub(crate) states: Vec<Value>,
    pub(crate) released: Option<Value>,
    pub(crate) held: Option<Value>,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) step: Option<f64>,
    pub(crate) unit: Option<String>,
    pub(crate) volatile: bool,
    pub(crate) initial: Option<Initial>,
    pub(crate) on_press: Option<Handler>,
    pub(crate) on_hold_start: Option<Handler>,
    pub(crate) on_hold_end: Option<Handler>,
    pub(crate) on_adjust: Option<Handler>,
    pub(crate) on_child_press: Option<Handler>,
    pub(crate) on_open: Option<OpenFn>,
    pub(crate) live: Option<(Duration, LiveFn)>,
    pub(crate) platforms: Vec<String>,
}

fn handler(f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Option<Handler> {
    Some(Arc::new(f))
}

impl Action {
    fn new(id: &str, label: &str, kind: Kind) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            kind,
            icon: None,
            inputs: vec![],
            states: vec![],
            released: None,
            held: None,
            min: 0.0,
            max: 0.0,
            step: None,
            unit: None,
            volatile: false,
            initial: None,
            on_press: None,
            on_hold_start: None,
            on_hold_end: None,
            on_adjust: None,
            on_child_press: None,
            on_open: None,
            live: None,
            platforms: vec![],
        }
    }

    pub fn button(id: &str, label: &str) -> Self {
        Self::new(id, label, Kind::Button)
    }
    /// A toggle: declare `states` (first is the default initial state).
    pub fn toggle(id: &str, label: &str) -> Self {
        Self::new(id, label, Kind::Toggle)
    }
    /// Push-to-hold: declare `released` / `held` visuals and `on_hold_start` / `on_hold_end`.
    pub fn hold(id: &str, label: &str) -> Self {
        Self::new(id, label, Kind::Hold)
    }
    /// A continuous control (volume, brightness…); the value is the instance state.
    pub fn slider(id: &str, label: &str, min: f64, max: f64) -> Self {
        let mut a = Self::new(id, label, Kind::Slider);
        a.min = min;
        a.max = max;
        a
    }
    /// A dynamic contextual wheel of up to 16 buttons, built by `on_open`.
    pub fn wheel(id: &str, label: &str) -> Self {
        Self::new(id, label, Kind::Wheel)
    }

    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.into());
        self
    }
    /// Config inputs; use the helpers in [`crate::input`] or raw `json!` objects.
    pub fn inputs(mut self, inputs: Vec<Value>) -> Self {
        self.inputs = inputs;
        self
    }
    /// Add a toggle state `{ value, label, icon }`.
    pub fn state(mut self, value: &str, label: &str, icon: &str) -> Self {
        self.states.push(json!({"value": value, "label": label, "icon": icon}));
        self
    }
    pub fn released(mut self, label: &str, icon: &str) -> Self {
        self.released = Some(json!({"label": label, "icon": icon}));
        self
    }
    pub fn held(mut self, label: &str, icon: &str) -> Self {
        self.held = Some(json!({"label": label, "icon": icon}));
        self
    }
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }
    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.into());
        self
    }
    /// Per-window state: re-run `initial` on every sync and, with the `os-window` feature, whenever
    /// the target window changes (another window comes to the front, or its title or
    /// maximize / minimize / full screen / topmost state changes). The watcher runs only while
    /// an instance of a volatile action is placed. Read the window with `os::window::target()`.
    pub fn volatile(mut self) -> Self {
        self.volatile = true;
        self
    }
    pub fn initial(mut self, state: impl ToString) -> Self {
        self.initial = Some(Initial::Static(state.to_string()));
        self
    }
    pub fn initial_with(mut self, f: impl Fn(&InitialCtx) -> Option<String> + Send + Sync + 'static) -> Self {
        self.initial = Some(Initial::Dynamic(Arc::new(f)));
        self
    }

    /// Button / toggle press (also the fallback for hold & slider triggers without a dedicated handler).
    pub fn on_press(mut self, f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Self {
        self.on_press = handler(f);
        self
    }
    pub fn on_hold_start(mut self, f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Self {
        self.on_hold_start = handler(f);
        self
    }
    pub fn on_hold_end(mut self, f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Self {
        self.on_hold_end = handler(f);
        self
    }
    /// Slider drag/nudge: check `ctx.value` first, else `ctx.delta`.
    pub fn on_adjust(mut self, f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Self {
        self.on_adjust = handler(f);
        self
    }
    /// Wheel: build the slices (`{ id, label?, icon? }`, max 16) on every open.
    pub fn on_open(mut self, f: impl Fn(&OpenCtx) -> Result<Vec<Value>> + Send + Sync + 'static) -> Self {
        self.on_open = Some(Arc::new(f));
        self
    }
    /// Live content (Stream Deck-style stats, or state polled from a device): every `every`,
    /// `f` runs for each placed instance (a newly placed one within a second) and the returned
    /// state/label/icon are pushed when they change. Leave `state` unset for display-only
    /// content; set it to keep a toggle or slider in step with something changed elsewhere.
    /// While the plugin is not running, the tile shows its configured icon and label.
    pub fn live(mut self, every: Duration, f: impl Fn(&InitialCtx) -> Result<Visual> + Send + Sync + 'static) -> Self {
        self.live = Some((every, Arc::new(f)));
        self
    }
    /// Limit this action to some OSes (`"windows"`, `"macos"`, `"linux"`). The host hides it
    /// from pickers elsewhere; already-placed slices report a clear error. Default: everywhere.
    pub fn platforms(mut self, os: &[&str]) -> Self {
        self.platforms = os.iter().map(|s| s.to_string()).collect();
        self
    }
    /// Wheel: a button child was pressed (`ctx.child_id`).
    pub fn on_child_press(mut self, f: impl Fn(&Ctx) -> Result + Send + Sync + 'static) -> Self {
        self.on_child_press = handler(f);
        self
    }
}

#[derive(Default, Clone)]
pub(crate) struct Meta {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub ack_timeout_ms: Option<u64>,
    pub query_timeout_ms: Option<u64>,
    /// Cargo bin name of the setup executable (see [`crate::run_setup`]).
    pub setup_bin: Option<String>,
    pub setup_page: Option<String>,
    pub setup_assets_dir: Option<String>,
    pub setup_inputs: Vec<Value>,
    /// OSes the plugin runs on (`"windows"`, `"macos"`, `"linux"`); empty = everywhere.
    pub platforms: Vec<String>,
}

pub struct Plugin {
    pub(crate) meta: Meta,
    pub(crate) actions: HashMap<String, Action>,
    pub(crate) order: Vec<String>,
    pub(crate) queries: HashMap<String, QueryFn>,
    pub(crate) on_ready: Option<ReadyFn>,
    pub(crate) on_event: Option<EventFn>,
    pub(crate) on_shutdown: Option<ShutdownFn>,
    pub(crate) subscriptions: Vec<String>,
    pub(crate) ready_fired: AtomicBool,
}

impl Plugin {
    pub fn new(id: &str, name: &str) -> Self {
        assert!(!id.is_empty(), "KnobDeck SDK: Plugin::new() requires an id");
        assert!(!name.is_empty(), "KnobDeck SDK: Plugin::new() requires a name");
        Plugin {
            meta: Meta { id: id.into(), name: name.into(), ..Default::default() },
            actions: HashMap::new(),
            order: vec![],
            queries: HashMap::new(),
            on_ready: None,
            on_event: None,
            on_shutdown: None,
            subscriptions: vec![],
            ready_fired: AtomicBool::new(false),
        }
    }

    pub fn version(mut self, v: &str) -> Self {
        self.meta.version = Some(v.into());
        self
    }
    pub fn author(mut self, v: &str) -> Self {
        self.meta.author = Some(v.into());
        self
    }
    pub fn description(mut self, v: &str) -> Self {
        self.meta.description = Some(v.into());
        self
    }
    pub fn icon(mut self, v: &str) -> Self {
        self.meta.icon = Some(v.into());
        self
    }
    /// OSes this plugin runs on (`"windows"`, `"macos"`, `"linux"`). The host refuses to
    /// start it elsewhere and shows it as unavailable. Default: everywhere.
    pub fn platforms(mut self, os: &[&str]) -> Self {
        self.meta.platforms = os.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn ack_timeout_ms(mut self, v: u64) -> Self {
        self.meta.ack_timeout_ms = Some(v);
        self
    }
    pub fn query_timeout_ms(mut self, v: u64) -> Self {
        self.meta.query_timeout_ms = Some(v);
        self
    }
    /// Declare a setup program: the Cargo bin name that calls [`crate::run_setup`],
    /// plus (optionally) an HTML page the host shows for it.
    pub fn setup(mut self, bin: &str, page: Option<&str>) -> Self {
        self.meta.setup_bin = Some(bin.into());
        self.meta.setup_page = page.map(String::from);
        self
    }
    /// Folder of static assets (css, images…) copied next to the setup page.
    pub fn setup_assets_dir(mut self, dir: &str) -> Self {
        self.meta.setup_assets_dir = Some(dir.into());
        self
    }
    /// Fields the host collects before running setup (`{ key, label, type, … }`).
    pub fn setup_inputs(mut self, inputs: Vec<Value>) -> Self {
        self.meta.setup_inputs = inputs;
        self
    }

    /// Register an action. Panics on duplicate ids or an invalid definition
    /// (this is author-time configuration, mirroring the JS SDK's throws).
    pub fn action(mut self, a: Action) -> Self {
        assert!(!a.id.is_empty(), "KnobDeck SDK: action must have an id");
        assert!(!self.actions.contains_key(&a.id), "KnobDeck SDK: duplicate action id \"{}\"", a.id);
        match a.kind {
            Kind::Wheel => {
                assert!(a.on_open.is_some(), "KnobDeck SDK: wheel action \"{}\" needs on_open", a.id);
                assert!(
                    a.states.is_empty() && a.released.is_none() && a.held.is_none(),
                    "KnobDeck SDK: wheel action \"{}\" cannot declare states/released/held",
                    a.id
                );
            }
            Kind::Slider => {
                assert!(a.on_adjust.is_some(), "KnobDeck SDK: slider action \"{}\" needs on_adjust", a.id);
                assert!(a.min < a.max, "KnobDeck SDK: slider action \"{}\" must have min < max", a.id);
                assert!(
                    a.states.is_empty() && a.released.is_none() && a.held.is_none(),
                    "KnobDeck SDK: slider action \"{}\" cannot declare states/released/held",
                    a.id
                );
            }
            _ => {}
        }
        self.order.push(a.id.clone());
        self.actions.insert(a.id.clone(), a);
        self
    }

    /// Register a config-time query provider (feeds `select` inputs with live options).
    /// Return `[{ value, label }]` or bare strings.
    pub fn query(mut self, id: &str, f: impl Fn(&QueryCtx) -> Result<Value> + Send + Sync + 'static) -> Self {
        assert!(!id.is_empty(), "KnobDeck SDK: query must have an id");
        assert!(!self.queries.contains_key(id), "KnobDeck SDK: duplicate query id \"{id}\"");
        self.queries.insert(id.into(), Arc::new(f));
        self
    }

    /// Fired once (on its own thread) after the first sync, when all placed
    /// instances are known. Start pollers / listeners here; move the `Runtime` into them.
    pub fn on_ready(mut self, f: impl Fn(&Runtime) -> Result + Send + Sync + 'static) -> Self {
        self.on_ready = Some(Arc::new(f));
        self
    }

    /// Subscribe to host events by name (none are defined at the moment; the window watcher of
    /// [`Action::volatile`] replaced `focus_changed`).
    pub fn subscribe(mut self, events: &[&str]) -> Self {
        self.subscriptions.extend(events.iter().map(|s| s.to_string()));
        self
    }

    pub fn on_event(mut self, f: impl Fn(&Event, &Runtime) -> Result + Send + Sync + 'static) -> Self {
        self.on_event = Some(Arc::new(f));
        self
    }

    /// Runs once when the host stops the plugin (or KnobDeck exits cleanly):
    /// flush state, close connections. Keep it short — the host kills the
    /// process 2 s after asking it to stop.
    pub fn on_shutdown(mut self, f: impl Fn(&Runtime) + Send + Sync + 'static) -> Self {
        self.on_shutdown = Some(Arc::new(f));
        self
    }

    /// `{ manifest, actions }` as the CLI writes them.
    pub fn to_json(&self, stage: Stage) -> std::result::Result<(Value, Value), String> {
        Ok((generate_manifest(self, stage), generate_actions(self)?))
    }

    /// Start the plugin. Under `KNOBDECK_CODEGEN=1` it prints the generated
    /// `{manifest, actions}` JSON to stdout and exits instead (used by the CLI);
    /// otherwise it talks to the host over stdio and never returns.
    pub fn run(self) -> ! {
        if std::env::var("KNOBDECK_CODEGEN").as_deref() == Ok("1") {
            let stage = if std::env::var("KNOBDECK_STAGE").as_deref() == Ok("build") { Stage::Build } else { Stage::Dev };
            match self.to_json(stage) {
                Ok((manifest, actions)) => {
                    println!("{}", json!({"manifest": manifest, "actions": actions, "meta": {
                        "setup_page": self.meta.setup_page, "setup_assets_dir": self.meta.setup_assets_dir,
                    }}));
                    std::process::exit(0)
                }
                Err(e) => {
                    eprintln!("Codegen error: {e}");
                    std::process::exit(1)
                }
            }
        }
        crate::runtime::run(self)
    }
}
