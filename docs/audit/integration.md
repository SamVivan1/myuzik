# Audit Integrasi & Batas Sistem — Myuzik

| | |
|---|---|
| Repo | `/home/samvivan/Documents/Myuzik`, branch `master` |
| Commit diaudit | `e096e20` (`fix(tray): make pin/unpin work without a main window`) |
| Tanggal audit | 2026-11-11 (Asia/Jakarta) |
| Auditor | integration-auditor (audit statis) |
| Sifat audit | **baca-saja**: tidak ada perubahan pada `src/`, `tests/`, `gnome-extension/`, atau docs lain |
| Metode | baca sumber penuh pada berkas scope, `grep` untuk bukti baris, MCP `codebase-memory` (`index_status`, `get_file_outline` tray.rs/provider.rs/client.rs, `check_index_coverage` 30 path), cross-check sumber crate (`ureq-3.5.1`, `tracing-subscriber-0.3.23`, `directories-6.0.0`) |
| Tidak dijalankan | aplikasi/GUI, DBus sesi, GNOME Shell, GStreamer, `install.sh`, network apa pun, `cargo check`/`build` |

Konvensi: setiap klaim perilaku wajib punya bukti `file:line`. Kolom **Bukti** hanya berisi rujukan kode; klaim yang butuh runtime ditandai eksplisit **perlu verifikasi runtime**.

---

## 1. Ringkasan

**Total temuan: 40 — P0: 2 · P1: 11 · P2: 27.**

Dua P0 (keduanya terlihat pengguna pada alur normal):

1. **INT-01 — Kredensial.** Dokumentasi (setup/troubleshooting/README) menjanjikan penyimpanan password lewat Secret Service (`keyring`), tetapi kode **tidak pernah** memakainya: password ditulis plaintext ke `credentials`. Dependensi `keyring` dideklarasikan tetapi menganggur. Ini klaim keamanan yang salah + jendela izin 0644 saat penulisan.
2. **INT-02 — Tray/overlay dead-end.** Predikat "sembunyikan ikon tray" memakai *jumlah semua window* (`app.windows().is_empty()`), bukan "jendela utama ada". Dengan overlay lirik terbuka (fitur utama) ikon tray selalu `Passive`, dan `TrayCommand::Show` gagal senyap karena hanya meng-`activate_window()` handle jendela utama yang sudah dihapus → proses hidup tanpa jendela dan tanpa ikon.

**P1 (11)** — semuanya terverifikasi dari kode:

- INT-03 MPRIS `Volume` basi: di-set sekali ke `0.8` saat start, tidak pernah disinkronkan dengan `config.volume`.
- INT-04 MPRIS tidak mengekspos `LoopStatus`/`Shuffle` walau queue punya `RepeatMode`/`shuffle`.
- INT-05 Tidak ada re-registrasi ke `StatusNotifierWatcher` → ikon hilang permanen bila shell/ekstensi muncul setelah app.
- INT-06 Tidak ada single-instance guard → dua instance = dua ikon tray, dua penulis config/kredensial, MPRIS kedua mati senyap.
- INT-07 Panic di thread player (`expect("bus")`) mematikan audio tanpa jejak UI.
- INT-08 Config rusak → reset senyap ke default tanpa backup, lalu ditimpa pada toggle pertama; tidak ada skema/versi/migrasi.
- INT-09 Kredensial ditulis dulu baru `chmod 0600` (jendela izin umask), kegagalan disenyapkan `let _ =`.
- INT-10 Error dipetakan ke `Debug`/teks mentah di UI (`Server("HTTP 500")`, `Message(...)`).
- INT-11 Error lirik ditelan: 401/timeout tampil sama dengan "tidak ada lirik".
- INT-12 Tidak ada retry/backoff; `429` tidak dibedakan dari `5xx`.
- INT-13 Cache lirik/cover tanpa invalidasi dan tanpa prune (`LyricsCache::prune` tidak pernah dipanggil) → data basi selamanya + disk tumbuh tanpa batas.

**P2 (27)** memuat: kontrak MPRIS lain (`Seeked`, metadata bisa regress), ketahanan tray (registrasi gagal senyap, `Tray::start` memblokir 10 s, worker panic tanpa recovery), hygiene `expect` di jalur startup, salt nol, token di query URL tanpa redaksi di batas player, tidak ada logout, `client.lyrics` menelan status error + cabang kode 70 mati, `check_status` mencocokkan teks bukan `error.code`, deteksi TLS yang rapuh, `cover_bytes` memotong 8 MB, `NotFound` lirik tidak di-cache + request LRCLIB duplikat, preferensi lirik tidak masuk key cache, payload synced Navidrome tanpa validasi + `offset` OpenSubsonic diabaikan, edge case LRC, coupling ekstensi GNOME lewat string judul tanpa versi, `install.sh --prefix DIR` ditolak, perintah docs tanpa `./`, 5 key `overlay_*` tidak terdokumentasi, `logs/` tak pernah ditulis, hook `MYUZIK_WINDOW_TEST` ikut release, `ashpd`/`keyring` menganggur dan `follow_os_accent` inert, serta tidak ada otomatisasi kontrak DBus/MPRIS.

---

## 2. Tabel temuan

### 2.1 P0 — blocker

| ID | Sev | file:line | Skenario | Dampak | Rekomendasi |
|---|---|---|---|---|---|
| INT-01 | P0 | `src/config.rs:114-126`; `src/config.rs:95-101`; `Cargo.toml:31`; `docs/setup.md:25-26`; `docs/troubleshooting.md:84-85`; `README.md:67-68` | Pengguna menyelesaikan form setup dan membaca docs yang menyatakan password disimpan di Secret Service. `grep -rn keyring src/` → **kosong**; `save_password` selalu `std::fs::write` ke `$XDG_CONFIG_HOME/myuzik/credentials` (plaintext) lalu `chmod 0600`. | Password Navidrome tersimpan plaintext di disk; docs/README menjanjikan enkripsi/Secret Service yang tidak ada. Siapa pun yang bisa membaca file user (backup, sync folder, `sudo`, proses lain sebagai user yang sama) mendapat password asli. `keyring` 4.2 ikut ter-compile tanpa dipakai. | Implementasikan `keyring::Entry` (`secret-service`) dengan fallback eksplisit *dan* beri tahu pengguna saat fallback dipakai; atau hapus klaim Secret Service dari `docs/setup.md`, `docs/troubleshooting.md`, `README.md` dan buang dependensi `keyring` dari `Cargo.toml`. Tambahkan test yang mengunci perilaku ini. |
| INT-02 | P0 | `src/main.rs:124-127`; `src/main.rs:128-131`; `src/main.rs:142-156`; `src/overlay/mod.rs:1000`; `src/ui/shell.rs:574-646` | Pengguna membuka overlay lirik (Ctrl+P / menu tray "Pin lyrics"), lalu menutup jendela utama (close-to-tray, default `close_to_tray=true`, `src/config.rs:48`). Supervisor 400 ms menghitung `app.windows().is_empty()` — overlay adalah window pada `App` yang sama (`cx.open_window` di `src/overlay/mod.rs:1000`) → `empty == false` → `tray.set_visible(false)` sehingga ikon ber-`Status=Passive` (tidak digambar). Klik `Activate`/menu "Show" masuk cabang `else` dan hanya `handle.update(... activate_window())` pada handle jendela utama yang sudah dihapus; error dibuang `let _ =` (`src/main.rs:152-155`). | Jendela utama tidak bisa dibuka kembali lewat tray (tray-nya tak terlihat), dan dengan `close_to_tray=false` app juga tidak quit (syarat `empty && !close_to_tray`, `src/main.rs:128`) → proses hidup tanpa jalan keluar selain `kill` atau menebak menutup overlay (Esc). Setiap percobaan "buka lagi" dari Activities alih-alih memulihkan window cenderung membuat instance kedua (lihat INT-06). | Simpan `AnyWindowHandle` jendela utama dan uji itu (mis. `main_window.is_none()`/`handle.update().is_err()`), bukan jumlah window. Pada `TrayCommand::Show`, bila handle utama mati → buka window baru dengan `main_window_options` (jalur yang sudah ada di `src/main.rs:140-150`) alih-alih `activate_window()`. Tambahkan test headless: overlay terbuka + window utama dihapus → tray `Active` dan `Show` membuka window. |

### 2.2 P1 — penting

