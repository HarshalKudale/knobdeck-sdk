//! AT-SPI: the focused element and its selected text, over the accessibility bus.
//!
//! The target window's app is found among the registry's apps by process id; its focused
//! object comes from the `Collection` interface (one call answered by the app), or, for toolkits
//! without it, a bounded walk of the app's showing objects. GTK, Qt, Firefox and LibreOffice
//! register when the desktop's accessibility bus is up. Apps that don't (most Electron apps
//! unless a screen reader runs) fall back to the PRIMARY selection for the selected text.
//!
//! Qt apps (Dolphin, Kate, Konsole) publish their tree only while accessibility is switched on
//! for the session (`org.a11y.Status.IsEnabled`, off by default on Plasma). [`ensure_enabled`]
//! switches it on, as a screen reader does, and waits for the target app to register.
//!
//! [`dolphin_selection`] reads Dolphin's selected files: each view is a list whose description
//! ends with its folder's `file://` URL, and its selected items are the selected files.

use super::bus;
use crate::focus::Element;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
#[cfg_attr(not(feature = "selection"), allow(dead_code))]
const TEXT: &str = "org.a11y.atspi.Text";
const STATE_EDITABLE: u32 = 7;
const STATE_FOCUSED: u32 = 12;
#[cfg_attr(not(feature = "files"), allow(dead_code))]
const STATE_SELECTED: u32 = 23;
const STATE_SHOWING: u32 = 25;
const ROLE_PASSWORD_TEXT: u32 = 40;
/// Objects visited by the fallback walk, and its time budget.
const WALK_LIMIT: usize = 4000;
const WALK_BUDGET: Duration = Duration::from_millis(250);

type Obj = (String, OwnedObjectPath);

static CONN: Mutex<Option<Connection>> = Mutex::new(None);

/// The accessibility bus (its address comes from `org.a11y.Bus` on the session bus).
fn a11y() -> Result<Connection> {
    if let Some(c) = CONN.lock().unwrap().clone() {
        return Ok(c);
    }
    let session = bus::session()?;
    let addr: String = bus::call(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", &())
        .map_err(|_| Error::msg("The desktop's accessibility bus isn't running"))?;
    let conn = zbus::blocking::connection::Builder::address(addr.as_str())
        .and_then(|b| b.method_timeout(Duration::from_millis(800)).build())
        .map_err(bus::err)?;
    *CONN.lock().unwrap() = Some(conn.clone());
    Ok(conn)
}

fn call<A, R>(c: &Connection, o: &Obj, iface: &str, method: &str, args: &A) -> Result<R>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    bus::call(c, &o.0, o.1.as_str(), iface, method, args)
}

fn states(c: &Connection, o: &Obj) -> Result<u64> {
    let s: Vec<u32> = call(c, o, ACCESSIBLE, "GetState", &())?;
    Ok(s.first().copied().unwrap_or(0) as u64 | (s.get(1).copied().unwrap_or(0) as u64) << 32)
}

fn has(states: u64, bit: u32) -> bool {
    states & (1 << bit) != 0
}

