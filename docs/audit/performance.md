# Audit performa & ketahanan beban — Myuzik

**Sifat dokumen: analisis statis.** Semua temuan di bawah berasal dari membaca kode, call-graph
(MCP `codebase-memory-mcp`), dan grep pola berbahaya — **bukan hasil profiling runtime**. Tidak ada
GUI yang dijalankan, tidak ada benchmark, tidak ada `cargo build --release` (sesuai batasan tugas).
Setiap angka dampak adalah **estimasi statis** yang dasar perhitungannya ditulis di kolom "dasar
estimasi"; angka yang hanya bisa dipastikan dengan pengukuran ditandai **perlu profiling**.
Satu-satunya eksekusi yang dilakukan: `cargo check --offline` → `Finished` dengan exit 0
(build cache `target/` sudah hangat, 0,41 s) — ini hanya bukti pohon kode ter-compile, bukan bukti performa.

Commit yang diaudit: `e096e20` (branch `master`).

---

## 1. Ringkasan

| Severity | Jumlah | Inti |
|---|---|---|
| **P0** | **2** | Decode + encode gambar di main thread setiap ganti track (PF-01); `radio.lock()` di main thread padahal lock dipegang selama HTTP blocking di background (PF-02) |
| **P1** | **8** | O(n²) clone di render daftar track (PF-03); tulis disk di main thread (PF-04); cache cover disk write-only **dan** tanpa prune → unduh ulang tiap launch + disk tumbuh (PF-05); `LyricsCache::prune` dead code (PF-06); fetch cover N+1 berurutan (PF-07); `ureq::Agent` baru per track untuk LRCLIB (PF-08); handshake tray DBus ≤10 s di main thread (PF-09); tanpa dedupe in-flight cover (PF-10) |
| **P2** | **10** | Overlay repaint 10 Hz + clone baris lirik (PF-11); alokasi per baris/animasi di lyrics view (PF-12); linear scan di `set_position` (PF-13); grid album tanpa virtualisasi/memoization (PF-14); `Queue::ids()` O(n) per refill (PF-15); search tanpa debounce (PF-16); polling supervisor 400 ms (PF-17); MPRIS posisi 10 Hz (PF-18); `a11y_focus` tumbuh monoton (PF-19); animasi skeleton `.repeat()` (PF-20) |

Kabar baiknya yang perlu ditegaskan lebih dulu: **tidak ada satu pun pemanggilan HTTP (`ureq`) di
render/update path**. Semua I/O jaringan dibungkus `cx.background_executor().spawn(...)` dan
di-`await` dari task `cx.spawn`. Semua fase HTTP juga sudah punya timeout eksplisit. Jadi masalah
"blocking di UI thread" di aplikasi ini bukan soal `ureq` dipanggil dari `render()`, melainkan soal
**kerja lain yang ikut berjalan di main thread** (decode gambar, tulis file, ambil mutex) — lihat §3.

---

## 2. Tabel temuan

Kolom `file:line` menunjuk bukti langsung. "Dasar estimasi" menyebut asal angkanya; tanpa itu,
angka **tidak boleh** dibaca sebagai hasil ukur.

### P0

| ID | Sev | file:line | Pola | Dampak (estimasi) + dasar estimasi | Rekomendasi |
|---|---|---|---|---|---|
| **PF-01** | P0 | `src/ui/shell.rs:806` (di dalam `cx.update`), `src/ui/shell.rs:842` (di `load_and_play`), implementasi `src/overlay/mod.rs:427-450` | `rounded_cover()` melakukan `image::load_from_memory` → `to_rgba8()` → loop per piksel (`mask_corners`, `src/overlay/mod.rs:454-500`) → encode ulang PNG (`PngEncoder`), dipanggil **di main thread pada setiap ganti track** | Est. statis **puluhan ms per ganti track**. Dasar: cover diambil 512 px (`src/ui/shell.rs:1210`, `:781`) = 262.144 piksel; tiga lintasan penuh atas gambar (decode JPEG, loop float per piksel, encode zlib) + alokasi buffer RGBA ≈ 1 MiB. 60 fps = 16,7 ms/frame, jadi 1–3 frame hilang → hitch terlihat saat track berganti. Diperparah: dipanggil **tanpa syarat**, termasuk saat window overlay tertutup (`src/ui/shell.rs:806-810` tidak mengecek `overlay_handle`) | Pindahkan ke `background_executor().spawn`, simpan hasil rounded sebagai `Arc` berkunci `image.id`, dan lewati sepenuhnya bila overlay tidak terbuka. Jangka panjang: jangan mask di CPU — pakai clip/GPU atau terima sudut siku di overlay |
| **PF-02** | P0 | holder: `src/ui/shell.rs:966-967`; pengunci di main thread: `src/ui/shell.rs:710`, `:735`, `:942` | `std::sync::Mutex` (`self.radio`) **dipegang selama HTTP blocking** di background, lalu dikunci dari main thread | Est. statis: `next_batch` (`src/library/radio.rs:100-200`) memanggil hingga **7 request berurutan** (1 `similar_songs_for_song` + 1 `similar_songs` + ≤3 `songs_by_genre` (`MAX_GENRE_PAGES`) + ≤2 `random_songs` (`MAX_RANDOM_DRAWS`), lihat `src/library/radio.rs:29-39`); tiap request dibatasi 8 s connect / 15 s recv / 25 s global (`src/navidrome/client.rs:51-56`) → lock dapat dipegang **puluhan–ratusan detik** bila server lambat. Skenario: refill autoplay in-flight (dipicu `src/ui/shell.rs:719`/`:747`) lalu user klik track → `play_track` memblokir di `:710`. Kasus LAN normal: 7 × ~20–50 ms = **~150–350 ms freeze per klik** (perlu profiling untuk angka pastinya) | Jangan pegang lock selama I/O: ambil snapshot state radio (seed, `genre_offset`, `exclude`, `recent`) dengan lock pendek → lepas → HTTP → lock lagi untuk commit hasil. Lebih baik: pindahkan `Radio` sepenuhnya ke task pemiliknya (tanpa `Mutex`), komunikasi lewat channel |