| ID | Sev | file:line | Skenario | Dampak | Rekomendasi |
|---|---|---|---|---|---|
| INT-03 | P1 | `src/platform/mpris.rs:89`; `src/platform/mpris.rs:114-116`; `src/ui/shell.rs:91-94`; `src/ui/shell.rs:2148-2150`; `src/ui/shell.rs:2193-2195` | `start_mpris` men-`set_volume(0.8)` sekali setelah `build()`, tanpa membaca config. `Shell::new` menerapkan `config.volume` ke `PlayerCommand::SetVolume` (baris 91-94) tetapi **tidak pernah** memanggil `MprisHandle::update_volume`. `update_volume` hanya dipanggil dari dua handler UI (keyboard/mouse). | `Volume` MPRIS selalu 80% setiap start walau `config.toml` berisi nilai lain (mis. 0.3). GNOME media widget/`playerctl volume` menampilkan nilai salah; menaikkan slider dari desktop melompat ke 0.8→nilai baru (bunyi melonjak). Nilai dibiarkan basi sampai pengguna menyentuh volume di dalam app. | Kirim volume awal setelah MPRIS siap: teruskan `config.volume` ke `start_mpris` (`src/main.rs:62`) atau panggil `mpris_handle.update_volume(config.volume)` di `Shell::new`. Hapus `set_volume(0.8)` hardcoded; tambahkan test yang mengunci sinkronisasi awal. |
| INT-04 | P1 | `src/platform/mpris.rs:40-57`; `src/player/queue.rs:10-23`; `src/player/queue.rs:56-61`; `src/platform/mpris.rs:98-99` | Queue punya `RepeatMode` dan `shuffle` (`queue.rs:10,20-21`), tetapi `Player::builder` tidak pernah memberi `can_set_loop_status`/`can_set_shuffle` dan tidak ada `set_loop_status`/`set_shuffle` yang dipanggil. `set_can_go_next(true)`/`set_can_go_previous(true)` dipaksa `true` setiap update metadata. | Fitur repeat/shuffle tidak bisa dikendalikan dari GNOME/`playerctl`; tombol Next/Previous tetap aktif walau queue kosong (klik tidak berefek). Kontrak MPRIS melaporkan kemampuan yang tidak lengkap/tidak akurat. | Petakan `RepeatMode`→`LoopStatus` (None/TrackList/Playlist) dan `shuffle`→`Shuffle`; kirim lewat `MprisCommand` saat berubah, dan hitung `can_go_next/previous` dari `queue.remaining()`. |
| INT-05 | P1 | `src/platform/tray.rs:777-781`; `src/platform/tray.rs:886-895`; `src/platform/tray.rs:791-802` | `Service::start` memanggil `RegisterStatusNotifierItem` **satu kali**. Tidak ada langganan `NameOwnerChanged` untuk `org.kde.StatusNotifierWatcher`, tidak ada `StatusNotifierHostRegistered`, tidak ada retry. | Bila app start sebelum watcher/host ada (kasus umum: ekstensi AppIndicator di-enable setelah login, atau GNOME Shell di-restart, atau app autostart lebih awal), ikon tray **tidak akan muncul sampai app di-restart**. Hanya `tracing::warn` (yang default tak terlihat, lihat INT-17). | Berlangganan `org.freedesktop.DBus.NameOwnerChanged` untuk watcher + retry terbatas (mis. 5×/30 s) dan panggil ulang `register_with_watcher`; log via sinyal yang sama seperti `Service::emit`. |
| INT-06 | P1 | `src/main.rs:31-255`; `src/platform/tray.rs:772-775`; `src/config.rs:140-148`; `src/config.rs:114-126`; `src/platform/mpris.rs:40-41` | Tidak ada lock file, tidak ada `request_name` untuk nama aplikasi, tidak ada pemeriksaan instance yang sudah jalan. Nama tray memakai PID (`tray.rs:772`), jadi dua instance = dua nama tray. | Dua instance: dua ikon tray, dua player/GStreamer (audio dobel), scrobble ganda, dua penulis `config.toml` + `credentials` (last-write-wins, pengaturan saling menimpa), MPRIS instance kedua kemungkinan gagal memiliki nama yang sama dan hanya `eprintln!` (`mpris.rs:53-56`). | Tambahkan single-instance: `request_name("io.github.samvivan1.Myuzik")` (atau `flock` di `$XDG_RUNTIME_DIR`) sebelum `app.run`; instance kedua mengirim `Show` ke instance pertama lalu exit. |
| INT-07 | P1 | `src/player/engine.rs:70`; `src/player/engine.rs:102`; `src/player/engine.rs:51-56` | `bus().expect("bus")` dan `add_watch(...).expect("bus watch")` berjalan di thread player. Thread panic → tidak ada `PlayerEvent` apa pun. Bandingkan jalur `gst::init()` gagal yang mengirim `PlayerEvent::Error` (baris 51-56) dan ditampilkan sebagai toast (`src/ui/shell.rs:335-344`). | Audio mati total tapi UI tetap "normal": posisi berhenti, tidak ada toast, pengguna tidak tahu apa yang rusak. | Ganti `expect` dengan pengiriman `PlayerEvent::Error(PlayerError::Message(...))` + `return`, sama seperti jalur inisialisasi lain. |
| INT-08 | P1 | `src/main.rs:41-47`; `src/config.rs:132-138`; `src/config.rs:16-37`; `src/ui/shell.rs:1100-1102`; `src/ui/shell.rs:1067-1068` | `config.toml` rusak/setengah tertulis → `main.rs` hanya `tracing::warn` (tak terlihat, INT-17) lalu memakai `AppConfig::default()`; `Shell` menampilkan form Setup seolah instalasi baru. Setiap toggle (`persist_config`) menulis ulang file → isi lama hilang tanpa backup. `AppConfig` tidak punya field versi; `#[serde(default)]` membuat key yang di-rename/dihapus diabaikan senyap. | Pengaturan pengguna (server, volume, offset lirik, geometri overlay) "hilang" tanpa penjelasan, dan file aslinya ditimpa sehingga tidak bisa dipulihkan. Tidak ada jalur migrasi antar versi config. | Sebelum fallback: rename file ke `config.toml.corrupt-<timestamp>` dan beri tahu pengguna. Tambahkan `version: u32` + fungsi migrasi, dan `deny_unknown_fields` (atau peringatan) agar typo terdeteksi. |
| INT-09 | P1 | `src/config.rs:114-126`; `src/ui/shell.rs:1068`; `src/ui/shell.rs:654`; `src/ui/shell.rs:1101` | `save_password` menulis file dengan `std::fs::write` (izin default umask, lazim 0644) lalu baru `set_permissions(0o600)`. Bila `chmod` gagal, fungsi mengembalikan `Err` tetapi file sudah berisi password. Pemanggil membuang hasilnya (`let _ =`). | Jendela waktu file password ber-izin `0644` (dapat dibaca user lain pada sistem multi-user). Kegagalan menyimpan kredensial tidak dilaporkan ke pengguna (setup "berhasil" tetapi password hilang saat restart). | Buat file dengan `OpenOptions::new().mode(0o600).create_new(true)` (atau `tempfile` + `chmod` + `rename`), dan tampilkan error penyimpanan di form setup. |
| INT-10 | P1 | `src/ui/shell.rs:1091`; `src/ui/shell.rs:693`; `src/ui/shell.rs:338`; `src/navidrome/error.rs:4-20` | Error tidak dipetakan ke bahasa pengguna: setup `format!("Connection failed: {e}")`, home `format!("{:?}", e)`, toast `format!("Playback error: {e:?}")`. `Debug` dari `NavidromeError` menghasilkan `Unauthorized`, `Server("HTTP 500")`, `Network("timeout")`; `PlayerError` menghasilkan `Message("gstreamer init failed")`. | Pesan yang dilihat pengguna berupa representasi Rust/HTTP mentah, tanpa saran tindakan; sulit membedakan "password salah", "server mati", dan "sedang rate-limited". | Tambahkan `NavidromeError::user_message(&self) -> &str`/`Display` yang ramah + `hint`, dan pakai di ketiga tempat; jangan pernah menampilkan `Debug` ke UI. |
| INT-11 | P1 | `src/ui/shell.rs:874-880`; `src/lyrics/provider.rs:141`; `src/lyrics/provider.rs:242-247`; `src/lyrics/provider.rs:258-267` | `p_bg.fetch(&t_bg).unwrap_or(LyricsStatus::NotFound)` — semua `Err` (401 karena password berganti, timeout, offline) dipetakan ke `NotFound`. Di dalam provider, kegagalan LRCLIB juga disenyapkan `.ok()?`/`let Ok(...) else { return Vec::new() }`. | Setelah kredensial kedaluwarsa atau server/LRCLIB mati, pengguna melihat "No lyrics" — bukan pesan auth/jaringan. Penyebab sebenarnya tidak pernah muncul, dan app tidak pernah menawarkan login ulang (lihat INT-24). | Pakai varian `LyricsStatus::Error(String)` yang sudah ada (`src/lyrics/model.rs:16`) atau teruskan `Result` ke UI: `Unauthorized` → arahkan ke Setup; `Network`/timeout → "cannot reach lyrics service"; hanya 404/`NotFound` asli → "No lyrics". |
| INT-12 | P1 | `src/navidrome/error.rs:34-52`; `src/navidrome/client.rs:51-57`; `src/navidrome/client.rs:86-92` | Tidak ada retry/backoff di mana pun; `429` jatuh ke `NavidromeError::Server(format!("HTTP {code}"))`. `Retry-After` tidak dibaca; 429, 500, 502, 503 tidak dibedakan. Timeout sudah dibatasi (8 s connect / 15 s respons / 25 s global) tetapi setiap aksi pengguna langsung memicu request baru (search, scroll, radio, lyrics). | Saat Navidrome membatasi laju (reverse proxy/Navidrome rate limit), pengguna melihat "server: HTTP 429" dan app terus menembak → pembatasan berlanjut; tidak ada jeda cerdas atau pemberitahuan "coba lagi dalam N detik". | Petakan `429` ke varian `RateLimited{retry_after}`; backoff eksponensial + jitter untuk 429/5xx pada jalur non-interaktif (radio, lyrics, cover), dan pesan "Server sibuk, mencoba lagi…" di UI. |
| INT-13 | P1 | `src/lyrics/cache.rs:47-62`; `src/lyrics/cache.rs:85-118`; `src/lyrics/provider.rs:120-125`; `src/ui/shell.rs:787-788`; `src/ui/shell.rs:3801-3804` | Cache lirik dibaca lebih dulu dan bila ada langsung dipakai (`fetch` baris 121-125) tanpa TTL/ETag/cek perubahan. `LyricsCache::prune` **tidak punya pemanggil** (`grep -rn "prune(" src/` hanya definisi). Cover ditulis ke `paths.covers` tanpa prune apa pun. Hanya versi format yang bisa membatalkan (`FORMAT_VERSION = "v2"`, `cache.rs:31`). | Lirik yang diperbaiki/dihapus di server atau LRCLIB tetap tampil basi selamanya. `~/.cache/myuzik/lyrics/` dan `~/.cache/myuzik/covers/` tumbuh tanpa batas (satu file per track/cover yang pernah dilihat) — pada library besar ini jutaan byte/hari tanpa batas. | Tambahkan TTL (mis. 30 hari) atau `If-None-Match`/hash; panggil `prune` saat startup (mis. 5000 file / 50 MB untuk lirik, 2000 file / 200 MB untuk cover); simpan `fetched_at` di `CacheEntry`. |

