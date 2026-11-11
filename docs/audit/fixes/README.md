# Perbaikan audit — status

Branch: **`audit-fixes`** (master tidak disentuh). Basis audit: `e096e20`.
Commit perbaikan: `63ffddf`, `36de9a2`, `2b7a344`, `e1426ac`, `0abf5b9`, `3dfe836`.

## Gerbang verifikasi (dijalankan ulang, bukan diklaim)

```
cargo fmt --check                          → bersih
cargo clippy --all-targets -D warnings     → bersih
cargo test -- --test-threads=1             → 257 passed, 0 failed, 18 ignored
cargo test --test config_credentials -- --ignored
                                           → keyring nyata: round trip OK, tanpa file plaintext
```

Baseline sebelum perbaikan: **151 passed, 0 failed, 13 ignored**. Jadi **+106 test**, semuanya hijau.
18 test ignored adalah live-Navidrome (9), live-player (1), screenshot (4), live MPRIS/DBus (2),
dan live keyring (1) — butuh server/DBus/display nyata.

Metode: TDD untuk setiap temuan (test merah → bukti merah → perbaikan minimal → hijau), kecuali
perubahan murni kosmetik tanpa perilaku (ukuran font, warna) yang diverifikasi lewat gate global.

---

## P0 — keenam-enamnya

| # | Temuan | Status | Bukti |
|---|---|---|---|
| 1 | **UI freeze**: `Mutex<Radio>` dipegang lintas HTTP | **Fixed** | `radio.rs` dipecah `begin` (lock pendek) → `fetch` (tanpa lock) → `commit` (lock pendek); `next_batch` tetap ada sebagai komposisi. Test: `a_batch_can_be_fetched_while_the_radio_state_stays_mutable`, `begin_refuses_a_stalled_seed_and_accepts_a_fresh_one` |
| 2 | **Keyboard tak bisa mengaktifkan kontrol** | **Fixed** | `Shell::activate_focused` + Enter di root handler. Test: `enter_activates_the_focused_nav_item`, `enter_opens_the_focused_album_card` |
| 3 | **Toast tak pernah dirender** | **Fixed** | elemen `Role::Alert` + `show_toast()` yang `cx.notify()` (dulu tak ada notify sama sekali). Test: `unreachable_server_shows_error_toast` (kini juga menuntut `debug_bounds("toast")`) |
| 4 | **Kredensial plaintext, docs janji Secret Service** | **Fixed** | Secret Service jadi **default**, fallback file `0600` (dibuat `0600` sejak byte pertama), `CredentialStore` dilaporkan ke user, dan setiap panggilan keyring dibatasi 1,5 s sehingga keyring yang menggantung tidak membekukan startup. `MYUZIK_KEYRING=0` memaksa mode file. Diverifikasi end-to-end dengan Secret Service nyata: `real_secret_service_round_trip_leaves_no_plaintext_file` |
| 5 | **Tray/overlay dead-end** | **Fixed** | liveness ikut `AnyWindowHandle` jendela utama, bukan jumlah window; `Show` membuka ulang window yang sudah mati; supervisor quit bila window utama hilang. Keputusan murni diuji: `tray_visibility_follows_the_main_window_not_the_window_count`, `show_focuses_an_open_window_and_reopens_a_closed_one` |
| 6 | **Decode+encode PNG di main thread** | **Fixed** | `rounded_cover` memo per `image.id` (LRU 32) + decode/mask/tulis file dipindah ke `background_executor`. Test: `rounded_cover_does_the_work_once_per_source_image`, `rounded_cover_cache_stays_bounded` |

## P1 yang ikut diselesaikan

