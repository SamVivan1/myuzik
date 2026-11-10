// Myuzik Pin — keeps the Myuzik lyrics overlay above other windows.
//
// The overlay is a second, borderless GPUI window created with
// app_id `io.github.samvivan1.Myuzik` and title `Myuzik Lyrics`
// (src/overlay/mod.rs). A plain Wayland client cannot raise itself above other
// toplevels, so this extension applies Mutter's `make_above()` + `stick()` to
// that one window and to nothing else.
//
// API surface used here, checked against the typelibs installed with GNOME
// Shell 51 / Mutter 51 (`gjs` + GI introspection, see docs/gnome-extension.md):
//   Meta.Window          make_above(), unmake_above(), stick(), unstick(),
//                        is_above(), is_on_all_workspaces(), get_title(),
//                        get_gtk_application_id(), get_wm_class(),
//                        get_compositor_private(), `title` property
//                        (so `notify::title` is a valid signal detail)
//   Meta.WindowActor     `first-frame` signal
//   Shell.WindowTracker  get_default(), get_window_app()
//   Shell.App            get_id()
// Everything else is the stock GNOME 45+ ESM extension API (`enable`/`disable`).

import Shell from 'gi://Shell';

const OVERLAY_TITLE = 'Myuzik Lyrics';
const APP_ID = 'io.github.samvivan1.Myuzik';
// Title of the app's main window (src/main.rs). It belongs to the same app id,
// so it must never be pinned.
const MAIN_WINDOW_TITLE = 'Myuzik';

const LOG_PREFIX = 'myuzik-pin:';

/**
 * Debug logging only.
 *
 * gjs routes console.debug() to GLib's debug level, which GNOME Shell discards
 * unless debug logging is enabled (G_MESSAGES_DEBUG=all). A normal session
 * therefore stays silent; console.log() would not. Verified with
 * `gjs -c 'console.debug("x")'` (no output) vs `console.log` (prints).
 *
 * @param {string} message what happened
 */
function debug(message) {
    console.debug(`${LOG_PREFIX} ${message}`);
}

/**
 * Normalise an app id / WM class for comparison: Shell.App.get_id() returns a
 * desktop file id ("io.github.samvivan1.Myuzik.desktop"), the GTK/Wayland app id
 * does not, and X11 WM_CLASS casing is not guaranteed.
 *
 * @param {?string} value raw id
 * @returns {string} comparable id, or '' when there is nothing to compare
 */
function normalizeId(value) {
    if (!value)
        return '';
    return String(value).toLowerCase().replace(/\.desktop$/, '');
}

export default class MyuzikPinExtension {
    constructor() {
        // Meta.Window -> {pinned, actor, firstFrameId, titleId, unmanagedId}
        this._windows = new Map();
        this._displaySignalId = 0;
    }

    enable() {
        // A second enable() without a disable() would leak the display handler.
        if (this._displaySignalId)
            this.disable();

        this._displaySignalId = global.display.connect('window-created',
            (_display, metaWindow) => this._track(metaWindow));

        // Windows that already exist: enabling the extension after the app
        // started must still pin the overlay.
        for (const actor of global.get_window_actors()) {
            const metaWindow = actor?.meta_window;
            if (metaWindow)
                this._track(metaWindow, actor);
        }
    }

    disable() {
        if (this._displaySignalId) {
            this._disconnect(global.display, this._displaySignalId);
            this._displaySignalId = 0;
        }

        // Drop every handler we own and undo the pin, so a disabled extension
        // leaves the overlay as an ordinary window.
        for (const metaWindow of [...this._windows.keys()])
            this._forget(metaWindow, {unpin: true});
    }

    /**
     * Start watching one window for the overlay title / app id.
     *
     * @param {Meta.Window} metaWindow window to consider
     * @param {?Meta.WindowActor} knownActor actor already known by the caller
     */
    _track(metaWindow, knownActor = null) {
        if (!metaWindow || this._windows.has(metaWindow))
            return;

        const entry = {
            pinned: false,
            actor: null,
            firstFrameId: 0,
            titleId: 0,
            unmanagedId: 0,
        };
        this._windows.set(metaWindow, entry);

        // The window is going away: drop our handlers and the bookkeeping.
        entry.unmanagedId = this._connect(metaWindow, 'unmanaged',
            () => this._forget(metaWindow, {unpin: false}));

        // A freshly created window usually has no title yet; it gets one once it
        // has drawn its first frame, so wait for that before matching.
        this._watchFirstFrame(metaWindow, entry, knownActor);
        entry.titleId = this._connect(metaWindow, 'notify::title',
            () => this._maybePin(metaWindow));

        // A window found by enable() may have drawn its first frame before this
        // extension was enabled, so `first-frame` will never fire for it.
        this._maybePin(metaWindow);
    }

    /**
     * Hook the actor's `first-frame` signal once.
     *
     * @param {Meta.Window} metaWindow our window
     * @param {object} entry bookkeeping for it
     * @param {?Meta.WindowActor} actor actor the caller already has
     */
    _watchFirstFrame(metaWindow, entry, actor) {
        let windowActor = actor;
        try {
            windowActor ??= metaWindow.get_compositor_private();
        } catch (error) {
            // The window was destroyed between the signal and this call.
            debug(`could not read the window actor: ${error.message ?? error}`);
            return;
        }
        // Without an actor we still react to notify::title, so this is not fatal.
        if (!windowActor)
            return;

        entry.actor = windowActor;
        entry.firstFrameId = this._connect(windowActor, 'first-frame', () => {
            this._disconnect(entry.actor, entry.firstFrameId);
            entry.firstFrameId = 0;
            entry.actor = null;
            this._maybePin(metaWindow);
        });
    }