### 2.3 P2 — polish / ketahanan

| ID | Sev | file:line | Skenario | Dampak | Rekomendasi |
|---|---|---|---|---|---|
| INT-14 | P2 | `src/platform/mpris.rs:110-113`; `src/ui/shell.rs:480-484` | Seek dari UI app (klik progress bar, tombol panah) hanya mengirim `PlayerCommand::Seek`; MPRIS hanya menerima `set_position` dari tick posisi, `Seeked` tidak pernah di-emit. | Klien MPRIS yang mengandalkan sinyal `Seeked` (bukan polling) menampilkan posisi yang meleset setelah seek dari dalam app. | Emit `player.seeked(...)` saat seek berasal dari app (beda dari seek yang diminta klien MPRIS). |
| INT-15 | P2 | `src/ui/shell.rs:773`; `src/ui/shell.rs:812-814`; `src/ui/shell.rs:860-883` | `cache_track_cover` mengirim `mh.update_metadata(&track, ...)` dengan `track` yang di-capture saat cover diminta; tidak ada pemeriksaan generasi seperti `lyrics_generation` pada jalur lirik. | Saat track di-skip cepat, metadata (judul/artist/artUrl) track sebelumnya bisa menimpa metadata track yang sedang diputar di media widget. | Tambahkan `metadata_generation`/cek `player_state.current.id == track.id` sebelum mengirim, pola yang sudah dipakai untuk lirik (`shell.rs:860-864, 881-883`). |
| INT-16 | P2 | `src/platform/mpris.rs:32-36`; `src/platform/mpris.rs:53-56`; `src/platform/mpris.rs:176-178`; `src/main.rs:62` | Kegagalan MPRIS memakai `eprintln!` (bukan `tracing::warn!`) dan `start_mpris(...).ok()` membuang `Err`. `MprisHandle::quit()` tidak pernah dipanggil. | Kegagalan MPRIS tidak masuk log terstruktur/journal, tidak terlihat saat diluncurkan dari desktop; MPRIS thread tidak pernah dihentikan secara eksplisit. | Ganti `eprintln!`→`tracing::error!`, laporkan `Err` di `main.rs`, dan panggil `mpris_handle.quit()` pada jalur quit. |
| INT-17 | P2 | `src/main.rs:32-34`; `src/main.rs:44`; `src/main.rs:199`; `src/platform/tray.rs:780` | `EnvFilter::from_default_env()` memakai default directive `ERROR` (terverifikasi di `tracing-subscriber-0.3.23/src/filter/env/mod.rs:289-294`), sehingga semua `warn!`/`info!` hilang bila `RUST_LOG` tidak diset. Tidak ada logging ke file. | Peluncuran dari menu desktop: pesan "system tray unavailable", "config could not be read", "StatusNotifierWatcher refused the item" tidak pernah terlihat; tidak ada cara meminta log setelah kejadian. | Set default directive `INFO` untuk target `myuzik` dan tulis juga ke `paths.logs/myuzik.log` (rotasi sederhana); atau tampilkan banner diagnostik di UI saat tray/config gagal. |
| INT-18 | P2 | `src/platform/tray.rs:43`; `src/platform/tray.rs:146`; `src/main.rs:106` | `Tray::start` memblokir pemanggil hingga `START_TIMEOUT` 10 s (`recv_timeout`) dan dipanggil dari closure `app.run` (jalur UI). | DBus lambat/macet menahan inisialisasi UI hingga 10 detik pada startup. | Turunkan timeout (1–2 s) dan/atau pindahkan `Tray::start` ke `background_executor` lalu `attach_tray` saat siap. |
| INT-19 | P2 | `src/platform/tray.rs:504-506`; `src/platform/tray.rs:491-502`; `src/platform/tray.rs:563-571`; `src/main.rs:106-111` | `Scroll` tidak melakukan apa pun (delta dibuang), `SecondaryActivate`/`ContextMenu`/`XAyatanaSecondaryActivate` semuanya → `TrayCommand::Show`, `IconName` kosong (hanya `IconPixmap`). Ikon mulai `visible=true` lalu di-`Passive` setelah `attach_tray`. | Perilaku scroll/klik kanan berbeda dari aplikasi SNI lain (tidak menyesuaikan volume, tidak membuka menu pada host tanpa DBusMenu); icon bisa berkedip `Active` sesaat saat start. | Implementasikan scroll → `SetVolume`, `ContextMenu` → kirim event menu, dan mulai dengan `visible=false` bila jendela sudah ada (set state awal sebelum registrasi). |
| INT-20 | P2 | `src/platform/tray.rs:897-914`; `src/platform/tray.rs:210-214`; `src/platform/tray.rs:615-619` | Worker thread menjalankan `Service::serve`; bila thread panic (mis. Mutex poison lain, bug D-Bus), `Tray::send` hanya `tracing::debug!` dan `Tray::start` tidak memantau status worker. | Semua update tray (judul, label Play/Pause) dan semua command tray hilang senyap; ikon tak bereaksi. Riwayat repo menunjukkan kelas bug ini sudah pernah terjadi (`c674233 fix(tray): stop the double-lease panic that killed every tray command`). | Pantau `JoinHandle` (simpan status + restart sekali) dan naikkan level log kegagalan pengiriman ke `warn` dengan dedup. |
| INT-21 | P2 | `src/main.rs:36-37`; `src/main.rs:95`; `src/overlay/mod.rs:1003`; `src/ui/shell.rs:187`; `src/ui/shell.rs:1071` | `expect` pada jalur startup/UI: `paths.ensure()`, `open_window`, `open_overlay`, serta `expect("client already validated")`/`expect("validated client")` di dalam task. | Crash tanpa dialog bila direktori cache tak bisa dibuat (disk penuh/read-only), kompositor menolak window, atau URL config berubah di antara dua konstruksi client. | Konversi ke `Result` + tampilkan layar error yang bisa ditindaklanjuti; khusus `open_overlay` kembalikan `Result` dan jatuhkan toast alih-alih panic. |
| INT-22 | P2 | `src/navidrome/auth.rs:3-10` | `random_salt` membuka `/dev/urandom` dan mengabaikan kegagalan (`if let Ok`) serta `let _ = f.read_exact(...)`; buffer nol tetap dipakai. | Salt jatuh ke 32 karakter nol (deterministik) pada sistem/chroot tanpa `/dev/urandom`, atau sebagian nol bila `read_exact` gagal → token Subsonic bisa diprediksi/di-replay. | Pakai `getrandom`/`std::random` (atau `rand` dengan `OsRng`) dan propagasikan kegagalan sebagai error request. |
| INT-23 | P2 | `src/navidrome/client.rs:309-334`; `src/navidrome/client.rs:336-347`; `src/player/engine.rs:143-147`; `src/platform/logging.rs:5-24` | `stream_url`/`cover_url` menanamkan `u`, `t`, `s` di query string dan string itu diserahkan ke GStreamer sebagai URI. `redact_url` hanya dipakai pada jalur error (`src/navidrome/error.rs:22-53`), tidak pada string URI yang keluar ke player. | Bila GStreamer/log sistem/proxy mencatat URI (mis. `GST_DEBUG=4`, log `playbin`, journal dari proses lain), token+salt bocor — cukup untuk replay selama salt itu. | Redaksi di batas player (jangan pernah log URI penuh), atau pindahkan kredensial ke header `Authorization`/cookie via `extra-headers` playbin. |
| INT-24 | P2 | `src/config.rs:128-130`; `src/config.rs:150-152`; `docs/troubleshooting.md:86` | `delete_password` dan `config_exists` tidak punya pemanggil (`grep` kosong) → tidak ada "logout"/"ganti akun" di UI. Satu-satunya cara adalah menghapus file manual seperti di docs. | Pengguna dengan password yang berubah harus keluar dari app dan menghapus file lewat terminal; tidak ada tombol "Disconnect"/"Sign out" di Settings. | Expose `delete_password` + reset `config.username` sebagai aksi Settings ("Disconnect"), lalu arahkan ke `Route::Setup`. |
| INT-25 | P2 | `src/navidrome/client.rs:393-395`; `src/navidrome/client.rs:404-408` | `client.lyrics` mengembalikan `Ok(None)` untuk setiap `status != "ok"`, sehingga cabang `err.code == 70 → Unsupported` di baris 404-408 tidak pernah tercapai (dead code). 401 di endpoint lirik juga jadi "tidak ada lirik". | Kegagalan auth/lisensi lirik tidak terbedakan dari "track tanpa lirik"; provider lalu menghabiskan waktu di LRCLIB. | Panggil `Self::check_status(&resp)` sebelum `Ok(None)`, dan pindahkan penanganan kode 70 ke atas pemeriksaan status. |
| INT-26 | P2 | `src/navidrome/client.rs:103-126`; `docs/troubleshooting.md:20` | `check_status` mengklasifikasi dengan `lower.contains("...")` pada `message`, bukan pada `error.code`. Kode 41 ("Token authentication not supported for LDAP users") mengandung "not supported" → `Unsupported`; pesan server yang dilokalisasi/diubah → salah kelas. Docs menyebut "error code 40" padahal kode tidak dibaca. | Salah klasifikasi: pengguna LDAP melihat "unsupported" padahal solusinya password auth; pesan Navidrome versi lain bisa jatuh ke `Server(msg)` mentah. | Cocokkan `e.code` (40/41/50/70, dst.) dan pakai `message` hanya sebagai detail; selaraskan docs. |
| INT-27 | P2 | `src/navidrome/error.rs:42-49`; `src/navidrome/error.rs:50` | Varian `Tls` dideteksi lewat substring `"tls"` pada `io::Error`; error sertifikat rustls umumnya berbunyi "invalid peer certificate"/"invalid certificate" tanpa kata "tls" → masuk `Network`. `connect` vs `read` timeout sama-sama menjadi `Network("timeout")`. | Pengguna dengan sertifikat self-signed (kasus yang didokumentasikan `docs/troubleshooting.md:21`) mendapat pesan "network: …" yang menyesatkan, bukan pesan TLS. Varian `Tls` praktis tak terjangkau. | Deteksi lewat tipe error rustls (`ureq::tls`) atau `io::ErrorKind::InvalidData` + teks kandidat ("certificate", "handshake", "tls"); tambahkan varian `Timeout{ phase }`. |
| INT-28 | P2 | `src/navidrome/client.rs:349-360` | `cover_bytes` memotong body di 8 MiB dengan `.take(8 * 1024 * 1024)` dan mengembalikan `Ok` atas data terpotong. | Sampul > 8 MiB (artwork resolusi tinggi / server yang mengabaikan `size`) menghasilkan JPEG/PNG cacat → decode gagal/placeholder tanpa penjelasan. | Deteksi `read == limit` → kembalikan `Err(Protocol("cover too large"))` atau turunkan `size` dan coba lagi. |
| INT-29 | P2 | `src/lyrics/provider.rs:170-171`; `src/lyrics/provider.rs:175-193`; `src/lyrics/provider.rs:209-220` | `store()` mengabaikan `LyricsStatus::NotFound` (`_ => {}`), jadi `NotFound` tidak pernah dipersistensi; `cache.read` hanya mengembalikan `NotFound` bila ada file dengan isi kosong (yang tidak pernah ditulis). Loop `fetch_lrclib` memanggil `lrclib_get` sekali, mengevaluasi, lalu memanggilnya **lagi** di baris 218; `candidates.pop()` di baris 215 selalu `None` (sisa refactor). | Setiap pemutaran track tanpa lirik mengulang 1 request Navidrome + hingga 4 request LRCLIB (2 di antaranya duplikat) — lambat dan membebani layanan publik; UI menunggu. | Cache `NotFound` dengan TTL pendek (mis. 7 hari); hapus panggilan ganda dan `candidates.pop()` mati; simpan hasil `lrclib_get` ke `candidates` sekali. |
| INT-30 | P2 | `src/lyrics/cache.rs:33-45`; `src/ui/shell.rs:1074-1078` | Key cache = versi format + artist + title + album + durasi(detik); `prefer_navidrome_lyrics` tidak termasuk. Mengubah setting di Settings tidak membatalkan entri lama. | Setelah pengguna mengganti preferensi sumber lirik, track yang sudah pernah diputar tetap memakai hasil lama; perubahan setting tampak tidak berfungsi. | Masukkan preferensi ke `FORMAT_VERSION`/key, atau invalidasi cache lirik saat setting diubah. |
| INT-31 | P2 | `src/lyrics/provider.rs:130-136`; `src/lyrics/provider.rs:345-350`; `src/navidrome/types.rs:218-233`; `src/navidrome/types.rs:340-350` | Payload `Synced` dari Navidrome diterima apa adanya (tanpa `looks_like_synced_lyrics`), padahal jalur LRCLIB divalidasi. DTO `StructuredLyrics` tidak memuat field OpenSubsonic `offset`, dan `start: null` dipetakan ke 0. | Lirik "synced" dari server dengan semua `start=0` akan membuat overlay menyorot baris terakhir sepanjang lagu; lirik server dengan `offset` non-nol tampil bergeser. | Jalankan validasi yang sama untuk hasil server (`MIN_SYNCED_LINES`, ada `time > 0`); tambahkan field `offset` di DTO dan terapkan sebagai `ParseOptions::extra_offset_ms`. |
| INT-32 | P2 | `src/lyrics/lrc.rs:31`; `src/lyrics/lrc.rs:166-189`; `src/lyrics/lrc.rs:145-159`; `src/lyrics/lrc.rs:131-143` | Parser hanya menerima `&str` UTF-8 (tanpa deteksi/lossy untuk file Latin-1/UTF-16), desimal koma `[00:01,500]` dianggap tag metadata (dibuang), `[offset:]` hanya dipindai pada 20 baris pertama dan case-sensitive, `clean_text` menelan sisa baris bila ada `<` tanpa `>`. Lihat §5. | Lirik non-UTF-8 hilang seluruhnya; file dengan format desimal Eropa kehilangan garis waktu (jatuh ke "plain"); `[Offset:+250]` diabaikan. | Fallback `String::from_utf8_lossy`, dukung `,` sebagai pemisah desimal, pindai `[offset:` case-insensitive di seluruh header, dan batasi penelanan tag `<...>` agar tidak menghapus teks setelah `<` tak tertutup. |
| INT-33 | P2 | `gnome-extension/.../extension.js:23-27`; `src/overlay/mod.rs:91`; `extension.js:41-43`; `extension.js:201-251`; `gnome-extension/.../metadata.json:5-6` | "Protokol" antara app dan ekstensi hanya string judul `Myuzik Lyrics` + app id — tidak ada versi/handshake/kanal. Bila judul berubah, pin gagal dan hanya tercatat `console.debug` (dibuang kecuali `G_MESSAGES_DEBUG`). Fallback app-id mem-pin **setiap** window Myuzik yang judulnya bukan `Myuzik`/kosong. `shell-version` hanya 45–51. | Perubahan judul/penambahan window kedua (mis. dialog Settings) mem-pin window yang salah atau tidak mem-pin apa pun, tanpa gejala yang bisa didiagnosis pengguna. GNOME 52+ → `OUT_OF_DATE`. | Jadikan judul/atribut penanda eksplisit (mis. judul `Myuzik Lyrics — overlay` atau penanda di app id) dan dokumentasikan sebagai kontrak; batasi fallback ke window dengan judul kosong saja; tambahkan `shell-version` baru saat GNOME baru rilis. |
| INT-34 | P2 | `install.sh:12`; `install.sh:62-63`; `install.sh:34-37` | Header mendokumentasikan `--prefix DIR` (spasi) tetapi skrip menolaknya (`echo "Use --prefix=DIR"; exit 1`). `--prefix=DIR` hanya memindahkan binary (`BIN_DIR`), sedangkan desktop entry/ikon tetap ke `XDG_DATA_HOME`. | Perintah yang didokumentasikan gagal; instalasi "ke /usr/local" menaruh binary di `/usr/local/bin` tetapi entri desktop di `~/.local/share` — tidak sesuai harapan dan tidak dijelaskan. | Terima kedua bentuk flag, dan dokumentasikan bahwa `--prefix` hanya untuk binary (data selalu XDG). |
| INT-35 | P2 | `docs/setup.md:33`; `docs/gnome-extension.md:80`; `docs/gnome-extension.md:187`; `docs/gnome-extension.md:260`; `docs/troubleshooting.md:63` | Docs menyuruh menjalankan `scripts/install-extension.sh` tanpa `./` atau `bash`. | `command not found` untuk pengguna yang menyalin perintah apa adanya (PATH normal tidak memuat `.`). | Ganti menjadi `./scripts/install-extension.sh` (atau `bash scripts/...`) di kelima tempat. |
| INT-36 | P2 | `README.md:148-160`; `docs/setup.md:41-42`; `src/config.rs:30-34` | `docs/setup.md` menyatakan `config.toml` menerima "the keys documented in the README", tetapi README tidak memuat `overlay_x`, `overlay_y`, `overlay_width`, `overlay_height`, `overlay_opacity` yang ada di `AppConfig`. | Pengguna yang ingin menyunting geometri/opasitas overlay tidak menemukan key-nya di docs rujukan. | Tambahkan 5 key itu (beserta default `620×330`, `0.74`) ke blok contoh README. |
| INT-37 | P2 | `src/config.rs:80`; `src/config.rs:85`; `src/main.rs:32-34`; `README.md:162`; `docs/troubleshooting.md:98-100` | `Paths::logs` selalu dibuat tetapi tidak pernah ditulis; tidak ada appender file (hanya `tracing_subscriber::fmt()` ke stderr). README menyebut `~/.cache/myuzik/logs/` sebagai lokasi cache. | Direktori kosong permanen; setelah crash/tray gagal tidak ada artefak untuk didiagnosis; docs menyesatkan tentang keberadaan log. | Hapus `logs` dari `Paths`, atau implementasikan `tracing_appender::rolling` ke sana dan dokumentasikan lokasinya. |
| INT-38 | P2 | `src/main.rs:205-253` | Hook uji `MYUZIK_WINDOW_TEST` (`close`/`resize`/`minimize`/default `zoom`) dikompilasi ke binary release dan membaca env proses. | Env var yang tak sengaja terwarisi (systemd unit, skrip pembungkus) bisa menutup/meresize/meminimize jendela pengguna — perilaku sulit dijelaskan saat dilaporkan. | Bungkus dengan `#[cfg(debug_assertions)]` atau feature `test-hooks`. |
| INT-39 | P2 | `src/platform/accent.rs:5-9`; `src/platform/accent.rs:22-35`; `src/config.rs:36`; `Cargo.toml:31-32`; `docs/design.md:200`; `docs/accessibility.md:45` | `read_os_accent()` selalu `fallback_accent()` (`#3584E4`) dengan komentar "full portal integration in Task 20"; `gnome_accent_name_to_rgb` tidak dipanggil siapa pun; `ashpd` dan `keyring` dideklarasikan tetapi tidak dipakai kode. | `follow_os_accent` (default `true`) tidak berefek: accent GNOME pengguna tidak pernah dipakai, padahal docs/design menyebut "read from portal" dan "OS accent". Dua dependensi mati menambah waktu build/permukaan audit. | Implementasikan `ashpd` `org.freedesktop.portal.Settings` (`accent-color`) + fallback `gsettings get org.gnome.desktop.interface accent-color` (pakai `gnome_accent_name_to_rgb`), atau hapus setting + perbarui docs dan buang dependensi. |
| INT-40 | P2 | `scripts/test-all.sh:12-20`; `src/platform/tray.rs:1277-1278`; `tests/mpris_metadata.rs:23-36`; `docs/acceptance.md:90-92`; `docs/acceptance.md:192` | `test-all.sh` menjalankan `cargo test` tanpa `--ignored`, jadi smoke test DBus tray (`#[ignore]`, butuh sesi bus) tidak pernah jalan otomatis; cakupan MPRIS hanya mapping metadata. `docs/acceptance.md` menyatakan tray/perilaku close-to-tray "verified on the running desktop" dan docs "consistent with the code". | Regresi kontrak DBus/MPRIS (mis. INT-03/INT-05) tidak tertangkap otomatis; klaim konsistensi docs tidak berlaku untuk INT-01/INT-36/INT-37. | Tambahkan job terpisah `dbus-run-session` yang menjalankan smoke tray + assert Properties/MPRIS (volume awal, LoopStatus) di CI bila memungkinkan; perbarui `docs/acceptance.md` agar menyebut status otomatis vs manual. |