| Temuan | Perbaikan |
|---|---|
| **FL-02** batch radio basi menimpa queue baru | `commit()` membuang batch bila seed sudah bukan yang aktif |
| **FL-03** autoplay mati permanen setelah stall | guard pindah dari caller ke `begin()`, setelah reset-per-seed |
| **FL-04** cover/metadata MPRIS track lama menimpa yang aktif | decode cover kini di background + `commit` re-filter id yang baru diputar |
| **FL-05** cover overlay hilang saat lirik tiba | `OverlayState::set_track()` (tidak menyentuh cover/progress) |
| **FL-09** MPRIS `Seek(offset)` dianggap absolut | dihitung relatif + saturasi, tidak wrap |
| **FL-14** `Shell::next/previous` mengirim `Stop` | dihapus (API jebakan) |
| **FL-15** field `scrobble_submitted` mati | dihapus |
| **FL-17** Previous mati setelah queue habis | restart track terakhir (termasuk varian shuffle) |
| **INT-03** MPRIS `Volume` basi | volume awal dari config |
| **INT-04** MPRIS tanpa `LoopStatus`/`Shuffle` | dipetakan dari `RepeatMode`/`shuffle`; `can_go_next/previous` dari queue; shell mendorong queue ke MPRIS (`sync_mpris_queue`) di 6 titik mutasi queue |
| **INT-05** ikon tray tak pernah daftar ulang | `NameOwnerChanged` + retry backoff terbatas |
| **INT-09** file kredensial 0644 lalu chmod | dibuat `0600` sejak awal, error tidak ditelan |
| **INT-10** error tampil sebagai `Debug` mentah | `NavidromeError::user_message()/hint()` dipakai di shell |
| **PF-06** `LyricsCache::prune` dead code | dipanggil dari `new()`; batas 500 file/50 MB kini nyata |
| **PF-09** handshake tray 10 s di main thread | `start_background`, timeout 2 s, laporan dari task background |
| **UI-06** `text_tertiary` 4.29:1 (gagal AA) | `#7C756A` → `#847C70` (4.75:1) + test kontras untuk `text_tertiary` & `danger` |
| **UI-08** reduce-motion tak dipulihkan saat start | `cx.set_reduce_motion(config.reduce_motion)` di `Shell::new` |
| **UI-10** centering lirik pakai aritmetika konstan | tinggi baris tetap + line-height dipatok, geometri disimpan, `target_offset` dipakai, dan tinggi panel **diukur** via `on_children_prepainted` |
| **UI-22** Ctrl+V tak berfungsi di form setup | paste + `Delete` membersihkan field |
| **PF-03** O(n²) clone di render daftar track | `Arc<Vec<Track>>`, clone O(1) per baris |

## P2 yang ikut diselesaikan

UI-04 (Enter memutar hasil pencarian), UI-07 (teks 8 px → 10 px), UI-12 (bar volume gold,
bukan aksen OS), UI-28 (bar volume tak lagi menyisakan 2 px saat mute).

## Dokumentasi yang dikoreksi agar sesuai kode

`docs/setup.md`, `docs/troubleshooting.md`, `README.md` (kredensial: file `0600` default,
keyring opt-in + alasannya), `docs/accessibility.md` (bagian **Activation**: Enter, dan kenapa
Space tetap play/pause), `docs/progress.md` (Task 12: shuffle/repeat belum punya jalur UI),
`docs/performance.md` (batas cache lirik kini benar-benar ditegakkan; tambah cache rounded cover).

## Gelombang 2 — temuan P1 tersisa yang berdampak tinggi

