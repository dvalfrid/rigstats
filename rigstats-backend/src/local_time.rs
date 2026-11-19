//! Local time for everything the app shows or logs (#753: moved off chrono,
//! which is unmaintained). Built on `time`; the local offset comes from
//! Windows for each instant, so a timestamp from before a DST change gets the
//! offset it had then, not today's.

use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Date, OffsetDateTime, UtcOffset};

/// `2026-08-22 14:32:05`
pub const SECONDS: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
/// `2026-08-22 14:32`
pub const MINUTES: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]");
/// `2026-08-22`
pub const DATE: &[BorrowedFormatItem<'_>] = format_description!("[year]-[month]-[day]");

/// `utc` in the local time zone, or `None` if Windows can't say the offset.
fn to_local(utc: OffsetDateTime) -> Option<OffsetDateTime> {
    let offset = UtcOffset::local_offset_at(utc).ok()?;
    Some(utc.to_offset(offset))
}

/// The local time now. Falls back to UTC if the offset can't be read (it
/// shouldn't on Windows).
pub fn now() -> OffsetDateTime {
    let utc = OffsetDateTime::now_utc();
    to_local(utc).unwrap_or(utc)
}

/// A Unix timestamp in local time, `None` if it is out of range or the offset
/// can't be read.
pub fn from_unix(secs: u64) -> Option<OffsetDateTime> {
    let utc = OffsetDateTime::from_unix_timestamp(i64::try_from(secs).ok()?).ok()?;
    to_local(utc)
}

/// `dt` in `format`; empty if formatting fails (only for years beyond 9999).
pub fn format(dt: OffsetDateTime, format: &[BorrowedFormatItem<'_>]) -> String {
    dt.format(format).unwrap_or_default()
}

/// A Unix timestamp in local time in `format`; empty if it can't be converted.
pub fn format_unix(secs: u64, fmt: &[BorrowedFormatItem<'_>]) -> String {
    from_unix(secs)
        .map(|dt| format(dt, fmt))
        .unwrap_or_default()
}

/// Today's local date.
pub fn today() -> Date {
    now().date()
}

/// A `YYYY-MM-DD` date, `None` if it doesn't parse.
pub fn parse_date(s: &str) -> Option<Date> {
    Date::parse(s, DATE).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_accepts_only_iso() {
        assert_eq!(
            parse_date("2026-08-22"),
            Date::from_calendar_date(2026, time::Month::August, 22).ok()
        );
        assert_eq!(parse_date("06/16/2026"), None);
        assert_eq!(parse_date("2026-8-22"), None);
        assert_eq!(parse_date(""), None);
    }

    #[test]
    fn formats_have_the_expected_shape() {
        let s = format_unix(1_785_000_000, SECONDS);
        assert_eq!(s.len(), 19, "{s}");
        assert_eq!(format_unix(1_785_000_000, MINUTES), s[..16]);
        assert_eq!(format_unix(1_785_000_000, DATE), s[..10]);
    }

    #[test]
    fn out_of_range_timestamp_is_empty() {
        assert_eq!(format_unix(u64::MAX, SECONDS), "");
    }
}
