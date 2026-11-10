// Headless behaviour test for the Myuzik Pin GNOME Shell extension.
//
// The extension only needs `gi://Shell` at import time and the shell's `global`
// object at enable() time, so the real extension.js can be exercised in plain
// gjs with a fake display / fake Meta.Window objects. This checks the matching,
// pinning, idempotency and teardown logic without touching a live session.
//
// Run from the repo root:
//   LD_LIBRARY_PATH=/usr/lib/gnome-shell:/usr/lib/x86_64-linux-gnu/mutter-51 \
//   GI_TYPELIB_PATH=/usr/lib/x86_64-linux-gnu/mutter-51:/usr/lib/gnome-shell \
//   gjs -m gnome-extension/test-extension.js
//
// (the two paths only provide the GNOME Shell typelib for `gi://Shell`; they are
//  not needed when the extension runs inside the shell).

import Shell from 'gi://Shell';
import MyuzikPinExtension from './myuzik-pin@samvivan1.github.io/extension.js';

// Shell.WindowTracker.get_default() needs a running shell — it segfaults in plain
// gjs (observed: SIGSEGV with `gjs -m` here) — so replace it with a controllable
// fake before the extension can call it. Inside the real shell the extension uses
// the real tracker; this only makes the fallback path testable.
let trackedAppId = null;
Shell.WindowTracker.get_default = () => ({
    get_window_app: () => (trackedAppId ? {get_id: () => trackedAppId} : null),
});

const APP_ID = 'io.github.samvivan1.Myuzik';
const OVERLAY_TITLE = 'Myuzik Lyrics';
const MAIN_TITLE = 'Myuzik';

let failures = 0;
let passes = 0;

function check(name, body) {
    try {
        body();
        passes++;
        print(`ok   ${name}`);
    } catch (error) {
        failures++;
        print(`FAIL ${name}: ${error.message}`);
    }
}

function assert(condition, message) {
    if (!condition)
        throw new Error(message);
}

function assertEqual(actual, expected, what) {
    if (actual !== expected)
        throw new Error(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

// --- fakes -----------------------------------------------------------------

class FakeSignals {
    constructor() {
        this._handlers = new Map();
        this._nextId = 1;
    }

    connect(signal, callback) {
        const id = this._nextId++;
        this._handlers.set(id, {signal, callback});
        return id;
    }

    disconnect(id) {
        if (!this._handlers.delete(id))
            throw new Error(`no handler ${id}`);
    }

    emit(signal, ...args) {
        for (const {signal: name, callback} of [...this._handlers.values()]) {
            if (name === signal)
                callback(this, ...args);
        }
    }

    get handlerCount() {
        return this._handlers.size;
    }
}

class FakeActor extends FakeSignals {}

class FakeWindow extends FakeSignals {
    constructor({title = '', appId = null, wmClass = null, failAbove = false,
        failUnpin = false, throwOnCompositor = false} = {}) {
        super();
        this.title = title;
        this.appId = appId;
        this.wmClass = wmClass;
        this.failAbove = failAbove;
        this.failUnpin = failUnpin;
        this.throwOnCompositor = throwOnCompositor;
        this.above = false;
        this.sticky = false;
        this.actor = new FakeActor();
        this.makeAboveCalls = 0;
        this.stickCalls = 0;
        this.unmakeAboveCalls = 0;
        this.unstickCalls = 0;
    }

    get_title() {
        return this.title;
    }

    // set a title the way Mutter does: the property changes and notifies
    setTitle(title) {
        this.title = title;
        this.emit('notify::title');
    }

    get_gtk_application_id() {
        return this.appId;
    }

    get_wm_class() {
        return this.wmClass;
    }

    get_compositor_private() {
        if (this.throwOnCompositor)
            throw new Error('window is already disposed');
        return this.actor;
    }

    is_above() {
        return this.above;
    }

    is_on_all_workspaces() {
        return this.sticky;
    }

    make_above() {
        if (this.failAbove)
            throw new Error('compositor refused');
        this.makeAboveCalls++;
        this.above = true;
    }

    stick() {
        this.stickCalls++;
        this.sticky = true;
    }

    unmake_above() {
        if (this.failUnpin)
            throw new Error('window already gone');
        this.unmakeAboveCalls++;
        this.above = false;
    }

    unstick() {
        if (this.failUnpin)
            throw new Error('window already gone');
        this.unstickCalls++;
        this.sticky = false;
    }
}

class FakeDisplay extends FakeSignals {}

// --- scene -----------------------------------------------------------------

function scene() {
    const display = new FakeDisplay();
    // windows that are already open when enable() runs; each fake actor carries
    // `meta_window`, which is how the extension finds them
    const existing = [];
    globalThis.global = {
        display,
        get_window_actors: () => existing.map(window => window.actor),
    };

    const extension = new MyuzikPinExtension();
    return {display, existing, extension};
}

function openWindow(display, options) {
    const window = new FakeWindow(options);
    window.actor.meta_window = window;
    display.emit('window-created', window);
    return window;
}

// --- tests -----------------------------------------------------------------

check('unrelated windows are never pinned', () => {
    const {display, extension} = scene();
    extension.enable();
    const browser = openWindow(display, {title: 'Mozilla Firefox', appId: 'org.mozilla.firefox'});
    browser.actor.emit('first-frame');
    browser.setTitle('Mozilla Firefox');
    assert(!browser.above && !browser.sticky, 'browser was pinned');
    assertEqual(browser.makeAboveCalls, 0, 'make_above calls');
    extension.disable();
});

check('the main window is never pinned, even with the Myuzik app id', () => {
    const {display, extension} = scene();
    extension.enable();
    const main = openWindow(display, {title: MAIN_TITLE, appId: APP_ID});
    main.actor.emit('first-frame');
    assert(!main.above, 'main window was pinned');
    // ... and neither is a window with no title yet that belongs to the app
    const untitled = openWindow(display, {title: '', appId: APP_ID});
    untitled.actor.emit('first-frame');
    assert(!untitled.above, 'untitled app window was pinned');
    extension.disable();
});

check('the overlay is pinned once its title arrives (notify::title)', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: '', appId: APP_ID});
    assert(!overlay.above, 'pinned before the title existed');

    overlay.actor.emit('first-frame'); // first frame, title still unset
    assert(!overlay.above, 'pinned before the title existed');

    overlay.setTitle(OVERLAY_TITLE);
    assert(overlay.above, 'overlay was not made above');
    assert(overlay.sticky, 'overlay was not stuck to all workspaces');
    assertEqual(overlay.makeAboveCalls, 1, 'make_above calls');
    assertEqual(overlay.stickCalls, 1, 'stick calls');
    extension.disable();
});