---

## 3. Kontrak DBus / MPRIS vs state yang seharusnya

### 3.1 `org.kde.StatusNotifierItem` (`/StatusNotifierItem`)

| Member (jenis) | Nilai sekarang (bukti) | State yang seharusnya | Sinkron? |
|---|---|---|---|
| `Property.Id` | `"Myuzik"` konstan (`src/platform/tray.rs:522-525`) | konstanta | ✅ |
| `Property.Category` | `"ApplicationStatus"` (`tray.rs:517-520`) | konstanta | ✅ |
| `Property.Title` | `TrayState::display_title()` = `"Judul — Artist"` / `"Myuzik"` (`tray.rs:527-530`, `263-270`) | judul item; dipakai sumber tooltip | ⚠️ menyimpang dari konvensi SNI (judul track ada di MPRIS) tetapi konsisten dengan dirinya sendiri; sinyal `NewTitle` + `PropertiesChanged` dikirim (`tray.rs:846-858`) |
| `Property.Status` | `"Active"` bila `visible`, `"Passive"` bila tidak (`tray.rs:532-541`) | status *item*, bukan status playback | ⚠️ benar secara spec, tetapi `visible=false` dipakai sebagai "sembunyikan ikon" dengan predikat yang salah (INT-02) |
| `Property.IconPixmap` | statis dari 32×32 PNG (`tray.rs:568-571`, `main.rs:100-106`); `NewIcon` tidak pernah di-emit | statis boleh | ✅ |
| `Property.IconName` | `""` (`tray.rs:563-566`) | boleh kosong bila `IconPixmap` ada | ✅ (perlu verifikasi runtime pada host yang hanya mau nama ikon) |
| `Property.ToolTip` | `("", pixmaps, judul, "Myuzik")` (`tray.rs:598-601`, `392-399`) | judul+subjudul track | ✅ |
| `Property.Menu` | `/MenuBar` (`tray.rs:553-556`) | path DBusMenu | ✅ |
| `Property.ItemIsMenu` | `false` (`tray.rs:558-561`) | `false` untuk item dengan menu terpisah | ✅ |
| `Method.Activate` | → `TrayCommand::Show` (`tray.rs:486-489`) | fokus/buka jendela utama | ❌ gagal senyap bila window utama sudah dihapus (INT-02) |
| `Method.SecondaryActivate` / `XAyatanaSecondaryActivate` | → `Show` (`tray.rs:491-494`, `512-515`) | biasanya membuka menu | ⚠️ perilaku berbeda dari mayoritas host (INT-19) |
| `Method.ContextMenu` | → `Show` (`tray.rs:496-502`) | menu pada host tanpa DBusMenu | ⚠️ tidak membuka menu (INT-19) |
| `Method.Scroll` | no-op (`tray.rs:504-506`) | konvensi: sesuaikan volume | ⚠️ INT-19 |
| Registrasi watcher | sekali (`tray.rs:777-781`, `886-895`) | registrasi ulang saat watcher/host muncul | ❌ INT-05 |

