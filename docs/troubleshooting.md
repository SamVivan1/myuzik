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