check('the overlay is pinned on first-frame when the title came with it', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: '', appId: APP_ID});
    overlay.title = OVERLAY_TITLE; // set without notifying, like a title at map time
    overlay.actor.emit('first-frame');
    assert(overlay.above && overlay.sticky, 'overlay was not pinned on first frame');
    extension.disable();
});

check('a retitled overlay is still recognised by app id', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: 'Lyrics', appId: APP_ID});
    overlay.actor.emit('first-frame');
    assert(overlay.above, 'app-id fallback did not pin the overlay');
    extension.disable();
});

check('pinning is idempotent', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE});
    for (let i = 0; i < 5; i++)
        overlay.setTitle(OVERLAY_TITLE);
    overlay.actor.emit('first-frame');
    assertEqual(overlay.makeAboveCalls, 1, 'make_above calls after repeated signals');
    assertEqual(overlay.stickCalls, 1, 'stick calls after repeated signals');
    extension.disable();
});

check('enable() pins an overlay that was already open', () => {
    const {existing, extension} = scene();
    const overlay = new FakeWindow({title: OVERLAY_TITLE, appId: APP_ID});
    overlay.actor.meta_window = overlay;
    existing.push(overlay);

    extension.enable(); // already drawn: no first-frame will ever arrive
    assert(overlay.above && overlay.sticky, 'already-open overlay was not pinned');
    extension.disable();
});

check('a window that vanishes mid-callback does not throw', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: '', appId: APP_ID});
    overlay.actor = null; // unmanaged between the signal and the pin
    overlay.setTitle(OVERLAY_TITLE);
    assert(!overlay.above, 'pinned a window without an actor');
    extension.disable();
});

check('a compositor refusal does not throw', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE, failAbove: true});
    overlay.actor.emit('first-frame');
    assert(!overlay.above, 'window was pinned although make_above() failed');
    extension.disable();
});

