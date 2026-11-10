//! LRC parsing.
//!
//! Handles the formats that actually turn up in the wild, because the overlay
//! renders whatever comes out of here:
//!
//! * `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]` and `[h:mm:ss.xxx]` stamps, several
//!   per line (`[00:10][00:20]same text`), and stamps that appear mid-line;
//! * the `[offset:±ms]` header tag, applied to every line;
//! * the enhanced-LRC word timing tags (`<00:12.80>`) which are stripped: they
//!   are karaoke word timing, not text;
//! * metadata tags (`[ar:]`, `[ti:]`, …) which are ignored;
//! * blank lines and whitespace-only lines, which are dropped rather than shown
//!   as empty rows in the overlay;
//! * a UTF-8 BOM and CRLF line endings.

use crate::lyrics::model::LyricLine;
use std::time::Duration;

/// A leading `[offset:...]` tag shifts every timestamp; LRC defines it as
/// "+ms means the lyrics appear earlier", so it is subtracted from the stamps.
#[derive(Debug, Default, Clone, Copy)]
pub struct ParseOptions {
    /// Extra shift in milliseconds applied on top of any `[offset:]` tag.
    pub extra_offset_ms: i64,
}

pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    parse_lrc_with(text, ParseOptions::default())
}

pub fn parse_lrc_with(text: &str, options: ParseOptions) -> Vec<LyricLine> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // The [offset:] tag applies to the whole file, so read it before the lines.
    let tag_offset_ms = header_offset_ms(text);
    let shift_ms = options.extra_offset_ms - tag_offset_ms;

    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.trim_end_matches('\r');
        for (time, value) in timed_entries(raw) {
            if value.is_empty() {
                // A stamp with no text is a gap marker; the overlay draws gaps
                // from the timing itself, so an empty row would just be noise.
                continue;
            }
            let shifted = shift_time(time, shift_ms);
            // Some files repeat a line verbatim; show it once.
            if lines
                .last()
                .is_some_and(|prev| prev.text == value && prev.time == shifted)
            {
                continue;
            }
            lines.push(LyricLine {
                time: shifted,
                text: value,
            });
        }
    }
    lines.sort_by_key(|l| l.time);
    lines.dedup_by(|a, b| a.time == b.time && a.text == b.text);
    lines
}

/// Every `(time, text)` pair on one physical line.
///
/// The rules the LRC format implies, all of which occur in real files:
///
/// * a run of stamps at the start shares one text (`[00:10][00:20]same`),
/// * a stamp in the middle starts a new text (`[00:10]a[00:20]b`),
/// * text before any stamp is untimed and therefore dropped (`a[00:10]b`),
/// * stamps that do not parse are metadata tags (`[ar:]`), not timing.
fn timed_entries(raw: &str) -> Vec<(Duration, String)> {
    let mut entries: Vec<(Duration, String)> = Vec::new();
    // Stamps collected for the text currently being accumulated.
    let mut stamps: Vec<Duration> = Vec::new();
    let mut text = String::new();
    let mut index = 0usize;

    while index < raw.len() {
        let rest = &raw[index..];
        let stamp = rest
            .strip_prefix('[')
            .and_then(|after| after.find(']').map(|close| (close, &after[..close])))
            .and_then(|(close, body)| parse_stamp(body).map(|time| (close, time)));

        match stamp {
            Some((close, time)) => {
                if stamps.is_empty() && !text.is_empty() {
                    // Text with no stamp of its own: untimed, so drop it.
                    text.clear();
                } else if !stamps.is_empty() && !text.is_empty() {
                    // A new stamp after text closes the previous segment.
                    let value = clean_text(&text);
                    for t in stamps.drain(..) {
                        entries.push((t, value.clone()));
                    }
                    text.clear();
                }
                stamps.push(time);
                index += close + 2; // '[' + body + ']'
            }
            None => {
                // A metadata tag is skipped without splitting the segment; any
                // other character is text.
                if rest.starts_with('[')
                    && let Some(close) = rest.find(']')
                    && parse_stamp(&rest[1..close]).is_none()
                {
                    index += close + 1;
                    continue;
                }
                let ch = rest.chars().next().expect("non-empty slice");
                text.push(ch);
                index += ch.len_utf8();
            }
        }
    }

    if !stamps.is_empty() {
        let value = clean_text(&text);
        for t in stamps {
            entries.push((t, value.clone()));
        }
    }
    entries
}

/// Strip the enhanced-LRC word tags and trim the result.
fn clean_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn header_offset_ms(text: &str) -> i64 {
    for raw in text.split('\n').take(20) {
        let raw = raw.trim();
        let Some(body) = raw.strip_prefix("[offset:") else {
            continue;
        };
        let Some(value) = body.strip_suffix(']') else {
            continue;
        };
        if let Ok(ms) = value.trim().parse::<i64>() {
            return ms;
        }
    }
    0
}

fn shift_time(time: Duration, shift_ms: i64) -> Duration {
    let ms = time.as_millis() as i64 + shift_ms;
    Duration::from_millis(ms.max(0) as u64)
}

fn parse_stamp(s: &str) -> Option<Duration> {
    // mm:ss.xxx or mm:ss or h:mm:ss.xxx
    let s = s.trim();
    // Hours and minutes must be numbers; the seconds field may carry a fraction.
    let parts: Vec<&str> = s.split(':').collect();
    let (hours, minutes, seconds) = match parts.len() {
        2 => (0u64, parts[0].parse::<u64>().ok()?, parts[1]),
        3 => (
            parts[0].parse::<u64>().ok()?,
            parts[1].parse::<u64>().ok()?,
            parts[2],
        ),
        _ => return None,
    };
    let (sec, frac) = if let Some(dot) = seconds.find('.') {
        (&seconds[..dot], &seconds[dot + 1..])
    } else {
        (seconds, "")
    };
    let sec: u64 = sec.parse().ok()?;
    let millis = fraction_to_ms(frac);
    let total_ms = hours * 3_600_000 + minutes * 60_000 + sec * 1000 + millis;
    Some(Duration::from_millis(total_ms))
}

