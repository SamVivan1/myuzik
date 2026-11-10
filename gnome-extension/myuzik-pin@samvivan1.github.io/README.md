# Myuzik Pin

A GNOME Shell extension that keeps Myuzik's pinned lyrics window (`Myuzik Lyrics`)
above other windows and on all workspaces.

Myuzik's overlay is a borderless second window. Wayland does not let a normal
application raise itself above other windows, so the app relies on this extension
to call Mutter's `make_above()` and `stick()` for that one window. Nothing else is
affected: the main Myuzik window is never pinned, and no other application's
windows are touched.

Full details, the compatibility matrix and the validation notes:
[`docs/gnome-extension.md`](../../docs/gnome-extension.md).

## Requirements

- GNOME Shell 45–51 (ESM extension API).
- `gnome-extensions` (ships with `gnome-shell`).
- No root: the extension is installed per user.

## Install

From the repository root:

```bash
scripts/install-extension.sh
```

That packs the extension, installs it into
`~/.local/share/gnome-shell/extensions/myuzik-pin@samvivan1.github.io/` and tries to
enable it. To only validate and pack (useful in CI, changes nothing):

```bash
scripts/install-extension.sh --pack-only
```

## Enable

GNOME Shell only scans extension directories when a session starts. On Wayland the
first install is therefore invisible to the running shell, and enabling fails with
`Extension "myuzik-pin@samvivan1.github.io" doesn't exist`.

Log out and back in, then:

```bash
gnome-extensions enable myuzik-pin@samvivan1.github.io
```

Do this after every update of `extension.js` as well: the shell does not swap
extension code inside a running session.

## Verify

```bash
gnome-extensions info myuzik-pin@samvivan1.github.io   # State: ENABLED
```

Then open the lyrics overlay in Myuzik and click another window behind it; the
overlay should stay visible, and it should follow you across workspaces.

The extension is silent in normal use (debug-level logging only).

## Troubleshooting

- **`doesn't exist` after install** — log out and back in (see Enable).
- **`OUT_OF_DATE`** — your shell's major version is missing from `shell-version` in
  `metadata.json`; add it and log out/in.
- **Overlay still gets covered** — a genuinely fullscreen window can still be above
  it; this is a Mutter limitation, not a bug in the extension.
- **Nothing in the logs** — intended (debug level only). Extension errors show up in
  `journalctl --user -b -o cat | grep -i myuzik-pin`; `Alt+F2`, then `lg`, for live
  JS errors.

## Uninstall

```bash
scripts/uninstall-extension.sh
```

Idempotent, no sudo: disables, uninstalls, removes leftovers and clears the UUID
from the enabled-extensions list if it lingers. Log out and back in afterwards.

## How it decides

A window is pinned when its title is exactly `Myuzik Lyrics`, or — as a fallback —
when it belongs to the Myuzik app id `io.github.samvivan1.Myuzik` and is not the
main window (titled `Myuzik`). The title often only exists once the first frame has
been drawn, so the extension waits for `first-frame` / `notify::title` before
matching, and it rescans already-open windows when it is enabled. Each window is
pinned exactly once; disabling the extension un-pins everything it pinned.
