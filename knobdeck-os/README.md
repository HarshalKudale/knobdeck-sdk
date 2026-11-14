# knobdeck-os

Windows, focus and installed apps on Windows, macOS and Linux, in one implementation for the
KnobDeck host and its plugins (plugins use it through `knobdeck_sdk::os`).

- `window::foreground()`, `window::target()` (the frontmost window that isn't KnobDeck's),
  `window::list()`, `window::focus(id)`, `window::focus_app(app, hint)`
- `window::watch(cb)`: event-driven (WinEvent hooks; workspace notifications + an AX observer;
  Hyprland's event socket; X11 property events; a KWin script; the GNOME extension's signal)
- `apps::installed()`

Linux: KDE Plasma and GNOME through `src/linux/kwin.js` and `src/linux/gnome-extension/`, plus
Hyprland and X11. `cargo test -p knobdeck-os live_read -- --ignored --nocapture` prints what the
running desktop reports.
