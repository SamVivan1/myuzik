# GNOME pin extension

Myuzik's lyrics overlay is a second, borderless window. A Wayland toplevel cannot
raise itself above other applications, so the app ships a small GNOME Shell
extension that applies Mutter's always-on-top and all-workspaces state to that one
window. Everything else — drawing, geometry, lyrics sync — stays in the app.

- Extension source: `gnome-extension/myuzik-pin@samvivan1.github.io/`
- Install / remove: `scripts/install-extension.sh`, `scripts/uninstall-extension.sh`
- Behaviour tests for the extension logic: `gnome-extension/test-extension.js`

## What the extension does

On `enable()` it hooks `window-created` on the shell's display **and** scans
`global.get_window_actors()`, so it also works when it is enabled after the app is
already running. For every window it then, once:

1. matches the window against the overlay (see below);
2. calls `make_above()` and `stick()` on it;
3. remembers that the window is pinned, so repeated signals do nothing.

`make_above()` keeps the overlay above normal windows, `stick()` keeps it on every
workspace. On `disable()` the extension disconnects every signal it connected and
reverses both calls, so a disabled extension leaves the overlay as an ordinary
window instead of a stuck floating one.

Matches are applied per window and are idempotent: the two Mutter calls run once per
window, and repeated `notify::title` or `first-frame` signals are no-ops. A window
that is unmanaged mid-callback is skipped rather than raising; all signal handlers
are disconnected when the window goes away.

## How it recognises the overlay

The overlay is created by `src/overlay/mod.rs` with the app id
`io.github.samvivan1.Myuzik` and the title `Myuzik Lyrics`.

- **Title (authoritative).** A window whose title is exactly `Myuzik Lyrics` is the
  overlay. The title usually does not exist yet when `window-created` fires — it
  arrives with the first frame — so the extension also listens for `first-frame`
  on the window actor and for `notify::title` on the window, and matches then.
- **App id (fallback).** If the title is *not* the overlay title but the window's
  GTK app id (Wayland) or `Shell.WindowTracker` app id or WM class (X11) is
  Myuzik's, the window is treated as the overlay too. This keeps the pin working if
  the overlay is ever retitled.
- **Main window excluded.** The app's main window has the same app id, so a window
  titled `Myuzik` — or one with no title at all — is never pinned. This is why the
  fallback requires a title that is neither the overlay title nor `Myuzik`.

Nothing else is ever touched: a window of another application never matches, and
the extension never raises, focuses, moves, or resizes anything.

## What it does not do

- **Fullscreen windows may still cover the overlay.** `make_above()` wins against
  normal windows, not against a genuinely fullscreen one. This is the same
  limitation `docs/troubleshooting.md` records for the overlay.
- **No blur.** Mutter has no `ext-background-effect-v1`, so the overlay's
  translucency is simulated by the app; the extension cannot add blur.
- **No geometry control.** Size and position come from the app's saved
  `OverlayGeometry`; the extension only changes stacking and workspaces.
- **No click-through.** The overlay is a normal toplevel and receives input; the
  extension does not shape its input region.
- **It is not needed for the overlay to exist.** Without the extension the overlay
  still opens and still follows the lyrics — it just is not always on top.

## Verified environment

| Component | Version on this machine |
|---|---|
| GNOME Shell | 51.0 |
| Mutter | 51.0 |
| `gnome-extensions` CLI | 51.0 |
| Session | Wayland (`XDG_SESSION_TYPE=wayland`) |
| `gjs` | 1.90.0 |
| `node` | v25.9.0 |

## Install

```bash
scripts/install-extension.sh              # pack, install, try to enable
scripts/install-extension.sh --pack-only  # validate + pack into target/gnome-extension/, change nothing
```

The CLI needs `gnome-extensions` (from `gnome-shell`) and no sudo: everything goes
into `~/.local/share/gnome-shell/extensions/myuzik-pin@samvivan1.github.io/`.

## Enable

GNOME Shell scans extension directories **at session start**. On Wayland, a fresh
install is invisible to the running shell, so `enable` fails until you log out and
back in:

```bash
gnome-extensions enable myuzik-pin@samvivan1.github.io
# Extension "myuzik-pin@samvivan1.github.io" doesn't exist   <- before the first logout
```

After logging out and back in, enable it once (or run
`scripts/install-extension.sh` again). The extension has no preferences.

The same rule applies after **updating** the files: the shell will not swap
extension code inside a session, so log out and back in after any update.

