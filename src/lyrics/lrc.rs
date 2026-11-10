use crate::lyrics::model::LyricLine;
use std::time::Duration;

pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.trim_end_matches('\r');
        // collect stamps
        let mut stamps: Vec<Duration> = Vec::new();
        let mut rest = raw;
        // parse leading [..] stamps
        while rest.starts_with('[') {
            if let Some(end) = rest.find(']') {
                let stamp = &rest[1..end];
                if let Some(t) = parse_stamp(stamp) {
                    stamps.push(t);
                } else {
                    // metadata tag like [ar:...]; stop stamp parsing
                    break;
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        let value = rest.trim().to_string();
        for t in stamps {
            lines.push(LyricLine {
                time: t,
                text: value.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.time);
    lines
}

fn parse_stamp(s: &str) -> Option<Duration> {
    // mm:ss.xxx or mm:ss or h:mm:ss.xxx
    let s = s.trim();
    // hours?
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
    // seconds may have fraction
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
    let mut s = frac.to_string();
    if s.len() > 3 {
        s.truncate(3);
    }
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
    fn ignores_metadata_blank_untimed() {
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("just a plain line").is_empty());
        let lines = parse_lrc("[ar:Someone]\n[00:00.00]");
        // metadata line produces no stamp; untimed timestamp line produces empty text
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "");
    }

    #[test]
    fn sorts_by_time() {
        let lines = parse_lrc("[00:30] b\n[00:10] a");
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[1].text, "b");
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
