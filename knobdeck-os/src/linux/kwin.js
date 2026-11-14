// KnobDeck window bridge for KWin (Plasma 6). linux-desktop/src/kwin.rs prepends `CMD`, `ARGS`
// and `reply(value, error)`, loads this through org.kde.kwin.Scripting, runs it once and unloads it.
// Same commands and JSON as the GNOME extension (see wm.rs).

function rect(g) {
    return { x: g.x, y: g.y, w: g.width, h: g.height };
}

function screens() {
    return workspace.screens;
}

function outputIndex(o) {
    const list = screens();
    for (let i = 0; i < list.length; i++) if (o && list[i].name === o.name) return i;
    return -1;
}

function outputs() {
    const primary = workspace.screenOrder && workspace.screenOrder.length ? workspace.screenOrder[0].name : null;
    return screens().map((s, i) => ({
        index: i,
        name: s.name,
        frame: rect(s.geometry),
        work: rect(workspace.clientArea(KWin.MaximizeArea, s, workspace.currentDesktop)),
        primary: primary ? s.name === primary : i === 0,
    }));
}

function isNormal(w) {
    return w && w.normalWindow && !w.skipTaskbar && !w.deleted && w.managed;
}

function info(w) {
    return {
        id: String(w.internalId),
        title: w.caption || "",
        app: w.resourceClass || w.resourceName || "",
        desktopFile: w.desktopFileName || "",
        pid: w.pid || 0,
        frame: rect(w.frameGeometry),
        output: outputIndex(w.output),
        minimized: !!w.minimized,
        maximized: w.maximizeMode === 3,
        fullscreen: !!w.fullScreen,
        above: !!w.keepAbove,
        noBorder: !!w.noBorder,
        opacity: w.opacity,
    };
}

function find(id) {
    if (!id) {
        const a = workspace.activeWindow;
        if (!a) throw new Error("No window has focus");
        return a;
    }
    const all = workspace.windowList();
    for (let i = 0; i < all.length; i++) if (String(all[i].internalId) === id) return all[i];
    throw new Error("That window is no longer open");
}

function unmaximize(w) {
    if (w.fullScreen) w.fullScreen = false;
    if (w.maximizeMode !== 0) w.setMaximize(false, false);
}

function run(cmd, a) {
    switch (cmd) {
        case "outputs":
            return outputs();
        case "list":
            return workspace.stackingOrder.filter(isNormal).reverse().map(info);
        case "active": {
            const w = workspace.activeWindow;
            return isNormal(w) ? info(w) : null;
        }
        case "info":
            return info(find(a.id));
        case "minimize":
            find(a.id).minimized = true;
            return null;
        case "unminimize":
            find(a.id).minimized = false;
            return null;
        case "maximize": {
            const w = find(a.id);
            w.minimized = false;
            if (w.fullScreen) w.fullScreen = false;
            w.setMaximize(true, true);
            return null;
        }
        case "restore": {
            const w = find(a.id);
            w.minimized = false;
            unmaximize(w);
            return null;
        }
        case "close":
            find(a.id).closeWindow();
            return null;
        case "activate": {
            const w = find(a.id);
            w.minimized = false;
            workspace.activeWindow = w;
            return null;
        }
        case "lower": {
            // KWin lowers the active window only.
            const w = find(a.id);
            if (workspace.activeWindow !== w) workspace.activeWindow = w;
            workspace.slotWindowLower();
            return null;
        }
        case "toggle-maximize": {
            const w = find(a.id);
            w.minimized = false;
            const max = w.maximizeMode !== 3;
            w.setMaximize(max, max);
            return max;
        }
        case "toggle-fullscreen": {
            const w = find(a.id);
            w.fullScreen = !w.fullScreen;
            return w.fullScreen;
        }
        case "toggle-above": {
            const w = find(a.id);
            w.keepAbove = !w.keepAbove;
            return w.keepAbove;
        }
        case "toggle-border": {
            const w = find(a.id);
            w.noBorder = !w.noBorder;
            return w.noBorder;
        }
        case "opacity":
            find(a.id).opacity = a.value;
            return null;
        case "move": {
            const w = find(a.id);
            w.minimized = false;
            unmaximize(w);
            w.frameGeometry = { x: a.x, y: a.y, width: a.w, height: a.h };
            return rect(w.frameGeometry);
        }
        case "to-output": {
            const w = find(a.id);
            const s = screens()[a.index];
            if (!s) throw new Error("That display is no longer connected");
            workspace.sendClientToScreen(w, s);
            return null;
        }
        case "desktop":
            if (a.dir === "next") workspace.slotSwitchDesktopNext();
            else if (a.dir === "prev") workspace.slotSwitchDesktopPrevious();
            else if (a.dir === "new") {
                workspace.createDesktop(workspace.desktops.length, "");
                workspace.currentDesktop = workspace.desktops[workspace.desktops.length - 1];
            } else if (a.dir === "close") {
                if (workspace.desktops.length < 2) throw new Error("This is the only desktop");
                workspace.removeDesktop(workspace.currentDesktop);
            }
            return null;
        case "show-desktop":
            workspace.slotToggleShowDesktop();
            return null;
        default:
            throw new Error("Unknown command " + cmd);
    }
}

try {
    reply(run(CMD, ARGS), "");
} catch (e) {
    reply(null, String(e && e.message ? e.message : e));
}