### P1

| ID | Sev | file:line | Pola | Dampak (estimasi) + dasar estimasi | Rekomendasi |
|---|---|---|---|---|---|
| **PF-03** | P1 | `src/ui/shell.rs:3528`, `src/ui/shell.rs:3541` | Clone seluruh `Vec<Track>` **di dalam** loop baris track → O(n²) | Dasar: `Track` punya 8 field ber-heap (`src/library/mod.rs:2-17`: `id`, `title`, `artist`, `artist_id`, `album`, `album_id`, `cover_art_id`, `genre`). n baris → n² clone `Track`. Album 20 track ≈ 400 clone (~3.200 alokasi, ~80 KB/render); 100 track ≈ 10.000 clone (~2 MB/render); 500 track ≈ 250.000 clone (~50 MB/render) → hitch ratusan ms + tekanan alokator. **Eskalasi ke P0** untuk album kompilasi besar. Catatan: baris 3528 sendiri sudah 1 clone penuh per render | Bungkus sekali: `let songs = std::sync::Arc::new(detail.songs.clone());` lalu per baris `let songs2 = songs.clone();` (clone `Arc`, O(1)). Alternatif lebih baik: `.on_click` cukup menangkap `(Arc<Vec<Track>>, i)` |
| **PF-04** | P1 | `src/ui/shell.rs:786-788` (`create_dir_all` + `fs::write`), `:3801-3803` (sama, 24×), `:654` (`save_config`) | I/O file **sinkron di main thread** di dalam body `cx.spawn` | Dasar: future `cx.spawn` di-poll di main thread — terbukti dari doc gpui `crates/gpui/src/app/context.rs:646` ("polled on the main thread") dan `src/app/async_context.rs:215` ("polled in the foreground"). Cover 512 px ≈ 30–150 KB (cap 8 MiB, `src/navidrome/client.rs:356`); 24 tulisan + 24× `create_dir_all` redundan saat startup; `save_config` (TOML) sinkron di tiap toggle setting/geometri overlay. Est. 0,1–1 ms per tulisan di disk lokal, **bisa 10–100 ms** bila disk lambat/penuh atau home NFS → jank saat grid terisi. Perlu profiling | Buka/tulis file di `background_executor().spawn`; `create_dir_all` cukup sekali di `Paths::ensure` (`src/config.rs:85`); `save_config` di-debounce atau async |
| **PF-05** | P1 | penulisan: `src/ui/shell.rs:786-788`, `:3801-3803`; pembacaan: **tidak ada** | Cache cover di disk **write-only** — tidak pernah dibaca ulang | Dasar: grep `fs::read` di seluruh `src/` = **0 hasil**; `paths.covers` hanya dipakai untuk `create_dir_all`/`fs::write` (`src/ui/shell.rs:786-788`, `:3801-3803`) dan cek `exists()` untuk art MPRIS (`:353-358`). Akibat: setiap launch mengunduh ulang 24 cover (`:3777`) + cover per track, walau byte-nya sudah ada di disk → boros bandwidth/waktu/baterai; satu-satunya "cache" adalah LRU 200 di memori yang hilang saat keluar | Jadikan cache-first: coba baca `<cover_id>.{png,jpg}` dari disk sebelum HTTP; bila ada, langsung `gpui::Image` dari byte tersebut |
| **PF-06** | P1 | `src/lyrics/cache.rs:85` (definisi), `scripts/measure-ram.sh:299` (klaim cetak) | `LyricsCache::prune` adalah **dead code** — tidak dipanggil siapa pun | Dasar: grep `prune` di `src/`, `tests/`, `scripts/` → hanya definisi (`cache.rs:85`) dan `prune_overlay_handle` (tak terkait, `src/ui/shell.rs:1465`). Padahal `docs/performance.md:126` dan `scripts/measure-ram.sh:299` menyatakan cache lirik dipangkas ke 500 file / 50 MB "on startup and after writes". Est. 1 JSON/track ≈ 2–50 KB → sesi berat bertahun-tahun = ratusan MB disk tanpa batas | Panggil `prune(500, 50*1024*1024)` di `LyricsCache::new` (`src/lyrics/cache.rs:21`) dan setiap N tulisan; perbaiki teks `docs/performance.md` + `scripts/measure-ram.sh` **hanya jika** prune benar-benar dipasang (atau perbaiki dokumen agar jujur) |
| **PF-07** | P1 | `src/ui/shell.rs:3777-3812` (cover), `:3724`, `:3753`, `:3759`, `:3766` (list) | N+1 **berurutan**: 24 cover di-`await` satu per satu di dalam loop; 4 request list juga berurutan | Dasar: `for album in albums.iter().take(24)` dengan `cx.background_executor().spawn(...).await` per item (serialisasi eksplisit); 4 list call (`Newest`, `artists`, `playlists`, `Starred`) juga berurutan. Waktu sampai grid penuh ≈ 28 RTT; LAN 20 ms ≈ **0,6 s**, WAN 100 ms ≈ **2,8 s**. Tidak memblok UI (background), tapi thumbnail muncul satu per satu | Batasi konkurensi (mis. 4–6 in-flight lewat `FuturesUnordered`), prioritaskan cover yang terlihat; jalankan 4 list call secara paralel |
| **PF-08** | P1 | `src/lyrics/provider.rs:198-205`, jalur `:216-235` | `ureq::Agent` **dibuat baru per track** di `fetch_lrclib` → tidak ada reuse koneksi/TLS; ditambah hingga ~5 request berurutan per track | Dasar: `agent` dibangun di dalam `fetch_lrclib` (per panggilan/track); `lrclib_get` dipanggil minimal 2× (dengan/tanpa duration) dan bisa 2× lagi, lalu `lrclib_search` → ~4–5 request; agent baru = TCP+TLS handshake baru ke `lrclib.net` setiap ganti track (CPU + latensi tambahan, est. puluhan–ratusan ms di luar RTT). Semua di background, jadi ini biaya latensi lirik + CPU, bukan freeze | Buat satu `Agent` di `LyricsProvider::new` (`src/lyrics/provider.rs:110-120`) dan pakai ulang; pendekkan jalur menjadi 1 lookup + 1 search |
| **PF-09** | P1 | `src/main.rs:106` (pemanggil), `src/platform/tray.rs:146` + `:43` (`START_TIMEOUT = 10 s`) | `Tray::start` melakukan handshake DBus **sinkron di main thread**, setelah `open_window` | Dasar: `ready_rx.recv_timeout(START_TIMEOUT)` dengan `START_TIMEOUT = 10 s` (`tray.rs:43`, `:146`); dipanggil di dalam closure `app.run` di `main.rs:106`, sedangkan `open_window` terjadi di `main.rs:90-95`. Bila session bus lambat/hilang: window sudah dibuat tetapi main thread tertahan **≤ 10 s** → budget startup ≤ 1,5 s (`docs/performance.md:13`) jebol tepat pada skenario gagal. Pada mesin sehat ini cepat (docs mencatat 1,07–1,12 s) | Jalankan `Tray::start` di `background_executor`/thread terpisah, atau turunkan timeout ke 1–2 s dan jangan menunggu sebelum frame pertama |
| **PF-10** | P1 | `src/ui/shell.rs:764-766` | Tanpa dedupe **in-flight**: pengecekan hanya pada cache memori, bukan pada request yang masih berjalan | Dasar: `covers.contains_key(&cid)` baru terisi setelah fetch selesai (`:799-802`). Skenario: skip cepat 3 track dari album yang sama (cover sama) → 3× download penuh + 3× `rounded_cover` + 3× tulis disk. Est. 3× trafik & 3× kerja CPU untuk 1 cover | Tambah `HashSet<String>` "in-flight" di `HomeState`; insert sebelum spawn, hapus setelah selesai |

