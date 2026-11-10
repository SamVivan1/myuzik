# Setup

## 1. Navidrome

You need a reachable Navidrome server and a Subsonic user. Confirm:

```bash
curl -s "https://your-server/rest/ping.view?u=USER&t=bad&s=salt&v=1.16.1&c=myuzik&f=json"
```

A `subsonic-response` JSON body means the server is reachable. The form in Myuzik uses token auth
(MD5 of password + random salt), so the password never travels in plain text.

## 2. First launch

`cargo run --release`, or `./install.sh` once and then `myuzik` / the Activities entry.

The setup form asks for:

- **Server URL** — `https://music.example.com` (http is accepted for LAN servers).
- **Username**
- **Password**

Tab moves between fields; Enter or **Connect** tests the server (`ping`) and saves on success.
Ctrl+V pastes into the focused field.

The password is kept in the Secret Service (`org.freedesktop.secrets`: GNOME Keyring, KWallet)
when that is enabled, and otherwise in a `0600` file at `~/.config/myuzik/credentials`. The
setup form tells you which one was used.

The Secret Service backend is **opt-in** for now, with `MYUZIK_KEYRING=1`. Reading the keyring
is a D-Bus round trip, and the shell currently loads the credential on the main thread during
startup, so enabling it by default could stall the first frame on a slow or hung secret
service. Until those lookups move off the startup path, the default is the `0600` file.

## 3. GNOME pin extension (optional)

`./install.sh` already does this; to (re)install just the extension:

```bash
scripts/install-extension.sh
```

Log out and back in (GNOME only scans new extensions at session start), then enable **Myuzik Pin**.
The extension watches for windows titled `Myuzik Lyrics`, calls `make_above()` and `stick()`.

## 4. Manual configuration

If you prefer editing files, `~/.config/myuzik/config.toml` accepts the keys documented in the
README. The app shows the setup screen whenever no usable client can be created.
