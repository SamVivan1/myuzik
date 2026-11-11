# Cache library, art cover, dan Sync

Desain ini meniru pendekatan **gelly** (`~/Downloads/gelly`, client Jellyfin Rust):
seluruh metadata library disimpan dalam satu snapshot JSON di disk, dimuat saat
startup, dan **pencarian dilakukan lokal di memori**. Audio **tidak** di-cache —
tetap di-stream dari server saat lagu dipilih.

## Yang diambil dari gelly

| Ide gelly | Diadopsi di Myuzik |
|---|---|
| `LibraryCache` menyimpan `library.json` (satu file, seluruh library) | `src/library/cache.rs` → `library.json` di direktori cache |
| Library dimuat ke memori saat startup → daftar instan | `Shell::new` memuat snapshot sebelum frame pertama |
| Pencarian = filter lokal, bukan request server | `src/library/search_index.rs` → `SearchIndex::search` |
| `ImageCache`: coba disk dulu, dedupe request, batasi konkurensi | `read_cover_from_disk` dipakai sebelum HTTP di 3 jalur cover |
| Audio di-stream saat dipilih | tidak diubah: `client.stream_url` tetap per-track |

Yang **tidak** diambil: gelly memakai `Rc<RefCell<...>>` untuk library di UI
thread (GTK single-threaded). Myuzik memakai `Arc<SearchIndex>` + snapshot di
disk, karena GPUI mem-poll future di main thread dan task fetch berjalan di
background executor.

## Alur

### Startup
1. `LibraryCache::load()` membaca `library.json`.
2. Snapshot ada → `HomeState.albums/artists/playlists` diisi langsung, dan
   `SearchIndex::build(...)` dijalankan. Daftar tampil **tanpa menunggu server**.
3. `spawn_home_fetch` tetap berjalan sebagai refresh latar (tidak memblokir).

Snapshot yang rusak, bukan UTF-8, atau beda `SNAPSHOT_VERSION` **tidak dihapus** —
`load()` mengembalikan `Err`, aplikasi lanjut dengan default dan sync berikutnya
menimpanya.

### Tombol Sync (satu tombol, di header Home)
`Shell::sync_library`:
1. `syncing` guard (klik kedua tidak melakukan apa-apa).
2. Tarik semua halaman `getAlbumList2` (500 per halaman, batas 50 000 album).
3. `getArtists` + `getPlaylists`.
4. Untuk setiap album: `getAlbum` → kumpulkan track (album yang gagal dilewati,
   tidak menggagalkan seluruh sync).
5. `LibraryCache::save` — **atomik**: tulis `library.json.tmp` → `sync_all` →
   `rename`, jadi crash di tengah tidak pernah meninggalkan cache separuh.
6. `SearchIndex::build` dibangun ulang, `HomeState` diperbarui, toast berisi
   ringkasan jumlah.

Satu request per album adalah satu-satunya cara API Subsonic memberi daftar lagu
lengkap, jadi sync sengaja eksplisit dan dilaporkan progresnya — bukan sesuatu
yang jalan diam-diam saat startup.

### Pencarian
`schedule_search` memakai `SearchIndex` bila ada isinya: hasil **instan**, tanpa
jaringan, dan bekerja offline. Server hanya ditanya bila belum ada snapshot
(run pertama, sebelum sync pertama). Debounce 250 ms tetap ada.

`SearchIndex` (dari agent, 11 test): case-insensitive, **diacritic-insensitive**
("bjork" menemukan "Björk"), multi-kata = AND ("miles kind" → "Kind of Blue"
oleh Miles Davis), ranking exact > prefix > substring, cap 20 per grup,
deterministik, 20k lagu tetap cepat.

### Art cover
Dulu direktori cover **hanya ditulis, tidak pernah dibaca** (temuan PF-05), jadi
setiap launch mengunduh ulang semuanya. Sekarang ketiga jalur (track, detail
album, grid Home) memanggil `read_cover_from_disk` lebih dulu; HTTP hanya jika
file belum ada. Hasil decode+mask tetap di-memo (LRU 32) dan dikerjakan di
background executor.

### Audio
Tidak berubah dan memang tidak di-cache: `load_and_play` memakai
`client.stream_url(&track.id, quality)` lalu mengirim `PlayerCommand::Load` ke
GStreamer. Cache hanya menyentuh metadata dan gambar.

## Berkas

| Berkas | Isi |
|---|---|
| `src/library/cache.rs` | `LibrarySnapshot`, `LibraryCache` (load/save atomik/clear), `is_stale` |
| `src/library/search_index.rs` | `SearchIndex` (build/search, folding, ranking, cap) |
| `src/ui/shell.rs` | muat snapshot saat startup, `sync_library`, `fetch_full_library`, pencarian lokal, `read_cover_from_disk`, tombol Sync |
| `tests/library_cache.rs` | 18 test cache |
| `tests/search_index.rs` | 11 test index |
| `tests/library_cache_flow.rs` | 3 test alur end-to-end |

## Batas & hal yang belum dikerjakan

- **Belum ada sync otomatis/berkala.** Sync hanya lewat tombol (sesuai
  permintaan). `LibrarySnapshot::is_stale` sudah tersedia kalau nanti mau
  menampilkan "cache berumur N hari".
- **Cover tidak pernah di-prune** — direktori cover tumbuh seiring jumlah cover
  unik yang pernah dilihat. Belum ada batas ukuran seperti pada cache lirik.
- **Sync satu request per album** (sekuensial). Library 1000 album berarti 1000
  request; bisa dipercepat dengan konkurensi terbatas bila perlu.
- **Tidak ada migrasi snapshot lama**: beda versi = Err, lalu sync ulang.
- `cargo test` butuh stack thread lebih besar (`.cargo/config.toml` mengatur
  `RUST_MIN_STACK=32 MiB`): pohon elemen GPUI cukup dalam sehingga stack default
  2 MiB milik thread test bisa overflow saat satu kontrol ditambahkan. Aplikasi
  sendiri berjalan di main thread dan tidak terpengaruh.