### P2

| ID | Sev | file:line | Pola | Dampak (estimasi) + dasar estimasi | Rekomendasi |
|---|---|---|---|---|---|
| **PF-11** | P2 | `src/ui/shell.rs:302-313`; `src/overlay/mod.rs:311-335`; `:737-763`; `:860-866` | Overlay di-`cx.notify()` pada **setiap** `PlayerEvent::Position` (10 Hz); tiap render memanggil `all_rows()` yang meng-clone seluruh baris lirik + 2 `Vec` baru | Dasar: tick 100 ms di `src/player/engine.rs:110` mengirim `Position` (`:116`); handler memanggil `st.sync_position(...)` + `cx.notify()` (`src/ui/shell.rs:307-313`); `all_rows()` = `lines.iter().map(|..| VisibleRow { text: line.text.clone(), .. }).collect()` (`overlay/mod.rs:311-335`); `heights`/`offsets` dialokasikan ulang per render (`:752-753`); saat baris aktif berganti, animasi 380 ms (`SCROLL_MS`, `:46`, `:860`) memaksa repaint 60 fps. Utilitas CPU kecil (perlu profiling) tapi konstan | Notify hanya bila `active_row` berubah (posisi saja tidak mempengaruhi tampilan); kirim irisan/`&[Line]` tanpa clone; cache `heights`/`offsets` selama `lines` & lebar viewport tidak berubah |
| **PF-12** | P2 | `src/ui/components/lyrics_view.rs:205-254`, `:193` | Per baris per render: `line.text.clone()` + `format!("lyric-{i}-{is_active}")`; mode `Plain` meng-clone seluruh transkrip | Dasar: 2 alokasi `String` per baris per render; transkrip 100 baris → ~200 alokasi/render, dan makin sering saat animasi berjalan | Gunakan `&str`/`SharedString`; id animasi dari tuple/`u64` hash, bukan `format!`; `plain_text` sebagai `Arc<str>` |
| **PF-13** | P2 | `src/ui/components/lyrics_view.rs:109-127` | `set_position` memakai **linear scan** O(n) — komentarnya sendiri menulis "binary search" | Dasar: `for (i, line) in self.lines.iter().enumerate()` (`:117-123`) berhenti di baris pertama yang waktunya > posisi → biaya O(jumlah baris sebelum posisi); dipanggil 10×/s (`src/ui/shell.rs:296`). 300 baris × 10 Hz = ~3.000 pembandingan/detik (kecil, tapi komentar menyesatkan) | `partition_point` (binary search) pada `lines[i].time`, atau simpan indeks terakhir dan scan lokal karena posisi monoton naik |
| **PF-14** | P2 | `src/ui/shell.rs:3279-3325`; kartu: `:3952-4045`; baris: `:4047-4113` | Grid album tanpa virtualisasi & tanpa memoization: `limit = source.len()` → ≤ 100 kartu dibangun ulang tiap render; ~4–5 alokasi `String` per kartu | Dasar: `limit` = seluruh album ter-load (`:3294`), dibatasi 100 oleh fetch (`:3724`, `:3766`); per kartu: `format!("album:{}", aid)` (`:3298`), `.id(format!(...))` (`:3302`), `.aria_label(format!(...))` (`:3317`), `title.to_string()`/`artist.to_string()` (`:4036`, `:4043`), initial uppercase (`:3958-3962`) → ~500 alokasi + ~1.500–2.000 elemen per render Shell. Render Shell terjadi pada tiap `cx.notify` (mis. tiap ketikan, `:2849`) | Id/aria dari `SharedString` atau hash `u64`; pertimbangkan virtualisasi/`uniform_list` bila daftar > 200; memoize bagian statis |
| **PF-15** | P2 | `src/player/queue.rs:209-211`; pemanggil `src/ui/shell.rs:953` | `Queue::ids()` meng-clone **semua** id ke `HashSet<String>` di main thread setiap refill radio | Dasar: `items.iter().map(|t| t.id.clone()).collect()`; n = panjang queue. n=500 (`MAX_QUEUE`) ≈ 500 alokasi (trivial); n=20.000 (playlist besar, lihat `tests/stress.rs:622`) ≈ 20.000 alokasi ≈ 1–2 MB per refill | Bangun `HashSet<&str>` selama queue tidak berubah, atau simpan versi id-ter-hash yang di-invalidate saat queue berubah |
| **PF-16** | P2 | `src/ui/shell.rs:2820-2841` | Search hanya dipicu `enter`, **tanpa debounce/coalesce**; Enter berulang = N request (generation guard hanya membuang hasil basi, bukan mencegah request) | Dasar: satu request per Enter (`client.search` → `search3`, `src/navidrome/client.rs:207-221`); guard `search_generation` (`:2825-2834`) membuang hasil lama; invariant ini sudah diuji `two_hundred_searches_keep_only_the_newest_result` + `stale_out_of_order_search_responses_are_discarded` (`tests/stress.rs:348`, `:450`) → yang belum dijaga: jumlah request | Debounce 250–300 ms; batalkan/tahan request in-flight bila query belum berubah |
| **PF-17** | P2 | `src/main.rs:119-135`; kanal `src/platform/tray.rs:127-128` | Loop supervisor 400 ms **selamanya**: 2× `cx.update` + `tray.set_visible` tiap iterasi; kanal kontrol `flume::unbounded` | Dasar: `timeout_add`-style loop `timer(400ms)` (`main.rs:120-123`) → 2,5 iterasi/detik seumur proses (bangunkan main thread + thread tray). `publish_visible` membandingkan state sebelum emit (`tray.rs:826-836`) sehingga **tidak** ada sinyal DBus berulang — jadi biayanya wakeup + pesan kanal, bukan trafik bus (0,7 % CPU idle di docs tetap masuk akal) | Naikkan interval (mis. 1 s) atau ganti polling dengan event window/tray (mis. listener close/visibility) |
| **PF-18** | P2 | `src/ui/shell.rs:262-264` → `src/platform/mpris.rs:168-170`, `:110-113` | Posisi MPRIS di-update 10×/detik | Dasar: tiap `PlayerEvent::Position` memanggil `mh.update_position(pos)` → `MprisCommand::UpdatePosition` → `player.set_position(...)` (`mpris.rs:110-113`). Pemain besar biasanya meng-emit `PropertiesChanged` posisi ~1×/s. **Perlu verifikasi runtime** apakah `mpris_server` meng-emit sinyal pada setiap `set_position`; bila ya: 10 sinyal/detik ke session bus + membangunkan setiap klien MPRIS | Kirim maksimum 1×/s atau hanya saat seek; biarkan `Position` dihitung klien dari `Rate` (yang memang disediakan MPRIS) |
| **PF-19** | P2 | `src/ui/shell.rs:505-520`, field `:79`, inisialisasi `:447` | `a11y_focus: RefCell<HashMap<String, FocusHandle>>` tumbuh monoton: setiap key unik disisipkan dan **tidak pernah dihapus** | Dasar: `focus_for` memakai `.entry(key.to_string()).or_insert_with(...)` (`:511-515`) tanpa `retain`/`clear` di mana pun. Key: `album:<id>` (`:3298`), `track-<i>` (`:3543`), `artist:<id>`, dst. Menjelajah 5.000 album → ~5.000 entri (String + handle) ≈ ratusan KB; `track-<i>` menambah n entri per album. Pertumbuhan lambat, bukan kebocoran cepat | Hapus key milik route yang ditinggalkan (mis. `retain` pada perubahan route), atau key berbasis hash `u64` |
| **PF-20** | P2 | `src/ui/shell.rs:3915-3926` (dipakai `:3288`); animasi lain `:3676-3704` | Animasi shimmer skeleton `.repeat()` (900 ms) memaksa repaint berkelanjutan selama state loading | Dasar: `with_animation(... .repeat(), ...)` pada `album_skeleton`. Selama `hs.loading`, tree Shell ikut dirender ulang tiap frame; bila grid sudah berisi 100 kartu (PF-14) bebannya ikut berulang. Terbatas pada durasi loading (detik), jadi bukan kebocoran | Animasi hanya pada elemen skeleton; hentikan setelah beberapa detik atau saat `reduce_motion` aktif; hindari mengikat animasi ke container besar |

