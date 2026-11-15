//! OS context handed to action handlers: the target window, the focused element, the selected
//! text, the selected files, the clipboard. Read in the plugin's own process from [`crate::os`].
//!
//! Every reader on [`Ctx`](crate::Ctx) is lazy: the first call reads the OS, later calls in the
//! same press reuse the value. [`Action::needs`](crate::Action::needs) declares what an action
//! uses: the runtime then reads those in parallel before the handler runs, within a time budget
//! (300 ms unless [`Action::needs_timeout`](crate::Action::needs_timeout) says otherwise), and the
//! plugins page lists them. A need that fails or runs out of time reads as `None`, with the reason
//! in [`Ctx::context_error`](crate::Ctx::context_error).
//!
//! Each kind exists only with its SDK feature (`os-window`, `os-focus`, `os-selection`,
//! `os-files`, `os-clipboard`), so naming one that isn't enabled doesn't compile.

#![cfg_attr(not(any(feature = "os-window", feature = "os-focus", feature = "os-selection", feature = "os-files", feature = "os-clipboard")), allow(dead_code, unused_imports, unreachable_code, unused_variables))]

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Default time the runtime waits for an action's needs before running its handler.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(300);

/// A kind of OS context an action reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Need {
    /// The target window (the frontmost one that isn't KnobDeck's): [`Ctx::window`](crate::Ctx::window).
    #[cfg(feature = "os-window")]
    Window,
    /// The focused element (role, editable, password): [`Ctx::focus`](crate::Ctx::focus).
    #[cfg(feature = "os-focus")]
    Focus,
    /// The selected text: [`Ctx::selection`](crate::Ctx::selection).
    #[cfg(feature = "os-selection")]
    Selection,
    /// The files selected in the target file manager: [`Ctx::files`](crate::Ctx::files).
    #[cfg(feature = "os-files")]
    Files,
    /// The clipboard's text and files: [`Ctx::clipboard_text`](crate::Ctx::clipboard_text),
    /// [`Ctx::clipboard_files`](crate::Ctx::clipboard_files).
    #[cfg(feature = "os-clipboard")]
    Clipboard,
}

impl Need {
    /// The name in `actions.json` (`needs`) and the manifest's `uses`.
    pub fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "os-window")]
            Need::Window => "window",
            #[cfg(feature = "os-focus")]
            Need::Focus => "focus",
            #[cfg(feature = "os-selection")]
            Need::Selection => "selection",
            #[cfg(feature = "os-files")]
            Need::Files => "files",
            #[cfg(feature = "os-clipboard")]
            Need::Clipboard => "clipboard",
        }
    }
}

/// What this plugin binary can read, from the SDK features it was built with (the manifest's
/// `uses`). Exact for SDK use; direct OS calls aren't seen.
pub(crate) fn uses() -> Vec<&'static str> {
    let mut u = vec![];
    if cfg!(feature = "os-window") {
        u.push("window");
    }
    if cfg!(feature = "os-apps") {
        u.push("apps");
    }
    if cfg!(feature = "os-focus") {
        u.push("focus");
    }
    if cfg!(feature = "os-selection") {
        u.push("selection");
    }
    if cfg!(feature = "os-files") {
        u.push("files");
    }
    if cfg!(feature = "os-clipboard") {
        u.push("clipboard");
    }
    u
}

/// The clipboard's text and files, read together.
#[cfg(feature = "os-clipboard")]
#[derive(Clone, Debug, Default)]
pub(crate) struct Clip {
    pub text: Option<String>,
    pub files: Vec<std::path::PathBuf>,
}

/// One press's context: each value read at most once.
#[derive(Default)]
pub(crate) struct Context {
    #[cfg(feature = "os-window")]
    pub window: OnceLock<Option<crate::os::window::Window>>,
    #[cfg(feature = "os-focus")]
    pub focus: OnceLock<Option<crate::os::focus::Element>>,
    #[cfg(feature = "os-selection")]
    pub selection: OnceLock<Option<String>>,
    #[cfg(feature = "os-files")]
    pub files: OnceLock<Option<Vec<std::path::PathBuf>>>,
    #[cfg(feature = "os-files")]
    pub folder: OnceLock<Option<std::path::PathBuf>>,
    #[cfg(feature = "os-clipboard")]
    pub clipboard: OnceLock<Option<Clip>>,
    pub errors: Mutex<HashMap<&'static str, String>>,
}

impl Context {
    fn fail(&self, need: &'static str, e: impl ToString) {
        self.errors.lock().unwrap().insert(need, e.to_string());
    }