/// Switch the session's accessibility on when it is off (Qt apps then publish their trees).
/// Returns whether it was off.
fn ensure_enabled() -> bool {
    let Ok(session) = bus::session() else { return false };
    let on: bool = bus::get(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled").unwrap_or(true);
    if on {
        return false;
    }
    bus::set(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled", true).is_ok()
}

/// [`app_of`], switching accessibility on first when the app isn't there; an app needs a moment
/// to register after that.
fn app_of_enabling(c: &Connection, pid: u32) -> Result<Obj> {
    if let Ok(app) = app_of(c, pid) {
        return Ok(app);
    }
    if !ensure_enabled() {
        return app_of(c, pid);
    }
    let deadline = Instant::now() + Duration::from_millis(2000);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        match app_of(c, pid) {
            Ok(app) => return Ok(app),
            Err(e) if Instant::now() > deadline => return Err(e),
            Err(_) => {}
        }
    }
}

fn pid_of(c: &Connection, bus_name: &str) -> Option<u32> {
    bus::call(c, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", &(bus_name,)).ok()
}

/// The app on the bus whose process is `pid`. The last answer is kept: a bus name is never
/// reused, so it stays right while that name still belongs to `pid` (one call instead of one
/// per app on the bus).
fn app_of(c: &Connection, pid: u32) -> Result<Obj> {
    static LAST: Mutex<Option<(u32, Obj)>> = Mutex::new(None);
    if let Some((p, app)) = LAST.lock().unwrap().clone() {
        if p == pid && pid_of(c, &app.0) == Some(pid) {
            return Ok(app);
        }
    }
    let root: Obj = ("org.a11y.atspi.Registry".into(), OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").unwrap());
    let apps: Vec<Obj> = call(c, &root, ACCESSIBLE, "GetChildren", &())?;
    for app in apps {
        if pid_of(c, &app.0) == Some(pid) {
            *LAST.lock().unwrap() = Some((pid, app.clone()));
            return Ok(app);
        }
    }
    Err(crate::not_exposed())
}

/// The app's focused object: `Collection.GetMatches` for STATE_FOCUSED, else a bounded walk.
fn focused(c: &Connection, app: &Obj) -> Option<Obj> {
    // MatchRule: states, state match, attributes, attr match, roles, role match, interfaces,
    // interface match, invert. MATCH_ALL = 1, MATCH_INVALID (ignored) = 0.
    let rule = (vec![1i32 << STATE_FOCUSED, 0i32], 1i32, HashMap::<String, String>::new(), 0i32, Vec::<i32>::new(), 0i32, Vec::<String>::new(), 0i32, false);
    let hits: Result<Vec<Obj>> = call(c, app, "org.a11y.atspi.Collection", "GetMatches", &(rule, 0u32, 1i32, true));
    if let Ok(Some(hit)) = hits.map(|h| h.into_iter().next()) {
        return Some(hit);
    }
    let deadline = Instant::now() + WALK_BUDGET;
    let mut stack: Vec<Obj> = call(c, app, ACCESSIBLE, "GetChildren", &()).unwrap_or_default();
    let mut seen = 0;
    while let Some(o) = stack.pop() {
        seen += 1;
        if seen > WALK_LIMIT || Instant::now() > deadline {
            return None;
        }
        let Ok(s) = states(c, &o) else { continue };
        if has(s, STATE_FOCUSED) {
            return Some(o);
        }
        if has(s, STATE_SHOWING) {
            stack.extend(call::<_, Vec<Obj>>(c, &o, ACCESSIBLE, "GetChildren", &()).unwrap_or_default());
        }
    }
    None
}

fn target_pid() -> Result<u32> {
    crate::window::target().map(|w| w.pid).filter(|p| *p != 0).ok_or_else(|| Error::msg("No window to read from"))
}

fn describe(c: &Connection, o: &Obj, pid: u32) -> Result<Element> {
    let s = states(c, o)?;
    let role: u32 = call(c, o, ACCESSIBLE, "GetRole", &()).unwrap_or(0);
    let name: String = call(c, o, ACCESSIBLE, "GetRoleName", &()).unwrap_or_default();
    Ok(Element { password: role == ROLE_PASSWORD_TEXT || name == "password text", editable: has(s, STATE_EDITABLE), role: name, pid })
}

pub fn element() -> Result<Option<Element>> {
    let pid = target_pid()?;
    let c = a11y()?;
    let app = app_of_enabling(&c, pid)?;
    match focused(&c, &app) {
        Some(o) => describe(&c, &o, pid).map(Some),
        None => Ok(None),
    }
}

#[cfg_attr(not(feature = "selection"), allow(dead_code))]
/// The focused object's selected text. `Err` only when the app isn't on the accessibility bus
/// or nothing in it has focus; those fall back to PRIMARY.
fn atspi_text() -> Result<Option<String>> {
    let pid = target_pid()?;
    let c = a11y()?;
    let app = app_of_enabling(&c, pid)?;
    let o = focused(&c, &app).ok_or_else(crate::not_exposed)?;
    let n: i32 = match call(&c, &o, TEXT, "GetNSelections", &()) {
        Ok(n) => n,
        Err(_) => return Ok(None), // no Text interface: a button, a list…
    };
    let mut out = String::new();
    for i in 0..n.max(0) {
        let (start, end): (i32, i32) = call(&c, &o, TEXT, "GetSelection", &(i,))?;
        if end > start {
            let t: String = call(&c, &o, TEXT, "GetText", &(start, end))?;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&t);
            if out.len() > crate::MAX_TEXT {
                break;
            }
        }
    }
    Ok(Some(out).filter(|s| !s.is_empty()))
}

#[cfg(feature = "selection")]
pub fn selected_text() -> Result<Option<String>> {
    match atspi_text() {
        Ok(t) => Ok(t),
        // Not on the bus: the PRIMARY selection holds the last text selected anywhere.
        Err(e) => super::clipboard::primary_text().map_err(|_| e),
    }
}

/// A Dolphin view: its folder and the items selected in it.
#[cfg(feature = "files")]
#[derive(Debug, Default)]
pub(crate) struct DolphinView {
    pub folder: Option<std::path::PathBuf>,
    pub selected: Vec<std::path::PathBuf>,
    pub focused: bool,
}

/// `… in location file:///home/me/My%20Pics` → `/home/me/My Pics` (the description is translated;
/// the URL is its last word).
#[cfg_attr(not(feature = "files"), allow(dead_code))]
fn folder_of_description(desc: &str) -> Option<std::path::PathBuf> {
    let at = desc.rfind("file://")?;
    let rest = desc[at + "file://".len()..].trim();
    let path = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    let p = crate::percent_decode(path);
    let p = if p.len() > 1 { p.trim_end_matches('/').to_string() } else { p };
    Some(std::path::PathBuf::from(p))
}

// ── Dolphin ──────────────────────────────────────────────────────────────────────────────────
//
// Reading a Dolphin window takes hundreds of AT-SPI calls (its tree has ~700 objects, and Qt's
// `Collection` finds nothing), each a round trip to Dolphin. So: the calls of one tree level go
// out together and are answered in one pass; leaves aren't asked for children; only the active
// window is walked; and the views found are remembered per window, so the next read checks
// those few objects instead of walking again.

#[cfg(feature = "files")]
mod dolphin {
    use super::*;
    use std::future::Future;
    use std::task::Poll;

    const STATE_ACTIVE: u32 = 1;
    /// How long a full walk of one window may take.
    const WALK_DEADLINE: Duration = Duration::from_millis(600);

    /// Roles never holding a folder view: the walk doesn't ask for their children. Item lists
    /// and tables (a folder's items, the Places panel) can be long; the rest are menus and leaves.
    const NOT_CONTAINERS: &[&str] = &[
        "list", "table", "tree", "tree table", "list box", "list item", "table cell", "menu", "menu bar", "popup menu",
        "menu item", "check menu item", "radio menu item", "separator", "push button", "toggle button", "radio button",
        "check box", "label", "text", "entry", "password text", "combo box", "button menu", "slider", "spin button",
        "progress bar", "scroll bar", "grip", "icon", "image", "tool tip", "page tab list", "tool bar", "status bar",
    ];

    /// Runs `futs` together; their outputs in order.
    async fn join_all<F: Future>(futs: impl IntoIterator<Item = F>) -> Vec<F::Output> {
        let mut futs: Vec<_> = futs.into_iter().map(Box::pin).collect();
        let mut out: Vec<Option<F::Output>> = futs.iter().map(|_| None).collect();
        std::future::poll_fn(|cx| {
            let mut pending = false;
            for (f, slot) in futs.iter_mut().zip(out.iter_mut()) {
                if slot.is_none() {
                    match f.as_mut().poll(cx) {
                        Poll::Ready(v) => *slot = Some(v),
                        Poll::Pending => pending = true,
                    }
                }
            }
            if pending { Poll::Pending } else { Poll::Ready(()) }
        })
        .await;
        out.into_iter().map(|v| v.expect("every future finished")).collect()
    }

    async fn acall<A, R>(c: &zbus::Connection, o: &Obj, iface: &str, method: &str, args: &A) -> Option<R>
    where
        A: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        let reply = c.call_method(Some(o.0.as_str()), o.1.as_str(), Some(iface), method, args).await.ok()?;
        reply.body().deserialize::<R>().ok()
    }

    /// An object's property through a plain `Properties.Get` (Qt's bridge answers that, but not
    /// the property proxy's `GetAll`).
    async fn aprop<T: TryFrom<zbus::zvariant::OwnedValue>>(c: &zbus::Connection, o: &Obj, iface: &str, name: &str) -> Option<T> {
        let v: zbus::zvariant::OwnedValue = acall(c, o, "org.freedesktop.DBus.Properties", "Get", &(iface, name)).await?;
        T::try_from(v).ok()
    }

    async fn astates(c: &zbus::Connection, o: &Obj) -> Option<u64> {
        let s: Vec<u32> = acall(c, o, ACCESSIBLE, "GetState", &()).await?;
        Some(s.first().copied().unwrap_or(0) as u64 | (s.get(1).copied().unwrap_or(0) as u64) << 32)
    }

    async fn children(c: &zbus::Connection, o: &Obj) -> Vec<Obj> {
        acall(c, o, ACCESSIBLE, "GetChildren", &()).await.unwrap_or_default()
    }

    /// What a walk of one window found: its folder views (lists described by a `file://` URL),
    /// each with the tab page it's on, and the tab bar and the stack of tab pages. Every tab's
    /// views are in the tree and read as showing, so only the tab bar tells which one is in front.
    #[derive(Clone, Default)]
    struct Found {
        tab_bar: Option<Obj>,
        /// The tab pages, in tab order (Qt keeps them in step when tabs are dragged).
        stack: Option<Obj>,
        /// (tab page, folder view).
        lists: Vec<(Option<Obj>, Obj)>,
    }

    /// Walks `frame` a level at a time (each level's calls go out together).
    async fn walk(c: &zbus::Connection, frame: &Obj) -> Found {
        let deadline = Instant::now() + WALK_DEADLINE;
        let mut found = Found::default();
        // (object, the tab page it's on)
        let mut level: Vec<(Obj, Option<Obj>)> = vec![(frame.clone(), None)];
        let mut seen = 0;
        while !level.is_empty() && seen < WALK_LIMIT && Instant::now() < deadline {
            seen += level.len();
            let roles: Vec<String> = join_all(level.iter().map(|(o, _)| async move { acall(c, o, ACCESSIBLE, "GetRoleName", &()).await.unwrap_or_default() })).await;
            let (mut lists, mut open) = (vec![], vec![]);
            for ((o, page), role) in level.into_iter().zip(&roles) {
                match role.as_str() {
                    "list" => lists.push((page, o)),
                    "page tab list" if found.tab_bar.is_none() => found.tab_bar = Some(o),
                    r if NOT_CONTAINERS.contains(&r) => {}
                    // The outermost layered pane is the stack of tab pages.
                    r => {
                        let stack = r == "layered pane" && found.stack.is_none();
                        if stack {
                            found.stack = Some(o.clone());
                        }
                        open.push((o, page, stack));
                    }
                }
            }
            let folders = join_all(lists.iter().map(|(_, l)| folder_of(c, l))).await;
            found.lists.extend(lists.into_iter().zip(folders).filter(|(_, f)| f.is_some()).map(|(l, _)| l));
            let kids = join_all(open.iter().map(|(o, _, _)| children(c, o))).await;
            level = open
                .into_iter()
                .zip(kids)
                .flat_map(|((_, page, stack), kids)| kids.into_iter().map(move |k| (k.clone(), if stack { Some(k) } else { page.clone() })))
                .collect();
        }
        found
    }

    /// The tab page in front: the stack's page at the selected tab's index. `None` when the window
    /// has no tab bar or none is selected.
    async fn front_page(c: &zbus::Connection, found: &Found) -> Option<Obj> {
        let (bar, stack) = (found.tab_bar.as_ref()?, found.stack.as_ref()?);
        let (tabs, pages) = futures_join(children(c, bar), children(c, stack)).await;
        let states = join_all(tabs.iter().map(|t| astates(c, t))).await;
        let i = states.iter().position(|s| s.is_some_and(|s| has(s, STATE_SELECTED)))?;
        pages.get(i).cloned()
    }

    /// The front tab's views now (every view when the front tab can't be told), and whether one
    /// of them is marked as the active view. `None` when the front tab has no known view or a view
    /// is gone: the window changed since `found` was walked.
    async fn read(c: &zbus::Connection, found: &Found, title: &str) -> Option<(Vec<(bool, DolphinView)>, bool)> {
        let page = front_page(c, found).await;
        let lists: Vec<&Obj> = found.lists.iter().filter(|(p, _)| page.is_none() || *p == page).map(|(_, l)| l).collect();
        if lists.is_empty() {
            return None;
        }
        let mut views: Vec<_> = join_all(lists.into_iter().map(|l| view(c, l))).await.into_iter().collect::<Option<_>>()?;
        let active = mark_active(&mut views, title);
        Some((views, active))
    }

    async fn folder_of(c: &zbus::Connection, list: &Obj) -> Option<std::path::PathBuf> {
        folder_of_description(&aprop::<String>(c, list, ACCESSIBLE, "Description").await?)
    }

    /// A view now; `None` when `list` is gone or no longer a folder view.
    async fn view(c: &zbus::Connection, list: &Obj) -> Option<(bool, DolphinView)> {
        let (folder, s) = futures_join(folder_of(c, list), astates(c, list)).await;
        let (folder, s) = (folder?, s?);
        let selected = selected_items(c, list, &folder).await;
        Some((has(s, STATE_SHOWING), DolphinView { selected, folder: Some(folder), focused: has(s, STATE_FOCUSED) }))
    }

    async fn futures_join<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
        let (mut a, mut b) = (Box::pin(a), Box::pin(b));
        let (mut ra, mut rb) = (None, None);
        std::future::poll_fn(|cx| {
            if ra.is_none() {
                if let Poll::Ready(v) = a.as_mut().poll(cx) {
                    ra = Some(v);
                }
            }
            if rb.is_none() {
                if let Poll::Ready(v) = b.as_mut().poll(cx) {
                    rb = Some(v);
                }
            }
            if ra.is_some() && rb.is_some() { Poll::Ready(()) } else { Poll::Pending }
        })
        .await;
        (ra.unwrap(), rb.unwrap())
    }

    /// A view's selected items as paths in `folder`: through the `Selection` interface, else each
    /// item's SELECTED state.
    async fn selected_items(c: &zbus::Connection, list: &Obj, folder: &std::path::Path) -> Vec<std::path::PathBuf> {
        const SELECTION: &str = "org.a11y.atspi.Selection";
        let items: Vec<Obj> = match aprop::<i32>(c, list, SELECTION, "NSelectedChildren").await {
            Some(n) if n >= 0 => {
                let n = n.min(crate::files::MAX_FILES as i32);
                join_all((0..n).map(|i| async move { acall::<_, Obj>(c, list, SELECTION, "GetSelectedChild", &(i,)).await })).await.into_iter().flatten().collect()
            }
            _ => {
                let all = children(c, list).await;
                let s = join_all(all.iter().map(|o| astates(c, o))).await;
                all.into_iter().zip(s).filter(|(_, s)| s.is_some_and(|s| has(s, STATE_SELECTED))).map(|(o, _)| o).collect()
            }
        };
        join_all(items.iter().map(|o| aprop::<String>(c, o, ACCESSIBLE, "Name")))
            .await
            .into_iter()
            .flatten()
            .filter(|n| !n.is_empty() && !n.contains('/'))
            .map(|n| folder.join(n))
            .filter(|p| p.symlink_metadata().is_ok())
            .collect()
    }

    /// What the last walk of each Dolphin window found (keyed by its frame object).
    static FOUND: Mutex<Vec<(Obj, Found)>> = Mutex::new(Vec::new());

    /// The views of `frame`'s front tab: from what the last walk found while that still holds and
    /// names the active view (focused, or named by the title); otherwise from a fresh walk.
    async fn frame_views(c: &zbus::Connection, frame: &Obj, title: &str) -> Vec<(bool, DolphinView)> {
        let known = FOUND.lock().unwrap().iter().find(|(f, _)| f == frame).map(|(_, found)| found.clone());
        if let Some(found) = known {
            if let Some((views, true)) = read(c, &found, title).await {
                return views;
            }
        }
        let found = walk(c, frame).await;
        let views = read(c, &found, title).await.map(|(v, _)| v).unwrap_or_default();
        let mut cache = FOUND.lock().unwrap();
        cache.retain(|(f, _)| f != frame);
        if !found.lists.is_empty() {
            // Closed windows are never told about; the oldest go past a few.
            if cache.len() >= 8 {
                cache.remove(0);
            }
            cache.push((frame.clone(), found));
        }
        views
    }

    /// Hidden views (a closed split, another tab) go when one is showing; when none has the keyboard
    /// focus, the one the window title names (its folder's full path or name) is marked focused.
    /// Whether a view is now marked focused.
    pub(super) fn mark_active(views: &mut Vec<(bool, DolphinView)>, title: &str) -> bool {
        if views.iter().any(|(showing, _)| *showing) {
            views.retain(|(showing, _)| *showing);
        }
        if views.iter().any(|(_, v)| v.focused) {
            return true;
        }
        let shown = title.rsplit_once(" — ").map_or(title, |(t, _)| t).trim();
        let full = views.iter().position(|(_, v)| v.folder.as_deref().is_some_and(|f| f.as_os_str() == shown));
        let named = || views.iter().position(|(_, v)| v.folder.as_deref().and_then(|f| f.file_name()).is_some_and(|n| n == shown));
        match full.or_else(named) {
            Some(i) => {
                views[i].1.focused = true;
                true
            }
            None => false,
        }
    }

    /// Dolphin's views in the process `pid`: those of its active window (all windows when none is
    /// active), with their selections.
    pub(crate) fn views(pid: u32) -> Result<Vec<DolphinView>> {
        let conn = a11y()?;
        let app = app_of_enabling(&conn, pid).map_err(|_| Error::msg("Dolphin isn't on the accessibility bus"))?;
        let c = conn.inner();
        zbus::block_on(async {
            let frames = children(c, &app).await;
            let info = join_all(frames.iter().map(|f| futures_join(astates(c, f), aprop::<String>(c, f, ACCESSIBLE, "Name")))).await;
            let any_active = info.iter().any(|(s, _)| s.is_some_and(|s| has(s, STATE_ACTIVE)));
            let chosen: Vec<(&Obj, String)> = frames
                .iter()
                .zip(info)
                .filter(|(_, (s, _))| !any_active || s.is_some_and(|s| has(s, STATE_ACTIVE)))
                .map(|(f, (_, name))| (f, name.unwrap_or_default()))
                .collect();
            // The title of the first chosen window names its active view.
            let title = chosen.first().map(|(_, t)| t.clone()).unwrap_or_default();
            let mut views: Vec<(bool, DolphinView)> = join_all(chosen.iter().map(|(f, t)| frame_views(c, f, t))).await.into_iter().flatten().collect();
            mark_active(&mut views, &title);
            let views: Vec<DolphinView> = views.into_iter().map(|(_, v)| v).collect();
            if views.is_empty() {
                return Err(Error::msg("Couldn't find Dolphin's folder view"));
            }
            Ok(views)
        })
    }
}