---

## 3. Peta thread: apa berjalan di mana (bukti, bukan dugaan)

GPUI mem-poll future `cx.spawn` **di main thread** (doc gpui: `crates/gpui/src/app/context.rs:646`,
`crates/gpui/src/app/async_context.rs:215`), sedangkan `cx.background_executor().spawn(...)`
menjalankan future di thread pool background. Itulah kunci pemetaan di bawah.

| Operasi | Thread | Bukti |
|---|---|---|
| `ureq` → Navidrome (list, album, artist, cover, search, radio) | background | `src/ui/shell.rs:2831`, `:3723`, `:3782`, `:1210`, `:967`; semua di dalam `background_executor().spawn` |
| `ureq` → LRCLIB | background | `src/ui/shell.rs:874-878` membungkus `p.fetch(&t)` di `background_executor()` |
| `image::load_from_memory` + mask + PNG encode (`rounded_cover`) | **main** | `src/ui/shell.rs:806` (di dalam `cx.update`), `:842` (di `load_and_play`, dipanggil dari handler klik) |
| `fs::create_dir_all`, `fs::write` (cover), `save_config` | **main** | `:786-788`, `:3801-3803`, `:654` — body `cx.spawn` setelah `await` kembali ke foreground |
| `LyricsCache::read`/`write_raw` (file lirik) | background | dipanggil di dalam `p.fetch()` (`src/lyrics/provider.rs:116-170`) yang sudah di background |
| `radio.lock()` | **main** (`:710`, `:735`, `:942`) vs holder background (`:966`) | lihat PF-02 |
| GStreamer: pipeline + tick 100 ms | thread pemain (`src/player/engine.rs:48`) | `glib::timeout_add_local(100ms)` (`:110`) → kirim `Position`/`Duration` ke flume |
| MPRIS (zbus) | thread sendiri, tokio `current_thread` (`src/platform/mpris.rs:26-30`) | command lewat `flume`, jadi main thread tidak menunggu DBus |
| Tray SNI/DBus | thread `myuzik-tray` (`src/platform/tray.rs:139-142`), worker `control.recv()` (`:792`) | **kecuali** handshake awal `Tray::start` → main thread, `recv_timeout(10s)` (`:146`) → PF-09 |
| `Tray` jembatan | `flume::unbounded` dua arah (`:127-128`) | pesan dari main thread tidak memblok, tapi tidak ada backpressure |