### 3.2 `com.canonical.dbusmenu` (`/MenuBar`)

| Member | Nilai sekarang (bukti) | Seharusnya | Sinkron? |
|---|---|---|---|
| `GetLayout` | 8 item tetap, `revision` dari `TrayState` (`tray.rs:644-659`, `445-459`) | layout dinamis | ✅ (revision naik pada playing/pinned: `tray.rs:861-883`) |
| `Property.label` item PlayPause | `"Pause"`/`"Play"` dari `state.playing` (`tray.rs:339-352`) | mengikuti playback | ✅ sumber: `PlayerEvent::StateChanged` → `tray.set_playing` (`src/ui/shell.rs:246-250`) |
| `Property.label` item Pin lyrics | `"Pin lyrics"`/`"Unpin lyrics"` dari `state.pinned` (`tray.rs:347-348`) | mengikuti status overlay | ✅/⚠️ pembaruan bergantung pada 4 pemanggil `set_pinned` (`shell.rs:141, 586, 1472, 1502`); jalur tutup-overlay lewat `on_window_closed` sudah diperbaiki (`shell.rs:127-148`) |
| Item Show/Hide | dua item statis, selalu `enabled=true` (`tray.rs:293-302`, `401-416`) | satu item yang menyesuaikan keadaan, atau `enabled` sesuai konteks | ⚠️ "Show" tetap aktif walau tak ada jendela untuk difokuskan |
| `Event` | hanya `event_id == "clicked"` (`tray.rs:710-715`, `359-364`) | `clicked` untuk item standar | ✅ |
| `AboutToShow` | selalu `true`, tanpa perubahan properti (`tray.rs:717-727`) | boleh | ✅ |

### 3.3 `org.mpris.MediaPlayer2.Player`

| Member (jenis) | Nilai sekarang (bukti) | State yang seharusnya | Sinkron? |
|---|---|---|---|
| `Identity` / `DesktopEntry` | `"Myuzik"` / `io.github.samvivan1.Myuzik` (`mpris.rs:40-43`) | cocok dengan `.desktop` terpasang (`install.sh:106`, `packaging/`) | ✅ |
| `PlaybackStatus` | dari `PlayerEvent::StateChanged` → `mh.update_status` (`shell.rs:252-254`; `mpris.rs:101-109`) | playing/paused/stopped | ✅ |
| `Metadata` | `TrackStarted` + cover (`shell.rs:345-366`, `812-814`; `mpris.rs:130-154`) | judul/artist/album/length/artUrl/trackid | ⚠️ bisa regress karena race cover (INT-15); artUrl hanya bila file cover sudah ada di disk |
| `Position` | push 10 Hz (`engine.rs:110-117` → `shell.rs:256-264`; `mpris.rs:110-113`) | posisi saat ini | ✅ (tidak ada `Seeked`, INT-14) |
| `Volume` | **`0.8` hardcoded saat start** (`mpris.rs:89`), diubah hanya via handler UI (`shell.rs:2148-2150`, `2193-2195`) | `config.volume` (`shell.rs:91-94`) | ❌ **basi** — INT-03 |
| `LoopStatus` | tidak pernah di-set | mengikuti `Queue::repeat` (`src/player/queue.rs:10,56-57`) | ❌ INT-04 |
| `Shuffle` | tidak pernah di-set | mengikuti `Queue::shuffle` (`queue.rs:21,60-61`) | ❌ INT-04 |
| `CanGoNext` / `CanGoPrevious` | selalu `true` (`mpris.rs:44-47`, dipaksa ulang `98-99`) | `queue.remaining() > 0` / ada previous | ❌ INT-04 |
| `CanSeek` | `true` (`mpris.rs:48`) | sesuai kemampuan stream | ⚠️ selalu benar walau belum ada track |
| `Seek` / `SetPosition` / `SetVolume` / `PlayPause` / `Next` / `Previous` | diteruskan ke `PlayerCommand` (`mpris.rs:61-87`) | kontrol penuh | ✅ |
| `Seeked` (signal) | tidak pernah di-emit | setelah seek dari app | ❌ INT-14 |
| `Quit` (root) | `MprisCommand::Quit` ada tetapi tak pernah dikirim (`mpris.rs:117, 176-178`) | hentikan thread MPRIS | ⚠️ thread hanya berhenti saat proses mati (INT-16) |

