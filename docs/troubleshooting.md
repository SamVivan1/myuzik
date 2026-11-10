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

- Preferred: Secret Service (`org.freedesktop.secrets`).
- Fallback: `~/.config/myuzik/credentials` (mode 0600).
- To reset: delete that file and run the app; the setup form appears again.

## MPRIS

```bash
playerctl -l
playerctl -p io.github.samvivan1.Myuzik status
playerctl -p io.github.samvivan1.Myuzik metadata
```

If the bus name is missing, check that the app is running and that the session bus is available.

## Logs

`RUST_LOG=debug myuzik` or `RUST_LOG=info myuzik`. Logs redact Subsonic tokens.