    /// Read one need now (on the calling thread) and store it.
    fn read(&self, need: Need) {
        match need {
            #[cfg(feature = "os-window")]
            Need::Window => {
                self.window.get_or_init(|| {
                    let w = crate::os::window::target();
                    if w.is_none() {
                        self.fail("window", "No window to act on");
                    }
                    w
                });
            }
            #[cfg(feature = "os-focus")]
            Need::Focus => {
                self.focus.get_or_init(|| crate::os::focus::element().unwrap_or_else(|e| {
                    self.fail("focus", e);
                    None
                }));
            }
            #[cfg(feature = "os-selection")]
            Need::Selection => {
                self.selection.get_or_init(|| crate::os::selection::text().unwrap_or_else(|e| {
                    self.fail("selection", e);
                    None
                }));
            }
            #[cfg(feature = "os-files")]
            Need::Files => {
                self.files.get_or_init(|| match crate::os::files::selected() {
                    Ok(f) => Some(f),
                    Err(e) => {
                        self.fail("files", e);
                        None
                    }
                });
            }
            #[cfg(feature = "os-clipboard")]
            Need::Clipboard => {
                self.clipboard.get_or_init(|| {
                    let text = crate::os::clipboard::text();
                    let files = crate::os::clipboard::files();
                    match (text, files) {
                        (Err(e), _) | (_, Err(e)) => {
                            self.fail("clipboard", e);
                            None
                        }
                        (Ok(text), Ok(files)) => Some(Clip { text, files }),
                    }
                });
            }
        }
    }

    /// Mark a need as read with no value (it ran out of time).
    fn give_up(&self, need: Need, budget: Duration) {
        let why = format!("Reading {} took longer than {} ms", need.as_str(), budget.as_millis());
        let set = match need {
            #[cfg(feature = "os-window")]
            Need::Window => self.window.set(None).is_ok(),
            #[cfg(feature = "os-focus")]
            Need::Focus => self.focus.set(None).is_ok(),
            #[cfg(feature = "os-selection")]
            Need::Selection => self.selection.set(None).is_ok(),
            #[cfg(feature = "os-files")]
            Need::Files => self.files.set(None).is_ok(),
            #[cfg(feature = "os-clipboard")]
            Need::Clipboard => self.clipboard.set(None).is_ok(),
        };
        if set {
            self.fail(need.as_str(), why);
        }
    }
}

/// Read `needs` in parallel, waiting at most `budget`.
pub(crate) fn prefetch(needs: &[Need], budget: Duration) -> Arc<Context> {
    let ctx = Arc::new(Context::default());
    if needs.is_empty() {
        return ctx;
    }
    let (tx, rx) = mpsc::channel();
    for &need in needs {
        let (c, tx) = (ctx.clone(), tx.clone());
        std::thread::spawn(move || {
            c.read(need);
            let _ = tx.send(need);
        });
    }
    drop(tx);
    let deadline = Instant::now() + budget;
    let mut done = vec![];
    while done.len() < needs.len() {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(n) => done.push(n),
            Err(_) => break,
        }
    }
    for &need in needs.iter().filter(|n| !done.contains(n)) {
        ctx.give_up(need, budget);
    }
    ctx
}

/// Lazy readers, shared by [`Ctx`](crate::Ctx).
impl Context {
    #[cfg(feature = "os-window")]
    pub fn window(&self) -> Option<&crate::os::window::Window> {
        self.read(Need::Window);
        self.window.get().and_then(Option::as_ref)
    }
    #[cfg(feature = "os-focus")]
    pub fn focus(&self) -> Option<&crate::os::focus::Element> {
        self.read(Need::Focus);
        self.focus.get().and_then(Option::as_ref)
    }
    #[cfg(feature = "os-selection")]
    pub fn selection(&self) -> Option<&str> {
        self.read(Need::Selection);
        self.selection.get().and_then(|s| s.as_deref())
    }
    #[cfg(feature = "os-files")]
    pub fn files(&self) -> Option<&[std::path::PathBuf]> {
        self.read(Need::Files);
        self.files.get().and_then(|f| f.as_deref())
    }
    #[cfg(feature = "os-files")]
    pub fn folder(&self) -> Option<&std::path::Path> {
        self.folder
            .get_or_init(|| {
                crate::os::files::current_folder().unwrap_or_else(|e| {
                    self.fail("folder", e);
                    None
                })
            })
            .as_deref()
    }
    #[cfg(feature = "os-clipboard")]
    pub fn clip(&self) -> Option<&Clip> {
        self.read(Need::Clipboard);
        self.clipboard.get().and_then(Option::as_ref)
    }
    pub fn error(&self, need: &'static str) -> Option<String> {
        self.errors.lock().unwrap().get(need).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_needs_reads_nothing() {
        let c = prefetch(&[], DEFAULT_BUDGET);
        assert!(c.errors.lock().unwrap().is_empty());
    }

    #[cfg(feature = "os-clipboard")]
    #[test]
    fn a_need_that_runs_out_of_time_reads_as_none() {
        let c = Context::default();
        c.give_up(Need::Clipboard, Duration::from_millis(1));
        assert!(c.clip().is_none());
        assert!(c.error("clipboard").unwrap().contains("longer than"));
    }
}