check('unmanaged drops every handler for that window', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE});
    assert(overlay.handlerCount > 0, 'no handlers were connected');
    overlay.emit('unmanaged');
    assertEqual(overlay.handlerCount, 0, 'handlers left on the window');
    assertEqual(overlay.actor.handlerCount, 0, 'handlers left on the actor');
    overlay.setTitle(OVERLAY_TITLE);
    assertEqual(overlay.makeAboveCalls, 1, 'make_above calls after unmanaged');
    extension.disable();
});

check('disable() disconnects everything and unpins', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE});
    assert(overlay.above, 'overlay was not pinned');

    extension.disable();
    assertEqual(display.handlerCount, 0, 'handlers left on the display');
    assertEqual(overlay.handlerCount, 0, 'handlers left on the window');
    assertEqual(overlay.actor.handlerCount, 0, 'handlers left on the actor');
    assert(!overlay.above, 'overlay stayed above after disable');
    assert(!overlay.sticky, 'overlay stayed stuck after disable');
    assertEqual(overlay.unmakeAboveCalls, 1, 'unmake_above calls');
    assertEqual(overlay.unstickCalls, 1, 'unstick calls');
});

check('disable() tolerates a window that is already gone', () => {
    const {display, extension} = scene();
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE, failUnpin: true});
    assert(overlay.above, 'overlay was not pinned');
    extension.disable(); // unmake_above()/unstick() throw here
    assertEqual(display.handlerCount, 0, 'handlers left on the display');
});

check('enable/disable/enable cycles do not leak handlers', () => {
    const {display, extension} = scene();
    for (let i = 0; i < 3; i++) {
        extension.enable();
        assertEqual(display.handlerCount, 1, `display handlers after enable #${i + 1}`);
        extension.disable();
        assertEqual(display.handlerCount, 0, `display handlers after disable #${i + 1}`);
    }

    // after a re-enable the overlay is pinned exactly once, not once per cycle
    extension.enable();
    const overlay = openWindow(display, {title: OVERLAY_TITLE});
    overlay.actor.emit('first-frame');
    assertEqual(overlay.makeAboveCalls, 1, 'make_above calls');
    extension.disable();
});

check('a window already disposed at window-created does not throw', () => {
    const {display, extension} = scene();
    extension.enable();
    const dead = new FakeWindow({title: OVERLAY_TITLE, throwOnCompositor: true});
    dead.actor.meta_window = dead;
    display.emit('window-created', dead); // get_compositor_private() throws here
    assert(!dead.above, 'a disposed window was pinned');
    extension.disable();
});

check('a second enable() without disable() does not double-connect', () => {
    const {display, extension} = scene();
    extension.enable();
    extension.enable();
    assertEqual(display.handlerCount, 1, 'display handlers after a repeated enable');
    const overlay = openWindow(display, {title: OVERLAY_TITLE});
    assertEqual(overlay.makeAboveCalls, 1, 'make_above calls');
    extension.disable();
});

check('the window tracker app id is a fallback when the window has none', () => {
    const {display, extension} = scene();
    extension.enable();
    trackedAppId = 'io.github.samvivan1.Myuzik.desktop'; // Shell.App ids carry .desktop
    const overlay = openWindow(display, {title: 'Lyrics', appId: null});
    overlay.actor.emit('first-frame');
    trackedAppId = null;
    assert(overlay.above, 'the desktop-file app id did not match');
    extension.disable();
});

check('WM_CLASS is the last-resort fallback', () => {
    const {display, extension} = scene();
    extension.enable();
    trackedAppId = null; // no tracker app
    const overlay = openWindow(display, {title: 'Lyrics', appId: null, wmClass: 'io.github.samvivan1.Myuzik'});
    overlay.actor.emit('first-frame');
    assert(overlay.above, 'WM_CLASS match did not pin the overlay');
    extension.disable();
});

check('the main window is not pinned through the app-id fallback either', () => {
    const {display, extension} = scene();
    extension.enable();
    trackedAppId = 'io.github.samvivan1.Myuzik.desktop';
    const main = openWindow(display, {title: MAIN_TITLE, appId: null, wmClass: 'io.github.samvivan1.Myuzik'});
    main.actor.emit('first-frame');
    trackedAppId = null;
    assert(!main.above, 'the main window was pinned via the app-id fallback');
    extension.disable();
});

// --- summary ---------------------------------------------------------------

print(`\n${passes} passed, ${failures} failed`);
if (failures > 0)
    throw new Error(`${failures} extension behaviour test(s) failed`);
