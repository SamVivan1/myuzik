# Audit Myuzik — Indeks & Sintesis

Commit diaudit: **`e096e20`** (branch `master`, working tree bersih saat audit).
Tanggal: 2026-11-11. Sifat: **baca-saja** — tidak ada perubahan pada `src/`, `tests/`,
`gnome-extension/`, atau docs lain. Satu-satunya tambahan adalah folder `docs/audit/`.

Audit dijalankan oleh **4 auditor paralel** dengan lingkup tulis yang terpisah, lalu
disintesis oleh Lead. Setiap klaim di laporan asal punya bukti `file:line`.

> **Status perbaikan:** lihat [fixes/README.md](fixes/README.md) — 6 P0 ditangani
> (1 separuh: kredensial), 20 P1 dan 4 P2 ditutup, gerbang `fmt`/`clippy`/`test` hijau
> dengan 206 test lulus. Daftar sisa yang belum dikerjakan ada di dokumen itu.

| Laporan | Lingkup | P0 | P1 | P2 | Total |
|---|---|---:|---:|---:|---:|
| [ui-ux.md](ui-ux.md) | `src/ui/*`, `theme.rs`, `accent.rs`, `tests/a11y.rs` | 2 | 11 | 17 | 30 |
| [flow-logic.md](flow-logic.md) | `player/*`, `library/*`, `overlay/*`, transisi state `shell.rs` | 1 | 8 | 13 | 22 |
| [integration.md](integration.md) | `platform/*`, `config.rs`, `main.rs`, `navidrome/*`, `lyrics/*`, `gnome-extension/` | 2 | 11 | 27 | 40 |
| [performance.md](performance.md) | jalur render/update, cache, startup, I/O | 2 | 8 | 10 | 20 |
| **Total dilaporkan** | | **7** | **38** | **67** | **112** |

Setelah dedupe silang (≈7 temuan dilaporkan dua auditor): **6 P0 unik, ≈35 P1, ≈64 P2, ≈105 unik.**

---

## 1. Enam P0 unik