| Temuan | Perbaikan |
|---|---|
| **FL-07** stream gagal, UI tetap bilang Playing | `PlaybackStatus::Error` kini benar-benar dibentuk, alasan ditampilkan segera, lalu retry berbackoff (3 percobaan) dan akhirnya lewati track; berhenti setelah 3 track mati berturut-turut. `Load`/`Seek` yang gagal dilaporkan, dan `Load` yang tak pernah mulai tidak lagi memicu `TrackStarted` (scrobble/metadata/lirik palsu) |
| **FL-12** buffering tak terlihat | `PlayerEvent::Buffering` → `PlayerState::set_buffering`, bukan lagi `tracing::debug!` |
| **FL-13** dua sumber status (ikon berkedip) | `set_status_from_engine` menyaring `Stopped` transien saat load; mirror UI/tray/MPRIS memakai status hasil saringan |
| **FL-08** shuffle/repeat tak bisa dicapai | **Ctrl+S** shuffle, **Ctrl+R** siklus repeat off→all→one→off. Tombol di player bar & baris Settings sempat dicoba dan gagal karena *stack overflow* di harness headless — ternyata itu keterbatasan stack thread test (2 MiB), bukan bug layout; sudah diperbaiki lewat `RUST_MIN_STACK` di `.cargo/config.toml`, jadi tombol yang terlihat kini memungkinkan |
| **UI-03** search hanya merender Songs | grup ARTISTS/ALBUMS/PLAYLISTS ikut dirender, tiap baris tab-stop yang bisa diaktifkan Enter, empty-state menghitung keempat koleksi |
| **UI-05** placeholder bohong, tanpa debounce | pencarian saat mengetik dengan debounce 250 ms (5 ketukan → 1 request), Enter langsung, Esc membersihkan dan keluar ke Home bila field sudah kosong |
| **UI-09** Now Playing kosong & header salah | empty state saat belum ada track; header mengikuti status lirik (SYNCED/UNSYNCED/INSTRUMENTAL/NO LYRICS/LYRICS ERROR/LOADING); footer tak lagi mencetak " — " |
| **UI-11** judul panjang membungkus | kartu album, baris track, dan player bar di-clamp 1 baris + ellipsis, dijaga test source-level |
| **INT-06** tanpa single-instance | flock `$XDG_RUNTIME_DIR/myuzik.lock` sebelum apa pun berjalan; instance kedua meminta instance pertama menampilkan window lalu keluar |
| **INT-08** config rusak hilang senyap | file dipindah ke `config.toml.corrupt-<ts>` (tidak pernah ditimpa/dihapus), default dipakai, pemulihan sampai ke user sebagai toast; ada `version` + titik migrasi; key tak dikenal dilaporkan dan ditulis balik |
| **INT-11** error lirik jadi "No lyrics" | kegagalan nyata (401/timeout/jaringan) kini `LyricsStatus::Error` dengan kalimat yang bisa ditindaklanjuti |
| **INT-12** 429 sama dengan 5xx, tanpa retry | varian `RateLimited` + `is_retryable()`/`retry_delay()` (eksponensial, batas 30 s); fetch cover (jalur non-interaktif) retry 3× dengan backoff |

---

## Sisa yang belum dikerjakan (jujur)

**Belum disentuh sama sekali** (semua masih persis seperti di laporan audit):

- FL-11 scrobble: `track_duration` basi, delta ≥2 s dibuang, gagal submit tanpa retry
- FL-19 shutdown thread pemutar
- INT-13 TTL cache lirik (prune sudah, TTL belum)
- PF-05 cache cover disk write-only · PF-07 N+1 fetch cover · PF-08 `ureq::Agent` per track
- PF-10 dedupe in-flight · PF-11…PF-20
- UI-01 sisa: `on_a11y_action` untuk screen reader (Enter sudah, AT client belum)
- UI-13 (OS accent) · UI-19 · UI-20 · UI-21 · UI-23 · UI-24 · UI-25 (model route) ·
  UI-26 (panah global) · UI-27 (warna mentah) · UI-29 · UI-30
- Kontrol shuffle/repeat yang terlihat (shortcut sudah ada; penghalangnya ternyata hanya
  stack thread test, kini sudah dikonfigurasi, jadi ini tinggal pekerjaan UI)

## Tidak diverifikasi

Semua perilaku runtime yang butuh mesin nyata: tray di GNOME Shell sungguhan, dialog
Secret Service (hanya lewat test `#[ignore]`), DBus/MPRIS dari client nyata, GStreamer,
timing frame pertama, dan efek visual (tidak ada GUI yang dijalankan). Angka performa tetap
estimasi statis.

Dua bagian yang **tidak** punya test otomatis, hanya verifikasi kompilasi + review kode:
(a) `sync_mpris_queue` — `MprisHandle` butuh bus sesi nyata, jadi jalur ini tidak bisa
dijalankan headless (bagian murninya, `queue_state`/`repeat_mode_to_loop_status`, diuji);
(b) pengukuran tinggi panel lirik lewat `on_children_prepainted` — harness headless tidak
memberi ukuran panel yang berarti.

## Capability

Skill `codebase-memory`, `test-driven-development`, `verification-before-completion`,
`subagent-driven-development` · MCP `codebase-memory-mcp` · 3 subagent implementer paralel
pada scope file terpisah · `cargo fmt`/`clippy`/`test`.
