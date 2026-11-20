# Troubleshooting

## The app starts but no window appears

GPUI on Linux needs the `wayland` (or `x11`) feature of `gpui_platform`. This repo enables
`features = ["wayland"]` in `Cargo.toml`. Without it, GPUI runs headless.

Check the log for GPU initialization:

```bash
RUST_LOG=info myuzik 2>&1 | grep -E "Compositor|Selected GPU"
```

If Vulkan is unavailable, install `libvulkan-dev` / `vulkan-loader` and a working Mesa driver.

## "Connection failed" in setup

- Verify the server URL includes `http://` or `https://` and no trailing `/rest`.
- Check the server is reachable with `curl ".../rest/ping.view?..."`.
- A wrong username/password returns Subsonic error code 40. The form reports it inline.
- Self-signed TLS certificates are not trusted by rustls by default.

## Myuzik does not appear in Activities search / the app grid

The desktop entry was installed but GNOME cannot load it, so it is missing from
the overview and from search. The usual cause is `Exec` pointing at a bare
command name:

```ini
Exec=myuzik          # GNOME cannot resolve this
```

GNOME Shell runs as a systemd user service and launches apps with the **session**
PATH, which on many systems does not contain `~/.local/bin`. When the executable
in `Exec` cannot be resolved, `GAppInfo` fails to load the entry entirely
(`gio: Unable to load application information for …`), and an entry that cannot
be loaded is never listed or searched.

Check what the shell would actually see:

```bash
# absolute Exec is required
grep '^Exec=' ~/.local/share/applications/io.github.samvivan1.Myuzik.desktop

# the session PATH — note whether ~/.local/bin is in it
systemctl --user show-environment | grep '^PATH='

# the decisive test: the same gio call the shell makes
SP="$(systemctl --user show-environment | sed -n 's/^PATH=//p')"
PATH="$SP" gio launch ~/.local/share/applications/io.github.samvivan1.Myuzik.desktop
```

Fix: reinstall with `./install.sh`, which writes the absolute path
(`Exec=/home/you/.local/bin/myuzik`) and then verifies through `GAppInfo` that
the entry is listed. Installing the binary somewhere already on the session PATH
(e.g. `/usr/local/bin` with sudo) works too, but is not required.

If the entry is listed but you still see nothing, log out and back in once so the
shell rebuilds its app list.

## Dock icon is missing or shows another app

Myuzik's integration is standard: desktop entry `io.github.samvivan1.Myuzik.desktop`,
`Icon=io.github.samvivan1.Myuzik`, `StartupWMClass=io.github.samvivan1.Myuzik`, and a
hicolor icon set in `~/.local/share/icons/hicolor`. The Wayland surface reports the same
app id, and both St and GTK resolve the icon name, so a wrong dock icon is usually the
dock extension, not the app.

Check the active dock:

```bash
scripts/fix-gnome-dock.sh
```

On GNOME 51, the installed Dash2Dock Animated build may list only Shell 45–50 in its
`metadata.json` while still being forced active (version validation disabled). Its icon
code reads Shell internals and can show a missing or another app's icon. Ubuntu Dock is
shipped for the running Shell and is the safe fallback:

```bash
scripts/fix-gnome-dock.sh --use-ubuntu-dock
```

Restore the animated dock later with:

```bash
scripts/fix-gnome-dock.sh --restore-dash2dock
```

If the issue remains on Ubuntu Dock, log out and back in once so GNOME Shell rescans
`~/.local/share/applications` and the icon theme.

## Overlay is not above other windows

- Install and enable the GNOME extension (`scripts/install-extension.sh`), then log out/in.
- Ensure the overlay window title is exactly `Myuzik Lyrics` (the app sets it).
- Fullscreen windows may still cover the overlay; `make_above()` is above normal windows.

## No blur behind the glass

GNOME/Mutter does not implement `ext-background-effect-v1`. Myuzik simulates glass with a
translucent gradient and ambient color; the window margin shows the desktop without blur.

## Codec / playback errors

Install the GStreamer plugin sets:

```bash
sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
```

Use the error toast/`RUST_LOG=debug` to see the pipeline message.

## Credentials

- Default: the Secret Service (`org.freedesktop.secrets`), filed under
  `io.github.samvivan1.Myuzik` / `navidrome`.
- Fallback when no keyring answers: `~/.config/myuzik/credentials`, created with mode `0600`
  from the first byte. `MYUZIK_KEYRING=0` forces this file-only path.
- The setup form says which store was used, and a failed save is reported instead of being
  ignored.
- Keyring calls are bounded to 1.5 s, so a wedged keyring daemon costs a short delay rather
  than a hang.
- To reset: delete the file and the `myuzik` entry in your keyring (or run
  `secret-tool clear service io.github.samvivan1.Myuzik`), then start the app; the setup form
  appears again.

## MPRIS

```bash
playerctl -l
playerctl -p io.github.samvivan1.Myuzik status
playerctl -p io.github.samvivan1.Myuzik metadata
```

If the bus name is missing, check that the app is running and that the session bus is available.

## Logs

`RUST_LOG=debug myuzik` or `RUST_LOG=info myuzik`. Logs redact Subsonic tokens.