---

## 4. Matriks error handling per jenis kegagalan

| Kegagalan | Jalur kode | Pemetaan sekarang (bukti) | Yang dilihat pengguna | Retry/backoff | Status |
|---|---|---|---|---|---|
| Password salah (HTTP 401/403) | `ureq` `.call()` → `From<ureq::Error>` | `Unauthorized` (`error.rs:37-38`) — body Subsonic tidak dibaca karena `http_status_as_error` default `true` (terverifikasi di `ureq-3.5.1/src/config.rs:1068`) | Setup: `Connection failed: unauthorized`; Home: `Unauthorized`; lirik: "No lyrics" (`shell.rs:874-880`) | tidak ada | ⚠️ INT-10, INT-11 |
| Subsonic `status:"failed"` dengan HTTP 200 | `check_status` | pencocokan teks pesan, bukan `error.code` (`client.rs:103-126`) | `server: <pesan server>` mentah | tidak ada | ⚠️ INT-26 |
| Kode 41 (token auth tidak didukung, LDAP) | `check_status` | `Unsupported` karena pesan memuat "not supported" (`client.rs:120-121`) | "unsupported" — tidak menjelaskan solusinya | tidak ada | ⚠️ INT-26 |
| 404 / track atau album hilang | `error.rs:39`; `client.rs:171,183,203` | `NotFound` | Home: `NotFound`; lirik: "No lyrics" | tidak ada | ✅ wajar (pesan masih Debug) |
| 429 rate limit | `error.rs:40` | `Server("HTTP 429")` | `Server("HTTP 429")` | **tidak ada**; `Retry-After` diabaikan | ❌ INT-12 |
| 5xx / 502 / 503 | `error.rs:40` | `Server("HTTP <code>")` | `Server("HTTP 503")` | tidak ada | ❌ INT-12 |
| Timeout (connect 8 s / recv 15 s / global 25 s, `client.rs:51-57`) | `error.rs:50` | `Network("timeout")` (fase hilang) | `Network("timeout")` | permintaan berikutnya dari aksi pengguna berikutnya | ⚠️ INT-27 |
| TLS/sertifikat self-signed | `error.rs:42-49` | heuristik substring `"tls"` → sering `Network(...)` | "network: invalid peer certificate: …" | tidak ada | ⚠️ INT-27 |
| DNS gagal / koneksi ditolak | `error.rs:42-49` (Io) | `Network(msg)` setelah `redact_url` | pesan io + URL teredaksi | tidak ada | ✅ redaksi benar |
| Body bukan JSON (HTML error page dari proxy) | `error.rs:28-32` | `Protocol(serde msg)` | `Protocol("expected value at line 1 column 1")` | tidak ada | ⚠️ pesan tidak informatif (INT-10) |
| Server mati saat start | `main.rs:41-47` + `Shell::new` (`shell.rs:176-193`) | client dibuat (tanpa network), route `Home`, lalu fetch gagal | Home kosong dengan `h.error = Some(Debug)` (`shell.rs:693`) | tidak ada; pengguna harus memicu ulang | ⚠️ tidak ada layar "server tidak terjangkau" |
| Kredensial tidak ada / file hilang | `shell.rs:176-183` | `client = None` → `Route::Setup` (`shell.rs:201-205`) | Form setup | n/a | ✅ |
| Config rusak | `main.rs:41-47` | default + `warn` (tak terlihat) | Form setup, seolah instalasi baru; file lama ditimpa | n/a | ❌ INT-08 |
| LRCLIB mati/timeout | `provider.rs:242-267` | `Option` hilang (`.ok()?`, `return Vec::new()`) | "No lyrics" | tidak ada; ulang tiap play | ❌ INT-11 |
| Endpoint lirik Navidrome error | `client.rs:393-395` | `Ok(None)` | "No lyrics" | tidak ada | ❌ INT-25 |
| GStreamer tidak terpasang | `engine.rs:51-56` | `PlayerEvent::Error(Message("gstreamer init failed"))` | Toast `Playback error: Message("gstreamer init failed")` (`shell.rs:335-344`) | n/a | ⚠️ pesan Debug (INT-10) |
| Error pipeline GStreamer (codec) | `engine.rs:79-83` | `PlayerEvent::Error(Message(pesan gst))` | Toast dengan `Message(...)` | n/a | ⚠️ INT-10 |
| Bus/watch GStreamer gagal | `engine.rs:70,102` | **panic di thread** | tidak ada apa pun; audio diam | n/a | ❌ INT-07 |
| DBus sesi tidak ada | `tray.rs:744-745` → `TrayError` | `main.rs:199` `tracing::warn` (default tak terlihat) | tidak ada ikon, tidak ada pesan | n/a | ⚠️ INT-17 |
| Watcher tidak ada saat start | `tray.rs:777-781` | warn; item tetap di-export | tidak ada ikon, tidak ada pesan | tidak ada | ❌ INT-05 |
| Watcher/host muncul belakangan | — | tidak ada mekanisme | ikon tidak pernah muncul sampai restart | tidak ada | ❌ INT-05 |
| MPRIS gagal build (nama dipakai instance lain) | `mpris.rs:51-57` | `eprintln!` | tidak ada MPRIS, tidak ada pesan | tidak ada | ⚠️ INT-16 |
| Tray worker panic | `tray.rs:210-214` | `tracing::debug!` | update tray hilang senyap | tidak ada | ⚠️ INT-20 |
| Simpan config/kredensial gagal (disk penuh, read-only) | `config.rs:140-148`, `114-126` | `let _ =` di 3 pemanggil (`shell.rs:654, 1068, 1101`) | seolah tersimpan; hilang saat restart | n/a | ❌ INT-09 |

---

## 5. Parsing LRC — matriks edge case

| Kasus | Perilaku sekarang | Bukti | Verdict |
|---|---|---|---|
| `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]`, `[h:mm:ss.xxx]` | didukung; fraksi dipotong 3 digit lalu dipad | `src/lyrics/lrc.rs:166-201`; test `236-259` | ✅ |
| Beberapa stamp per baris `[00:10][00:20]teks` | teks dibagi ke semua stamp | `lrc.rs:74-127`; test `255-258` | ✅ |
| Stamp di tengah baris `[00:10]a[00:20]b` | segmen baru | `lrc.rs:93-100`; test `302-309` | ✅ |
| Teks sebelum stamp | dibuang (untimed) | `lrc.rs:90-92` | ✅ (sesuai desain) |
| `[offset:±ms]` | dibaca dari 20 baris pertama, case-sensitive, hanya bentuk `[offset:` tanpa spasi sebelum tag | `lrc.rs:145-159`; test `281-291` | ⚠️ `[Offset:…]`/baris ke-21 diabaikan (INT-32) |
| Metadata `[ar:]`, `[ti:]` | diabaikan tanpa memecah segmen | `lrc.rs:104-113`; test `262-271` | ✅ |
| Timestamp rusak `[abc]` | dianggap metadata, dibuang | `lrc.rs:104-113` | ✅ |
| Timestamp negatif `[-00:10]` | gagal parse → dibuang | `lrc.rs:171-179` | ✅ |
| Desimal koma `[00:01,500]` | gagal parse → **dibuang** | `lrc.rs:180-186` | ❌ hilangnya baris (INT-32) |
| Menit > 59 `[60:00]` | diterima = 3600 s | `lrc.rs:187`; test `254` | ✅ (permisif, sengaja) |
| Duplikat identik berurutan | di-dedup | `lrc.rs:49-54`, `62`; test `312-315` | ✅ |
| Timestamp tidak urut | di-sort | `lrc.rs:61`; test `274-278` | ✅ |
| Baris kosong / hanya spasi | dibuang | `lrc.rs:42-46`; test `262-271` | ✅ |
| Enhanced LRC `<00:12.80>` | tag kata di-strip | `lrc.rs:131-143`; test `294-299` | ✅ |
| `<` tanpa `>` | sisa baris ditelan (`depth` tak pernah turun) | `lrc.rs:131-143` | ❌ INT-32 |
| Offset `[offset:]` + `ParseOptions::extra_offset_ms` | dijumlah: `extra - tag` | `lrc.rs:34-36`; test `326-335` | ✅ |
| Offset pengguna | diterapkan terpisah di render (`apply_offset`) | `lrc.rs:223-229`; `src/overlay/mod.rs:259`; `src/ui/shell.rs:216-218, 300-313` | ✅ (tidak dobel dengan tag) |
| BOM + CRLF | di-strip | `lrc.rs:32, 40`; test `318-323` | ✅ |
| Encoding non-UTF-8 (Latin-1/UTF-16) | tidak didukung; `&str` UTF-8 wajib | `lrc.rs:31` | ❌ INT-32 (perlu verifikasi runtime apakah ada jalur yang bisa memberi non-UTF-8; sumber saat ini JSON UTF-8) |