/// Dolphin's views (one, or two in split view, per tab) in the process `pid`, with their selections.
#[cfg(feature = "files")]
pub(crate) fn dolphin_views(pid: u32) -> Result<Vec<DolphinView>> {
    dolphin::views(pid)
}

#[cfg(test)]
mod tests {
    #[test]
    fn folder_from_dolphin_description() {
        assert_eq!(super::folder_of_description("in a grid layout in location file:///home/me/My%20Pics"), Some("/home/me/My Pics".into()));
        assert_eq!(super::folder_of_description("im Rasterlayout am Ort file:///home/me/"), Some("/home/me".into()));
        assert_eq!(super::folder_of_description("in location file:///"), Some("/".into()));
        assert_eq!(super::folder_of_description("Glob Pattern"), None);
    }

    #[cfg(feature = "files")]
    #[test]
    fn active_view_from_focus_or_title() {
        use super::DolphinView;
        let v = |folder: &str, focused| (true, DolphinView { folder: Some(folder.into()), selected: vec![], focused });
        // Focus wins.
        let mut views = vec![v("/a/Pics", false), v("/b", true)];
        assert!(super::dolphin::mark_active(&mut views, "Pics — Dolphin"));
        assert!(views[1].1.focused && !views[0].1.focused);
        // No focus (Dolphin isn't in front): the title's folder name, or its full path.
        let mut views = vec![v("/a/Apps", false), v("/a/Pics", false)];
        assert!(super::dolphin::mark_active(&mut views, "Pics — Dolphin"));
        assert!(views[1].1.focused);
        let mut views = vec![v("/a/Pics", false), v("/b/Pics", false)];
        assert!(super::dolphin::mark_active(&mut views, "/b/Pics — Dolphin"));
        assert!(views[1].1.focused);
        // Nothing matches: no view is marked, so a remembered layout is walked again.
        let mut views = vec![v("/a/Apps", false)];
        assert!(!super::dolphin::mark_active(&mut views, "Downloads — Dolphin"));
        // Hidden views go when one is showing.
        let mut views = vec![(false, DolphinView::default()), v("/a/Pics", true)];
        super::dolphin::mark_active(&mut views, "");
        assert_eq!(views.len(), 1);
    }

    /// Prints the focused element and selection of the frontmost window.
    #[test]
    #[ignore]
    fn live_focus() {
        eprintln!("element: {:?}", super::element());
        eprintln!("atspi text: {:?}", super::atspi_text());
        // Every app on the bus, without touching focus: its name, pid and focused object.
        let c = super::a11y().unwrap();
        let root: super::Obj = ("org.a11y.atspi.Registry".into(), zbus::zvariant::OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").unwrap());
        for app in super::call::<_, Vec<super::Obj>>(&c, &root, super::ACCESSIBLE, "GetChildren", &()).unwrap() {
            let name: String = super::bus::get(&c, &app.0, app.1.as_str(), super::ACCESSIBLE, "Name").unwrap_or_default();
            let f = super::focused(&c, &app);
            eprintln!("app {name:?} {}: focused {:?}", app.0, f.as_ref().map(|o| super::describe(&c, o, 0)));
        }
    }
}
