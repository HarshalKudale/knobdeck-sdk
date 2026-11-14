// KnobDeck bridge for GNOME Shell 45+.
//
// GNOME on Wayland gives other programs no way to list, focus or move windows, and the Shell's
// own D-Bus methods for that (Eval, Introspect, FocusSearch, ShowApplications) are restricted to
// GNOME's own callers since GNOME 41. This extension exposes the few things KnobDeck and its
// Linux Tools plugin need on the session bus as `dev.knobdeck.Shell` (/dev/knobdeck/Shell):
//
//   Call(command s, args s) -> (result s)   result is JSON: {"ok": value} or {"error": "text"}
//   signal WindowChanged()                  the focused window changed, or its title or
//                                           maximize / full screen / minimize / above state did
//
// The commands match knobdeck-os/src/linux/kwin.js (see linux-desktop/src/wm.rs in
// knobdeck-plugins), plus the GNOME-only ones at the end of run(). Lives in knobdeck-sdk's
// knobdeck-os crate; installed and updated by the Linux Tools plugin.

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'dev.knobdeck.Shell';
const OBJECT_PATH = '/dev/knobdeck/Shell';
const VERSION = 2;
const MAX_CLIPS = 30;

const IFACE = `
<node>
  <interface name="dev.knobdeck.Shell">
    <method name="Call">
      <arg type="s" direction="in" name="command"/>
      <arg type="s" direction="in" name="args"/>
      <arg type="s" direction="out" name="result"/>
    </method>
    <property name="Version" type="u" access="read"/>
    <signal name="WindowChanged"/>
  </interface>
</node>`;

const now = () => global.get_current_time();
const rect = r => ({x: r.x, y: r.y, w: r.width, h: r.height});

function isNormal(w) {
    return w && w.get_window_type() === Meta.WindowType.NORMAL && !w.is_skip_taskbar();
}

// Mutter 49 replaced get_maximized()/maximize(flags) with is_maximized()/maximize().
function isMaximized(w) {
    if (typeof w.is_maximized === 'function')
        return w.is_maximized();
    return w.get_maximized() === Meta.MaximizeFlags.BOTH;
}

function maximize(w) {
    try {
        w.maximize(Meta.MaximizeFlags.BOTH);
    } catch (e) {
        w.maximize();
    }
}

function unmaximize(w) {
    if (w.is_fullscreen())
        w.unmake_fullscreen();
    if (!isMaximized(w))
        return;
    try {
        w.unmaximize(Meta.MaximizeFlags.BOTH);
    } catch (e) {
        w.unmaximize();
    }
}

function allWindows() {
    return global.get_window_actors().map(a => a.meta_window).filter(w => w);
}

function info(w) {
    const app = Shell.WindowTracker.get_default().get_window_app(w);
    const actor = w.get_compositor_private();
    return {
        id: String(w.get_id()),
        title: w.get_title() ?? '',
        app: w.get_wm_class() ?? w.get_gtk_application_id?.() ?? '',
        desktopFile: app?.get_id() ?? '',
        pid: w.get_pid(),
        frame: rect(w.get_frame_rect()),
        output: w.get_monitor(),
        minimized: w.minimized,
        maximized: isMaximized(w),
        fullscreen: w.is_fullscreen(),
        above: w.is_above(),
        noBorder: !w.decorated,
        opacity: actor ? actor.opacity / 255 : 1,
    };
}

function find(id) {
    if (!id) {
        const w = global.display.get_focus_window();
        if (!w)
            throw new Error('No window has focus');
        return w;
    }
    const w = allWindows().find(x => String(x.get_id()) === id);
    if (!w)
        throw new Error('That window is no longer open');
    return w;
}

function outputs() {
    const n = global.display.get_n_monitors();
    const primary = global.display.get_primary_monitor();
    const list = [];
    for (let i = 0; i < n; i++) {
        list.push({
            index: i,
            name: `Display ${i + 1}`,
            frame: rect(global.display.get_monitor_geometry(i)),
            work: rect(Main.layoutManager.getWorkAreaForMonitor(i)),
            primary: i === primary,
        });
    }
    return list;
}

// Properties of the focused window whose change KnobDeck's window watchers want to hear about.
// `maximized-*` are gone in Mutter 49; connecting to an unknown property throws, so each is tried.
const WATCHED = ['title', 'maximized-horizontally', 'maximized-vertically', 'maximized', 'fullscreen', 'minimized', 'above'];

class Bridge {
    constructor() {
        this._clips = [];
        this._hidden = [];
        this._emit = null;
        this._window = null;
        this._windowIds = [];
        this._selection = global.display.get_selection();
        this._ownerId = this._selection.connect('owner-changed', (_sel, type) => {
            if (type === Meta.SelectionType.SELECTION_CLIPBOARD)
                this._recordClipboard();
        });
        this._focusId = global.display.connect('notify::focus-window', () => this._follow(global.display.get_focus_window()));
        this._follow(global.display.get_focus_window());
    }

    destroy() {
        this._selection.disconnect(this._ownerId);
        global.display.disconnect(this._focusId);
        this._follow(null);
        this._emit = null;
        this._clips = [];
        this._hidden = [];
    }

    // Track the focused window's own changes, then tell listeners.
    _follow(w) {
        if (this._window) {
            for (const id of this._windowIds) {
                try {
                    this._window.disconnect(id);
                } catch (e) {
                    // The window is already gone.
                }
            }
        }
        this._window = w;
        this._windowIds = [];
        if (w) {
            for (const prop of WATCHED) {
                try {
                    this._windowIds.push(w.connect(`notify::${prop}`, () => this._changed()));
                } catch (e) {
                    // Not a property on this Mutter version.
                }
            }
        }
        this._changed();
    }

