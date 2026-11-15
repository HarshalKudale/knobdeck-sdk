//! Linux clipboard and PRIMARY selection, read only.
//!
//! A Wayland client can't read the clipboard unless it has focus, so this asks the compositor:
//! - GNOME: the KnobDeck Shell extension (`Clipboard` method, St.Clipboard);
//! - KDE Plasma, Hyprland, sway, other wlroots desktops: `ext-data-control-v1`, else
//!   `wlr-data-control-unstable-v1`, which let a client read the current selection;
//! - X11 (and Wayland desktops with neither, through Xwayland): the X selection.

use crate::clipboard::{parse_uri_list, Image, Kind};
use crate::{Error, Result};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Largest payload read (images).
const MAX_BYTES: u64 = 64 << 20;
/// How long the app that owns the selection gets to send it.
const SEND_TIMEOUT: Duration = Duration::from_millis(1500);

const TEXT: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"];
const FILES: &[&str] = &["text/uri-list", "x-special/gnome-copied-files"];
const IMAGE: &[&str] = &["image/png", "image/*"];
const HTML: &[&str] = &["text/html"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sel {
    Clipboard,
    Primary,
}

/// The first of `prefs` that `mimes` offers (`image/*` matches any image type).
fn choose(mimes: &[String], prefs: &[&str]) -> Option<String> {
    prefs.iter().find_map(|p| match p.strip_suffix('*') {
        Some(prefix) => mimes.iter().find(|m| m.starts_with(prefix)).cloned(),
        None => mimes.iter().find(|m| m.eq_ignore_ascii_case(p)).cloned(),
    })
}

fn kinds(mimes: &[String]) -> Vec<Kind> {
    let mut k = vec![];
    if choose(mimes, FILES).is_some() {
        k.push(Kind::Files);
    }
    if choose(mimes, TEXT).is_some() {
        k.push(Kind::Text);
    }
    if choose(mimes, IMAGE).is_some() {
        k.push(Kind::Image);
    }
    if choose(mimes, HTML).is_some() {
        k.push(Kind::Html);
    }
    k
}

/// The selection's mime types, and the data of the first of `prefs` it offers (empty `prefs`:
/// types only).
fn read(sel: Sel, prefs: &[&str]) -> Result<(Vec<String>, Option<(String, Vec<u8>)>)> {
    if super::is_wayland() {
        if super::bus::session_has_owner("org.gnome.Shell") {
            return super::gnome::clipboard(sel == Sel::Primary, prefs);
        }
        if let Some(r) = data_control::read(sel, prefs)? {
            return Ok(r);
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        return x11::read(sel, prefs);
    }
    Err(Error::msg("Reading the clipboard needs GNOME, KDE Plasma, a wlroots desktop or X11"))
}

pub fn types() -> Result<Vec<Kind>> {
    Ok(kinds(&read(Sel::Clipboard, &[])?.0))
}

fn text_of(sel: Sel) -> Result<Option<String>> {
    Ok(read(sel, TEXT)?.1.map(|(_, b)| String::from_utf8_lossy(&b).into_owned()))
}

pub fn text() -> Result<Option<String>> {
    text_of(Sel::Clipboard)
}

/// The PRIMARY selection's text (the last text selected anywhere).
#[cfg_attr(not(feature = "selection"), allow(dead_code))]
pub(crate) fn primary_text() -> Result<Option<String>> {
    text_of(Sel::Primary)
}

pub fn files() -> Result<Vec<PathBuf>> {
    Ok(match read(Sel::Clipboard, FILES)?.1 {
        // GNOME's own list starts with a `copy` / `cut` line, then URIs.
        Some((_, b)) => parse_uri_list(&String::from_utf8_lossy(&b)),
        None => vec![],
    })
}

/// The files in the PRIMARY selection (a file manager's selected items).
#[cfg(feature = "files")]
pub(crate) fn primary_files() -> Result<Vec<PathBuf>> {
    Ok(match read(Sel::Primary, FILES)?.1 {
        Some((_, b)) => parse_uri_list(&String::from_utf8_lossy(&b)),
        None => vec![],
    })
}

pub fn image() -> Result<Option<Image>> {
    Ok(read(Sel::Clipboard, IMAGE)?.1.map(|(mime, bytes)| Image { mime, bytes }))
}

/// Read a pipe to its end, giving the sending app [`SEND_TIMEOUT`].
fn read_all(r: impl Read + Send + 'static) -> Result<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![];
        let res = r.take(MAX_BYTES).read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(res);
    });
    rx.recv_timeout(SEND_TIMEOUT).map_err(|_| Error::msg("The app holding the clipboard didn't send it"))?.map_err(Error::from)
}

// ── Wayland data-control ───────────────────────────────────────────────────

mod data_control {
    use super::{read_all, Sel};
    use crate::{Error, Result};

    type Read = Option<(Vec<String>, Option<(String, Vec<u8>)>)>;