## Verify

```bash
gnome-shell --version
gnome-extensions list | grep myuzik-pin
gnome-extensions info myuzik-pin@samvivan1.github.io
gsettings get org.gnome.shell enabled-extensions
```

`info` should report `State: ENABLED`. Then, with the app running:

1. Open the lyrics overlay (Ctrl+P toggles it, Esc closes it).
2. Click a window behind it — the overlay must stay visible.
3. Switch workspaces — the overlay must follow.

The extension logs at debug level only, so it is silent in normal use. Extension
errors and warnings appear in the user journal:

```bash
journalctl --user -b -o cat | grep -i myuzik-pin
```

Its own `myuzik-pin:` messages are debug level, so they only show up when the shell
runs with debug logging enabled (`G_MESSAGES_DEBUG=all` in the session environment,
set before logging in — a Wayland session cannot be restarted with
`gnome-shell --replace`). For live JS errors use `Alt+F2`, then `lg` (Looking
Glass).

## Compatibility matrix

Legend: **✅ verified here** = exercised on this machine today;
**⚠️ logic only** = the API and the logic were checked here, but not inside a live
shell session; **📄 documented** = not testable here, no version of that shell
installed.

### Sessions

| GNOME Shell | Session | Status | Basis |
|---|---|---|---|
| 51 | Wayland | ⚠️ logic only | Extension API (Mutter 51/Shell 51 typelibs), metadata validation and the matcher/pin/teardown logic were all checked here; no live session install was performed, so the actual restack of the real overlay is unobserved. |
| 51 | X11 | 📄 documented | Same extension code paths; the X11 branch matches on `WM_CLASS`, which is unverified. No X11 session here. |
| 50 | Wayland / X11 | 📄 documented | APIs used are the GNOME 45+ ESM API; no Mutter/Shell 50 available to test. |
| 49 | Wayland / X11 | 📄 documented | As above. |
| 48 | Wayland / X11 | 📄 documented | As above. |
| 47 | Wayland / X11 | 📄 documented | As above. |
| 46 | Wayland / X11 | 📄 documented | As above. |
| 45 | Wayland / X11 | 📄 documented | As above. |

Nothing older: GNOME 44 and earlier use the legacy `imports.` extension API and
GNOME 45's ESM loader rejects this file. `metadata.json` therefore declares
`["45", …, "51"]` and no lower version.

### API surface used by the extension

Checked by reading the typelibs installed with Mutter 51 / Shell 51 through `gjs`
(see "How this was validated"):

| API | Present in Mutter/Shell 51 | Used for |
|---|---|---|
| `Meta.Window.make_above()` / `unmake_above()` | ✅ present | pin / unpin |
| `Meta.Window.stick()` / `unstick()` | ✅ present | all workspaces |
| `Meta.Window.is_above()` / `is_on_all_workspaces()` | ✅ present | idempotency |
| `Meta.Window.get_title()`, `title` property (`notify::title`) | ✅ present | matching |
| `Meta.Window.get_gtk_application_id()`, `get_wm_class()` | ✅ present | app-id fallback |
| `Meta.Window.get_compositor_private()` | ✅ present | actor lookup |
| `Meta.WindowActor` signal `first-frame` | ✅ present | wait for the title |
| `Shell.Global.get_window_actors()` | ✅ present | scan on enable |
| `Shell.WindowTracker.get_default()`, `get_window_app()` | ✅ present | app lookup |
| `Shell.App.get_id()` | ✅ present | app-id fallback |
| `metadata.json` keys `uuid`, `name`, `description`, `shell-version` | ✅ required by the shell | extension metadata |

`metadata.json` needs no `api-version`. GNOME Shell 51's metadata validation
(`/org/gnome/shell/misc/extensionUtils.js`, extracted from
`/usr/lib/gnome-shell/libshell-51.so`) requires exactly `uuid`, `name`, `description`
and `shell-version`, and reads no `api-version` key; the ESM API version is implied
by `shell-version`. `metadata.version` is only the extension's own version number.

## How this was validated

No live install: the extension was packed and behaviour-tested out of tree.