fn fraction_to_ms(frac: &str) -> u64 {
    if frac.is_empty() {
        return 0;
    }
    // take up to 3 digits, pad
    let mut s = frac.chars().take(3).collect::<String>();
    while s.len() < 3 {
        s.push('0');
    }
    s.parse::<u64>().unwrap_or(0)
}

pub fn active_line(lines: &[LyricLine], position: Duration) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let mut found = None;
    // binary search
    let mut lo = 0usize;
    let mut hi = lines.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if lines[mid].time <= position {
            found = Some(mid);
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    found
}

pub fn apply_offset(position: Duration, offset_ms: i64) -> Duration {
    if offset_ms >= 0 {
        position + Duration::from_millis(offset_ms as u64)
    } else {
        position.saturating_sub(Duration::from_millis((-offset_ms) as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_timestamps_and_text() {
        let lines = parse_lrc("[00:12.50] Hello\n[01:05] World");
        assert_eq!(lines[0].time, Duration::from_millis(12500));
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].time, Duration::from_secs(65));
        assert_eq!(lines[1].text, "World");
    }

    #[test]
    fn supports_centiseconds_ms_hours_multi() {
        assert_eq!(
            parse_lrc("[00:01.5] a")[0].time,
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_lrc("[00:01.005] b")[0].time,
            Duration::from_millis(1005)
        );
        assert_eq!(parse_lrc("[60:00] c")[0].time, Duration::from_secs(3600));
        let lines = parse_lrc("[00:10][00:20] la");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time, Duration::from_secs(10));
        assert_eq!(lines[1].time, Duration::from_secs(20));
    }

    #[test]
    fn ignores_metadata_and_drops_blank_lines() {
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("just a plain line").is_empty());
        // A stamp with no text, blank lines and whitespace are all dropped: the
        // overlay must never render empty rows.
        let lines = parse_lrc("[ar:Someone]\n[00:00.00]\n\n   \n[00:05.00] real");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "real");
        assert_eq!(lines[0].time, Duration::from_secs(5));
    }

    #[test]
    fn sorts_by_time() {
        let lines = parse_lrc("[00:30] b\n[00:10] a");
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[1].text, "b");
    }

    #[test]
    fn header_offset_tag_shifts_every_line() {
        // "+250" means the lyrics should appear 250 ms earlier.
        let lines = parse_lrc("[offset:+250]\n[00:10.00] a\n[00:20.00] b");
        assert_eq!(lines[0].time, Duration::from_millis(9_750));
        assert_eq!(lines[1].time, Duration::from_millis(19_750));
        // A negative tag delays them, and the shift never goes below zero.
        let lines = parse_lrc("[offset:-500]\n[00:00.10] a");
        assert_eq!(lines[0].time, Duration::from_millis(600));
        let lines = parse_lrc("[offset:+5000]\n[00:01.00] a");
        assert_eq!(lines[0].time, Duration::ZERO);
    }

    #[test]
    fn strips_enhanced_lrc_word_timing() {
        // Enhanced LRC: the <mm:ss.xx> tags are word timings, not lyrics.
        let lines = parse_lrc("[00:10.00]<00:10.00>Hello <00:10.80>world");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello world");
    }

    #[test]
    fn handles_inline_stamps_and_text_before_them() {
        let lines = parse_lrc("[00:10.00]first[00:20.00]second");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "first");
        assert_eq!(lines[0].time, Duration::from_secs(10));
        assert_eq!(lines[1].text, "second");
        assert_eq!(lines[1].time, Duration::from_secs(20));
    }

    #[test]
    fn drops_consecutive_duplicate_lines() {
        let lines = parse_lrc("[00:10.00]same\n[00:10.00]same");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn strips_bom_and_handles_crlf() {
        let lines = parse_lrc("\u{feff}[00:10.00] a\r\n[00:20.00] b\r\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[1].text, "b");
    }

    #[test]
    fn extra_offset_option_stacks_on_the_header_tag() {
        let lines = parse_lrc_with(
            "[offset:+1000]\n[00:10.00] a",
            ParseOptions {
                extra_offset_ms: 500,
            },
        );
        // -1000 from the tag, +500 from the option.
        assert_eq!(lines[0].time, Duration::from_millis(9_500));
    }

    #[test]
    fn active_line_boundaries() {
        let lines = parse_lrc("[00:10] a\n[00:20] b\n[00:30] c");
        assert_eq!(active_line(&lines, Duration::from_secs(5)), None);
        assert_eq!(active_line(&lines, Duration::from_secs(10)), Some(0));
        assert_eq!(active_line(&lines, Duration::from_millis(19900)), Some(0));
        assert_eq!(active_line(&lines, Duration::from_secs(25)), Some(1));
        assert_eq!(active_line(&lines, Duration::from_secs(100)), Some(2));
        assert_eq!(active_line(&[], Duration::from_secs(5)), None);
    }

    #[test]
    fn offset_clamps_at_zero() {
        assert_eq!(
            apply_offset(Duration::from_secs(1), -500),
            Duration::from_millis(500)
        );
        assert_eq!(
            apply_offset(Duration::from_millis(100), -500),
            Duration::ZERO
        );
        assert_eq!(
            apply_offset(Duration::from_secs(1), 500),
            Duration::from_millis(1500)
        );
    }
}
