use std::time::Duration;

/// Format duration as HH:MM:SS
pub(crate) fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}")
}

/// Parse duration from HH:MM:SS format
pub(crate) fn parse_duration(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: u64 = parts[0].parse().ok()?;
    let mins: u64 = parts[1].parse().ok()?;
    let secs: u64 = parts[2].split('.').next()?.parse().ok()?;

    Some(Duration::from_secs(hours * 3600 + mins * 60 + secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_duration(Duration::from_secs(61)), "00:01:01");
        assert_eq!(format_duration(Duration::from_secs(3661)), "01:01:01");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("00:00:00"), Some(Duration::from_secs(0)));
        assert_eq!(parse_duration("00:01:01"), Some(Duration::from_secs(61)));
        assert_eq!(parse_duration("01:01:01"), Some(Duration::from_secs(3661)));
        assert_eq!(
            parse_duration("00:00:30.123"),
            Some(Duration::from_secs(30))
        );
    }
}