| # | Temuan | Bukti | Dilaporkan |
|---|---|---|---|
| **1** | **UI freeze.** `Mutex<Radio>` dipegang thread background selama `next_batch` (hingga 7 HTTP `ureq` blocking, timeout 25 s/req) sementara UI thread mengunci mutex yang sama di setiap klik Play/Next. Worst case ≈175 s tidak merespons. | [shell.rs:966-968](../../src/ui/shell.rs#L966-L968) (holder) vs [shell.rs:710](../../src/ui/shell.rs#L710), [735](../../src/ui/shell.rs#L735), [942](../../src/ui/shell.rs#L942) (lock di UI) | FL-01 = PF-02 |
| **2** | **Keyboard tidak bisa mengaktifkan apa pun.** Tab menyorot ≥25 kontrol, tetapi **Enter/Space tidak pernah menjalankan aksi**: `on_click` di gpui hanya mouse, tidak ada `on_a11y_action`, tidak ada key binding, dan handler root tidak punya arm `"enter"`. Space malah men-toggle playback. Blocker WCAG 2.1.1 Level A. | [shell.rs:2268-2340](../../src/ui/shell.rs#L2268-L2340); `grep on_a11y_action\|bind_keys` di `src/` → kosong | UI-01 |
| **3** | **Toast tidak pernah dirender.** Pesan `Playback error`, `Autoplay unavailable`, `Library error` ditulis ke field lalu dibuang; `toast_text()` hanya dipakai test. Semua kegagalan asinkron senyap. | ditulis di [shell.rs:341](../../src/ui/shell.rs#L341), [1003](../../src/ui/shell.rs#L1003), [3738](../../src/ui/shell.rs#L3738); tidak ada di `render` 1525–3683 | UI-02 |
| **4** | **Password plaintext, docs menjanjikan Secret Service.** `keyring` ada di `Cargo.toml:31` tetapi **nol pemakaian** di `src/`; `save_password` selalu `fs::write` ke `~/.config/myuzik/credentials` (lalu `chmod 0600` *setelah* menulis). | [config.rs:114-126](../../src/config.rs#L114-L126) vs [docs/setup.md:25](../setup.md#L25), [docs/troubleshooting.md:84](../troubleshooting.md#L84), `README.md:67` | INT-01 |
| **5** | **Tray/overlay dead-end.** Ikon tray disembunyikan berdasarkan `app.windows().is_empty()` — overlay lirik adalah window, jadi dengan overlay terbuka tray selalu `Passive`; `TrayCommand::Show` hanya meng-`activate_window()` handle jendela utama yang sudah dihapus dan errornya dibuang `let _ =`. Proses hidup tanpa jendela dan tanpa ikon. | [main.rs:124-127](../../src/main.rs#L124-L127), [main.rs:142-156](../../src/main.rs#L142-L156) | INT-02 |
| **6** | **Decode + encode gambar di main thread setiap ganti track.** `rounded_cover()` = `load_from_memory` → `to_rgba8` → loop per piksel → `PngEncoder`, dipanggil tanpa syarat walau overlay tertutup. Estimasi statis puluhan ms/track → hitch 1–3 frame. | [overlay/mod.rs:427-450](../../src/overlay/mod.rs#L427-L450); pemanggil [shell.rs:806](../../src/ui/shell.rs#L806), [842](../../src/ui/shell.rs#L842) | PF-01 |

### Diverifikasi ulang sendiri oleh Lead

Sebelum sintesis, Lead membaca ulang sumber untuk 9 klaim paling berdampak — semuanya
**cocok** dengan laporan: INT-01, INT-02, PF-01, PF-03 (`songs.clone()` di dalam loop baris),
PF-05 (tidak ada `fs::read` untuk cover), FL-01/PF-02 (lock lintas HTTP), FL-08 (`set_repeat`/
`set_shuffle` tanpa pemanggil), UI-01, UI-02.

---

## 2. Tujuh tema lintas-laporan

Bukan daftar baru — ini cara membaca 105 temuan sebagai **tujuh akar masalah**.

### T1. Main thread masih mengerjakan kerja berat
Bukan soal `ureq` di `render()` (itu bersih — semua HTTP di `background_executor`, semua fase
punya timeout). Yang tersisa: **mengambil mutex yang dipegang I/O** (P0 #1), **decode/encode
gambar** (P0 #6), **O(n²) clone di render** (`songs.clone()` di dalam loop baris —
[shell.rs:3541](../../src/ui/shell.rs#L3541)), **tulis file sinkron** di body `cx.spawn`
([shell.rs:786-788](../../src/ui/shell.rs#L786-L788)), dan **handshake DBus tray ≤10 s** setelah
`open_window` ([tray.rs:43](../../src/platform/tray.rs#L43), [146](../../src/platform/tray.rs#L146)).
→ PF-01, PF-02/FL-01, PF-03, PF-04, PF-09.

### T2. Kegagalan tidak punya permukaan
Toast ditulis tapi tidak dirender (P0 #3); `PlayerEvent::Error` hanya toast — status tetap
`Playing`, tanpa retry, `PlaybackStatus::Error` **tidak pernah dibentuk**
([shell.rs:335-344](../../src/ui/shell.rs#L335-L344)); error lirik 401/timeout dipetakan jadi
"No lyrics" ([shell.rs:874-880](../../src/ui/shell.rs#L874-L880)); error fetch artist/playlist
ditelan; `expect("bus")` di thread player mematikan audio tanpa jejak; pesan UI berupa `Debug`
mentah (`Server("HTTP 500")`). → UI-02, FL-07, INT-07, INT-10, INT-11, FL-22.

### T3. Pola generation-guard tidak dipakai seragam
Guard generasi **sudah ada dan diuji** untuk pencarian
([shell.rs:2825-2839](../../src/ui/shell.rs#L2825-L2839)) dan lirik
([shell.rs:860-883](../../src/ui/shell.rs#L860-L883)) — tetapi tidak untuk **cover**
(art + metadata MPRIS track lama menimpa track aktif), **radio** (batch milik queue lama
langsung diputar menggantikan pilihan user), atau **album/artist** (klik cepat A→B bisa
menampilkan A). → FL-02, FL-04, FL-06, PF-10, INT-15.

### T4. Dokumentasi menjanjikan perilaku yang tidak ada
Ini tema terbesar dan paling murah diperbaiki. Klaim "sudah selesai" yang tidak sesuai kode:
`keyring`/Secret Service (INT-01) · repeat & shuffle "wired" padahal **tanpa jalur UI sama
sekali** (FL-08, INT-04) · Reduce motion tersimpan tetapi tidak diterapkan saat start
(UI-08) · OS accent dibaca dari portal padahal `read_os_accent()` langsung `return fallback`
dan tak pernah dipanggil (UI-13) · cache cover disk (ditulis, tidak pernah dibaca — PF-05) ·
`LyricsCache::prune` disebut "on startup and after writes" padahal **dead code** (PF-06,
INT-13) · debounce pencarian 250 ms (UI-05/PF-16) · MPRIS `Volume` (INT-03) · batas cache lirik
500 file/50 MB di `docs/performance.md:126` dan `scripts/measure-ram.sh:299`.
→ Juga lihat §5 di [flow-logic.md](flow-logic.md) dan tabel klaim-docs di
[integration.md](integration.md) & [performance.md](performance.md).

### T5. Kode mati / API yang tidak pernah tersambung
`set_repeat`/`set_shuffle` (0 pemanggil) · `LyricsCache::prune` · `read_os_accent` ·
`set_viewport_height` · `target_offset` (hanya dipakai test, padahal ini yang benar) ·
`Route::Album/Artist/Playlist` (tak pernah dikonstruksi) · `album_loading` (di-set, tak dibaca) ·
`Shell::next/previous` (mengirim `Stop`!) · `scrobble_submitted` (ditulis, tak dibaca) ·
`PlaybackStatus::Error`/`Buffering` · dependensi `keyring` & `ashpd`.
Konsekuensinya: **test menguji API, bukan aplikasi** — suite hijau tidak berarti fitur bisa dipakai.

### T6. Aksesibilitas: fokus ada, operabilitas tidak
Tab stop lengkap dan diuji (`tests/a11y.rs`), ring fokus tanpa layout shift — tetapi Enter/Space
mati (P0 #2), `text_tertiary` `#7C756A` = **4.29:1** gagal AA dan dipakai sebagai teks nyata di
12 tempat termasuk 8–10 px ([theme.rs:80](../../src/theme.rs#L80)), waktu/timestamp 8 px di bawah
lantai 9–11 px yang didokumentasikan sendiri, Reduce motion tidak dipulihkan saat start, Ctrl+V
tidak berfungsi di form setup, dan panah global membajak navigasi list. → UI-01, UI-04, UI-06,
UI-07, UI-08, UI-22, UI-26.

### T7. Lifecycle proses, window, dan bus
Tidak ada single-instance guard (dua ikon tray, dua penulis config/kredensial, audio dobel) ·
tidak ada re-registrasi ke `StatusNotifierWatcher` (ikon hilang sampai restart) · tidak ada
shutdown thread player · `MprisHandle::quit()` tak pernah dipanggil · supervisor polling 400 ms
selamanya. → INT-05, INT-06, FL-19, PF-17.

---

## 3. Urutan perbaikan yang disarankan

**Gelombang 1 — perubahan kecil, dampak besar (bisa dalam satu sesi).**
UI-08 (satu baris `cx.set_reduce_motion` di `Shell::new`) · UI-06 (satu nilai token
`text_tertiary`) · FL-05 (jangan panggil `OverlayState::begin()` saat lirik tiba) · PF-03
(`Arc<Vec<Track>>`) · FL-09 (`saturating_add_signed` untuk `MPRIS Seek`) · INT-03 (teruskan
`config.volume`) · UI-12 & UI-28 (satu token / hapus `.max(2.0)`) · FL-17 (`previous` setelah
queue habis) · INT-07 (ganti `expect("bus")` dengan `PlayerEvent::Error`).

**Gelombang 2 — perbaikan struktural (akar masalah).**
Disiplin lock radio: lepas lock sebelum I/O, `try_lock` di jalur UI (P0 #1) · aktivasi keyboard
+ render toast (P0 #2, #3) — keduanya prasyarat agar sisa UI bisa dipakai tanpa mouse dan
kegagalan bisa dilaporkan · handle jendela utama, bukan jumlah window (P0 #5) · keyring **atau**
koreksi docs (P0 #4) · generation guard untuk cover/radio/album (T3) · pindahkan `rounded_cover`
ke background (P0 #6) · state `Error` + retry/auto-advance (FL-07).

**Gelombang 3 — produk & kontrak.**
Search: debounce + render 4 grup + keyboard (UI-03/04/05) · empty/error state Now Playing &
album (UI-09/19) · truncation judul (UI-11) · repeat/shuffle: UI + `LoopStatus`/`Shuffle` MPRIS
(FL-08, INT-04) · single-instance + re-registrasi tray (INT-05/06) · cache disk cache-first +
prune (PF-05, PF-06) · sisa P2.

**Gelombang 0 — jangan lupa:** setelah tiap perbaikan, **koreksi dokumen** yang menjanjikan
perilaku tersebut (`progress.md`, `acceptance.md`, `performance.md`, `setup.md`,
`design.md`, `design-audit.md`). Selama ini dokumen ditulis dari niat, bukan dari kode — itu
yang membuat 6 P0 lolos dari "v0.1.0 selesai".

---

## 4. Yang sudah baik — jangan tersapu refactor

- **Tidak ada `ureq` di render/update path.** Semua I/O jaringan di
  `cx.background_executor()`, semua fase HTTP punya timeout eksplisit (8 s connect / 15 s recv /
  25 s global; LRCLIB 8/12/20 s).
- **Guard generasi** untuk pencarian dan lirik — pola yang benar; temuan T3 cukup menyalinnya.
- **`Queue`**: batas keras `MAX_QUEUE = 500`, `trim_front_to` hanya membuang yang sudah diputar,
  dedup radio `seen` + ring `RECENT_CAP = 100`, dan 16 test queue + 9 test radio + 3 test autoplay
  end-to-end + stress 20k track.
- **Disiplin token material** `theme.rs` (40 field glass/clay/gold/ambient); penyimpangan warna
  mentah bisa dihitung jari.
- **Ring fokus tanpa layout shift** (`focus_border` dengan `border_2()` selalu dipesan) — solusi
  tepat untuk rev gpui ini; urutan fokus 4 grup nyata dan diuji bolak-balik.
- **`OverlayState`** = state machine murni tanpa I/O; keenam `LyricsStatus` selalu berkalimat,
  tidak pernah kotak kosong.
- **Reduced motion dihormati oleh mesin animasi** gpui — begitu cfg diterapkan, semua
  `with_animation` otomatis statis; tidak perlu gate manual per animasi.
- **Setup tidak mem-prefill password**; log URL/token diredaksi.
- **Minimum window size konsisten** antara kebijakan window dan grip resize.

---

## 5. Batas audit — jujur, bukan "aman"

Semua laporan adalah **analisis statis**. Tidak ada GUI, GStreamer, Navidrome, DBus, GNOME Shell,
`install.sh`, atau benchmark yang dijalankan; `cargo build`/`cargo test` penuh tidak dijalankan
(hanya `cargo check --offline` → exit 0 di dua auditor). Konsekuensinya:

1. **Klaim runtime belum terbukti** — khususnya: apakah Enter benar-benar tidak mengaktifkan
   kontrol (UI-01), durasi freeze nyata (FL-01), urutan pesan bus GStreamer (FL-10/12/13),
   apakah `rounded_cover` cukup mahal untuk terlihat (PF-01), perilaku `mpris_server` saat
   `set_position` 10 Hz (PF-18), dan urutan balapan jaringan (FL-02/04/06).
2. **Angka performa adalah estimasi statis**, bukan hasil ukur — dasarnya ditulis per temuan.
3. **Kontras dihitung analitis** dari token `theme.rs`, bukan dari piksel; angka UI-06 (4.29:1)
   adalah batas atas.
4. **Tidak ada reader layar (Orca)** dan tidak ada uji 200% scaling.
5. **Screenshot headless tidak menggambar tekstur gambar**, jadi kualitas crop cover tidak dinilai.
6. **Tidak ada server** — perilaku offline/lambat/timeout tidak diuji.

Yang **paling layak diverifikasi lebih dulu** dengan menjalankan aplikasi: P0 #2 (tekan Enter pada
nav item), P0 #3 (matikan Wi-Fi saat memutar), P0 #1 (klik track saat fetch radio jalan),
P0 #5 (buka overlay → tutup jendela utama → cari ikon tray).

---

## 6. Capability yang dipakai

Skill `codebase-memory`, `impeccable`, `systematic-debugging` · MCP `codebase-memory-mcp`
(`index_status`, `get_architecture`, `get_file_outline`, `search_graph`, `trace_path`,
`query_graph`, `get_code_snippet`, `check_index_coverage`) · tool `read_image` untuk bukti
screenshot · `cargo check --offline`.