`std::thread::sleep` yang muncul di grep hanya berada di `#[cfg(test)] mod tests`
(`src/platform/tray.rs:916` ke atas — baris 1258/1260/1334/1432), **bukan** jalur produksi.
`Rc::new`/`RefCell` hanya dipakai local (`src/ui/shell.rs:79`).

---

## 4. Pertumbuhan tak terbatas: apa yang benar-benar dibatasi

| Struktur | Batas | Status |
|---|---|---|
| Cover LRU di memori (`HomeState::covers`/`cover_order`) | 200 entri, evict tertua | **Aman** — `src/ui/shell.rs:4124` (`COVER_CACHE_CAP`), `:4140-4152`; diuji `tests/stress.rs:533` |
| Cache cover **di disk** (`~/.cache/myuzik/covers`) | **tidak ada** | **Tumbuh tanpa batas** + write-only → PF-05 |
| Cache lirik di disk (`~/.cache/myuzik/lyrics`) | Ada fungsi `prune`, **tidak pernah dipanggil** | **Tumbuh tanpa batas** → PF-06 |
| Queue putar (`Queue`) | `MAX_QUEUE = 500` dipangkas hanya saat refill radio | **Terbatas dalam praktik** — `src/player/queue.rs:7`, `trim_front_to` di `src/ui/shell.rs:984`, `:995`. `play_now` sendiri tidak memangkas, jadi playlist 20 000 track tetap 20 000 item (≈ 4 MB, aman) |
| Riwayat radio | `RECENT_CAP = 100`, `BATCH = 20`, `MAX_EMPTY_BATCHES = 3`, `MAX_GENRE_PAGES = 3`, `MAX_RANDOM_DRAWS = 2` | **Aman** — `src/library/radio.rs:29-39`; diuji `tests/recent_history_is_bounded_and_deduped` (`radio.rs:266`) |
| `a11y_focus` (HashMap handle fokus) | tidak ada | Tumbuh monoton tapi lambat → PF-19 |
| Channel flume (player, tray, mpris) | `unbounded` semua | Risiko teoretis bila konsumen lebih lambat dari produsen; pada pola pesan saat ini (per track/state) tidak ada produsen deras. Bukan temuan, tapi perlu diingat bila menambah event per-frame |