    /// `None` when the compositor offers neither protocol.
    pub fn read(sel: Sel, prefs: &[&str]) -> Result<Read> {
        if let Some(r) = ext::read(sel, prefs)? {
            return Ok(Some(r));
        }
        wlr::read(sel, prefs)
    }

    fn err(e: impl std::fmt::Display) -> Error {
        Error::msg(format!("Wayland clipboard: {e}"))
    }

    // The two protocols are the same apart from their names.
    macro_rules! protocol {
        ($name:ident, $global:literal, $max_version:literal, $manager:path, $device_mod:path, $device_ty:ident, $offer_mod:path, $offer_ty:ident) => {
            mod $name {
                use super::{err, read_all, Read, Sel};
                use crate::Result;
                use std::collections::HashMap;
                use std::os::fd::AsFd;
                use wayland_client::backend::ObjectId;
                use wayland_client::protocol::{wl_registry, wl_seat};
                use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
                use $device_mod as device;
                use $offer_mod as offer;
                use $manager as Manager;
                use device::$device_ty as Device;
                use offer::$offer_ty as Offer;

                #[derive(Default)]
                struct State {
                    seat: Option<wl_seat::WlSeat>,
                    manager: Option<Manager>,
                    mimes: HashMap<ObjectId, Vec<String>>,
                    selection: Option<Offer>,
                    primary: Option<Offer>,
                }

                impl Dispatch<wl_registry::WlRegistry, ()> for State {
                    fn event(st: &mut Self, reg: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
                        if let wl_registry::Event::Global { name, interface, version } = ev {
                            if interface == "wl_seat" && st.seat.is_none() {
                                st.seat = Some(reg.bind(name, version.min(1), qh, ()));
                            } else if interface == $global {
                                st.manager = Some(reg.bind(name, version.min($max_version), qh, ()));
                            }
                        }
                    }
                }

                impl Dispatch<wl_seat::WlSeat, ()> for State {
                    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
                }

                impl Dispatch<Manager, ()> for State {
                    fn event(_: &mut Self, _: &Manager, _: <Manager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
                }

                impl Dispatch<Device, ()> for State {
                    fn event(st: &mut Self, _: &Device, ev: device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                        match ev {
                            device::Event::DataOffer { id } => {
                                st.mimes.insert(id.id(), vec![]);
                            }
                            device::Event::Selection { id } => st.selection = id,
                            device::Event::PrimarySelection { id } => st.primary = id,
                            _ => {}
                        }
                    }

                    event_created_child!(State, Device, [device::EVT_DATA_OFFER_OPCODE => (Offer, ())]);
                }

                impl Dispatch<Offer, ()> for State {
                    fn event(st: &mut Self, o: &Offer, ev: offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                        if let offer::Event::Offer { mime_type } = ev {
                            st.mimes.entry(o.id()).or_default().push(mime_type);
                        }
                    }
                }

                pub fn read(sel: Sel, prefs: &[&str]) -> Result<Read> {
                    let Ok(conn) = Connection::connect_to_env() else { return Ok(None) };
                    let mut queue = conn.new_event_queue();
                    let qh = queue.handle();
                    let _registry = conn.display().get_registry(&qh, ());
                    let mut st = State::default();
                    queue.roundtrip(&mut st).map_err(err)?;
                    let (Some(seat), Some(manager)) = (st.seat.clone(), st.manager.clone()) else { return Ok(None) };
                    let dev = manager.get_data_device(&seat, &qh, ());
                    queue.roundtrip(&mut st).map_err(err)?;
                    let offer = if sel == Sel::Primary { st.primary.clone() } else { st.selection.clone() };
                    let Some(offer) = offer else {
                        dev.destroy();
                        return Ok(Some((vec![], None)));
                    };
                    let mimes = st.mimes.get(&offer.id()).cloned().unwrap_or_default();
                    let data = match super::super::choose(&mimes, prefs) {
                        Some(mime) => {
                            let (r, w) = std::io::pipe()?;
                            offer.receive(mime.clone(), w.as_fd());
                            conn.flush().map_err(err)?;
                            drop(w);
                            Some((mime, read_all(r)?))
                        }
                        None => None,
                    };
                    offer.destroy();
                    dev.destroy();
                    let _ = conn.flush();
                    Ok(Some((mimes, data)))
                }
            }
        };
    }

    protocol!(
        ext,
        "ext_data_control_manager_v1",
        1,
        wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1,
        wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1,
        ExtDataControlDeviceV1,
        wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1,
        ExtDataControlOfferV1
    );

    protocol!(
        wlr,
        "zwlr_data_control_manager_v1",
        2,
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1,
        ZwlrDataControlDeviceV1,
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1,
        ZwlrDataControlOfferV1
    );
}

// ── X11 selection ──────────────────────────────────────────────────────────

mod x11 {
    use super::{Sel, SEND_TIMEOUT};
    use crate::{Error, Result};
    use std::time::Instant;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, CreateWindowAux, EventMask, Property, WindowClass};
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;
    use x11rb::{CURRENT_TIME, NONE};

