// KnobDeck window watcher for KWin (Plasma 6, and Plasma 5's names as a fallback).
// knobdeck-os/src/linux/kwin.rs prepends SERVICE, PATH and IFACE and keeps this loaded while a
// watcher lives. It calls `Changed` (no arguments) when the active window changes, or the active
// window's caption, maximize, full screen, minimize or keep-above state does; the watcher then
// reads the window itself.

function ping() {
    callDBus(SERVICE, PATH, IFACE, "Changed");
}

function signals(w) {
    // `maximizedChanged` is Plasma 6.x; older builds only report the geometry change.
    const max = w.maximizedChanged ? "maximizedChanged" : "frameGeometryChanged";
    return ["captionChanged", "fullScreenChanged", "minimizedChanged", "keepAboveChanged", max]
        .map(name => w[name])
        .filter(s => s && s.connect);
}

let current = null;

function follow(w) {
    if (current) {
        for (const s of signals(current)) {
            try {
                s.disconnect(ping);
            } catch (e) {
                // The window may be gone.
            }
        }
    }
    current = w || null;
    if (current) {
        for (const s of signals(current)) s.connect(ping);
    }
    ping();
}

(workspace.windowActivated || workspace.clientActivated).connect(follow);
if (workspace.clientMaximizeSet) workspace.clientMaximizeSet.connect(ping);
follow(workspace.activeWindow || workspace.activeClient);