---

## 5. Startup (apa yang terjadi sebelum/di sekitar frame pertama)

Urutan aktual dari `src/main.rs`:

1. `tracing_subscriber` init + `Paths::new`/`ensure` (buat 4 direktori) + `load_config` — sebelum `app.run` (`main.rs:32-47`). Kecil; config rusak → fallback default (`:41-47`), perilaku baik.
2. Di dalam `app.run`: `add_fonts` dua TTF (`:52-58`) → `spawn_player` (thread, kembali cepat) → `start_mpris` (thread, kembali cepat, `mpris.rs:26-30`) → `Shell::new`.
3. `Shell::new` (`src/ui/shell.rs:83-450`): `load_password` **dua kali** (file read, `:176` dan `:185`), membangun `NavidromeClient` dua kali (`:179`, `:186`), `LyricsCache::new` (belum I/O), lalu **`spawn_home_fetch` langsung** (`:221-229`).
4. `open_window` (`main.rs:90-95`).
5. **`Tray::start`** (`main.rs:106`) → handshake DBus sinkron di main thread (≤ 10 s) → PF-09.
6. Loop supervisor 400 ms (§PF-17) dan reader `TrayCommand` di-spawn (`main.rs:119-197`).

Kesimpulan statis: tidak ada "kerja berat" besar sebelum `open_window` **pada mesin sehat** (font + thread spawn). Yang perlu diwaspadai ada tiga: `Tray::start` yang sinkron (PF-09), dua `load_password` + dua `NavidromeClient::new` yang redundan (murah, tapi mudah dirapikan), dan 24 tulis cover di main thread saat grid terisi (PF-04).

---

## 6. N+1, debounce/coalesce, dan backpressure

- **Fetch list awal**: 4 request berurutan (`src/ui/shell.rs:3724`, `:3753`, `:3759`, `:3766`) → bisa paralel.
- **Cover**: N+1 melekat pada API Subsonic (1 request per cover art) — tidak bisa dihindari, tetapi **eksekusinya berurutan** (PF-07) dan **tanpa dedupe** (PF-10) dan **tanpa baca cache disk** (PF-05).
- **Radio**: `next_batch` hingga 7 request berurutan per refill (`radio.rs:100-200`) — mahal, tapi ada **coalescing nyata**: `radio_in_flight.swap(true)` + `radio_continue_pending` (`src/ui/shell.rs:925-931`), jadi klik beruntun tidak menumpuk refill. Yang perlu diperbaiki adalah lock-nya (PF-02), bukan jumlah request-nya.
- **Search**: tanpa debounce, tapi hanya pada `enter` (PF-16); generation guard sudah menutup race hasil basi (diuji di `tests/stress.rs:348`, `:450`).
- **Lirik**: ~4–5 request LRCLIB per track + 1 Navidrome, dengan agent baru tiap track (PF-08).
- **Backpressure**: semua kanal `unbounded`; tidak ada antrean yang bisa meledak hari ini karena produsen event adalah per-track/per-state, bukan per-frame.

---

## 7. Klaim `docs/performance.md`: masih valid atau usang?