    _changed() {
        this._emit?.();
    }

    get Version() {
        return VERSION;
    }

    Call(command, args) {
        try {
            const value = this._run(command, args ? JSON.parse(args) : {});
            return JSON.stringify({ok: value === undefined ? null : value});
        } catch (e) {
            return JSON.stringify({error: String(e?.message ?? e)});
        }
    }

    _recordClipboard() {
        St.Clipboard.get_default().get_text(St.ClipboardType.CLIPBOARD, (_cb, text) => {
            if (!text || !text.trim())
                return;
            this._clips = [text, ...this._clips.filter(t => t !== text)].slice(0, MAX_CLIPS);
        });
    }

    _showDesktop() {
        // GNOME has no "show desktop"; minimize this workspace's windows, and bring them back
        // on the next press if none was touched since.
        const alive = this._hidden.filter(w => allWindows().includes(w) && w.minimized);
        if (alive.length) {
            alive.reverse().forEach(w => w.unminimize());
            this._hidden = [];
            return;
        }
        const ws = global.workspace_manager.get_active_workspace();
        this._hidden = global.display.get_tab_list(Meta.TabList.NORMAL, ws).filter(w => !w.minimized);
        this._hidden.forEach(w => w.minimize());
    }

    _run(cmd, a) {
        switch (cmd) {
        case 'outputs':
            return outputs();
        case 'list':
            return global.display.get_tab_list(Meta.TabList.NORMAL_ALL, null).filter(isNormal).map(info);
        case 'active': {
            const w = global.display.get_focus_window();
            return isNormal(w) ? info(w) : null;
        }
        case 'info':
            return info(find(a.id));
        case 'minimize':
            find(a.id).minimize();
            return null;
        case 'unminimize':
            find(a.id).unminimize();
            return null;
        case 'maximize': {
            const w = find(a.id);
            w.unminimize();
            if (w.is_fullscreen())
                w.unmake_fullscreen();
            maximize(w);
            return null;
        }
        case 'restore': {
            const w = find(a.id);
            w.unminimize();
            unmaximize(w);
            return null;
        }
        case 'close':
            find(a.id).delete(now());
            return null;
        case 'activate':
            Main.activateWindow(find(a.id));
            return null;
        case 'lower':
            find(a.id).lower();
            return null;
        case 'toggle-maximize': {
            const w = find(a.id);
            w.unminimize();
            if (isMaximized(w)) {
                unmaximize(w);
                return false;
            }
            maximize(w);
            return true;
        }
        case 'toggle-fullscreen': {
            // Report the requested state: on Wayland the client confirms it asynchronously.
            const w = find(a.id);
            const full = !w.is_fullscreen();
            if (full)
                w.make_fullscreen();
            else
                w.unmake_fullscreen();
            return full;
        }
        case 'toggle-above': {
            const w = find(a.id);
            const above = !w.is_above();
            if (above)
                w.make_above();
            else
                w.unmake_above();
            return above;
        }
        case 'toggle-border':
            throw new Error('GNOME does not let apps remove window borders');
        case 'opacity': {
            const actor = find(a.id).get_compositor_private();
            if (actor)
                actor.opacity = Math.round(Math.max(0, Math.min(1, a.value)) * 255);
            return null;
        }
        case 'move': {
            const w = find(a.id);
            w.unminimize();
            unmaximize(w);
            w.move_resize_frame(true, a.x, a.y, a.w, a.h);
            return rect(w.get_frame_rect());
        }
        case 'to-output': {
            if (a.index < 0 || a.index >= global.display.get_n_monitors())
                throw new Error('That display is no longer connected');
            find(a.id).move_to_monitor(a.index);
            return null;
        }
        case 'desktop': {
            const wm = global.workspace_manager;
            const ws = wm.get_active_workspace();
            if (a.dir === 'next' || a.dir === 'prev') {
                const to = ws.get_neighbor(a.dir === 'next' ? Meta.MotionDirection.RIGHT : Meta.MotionDirection.LEFT);
                if (to && to !== ws)
                    to.activate(now());
            } else if (a.dir === 'new') {
                wm.append_new_workspace(true, now());
            } else if (a.dir === 'close') {
                if (wm.get_n_workspaces() < 2)
                    throw new Error('This is the only workspace');
                wm.remove_workspace(ws, now());
            }
            return null;
        }
        case 'show-desktop':
            this._showDesktop();
            return null;
        // GNOME-only: Shell UI that has no other entry point for third-party programs.
        case 'overview':
            Main.overview.toggle();
            return null;
        case 'app-grid':
            if (Main.overview.dash?.showAppsButton?.checked && Main.overview.visible)
                Main.overview.hide();
            else
                Main.overview.showApps();
            return null;
        case 'search':
            if (typeof Main.overview.focusSearch === 'function')
                Main.overview.focusSearch();
            else
                Main.overview.show();
            return null;
        case 'screenshot':
            Main.screenshotUI.open();
            return null;
        case 'clipboard-history':
            return this._clips;
        case 'clipboard-set':
            St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, String(a.text ?? ''));
            return null;
        case 'version':
            return VERSION;
        default:
            throw new Error(`Unknown command ${cmd}`);
        }
    }
}

export default class KnobDeckExtension extends Extension {
    enable() {
        this._bridge = new Bridge();
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this._bridge);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._bridge._emit = () => this._dbus?.emit_signal('WindowChanged', null);
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.REPLACE, null, null);
    }

    disable() {
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = 0;
        }
        this._dbus?.unexport();
        this._dbus = null;
        this._bridge?.destroy();
        this._bridge = null;
    }
}
