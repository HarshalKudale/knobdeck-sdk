//! Companions: an add-on inside another app (a Photoshop panel, a Blender add-on…) that dials
//! KnobDeck's companion hub and is relayed to this plugin. The plugin never opens a port.
//!
//! ```no_run
//! use knobdeck_sdk::{companion, json, Action, Plugin};
//!
//! Plugin::new("photoshop", "Photoshop")
//!     .companions() // accept companions whose `hello.plugin` is "photoshop"
//!     .on_event(|ev, rt| {
//!         match companion::parse(ev) {
//!             Some(companion::Update::Connected(link)) => rt.log(format!("{} connected", link.app)),
//!             Some(companion::Update::Event(e)) => rt.log(format!("{}: {}", e.event, e.payload)),
//!             _ => {}
//!         }
//!         Ok(())
//!     })
//!     .action(Action::button("brush", "Brush").on_press(|_| {
//!         let answer = companion::request(None, "set-tool", json!({"tool": "brush"}))?;
//!         companion::check(answer).map(|_| ())
//!     }))
//!     .run()
//! ```
//!
//! Needs a KnobDeck with the `companion_hub` feature; on an older one [`request`] and [`links`]
//! return an error and no companion connects.

use crate::ctx::Event;
use crate::error::{Error, Result};
use crate::runtime::global;
use knobdeck_protocol::{events, features, host_calls};
use serde_json::Value;

pub use knobdeck_protocol::companion::{ActionResult, Link, LinkEvent, LinkGone, LinkRequest};

/// The events a plugin subscribes to to accept companions ([`crate::Plugin::companions`]).
pub const EVENTS: &[&str] = &[events::COMPANION_CONNECTED, events::COMPANION_DISCONNECTED, events::COMPANION_EVENT];

/// A companion update, from a host event.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Connected(Link),
    Disconnected(LinkGone),
    Event(LinkEvent),
}

/// The companion update an event carries, or `None` for any other event.
pub fn parse(ev: &Event) -> Option<Update> {
    let p = ev.payload.clone();
    match ev.name.as_str() {
        events::COMPANION_CONNECTED => serde_json::from_value(p).ok().map(Update::Connected),
        events::COMPANION_DISCONNECTED => serde_json::from_value(p).ok().map(Update::Disconnected),
        events::COMPANION_EVENT => serde_json::from_value(p).ok().map(Update::Event),
        _ => None,
    }
}

fn hub() -> Result<&'static crate::Runtime> {
    let rt = global().ok_or_else(|| Error::msg("KnobDeck host bridge is not connected"))?;
    if !rt.has_feature(features::COMPANION_HUB) {
        return Err(Error::msg("this KnobDeck has no companion hub; update KnobDeck"));
    }
    Ok(rt)
}

/// Run `action` in a companion (`link`, or the most recently connected one when `None`) and wait
/// up to 5 s. The answer is the companion's own [`ActionResult`]: check `ok`, or use [`check`].
/// Errors when no companion is connected or it didn't answer in time.
pub fn request(link: Option<&str>, action: &str, params: Value) -> Result<ActionResult> {
    request_with_timeout(link, action, params, None)
}

/// [`request`] with a timeout in milliseconds (the host caps it at 30 000).
pub fn request_with_timeout(link: Option<&str>, action: &str, params: Value, timeout_ms: Option<u64>) -> Result<ActionResult> {
    let rt = hub()?;
    let req = LinkRequest { link: link.map(str::to_string), action: action.to_string(), params, timeout_ms };
    let v = rt.host_call(host_calls::COMPANION_REQUEST, serde_json::to_value(req).expect("serializes"))?;
    serde_json::from_value(v).map_err(|e| Error::msg(format!("companion_request: unexpected answer ({e})")))
}

/// The companion's result when it succeeded; its message as the error otherwise ("unsupported"
/// when the app has no API for the action).
pub fn check(answer: ActionResult) -> Result<Value> {
    if answer.ok {
        Ok(answer.result.unwrap_or(Value::Null))
    } else if answer.unsupported {
        Err(Error::msg("the app can't do this"))
    } else {
        Err(Error::msg(answer.error.unwrap_or_else(|| "the companion reported an error".into())))
    }
}

/// This plugin's connected companions.
pub fn links() -> Result<Vec<Link>> {
    let v = hub()?.host_call(host_calls::COMPANION_LINKS, serde_json::json!({}))?;
    serde_json::from_value(v).map_err(|e| Error::msg(format!("companion_links: unexpected answer ({e})")))
}