| # | Klaim (baris) | Status | Bukti |
|---|---|---|---|
| 1 | Budget: idle ≤ 200 MB, playing+overlay ≤ 250 MB, startup ≤ 1,5 s (`:11-13`) | **Tidak bisa diverifikasi statis** | angka hasil ukur; audit ini tidak menjalankan binary. Yang bisa dikatakan: tidak ada struktur yang *pasti* melampaui budget, kecuali skenario gagal di PF-09 |
| 2 | Cover LRU 200 entri, evict least-recently-added (`:125`) | **VALID** | `COVER_CACHE_CAP = 200` (`src/ui/shell.rs:4124`), `insert_cover` (`:4140-4152`), diuji `tests/stress.rs:533` |
| 3 | Cache lirik di disk: 500 file / 50 MB terbaru, dipangkas saat startup dan setelah tulis (`:126`) | **USANG / TIDAK BENAR** | `LyricsCache::prune` (`src/lyrics/cache.rs:85`) **tidak pernah dipanggil** — dead code (PF-06). Klaim yang sama dicetak `scripts/measure-ram.sh:299` |
| 4 | Paging album `size ≤ 500` per `getAlbumList2` (`:127`) | **SEBAGIAN VALID** | `album_list(size, offset)` meneruskan nilai apa adanya (`src/navidrome/client.rs:138-154`) tanpa enforcement; UI hanya memakai `size=100, offset=0` (`src/ui/shell.rs:3724`, `:3766`) dan **tidak pernah** memakai `offset` → kapabilitas paging ada di client & diuji (`tests/stress.rs:274`), tapi aplikasi tidak memakainya |
| 5 | Ukuran hasil search `songCount=40`, `albumCount=20`, `artistCount=20` (`:128`) | **VALID** | `src/navidrome/client.rs:212-214`; diuji `tests/stress.rs:387-389` |
| 6 | Unduhan cover ≤ 8 MB per cover (`:129`) | **VALID** | `take(8 * 1024 * 1024)` di `src/navidrome/client.rs:356` |
| 7 | `tests/stress.rs` berisi 6 test (`:96`) | **VALID** | `grep -c '#\[test\]'` = 6; nama-nama sesuai tabel dokumen |
| 8 | Tabel "what each test proves" (`:103-110`) sebagai invariant | **VALID sebagai invariant, bukan bukti performa** | test memeriksa bound/urutan (`tests/stress.rs:204-660`); dokumen sendiri mengakui "tests are not micro-benchmarks" (`:145`) |
| 9 | "Whole suite 6 passed in ~1,0 s" (`:112`) | **Tidak diverifikasi** | butuh menjalankan test; di luar batas tugas ini |
| 10 | Angka terukur (133 MB, 0,7 % CPU, 1,12 s, 33 thread/51 fd) (`:68-73`) | **Tidak diverifikasi** | hasil ukur mesin lain waktu lain |
| 11 | Timeout HTTP (tidak diklaim eksplisit di docs, tapi menjadi prasyarat "Known limits") | **TERPENUHI di kode** | `src/navidrome/client.rs:48-56` (8 s/15 s/25 s) dan `src/lyrics/provider.rs:198-205` (8 s/12 s/20 s) |
| 12 | "Known limits": RSS tanpa memori GPU, startup diukur sampai window bukan frame, soak 24 jam tidak otomatis, test bukan micro-benchmark (`:131-148`) | **VALID / jujur** | sesuai kode & konfigurasi yang ada |
| 13 | (Klaim yang *tidak* ada, jadi celah) Batas cache cover **di disk** | **Celah** | tidak ada prune untuk `paths.covers` (`src/config.rs:66`, `:78`); tabel "Bounded structures" (`:123-129`) tidak menyebutnya, padahal direktori itu tumbuh terus → PF-05 |

Kesimpulan baseline: `docs/performance.md` sebagian besar masih akurat **kecuali** klaim prune
cache lirik (butir 3) yang tidak didukung kode, dan klaim paging album (butir 4) yang benar di
level client tetapi tidak dipakai aplikasi.

---

## 8. Yang sudah baik (jangan diubah arah desainnya)

- **Tidak ada HTTP di render/update path.** Semua `ureq` berada di `background_executor` (§3). Ini pencapaian desain yang benar dan harus dipertahankan.
- **Timeout eksplisit di semua fase HTTP** untuk Navidrome dan LRCLIB (commit `ec077db`), plus body dibatasi `take(8 MiB)` — menghilangkan kelas hang "loading selamanya".
- **Cover LRU 200 di memori** dengan eviction benar dan diuji; `MAX_QUEUE` dipangkas saat radio menambah; riwayat radio dibatasi beberapa konstanta.
- **Coalescing radio** (`radio_in_flight` + `radio_continue_pending`) dan **generation guard search** — dua pola anti-thundering-herd yang sudah ada dan diuji.
- **Config rusak tidak menggagalkan startup** (`src/main.rs:41-47`).
- **Tidak ada `unwrap()`/`expect()` di jalur I/O runtime**: sisa `unwrap`/`expect` hanya di test (`src/navidrome/types.rs:373-404`, `src/config.rs:170-171`) dan `gst::bus().expect("bus")` di thread pemain (`src/player/engine.rs:70`) yang bila gagal menghasilkan `PlayerEvent::Error`.
- **MPRIS dan tray di thread sendiri**, main thread tidak menunggu DBus (kecuali handshake tray di PF-09).

