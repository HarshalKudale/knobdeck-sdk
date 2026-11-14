//! macOS: Accessibility (windows, focus, the AX observer), CGWindowList (window order without
//! Screen Recording) and NSWorkspace (apps, activation).

#[cfg(feature = "window")]
pub(crate) mod window;

/// Run the main thread's run loop for the rest of the process. Call it on the main thread, after
/// moving the program's own work to other threads.
///
/// [`crate::window::watch`] gets the workspace's app-activation notifications and the
/// Accessibility observer's callbacks on the main run loop. The KnobDeck app runs it already; a
/// command-line process (a plugin) must run it itself. The SDK does this for plugins built with
/// its `os-window` feature.
pub fn run_main_loop() -> ! {
    core_foundation::runloop::CFRunLoop::run_current();
    loop {
        std::thread::park();
    }
}