---

## 6. Cache: invalidasi & pembatas

| Cache | Lokasi | Key | Invalidasi | Prune | Temuan |
|---|---|---|---|---|---|
| Lirik | `$XDG_CACHE_HOME/myuzik/lyrics/*.json` (`src/config.rs:79`) | `SHA256(FORMAT_VERSION + artist + title + album + durasi_detik)` (`cache.rs:33-45`) | hanya `FORMAT_VERSION = "v2"` (`cache.rs:31`); tidak ada TTL, tidak ada ETag/hash, preferensi tidak ikut | `prune()` ada tetapi **tak pernah dipanggil** (`cache.rs:85`) | INT-13, INT-29, INT-30 |
| Cover (disk) | `$XDG_CACHE_HOME/myuzik/covers/{cover_id}.{png,jpg}` (`shell.rs:787`, `3801-3804`) | `cover_id` | tidak ada (art berubah = file lama dipakai) | tidak ada | INT-13 |
| Cover (memori) | `HomeState::covers` + LRU | `cover_id` | LRU (`tests/cover_lru.rs`) | n/a | ✅ (memori dibatasi; disk tidak) |
| `NotFound` lirik | — | — | tidak pernah disimpan | — | INT-29 |
| Config | `$XDG_CONFIG_HOME/myuzik/config.toml` | — | tulis atomik `tmp`+`rename` (`config.rs:140-148`) | n/a | ✅ atomik; ⚠️ tanpa versi skema (INT-08) |

---

## 7. Protokol GNOME extension vs app

Ringkas: **tidak ada socket, tidak ada file protokol, tidak ada negosiasi versi.** Koplingnya hanya dua string yang diduplikasi:

| Kontrak | Nilai | Definisi app | Definisi ekstensi | Risiko |
|---|---|---|---|---|
| Judul overlay | `"Myuzik Lyrics"` | `src/overlay/mod.rs:91` (dipakai `shell.rs:641`, `overlay/mod.rs:1007`) | `extension.js:23`, `test-extension.js:29` | rename judul = pin mati senyap (debug-only, `extension.js:41-43`) |
| App id | `io.github.samvivan1.Myuzik` | `src/main.rs:21,78`; `src/overlay/mod.rs:996`; `mpris.rs:40-42` | `extension.js:24` | aman selama konsisten |
| Judul jendela utama (pengecualian) | `"Myuzik"` | `src/main.rs:23,80` | `extension.js:27` | bila judul utama berubah, jendela utama bisa ter-pin lewat fallback app-id (`extension.js:201-215`) |
| Versi | tidak ada | — | `metadata.json:6` `version: 2`; `shell-version 45–51` (`metadata.json:5`) | tidak ada cara app memberi tahu ekstensi versi protokolnya; GNOME 52+ = `OUT_OF_DATE` |

Perilaku saat ekstensi tidak ada/di-disable: overlay tetap terbuka sebagai toplevel normal (tidak selalu di atas, terikat workspace) — didokumentasikan di `docs/gnome-extension.md:220-228` dan konsisten dengan kode app (app tidak pernah memeriksa keberadaan ekstensi; tidak ada ketergantungan balik).

---

## 8. Config & path XDG

| Aspek | Kenyataan kode | Bukti | Catatan |
|---|---|---|---|
| Direktori | `ProjectDirs::from("io.github", "samvivan1", "Myuzik")` | `src/config.rs:73` | Linux memakai nama aplikasi di-lowercase → `myuzik` (terverifikasi di `directories-6.0.0/src/lin.rs:91-104`: `trim_and_lowercase_then_replace_spaces(application, "")`) |
| Config | `$XDG_CONFIG_HOME/myuzik/config.toml` | `config.rs:76` | sesuai `docs/setup.md:41`, `README.md:148`, dan purge `uninstall.sh:98` |
| Kredensial | `$XDG_CONFIG_HOME/myuzik/credentials` (0600 setelah tulis) | `config.rs:95-126` | plaintext; lihat INT-01/INT-09 |
| Cache | `$XDG_CACHE_HOME/myuzik/{covers,lyrics,logs}` | `config.rs:77-80` | `logs/` tak terpakai (INT-37) |
| Default | 16 key; semua `#[serde(default)]` | `config.rs:16-60` | `close_to_tray=true`, `autoplay=true`, `follow_os_accent=true`, `overlay 620×330` |
| Validasi nilai | tidak ada (volume/opacity/offset dari file dipakai apa adanya) | `config.rs:132-138`; `shell.rs:91-94`, `154-156` | nilai rusak (mis. `volume = 9.0`, `overlay_width = 0`) baru diklem sebagian di overlay (`shell.rs:154-156` → `MIN_WIDTH/MIN_HEIGHT`), volume tidak |
| Migrasi antar versi | tidak ada field versi | `config.rs:16-37` | key hilang/rename diabaikan senyap; tidak ada backup (INT-08) |
| Tulis | `tmp` + `rename` (atomik) | `config.rs:140-148` | ✅ |
| Baca | error apa pun → default di `main.rs:41-47` | | INT-08 |

---

## 9. `install.sh` / `uninstall.sh` vs docs

| Langkah | install.sh | docs | Konsisten? |
|---|---|---|---|
| Build release | `cargo build --release` kecuali `--skip-build` (`install.sh:85-92`) | `docs/setup.md:16`, `README.md:73-77` | ✅ |
| Binary | `install -Dm755` ke `$PREFIX/bin` (`install.sh:95`) | ✅ | ✅ |
| Desktop entry | `sed` menulis `Exec=` absolut + `chmod 644` (`install.sh:98-108`) | `docs/troubleshooting.md:23-56` menjelaskan alasannya | ✅ idempoten |
| Ikon | 1 SVG + 8 PNG + symbolic (`install.sh:110-123`) | ✅ | ✅ |
| AppStream | `packaging/*.metainfo.xml` → `$DATA_HOME/metainfo` (`install.sh:124-128`) | tidak disebut di `docs/setup.md` | ⚠️ docs tidak lengkap, bukan bug |
| Cache desktop/ikon | `update-desktop-database`, `gtk-update-icon-cache` best-effort (`install.sh:130-138`) | — | ✅ |
| Ekstensi GNOME | `bash scripts/install-extension.sh` bila `gnome-extensions` ada; kegagalan hanya `warn` (`install.sh:140-155`) | `docs/setup.md:28-37`, `docs/gnome-extension.md:77-102` | ✅ (tapi lihat INT-35 untuk cara memanggil di docs) |
| Verifikasi | cek binary, `desktop-file-validate`, query `GAppInfo` dengan PATH sesi (`install.sh:157-200`) | `docs/troubleshooting.md:44-56` | ✅ (bagus) |
| `--prefix DIR` (spasi) | **ditolak** (`install.sh:62-63`) padahal didokumentasikan (`install.sh:12`) | — | ❌ INT-34 |
| `--prefix=` | hanya `BIN_DIR`; entri desktop tetap di `XDG_DATA_HOME` (`install.sh:34-37, 62`) | — | ⚠️ tidak dijelaskan |
| Prasyarat runtime | tidak memeriksa GStreamer/plugin codec | `docs/troubleshooting.md:72-80` | ⚠️ app terpasang tapi audio bisa gagal; docs menutupi |
| Uninstall | hapus binary/entri/ikon/metainfo + ekstensi + cache refresh (`uninstall.sh:73-116`); `--purge` menghapus `$CONFIG_HOME/myuzik` & `$CACHE_HOME/myuzik` (`uninstall.sh:96-105`) | `README.md:92-94`, `docs/gnome-extension.md:257-265` | ✅ idempoten (`remove_path` melaporkan "not installed"), path sesuai `directories` (§8) |
| Uninstall: `--prefix DIR` | juga hanya bentuk `--prefix=` (`uninstall.sh:46`) | — | ⚠️ konsisten dengan install, tapi keduanya bertentangan dengan docs internal |
| Wrapper | `scripts/install.sh` meneruskan ke root + `--uninstall` (`scripts/install.sh:9-18`) | `docs/setup.md:16` memakai `./install.sh` | ✅ (dua pintu masuk, satu implementasi) |

Catatan: `uninstall.sh:36` (`usage`) memakai `sed -n '3,14p'` sehingga ikut mencetak baris non-opsi (`# It is safe to run repeatedly…`); hal sama di `install.sh:52`. P2 kosmetik, tidak diberi ID terpisah.

---

## 10. Klaim dokumentasi: diverifikasi / terbantah

