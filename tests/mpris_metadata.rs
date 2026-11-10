use myuzik::library::Track;
use myuzik::platform::mpris::{metadata_for, position_us};
use std::time::Duration;

fn sample_track() -> Track {
    Track {
        id: "abc".into(),
        title: "Paper Satellites".into(),
        artist: "Kite Garden".into(),
        artist_id: None,
        album: "Moss & Static".into(),
        album_id: None,
        cover_art_id: Some("cover1".into()),
        duration: Duration::from_secs(247),
        track_number: Some(2),
        disc_number: Some(1),
        year: Some(2026),
        starred: false,
        genre: None,
    }
}

#[test]
fn metadata_maps_all_fields() {
    let meta = metadata_for(&sample_track(), Some("file:///tmp/cover.png".into()));
    assert_eq!(meta.title(), Some("Paper Satellites"));
    assert_eq!(meta.artist(), Some(vec!["Kite Garden".to_string()]));
    assert_eq!(meta.album(), Some("Moss & Static"));
    assert_eq!(
        meta.art_url().map(|u| u.to_string()),
        Some("file:///tmp/cover.png".to_string())
    );
    assert_eq!(meta.length().map(|t| t.as_micros()), Some(247_000_000));
    assert_eq!(meta.track_number(), Some(2));
    assert_eq!(meta.disc_number(), Some(1));
}

#[test]
fn position_is_microseconds() {
    assert_eq!(position_us(Duration::from_millis(1500)), 1_500_000);
}
