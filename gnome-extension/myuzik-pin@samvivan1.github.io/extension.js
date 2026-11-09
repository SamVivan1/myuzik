import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import Shell from 'gi://Shell';

const TARGET_TITLE = 'Myuzik Lyrics';
const TARGET_APP_ID = 'io.github.samvivan1.Myuzik';

export default class MyuzikPinExtension {
    constructor() {
        this.windowCreatedId = 0;
        this.pending = new Set();
    }

    enable() {
        this.windowCreatedId = global.display.connect('window-created', (_display, metaWindow) => {
            this.handleWindow(metaWindow);
        });
        // Apply to windows that already exist.
        for (const actor of global.get_window_actors()) {
            this.handleWindow(actor.meta_window);
        }
    }

    disable() {
        if (this.windowCreatedId) {
            global.display.disconnect(this.windowCreatedId);
            this.windowCreatedId = 0;
        }
        this.pending.clear();
    }

    matches(metaWindow) {
        try {
            if (metaWindow.get_title() === TARGET_TITLE) return true;
            const app = Shell.WindowTracker.get_default().get_window_app(metaWindow);
            if (app && app.get_id() === TARGET_APP_ID) {
                // Only pin the lyrics window, not the main window.
                return metaWindow.get_title() === TARGET_TITLE;
            }
        } catch (e) {
            // ignore
        }
        return false;
    }

    handleWindow(metaWindow) {
        if (!metaWindow || !this.matches(metaWindow)) return;

        const wrapper = metaWindow.get_compositor_private();
        if (!wrapper) return;

        const firstFrame = wrapper.connect('first-frame', () => {
            wrapper.disconnect(firstFrame);
            this.pin(metaWindow);
        });
        this.pending.add(metaWindow);
    }

    pin(metaWindow) {
        this.pending.delete(metaWindow);
        if (!metaWindow) return;
        try {
            if (metaWindow.has_focus()) {
                const prev = metaWindow.previously_focused;
                if (prev) Main.activateWindow(prev);
            }
            metaWindow.unmaximize?.(3);
            metaWindow.make_above();
            metaWindow.stick();
            metaWindow.hide_from_window_list?.();
        } catch (e) {
            // Compositor may not support one of these; keep going.
        }
    }
}