| Klaim | Lokasi | Status | Bukti |
|---|---|---|---|
| Password disimpan lewat Secret Service (`keyring`), fallback file 0600 | `docs/setup.md:25-26`; `docs/troubleshooting.md:84-85`; `README.md:67-68` | **TERBANTAH** | `grep -rn keyring src/` kosong; `src/config.rs:114-126` selalu menulis plaintext |
| Config key terdokumentasi di README | `docs/setup.md:41-42` | **SEBAGIAN TERBANTAH** | 5 key `overlay_*` tidak ada di `README.md:148-160` |
| Auth memakai token (MD5 password+salt), password tidak lewat jaringan | `docs/setup.md:11-12` | ✅ | `src/navidrome/auth.rs:12-29`; `client.rs:66-68, 79-84` |
| Salah password → kode 40 ditampilkan inline | `docs/troubleshooting.md:20` | ⚠️ USANG | kode tidak dibaca; 401 HTTP → `Unauthorized` (`error.rs:37-40`), teks = "unauthorized" (`shell.rs:1091`) |
| Self-signed TLS tidak dipercaya rustls | `docs/troubleshooting.md:21` | ✅ | `Cargo.toml:25` (fitur `rustls` pada `ureq`); `error.rs:42-49` |
| Tidak ada blur (Mutter tanpa `ext-background-effect-v1`) | `docs/troubleshooting.md:67-70` | ✅ (konsisten kode) | tidak ada kode blur; `src/main.rs:73` transparan |
| Overlay berjudul tepat `Myuzik Lyrics` | `docs/troubleshooting.md:64`; `docs/gnome-extension.md:35` | ✅ | `src/overlay/mod.rs:91` |
| Ekstensi match judul + app-id, idempoten, teardown bersih | `docs/gnome-extension.md:14-50` | ✅ | `extension.js:66-93, 101-127, 259-290` |
| Ekstensi tanpa sesi live (test gjs 18 check) | `docs/gnome-extension.md:181-218`; `docs/acceptance.md:147-148` | ✅ sesuai kode; **tidak diverifikasi ulang di audit ini** | `gnome-extension/test-extension.js` bukan sesi nyata |
| Tray terverifikasi di desktop nyata (registrasi, `AboutToShow`, `Event(clicked)`, `shutdown` melepas nama) | `docs/acceptance.md:90-92` | ⚠️ klaim manual, **tidak dapat diverifikasi tanpa sesi**; catat bahwa `scripts/test-all.sh:13` tidak menjalankan smoke test | `src/platform/tray.rs:1277-1442` (#[ignore]) |
| Docs "present and consistent with the code" | `docs/acceptance.md:192` | **TERBANTAH** | INT-01, INT-34, INT-36, INT-37, INT-39 |
| OS accent dibaca dari portal | `docs/design.md:200`; `docs/accessibility.md:45` | **TERBANTAH** | `src/platform/accent.rs:5-9` selalu fallback |
| `~/.cache/myuzik/logs/` | `README.md:162` | ⚠️ direktori dibuat, tidak pernah ditulis | `src/config.rs:85` vs `src/main.rs:32-34` |
| Logs meredaksi token Subsonic | `docs/troubleshooting.md:100` | ⚠️ hanya pada jalur error HTTP | `error.rs:22-53`; URI stream ke player tanpa redaksi (INT-23) |
| Tanpa ekstensi overlay tetap berfungsi (hanya tidak selalu di atas) | `docs/gnome-extension.md:220-228` | ✅ | app tidak pernah memeriksa ekstensi (`src/ui/shell.rs:574-646`) |

---

## 11. Yang sudah baik (jangan diubah tanpa alasan)

- **Redaksi kredensial** di jalur error HTTP terpusat dan ber-test (`src/platform/logging.rs:5-44`).
- **Tulis config atomik** (`tmp`+`rename`) — tidak ada config setengah tertulis (`config.rs:140-148`).
- **Batas waktu HTTP eksplisit** di ketiga fase, dengan komentar yang menjelaskan regresi sebelumnya (`client.rs:48-57`, `provider.rs:196-204`).
- **Model tray dipisah dari bus**: `TrayState`/`MenuEntry` murni, diuji ketat (id stabil, label, filter properti, pixmap big-endian, buffer pendek) — `tray.rs:916-1145`.
- **Smoke test DBus nyata** tersedia sebagai `#[ignore]` yang memverifikasi nama bus, watcher, `NewTitle`/`NewToolTip`/`PropertiesChanged`/`LayoutUpdated`, `GetProperty` label, dan pelepasan nama saat shutdown (`tray.rs:1277-1442`) — tinggal dijalankan.
- **Validasi match lirik LRCLIB** (durasi ±3 s, minimal 3 baris, ada timestamp > 0, ranking, bukan "first hit") beserta test (`provider.rs:36-40, 280-350, 438-562`).
- **Pemisahan thread MPRIS** dengan `tokio` `LocalSet` sehingga UI tidak pernah memblokir bus (`mpris.rs:26-39`).
- **Pola generation guard** untuk lirik sudah benar (`shell.rs:860-883`) — tinggal dipakai untuk metadata MPRIS.
- **Ekstensi GNOME** defensif: `_connect`/`_disconnect` tak pernah throw, idempoten, teardown penuh, `enable()` ganda tidak dobel-hook (`extension.js:59-127, 259-325`).
- **`install.sh` memakai `Exec=` absolut dan memverifikasi lewat `GAppInfo` dengan PATH sesi** — solusi tepat untuk masalah nyata yang didokumentasikan (`install.sh:98-108, 169-200`).
- **Uninstall idempoten** dengan mode `--dry-run` dan pelaporan "not installed" (`uninstall.sh:55-69`).

---

## 12. Tidak diverifikasi (batasan audit)

Tidak ada runtime DBus/GNOME/audio pada audit ini, jadi hal berikut **tidak** diverifikasi:

1. **DBus sesi**: tidak ada `org.kde.StatusNotifierWatcher`/host, jadi tidak bisa menguji `RegisterStatusNotifierItem`, `GetLayout` di shell nyata, atau pelepasan nama saat `Tray::shutdown`. Smoke test ada di `tray.rs:1277-1442` tetapi tidak dijalankan.
2. **GNOME Shell/Mutter**: perilaku `make_above()`/`stick()` pada overlay nyata, jalur X11 `WM_CLASS`, `OUT_OF_DATE` pada shell 52+.
3. **MPRIS pada klien nyata** (`playerctl`/GNOME media widget): nilai `Volume`, `LoopStatus`, `Seeked` yang benar-benar dibaca klien.
4. **GStreamer**: apakah URI (berisi token) muncul di log GStreamer saat `GST_DEBUG` aktif (INT-23).
5. **Jaringan/Navidrome nyata**: perilaku 429/5xx dari reverse proxy, apakah Navidrome mengirim `offset` pada `structuredLyrics` (INT-31), apakah body Subsonic dibaca untuk 4xx (disimpulkan dari `ureq-3.5.1/src/config.rs:1068`, bukan observasi).
6. **Perilaku GPUI**: apakah menutup jendela utama benar-benar mempertahankan overlay (INT-02). Predikat `app.windows().is_empty()` dan akibatnya bagi ikon tray terbukti dari kode; urutan penghancuran window adalah perilaku GPUI.
7. **`install.sh`/`uninstall.sh` tidak dieksekusi** (sesuai batasan tugas); analisis hanya statis, termasuk tidak memverifikasi `gnome-extensions pack` SIGSEGV yang didokumentasikan.
8. **`cargo check --offline` tidak dijalankan**: dependensi GPUI berasal dari `git` (`Cargo.toml:10-11`), sehingga `--offline` kemungkinan gagal di resolusi dependency dan/atau melewati batas waktu; tidak ada klaim build yang dibuat.
9. **Encoding LRC non-UTF-8**: tidak ada jalur runtime yang diuji; semua sumber saat ini JSON UTF-8, jadi dampak praktisnya perlu bukti (file LRC lokal tidak dibaca oleh app — lihat catatan di INT-32).

---

## 13. Lampiran

**Berkas yang dibaca penuh**: `src/platform/tray.rs` (1443), `src/platform/mpris.rs` (179), `src/platform/logging.rs` (45), `src/platform/accent.rs` (62), `src/config.rs` (174), `src/main.rs` (255), `src/app.rs` (21), `src/navidrome/client.rs` (411), `src/navidrome/types.rs` (408), `src/navidrome/auth.rs` (29), `src/navidrome/error.rs` (54), `src/lyrics/provider.rs` (563), `src/lyrics/cache.rs` (119), `src/lyrics/lrc.rs` (363), `src/player/engine.rs` (183), `gnome-extension/.../extension.js` (326), `gnome-extension/.../metadata.json`, `install.sh` (212), `uninstall.sh` (126), `docs/setup.md`, `docs/troubleshooting.md`, `docs/gnome-extension.md`, `tests/mpris_metadata.rs`, `tests/navidrome_auth.rs`, `tests/live_navidrome.rs`, `Cargo.toml`, `README.md` (bagian relevan).
**Dibaca sebagian (range relevan)**: `src/ui/shell.rs` (120-320, 325-384, 440-470, 600-1010, 1020-1140, 1340-1360), `src/overlay/mod.rs` (978-1017), `scripts/install.sh`, `scripts/install-extension.sh`, `scripts/test-all.sh`, `docs/acceptance.md`, `docs/accessibility.md`, `docs/design.md` (baris yang dikutip), `tests/cover_lru.rs`.

**Perkakas**: `skill codebase-memory` (evidence tier Verify/partial Auditor) + MCP `codebase-memory-mcp` (`index_status`: ready, generation 2026-11-10T18:27:42Z; `get_file_outline` tray/provider/client; `check_index_coverage` 30/30 path `no_recorded_issue`, `metadata_match`); `grep`/`read` untuk verifikasi baris; pembacaan sumber crate vendored (`ureq-3.5.1`, `tracing-subscriber-0.3.23`, `directories-6.0.0`) untuk memastikan semantik pustaka yang diklaim.

**Indeks kode**: `home-samvivan-Documents-Myuzik` (1767 node, 7361 edge). Semua path bukti dilaporkan tanpa gap tercatat; berkas yang tidak terindeks (27) dibaca manual.
