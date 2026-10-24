//! Timestamps. Every persisted timestamp is UTC RFC 3339 with exactly three
//! fractional digits and a `Z` suffix (e.g. `2026-10-24T14:26:54.213Z`), so
//! lexicographic order equals chronological order. This matches the
//! `strftime('%Y-%m-%dT%H:%M:%fZ')` format used by schema version 1.

use time::macros::format_description;
use time::OffsetDateTime;

pub fn now() -> String {
    format(OffsetDateTime::now_utc())
}

pub fn format(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap_or_default()
}

/// Compact UTC timestamp for file names (`20261024T142654Z`).
pub fn file_stamp() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_utc_millis() {
        let at = OffsetDateTime::from_unix_timestamp_nanos(1_793_000_000_123_456_789).unwrap();
        assert_eq!(format(at), "2026-10-26T07:33:20.123Z");
        assert_eq!(now().len(), 24);
        assert!(now().ends_with('Z'));
    }
}