    /**
     * Pin the window if it is the overlay. Called from several signals, so it
     * must be idempotent and must never throw.
     *
     * @param {Meta.Window} metaWindow window to test
     */
    _maybePin(metaWindow) {
        const entry = this._windows.get(metaWindow);
        if (!entry || entry.pinned)
            return;

        if (!this._matches(metaWindow))
            return;

        try {
            // The window can be unmanaged between the signal and this call.
            if (!metaWindow.get_compositor_private())
                return;

            if (!metaWindow.is_above())
                metaWindow.make_above();
            if (!metaWindow.is_on_all_workspaces())
                metaWindow.stick();

            entry.pinned = true;
            debug('pinned the lyrics overlay');
        } catch (error) {
            // Mutter may refuse on a window that is tearing down; keep going.
            debug(`could not pin window: ${error.message ?? error}`);
        }
    }

    /**
     * Is this the Myuzik lyrics overlay?
     *
     * The title is authoritative: the app sets it when the window is created.
     * The app id is a fallback for a renamed or otherwise retitled overlay — it
     * only applies to windows that are clearly not the main window, so the app's
     * main window is never pinned by accident.
     *
     * @param {Meta.Window} metaWindow window to test
     * @returns {boolean} true when the window is the overlay
     */
    _matches(metaWindow) {
        try {
            const title = metaWindow.get_title();
            if (title === OVERLAY_TITLE)
                return true;

            if (!title || title === MAIN_WINDOW_TITLE)
                return false;

            return this._belongsToApp(metaWindow);
        } catch (error) {
            debug(`could not read window title: ${error.message ?? error}`);
            return false;
        }
    }

    /**
     * Does the window report the Myuzik app id?
     *
     * @param {Meta.Window} metaWindow window to test
     * @returns {boolean} true when the window belongs to Myuzik
     */
    _belongsToApp(metaWindow) {
        const target = normalizeId(APP_ID);

        // Wayland: the app id the client set (WindowOptions.app_id).
        try {
            const gtkAppId = normalizeId(metaWindow.get_gtk_application_id?.());
            if (gtkAppId)
                return gtkAppId === target;
        } catch (error) {
            debug(`could not read gtk application id: ${error.message ?? error}`);
        }

        // Both: the shell's own idea of the owning application.
        try {
            const app = Shell.WindowTracker.get_default().get_window_app(metaWindow);
            if (app)
                return normalizeId(app.get_id()) === target;
        } catch (error) {
            debug(`could not look up the owning app: ${error.message ?? error}`);
        }

        // X11: WM_CLASS usually carries the same string.
        try {
            return normalizeId(metaWindow.get_wm_class?.()) === target;
        } catch (error) {
            debug(`could not read wm class: ${error.message ?? error}`);
            return false;
        }
    }

    /**
     * Stop tracking a window: disconnect everything we connected for it.
     *
     * @param {Meta.Window} metaWindow window to forget
     * @param {{unpin: boolean}} options also undo make_above()/stick()
     */
    _forget(metaWindow, {unpin}) {
        const entry = this._windows.get(metaWindow);
        if (!entry)
            return;

        this._windows.delete(metaWindow);

        this._disconnect(metaWindow, entry.titleId);
        this._disconnect(metaWindow, entry.unmanagedId);
        if (entry.actor && entry.firstFrameId)
            this._disconnect(entry.actor, entry.firstFrameId);

        if (unpin && entry.pinned)
            this._unpin(metaWindow);
    }

    /**
     * Return a pinned window to normal stacking.
     *
     * @param {Meta.Window} metaWindow window to unpin
     */
    _unpin(metaWindow) {
        try {
            if (metaWindow.is_above())
                metaWindow.unmake_above();
            if (metaWindow.is_on_all_workspaces())
                metaWindow.unstick();
            debug('unpinned the lyrics overlay');
        } catch (error) {
            debug(`could not unpin window: ${error.message ?? error}`);
        }
    }

    /**
     * connect() that never throws (an object can be gone by the time we hook it).
     *
     * @param {?object} gobject object to hook
     * @param {string} signal signal name
     * @param {Function} callback handler
     * @returns {number} handler id, 0 when connecting failed
     */
    _connect(gobject, signal, callback) {
        if (!gobject)
            return 0;
        try {
            return gobject.connect(signal, callback);
        } catch (error) {
            debug(`could not connect ${signal}: ${error.message ?? error}`);
            return 0;
        }
    }

    /**
     * disconnect() that never throws (the object may already be destroyed).
     *
     * @param {?object} gobject object to unhook
     * @param {number} id handler id from _connect()
     */
    _disconnect(gobject, id) {
        if (!gobject || !id)
            return;
        try {
            gobject.disconnect(id);
        } catch (error) {
            debug(`could not disconnect handler ${id}: ${error.message ?? error}`);
        }
    }
}