```bash
# metadata + packing (writes only into target/)
scripts/install-extension.sh --pack-only
# -> target/gnome-extension/myuzik-pin@samvivan1.github.io.shell-extension.zip

# syntax (ESM) — .js is copied to .mjs so the parser is not ambiguous
node --check /tmp/mp-syntax/extension.mjs

# behaviour: 18 checks against fake Meta.Windows in plain gjs
LD_LIBRARY_PATH=/usr/lib/gnome-shell:/usr/lib/x86_64-linux-gnu/mutter-51 \
GI_TYPELIB_PATH=/usr/lib/x86_64-linux-gnu/mutter-51:/usr/lib/gnome-shell \
gjs -m gnome-extension/test-extension.js
# -> 18 passed, 0 failed
```

`gnome-extension/test-extension.js` covers: unrelated windows never pinned, the main
window and untitled app windows never pinned, pin on `notify::title`, pin on
`first-frame`, the app-id fallback (GTK app id, `Shell.WindowTracker` desktop-file
id, and `WM_CLASS`; the main window stays unpinned through all three), idempotency,
an overlay that was already open when the extension was enabled, a window that
vanishes mid-callback, a window that is already disposed at `window-created`, a
compositor that refuses `make_above()`, handler cleanup on `unmanaged`, full
teardown and unpin on `disable()`, repeated enable/disable cycles leaking nothing,
and a repeated `enable()` not double-connecting.

Two things the harness has to fake, both harmless to the production code:

- `global` (the shell's `Shell.Global` instance) — supplied before `enable()`.
- `Shell.WindowTracker.get_default()` — it needs a running shell and **segfaults**
  in plain `gjs` (SIGSEGV observed here), so the harness replaces it with a fake
  tracker to exercise the fallback path. Inside a session the real tracker is used.

The two `LD_LIBRARY_PATH`/`GI_TYPELIB_PATH` entries only make `gi://Shell` loadable
outside the shell; they are not needed inside a session.

## Behaviour when the extension is disabled or missing

| Situation | Behaviour |
|---|---|
| Extension not installed | The overlay opens as a normal window: it can be covered by other windows and stays on the workspace it was opened on. |
| Installed but disabled | Same as above. |
| Disabled while the overlay is pinned | `disable()` calls `unmake_above()` and `unstick()`, so the overlay returns to normal stacking immediately. |
| Enabled while the overlay is open | `enable()` scans existing windows, so the overlay is pinned without reopening it. |
| App restarted | The new overlay window is matched by the same rules; nothing stale is kept. |

## Troubleshooting

**`Extension "myuzik-pin@samvivan1.github.io" doesn't exist` right after installing.**
Expected on Wayland: the shell has not scanned the directory yet. Log out and back
in, then `gnome-extensions enable myuzik-pin@samvivan1.github.io`.

**`gnome-extensions info` says `OUT_OF_DATE`.** `metadata.json` does not list your
shell's major version. Add it to `shell-version` and log out/in. (This machine has
`disable-extension-version-validation` set to `true`, which hides that state.)

**The extension is enabled but the overlay is still covered.** Check the title is
exactly `Myuzik Lyrics` (the app sets it) — for example with Looking Glass
(`Alt+F2`, then `lg`, Windows tab) or `grep` in the journal. A genuinely fullscreen
window still wins; see "What it does not do".

**The overlay lost its pin after a window-menu action.** The pin is applied once
per window. Un-pinning from the window menu is a user action the extension does not
fight; close and reopen the overlay to re-pin it.

**Nothing in the logs.** That is intended: the extension only writes at debug level.

**`gnome-extensions pack` dies with a segmentation fault.** Seen on the
`gnome-extensions` CLI 51.0 here when `--out-dir` does not exist yet (SIGSEGV,
independent of whether the source directory is relative or absolute). Create the
output directory first — `scripts/install-extension.sh` already does
(`mkdir -p target/gnome-extension`).

## Uninstall

```bash
scripts/uninstall-extension.sh
```

Disables, uninstalls, removes the install directory and the packed zip, and clears
the UUID from `org.gnome.shell enabled-extensions` if it lingers. Idempotent, no
sudo. Log out and back in to be sure the shell has dropped it from memory.

## Files

| Path | Purpose |
|---|---|
| `gnome-extension/myuzik-pin@samvivan1.github.io/metadata.json` | UUID, name, `shell-version` 45–51, version |
| `gnome-extension/myuzik-pin@samvivan1.github.io/extension.js` | the extension |
| `gnome-extension/myuzik-pin@samvivan1.github.io/README.md` | short user-facing readme |
| `gnome-extension/test-extension.js` | behaviour tests (gjs, no session needed) |
| `scripts/install-extension.sh` | pack / install / enable |
| `scripts/uninstall-extension.sh` | disable / uninstall / clean up |