---

## 9. Tidak diverifikasi / perlu profiling

1. **Semua angka** di dokumen ini adalah estimasi statis. Tidak ada profiler (`perf`, `flamegraph`), tidak ada `heaptrack`, tidak ada pengukuran RSS/CPU/frame-time.
2. **Frekuensi render Shell** tidak bisa dipastikan statis: `Entity::update` di GPUI **tidak** memanggil `cx.notify()` (hanya helper `write` yang melakukannya, `crates/gpui/src/app/entity_map.rs:512-519`), sehingga render dipicu oleh `cx.notify()` eksplisit dan event UI. Berapa kali per detik pada pemakaian nyata → perlu instrumentasi.
3. **Biaya `rounded_cover`** (PF-01) — orde puluhan ms adalah perkiraan dari struktur (3 lintasan + zlib), bukan pengukuran. Perlu profiling dengan cover 512 px nyata.
4. **Apakah `mpris_server::set_position` meng-emit `PropertiesChanged`** tiap panggilan (PF-18) — perlu `dbus-monitor` saat runtime.
5. **Waktu `Tray::start`** di mesin tanpa StatusNotifierWatcher / bus lambat (PF-09) — hanya alur kode yang dianalisis (batas 10 s pasti dari konstanta), bukan waktu tempuh nyata.
6. **Perilaku disk lambat/penuh** untuk PF-04/PF-05 — perlu uji dengan `~/.cache` di tmpfs/NFS dan disk penuh.
7. **Biaya layout GPUI** untuk 100 kartu album + 2.000 elemen: tidak diukur; GPUI melakukan layout taffy per frame dan tidak ada virtualisasi di sini.
8. **Tidak dijalankan**: `cargo build --release`, GUI, benchmark, `scripts/measure-ram.sh`. **Dijalankan**: `cargo check --offline` (exit 0, 0,41 s, cache hangat) — hanya membuktikan kompilasi.
9. **Cakupan graph**: `index_status` menunjukkan index `full`, generation cocok, dan `check_index_coverage` untuk 17 path bukti melaporkan `no_recorded_issue` (generation match, best-effort). Namun **fan-in hotspot dari `get_architecture` tidak dapat dipercaya apa adanya** di repo ini: nama method pendek bertabrakan (`Entity::read`, `Vec::len`, `cx.new`, `AppConfig::default`), sehingga mis. `LyricsCache.read` tercatat punya 29 pemanggil yang sebenarnya adalah `self.home_state.read(cx)`, dan `Queue.len` 48 pemanggil yang sebagian besar `Vec::len`. Semua klaim di dokumen ini karena itu diverifikasi ulang dengan membaca pemanggil sebenarnya (grep + baca sumber), bukan dari angka fan-in.
10. **Index bisa basi**: `indexed_at = 2026-11-10T18:27:42Z`, sedangkan HEAD (`e096e20`) bertanggal 2026-11-11T01:25+07:00 (= 2026-11-10T18:25Z) — masih lebih tua dari index, jadi aman, tetapi watcher tidak aktif (`watch.watched = false`), sehingga commit berikutnya tidak akan otomatis masuk graph.

---

## 10. Urutan perbaikan yang disarankan

1. **PF-02** (lock radio di main thread) — risikonya freeze multi-detik dan perbaikannya lokal (`snapshot → unlock → HTTP → lock`).
2. **PF-01** (`rounded_cover` di main thread) — hitch di setiap ganti track; pindahkan ke background + gate pada overlay terbuka.
3. **PF-03** (O(n²) clone track rows) — satu perubahan `Arc<Vec<Track>>` menghapus seluruh kelas masalah.
4. **PF-06 + PF-05 + `docs/performance.md`** — pasang `prune`, jadikan cache cover disk cache-first, lalu perbaiki klaim dokumen agar sesuai kode.
5. **PF-04, PF-09, PF-07, PF-10, PF-08** — tulis file & handshake tray ke background, batasi konkurensi cover, dedupe in-flight, satu `Agent` LRCLIB.
6. P2 sesuai kesempatan (dimulai dari PF-11/PF-13 karena murah dan sering berjalan).

---

*Dokumen ini adalah analisis statis tanpa profiling runtime, bukan hasil pengukuran. Disusun dengan
skill `codebase-memory` + MCP `codebase-memory-mcp` (project
`home-samvivan-Documents-Myuzik`: `index_status`, `get_architecture(hotspots)`, `trace_path`,
`search_graph`, `check_index_coverage`), lalu diverifikasi ulang dengan membaca sumber dan grep.*