    fn err(e: impl std::fmt::Display) -> Error {
        Error::msg(format!("X11 clipboard: {e}"))
    }

    fn atom(c: &RustConnection, name: &str) -> Result<u32> {
        Ok(c.intern_atom(false, name.as_bytes()).map_err(err)?.reply().map_err(err)?.atom)
    }

    /// Wait for an event matching `want`, up to the send timeout.
    fn wait(c: &RustConnection, deadline: Instant, want: impl Fn(&Event) -> bool) -> Result<Event> {
        loop {
            if let Some(ev) = c.poll_for_event().map_err(err)? {
                if want(&ev) {
                    return Ok(ev);
                }
                continue;
            }
            if Instant::now() > deadline {
                return Err(Error::msg("The app holding the clipboard didn't send it"));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Convert `sel` to `target` into `prop` on `win`; `None` when the owner refuses.
    fn convert(c: &RustConnection, win: u32, sel: u32, target: u32, prop: u32) -> Result<Option<(u32, Vec<u8>)>> {
        c.convert_selection(win, sel, target, prop, CURRENT_TIME).map_err(err)?;
        c.flush().map_err(err)?;
        let deadline = Instant::now() + SEND_TIMEOUT;
        let ev = wait(c, deadline, |e| matches!(e, Event::SelectionNotify(n) if n.requestor == win))?;
        let Event::SelectionNotify(n) = ev else { unreachable!() };
        if n.property == NONE {
            return Ok(None);
        }
        let r = c.get_property(true, win, prop, AtomEnum::ANY, 0, u32::MAX / 4).map_err(err)?.reply().map_err(err)?;
        let incr = atom(c, "INCR")?;
        if r.type_ != incr {
            return Ok(Some((r.type_, r.value)));
        }
        // INCR: the owner sends the data in chunks, each announced by a new property value.
        let mut data = vec![];
        let mut ty = 0;
        loop {
            wait(c, Instant::now() + SEND_TIMEOUT, |e| matches!(e, Event::PropertyNotify(p) if p.window == win && p.atom == prop && p.state == Property::NEW_VALUE))?;
            let r = c.get_property(true, win, prop, AtomEnum::ANY, 0, u32::MAX / 4).map_err(err)?.reply().map_err(err)?;
            if r.value.is_empty() {
                return Ok(Some((ty, data)));
            }
            ty = r.type_;
            data.extend_from_slice(&r.value);
            if data.len() as u64 > super::MAX_BYTES {
                return Err(Error::msg("The clipboard holds too much data"));
            }
        }
    }

    pub fn read(sel: Sel, prefs: &[&str]) -> Result<(Vec<String>, Option<(String, Vec<u8>)>)> {
        let (c, screen) = x11rb::connect(None).map_err(err)?;
        let root = c.setup().roots[screen].root;
        let win = c.generate_id().map_err(err)?;
        c.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE)).map_err(err)?;
        let selection = atom(&c, if sel == Sel::Primary { "PRIMARY" } else { "CLIPBOARD" })?;
        let prop = atom(&c, "KNOBDECK_SELECTION")?;
        let targets = atom(&c, "TARGETS")?;
        let result = (|| {
            let mut mimes = vec![];
            if let Some((_, bytes)) = convert(&c, win, selection, targets, prop)? {
                for id in bytes.chunks_exact(4).map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]])) {
                    if let Ok(name) = c.get_atom_name(id).map_err(err).and_then(|r| r.reply().map_err(err)) {
                        mimes.push(String::from_utf8_lossy(&name.name).into_owned());
                    }
                }
            }
            let data = match super::choose(&mimes, prefs) {
                Some(mime) => convert(&c, win, selection, atom(&c, &mime)?, prop)?.map(|(_, b)| (mime, b)),
                None => None,
            };
            Ok((mimes, data))
        })();
        let _ = c.destroy_window(win);
        let _ = c.flush();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_by_preference() {
        let m: Vec<String> = ["TARGETS", "image/jpeg", "text/plain", "UTF8_STRING"].iter().map(|s| s.to_string()).collect();
        assert_eq!(choose(&m, TEXT).as_deref(), Some("UTF8_STRING"));
        assert_eq!(choose(&m, IMAGE).as_deref(), Some("image/jpeg"));
        assert_eq!(choose(&m, FILES), None);
        assert_eq!(kinds(&m), vec![Kind::Text, Kind::Image]);
    }

    /// Reads whatever is on this session's clipboard; prints it.
    #[test]
    #[ignore]
    fn live_read() {
        eprintln!("types: {:?}", types());
        eprintln!("text: {:?}", text().map(|t| t.map(|t| t.chars().take(80).collect::<String>())));
        eprintln!("files: {:?}", files());
        eprintln!("image: {:?}", image().map(|i| i.map(|i| (i.mime, i.bytes.len()))));
        eprintln!("primary: {:?}", primary_text().map(|t| t.map(|t| t.chars().take(80).collect::<String>())));
    }
}
