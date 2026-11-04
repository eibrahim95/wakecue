//! Day numbers are days since 1970-01-01 (the Unix epoch); minutes are minutes since it, in UTC.

pub fn days_from_civil(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u32;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i32 - 719468
}

pub fn civil_from_days(z: i32) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Sunday .. 6 = Saturday. 1970-01-01 was a Thursday.
pub fn weekday(day: i32) -> u8 {
    (day + 4).rem_euclid(7) as u8
}

/// Day number of the last given weekday (0 = Sunday) of a month.
fn last_weekday(y: i32, m: u32, wd: u8) -> i32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let last = days_from_civil(ny, nm, 1) - 1;
    last - (weekday(last) + 7 - wd) as i32 % 7
}

/// Egypt's summer time (law 2023): from 00:00 on the last Friday of April
/// until 24:00 on the last Thursday of October (so it ends at 00:00 Friday).
/// Returns the UTC offset in minutes: 120 in winter, 180 in summer.
pub fn egypt_offset_min(utc_min: i64) -> i64 {
    let day = utc_min.div_euclid(1440) as i32;
    let (y, _, _) = civil_from_days(day);
    // Start: local 00:00 Friday (UTC+2) = 22:00 UTC the day before.
    let start = (last_weekday(y, 4, 5) as i64) * 1440 - 120;
    // End: local 24:00 Thursday in summer time (UTC+3) = 21:00 UTC that Thursday.
    let end = (last_weekday(y, 10, 4) as i64) * 1440 + 21 * 60;
    if utc_min >= start && utc_min < end { 180 } else { 120 }
}

/// Minutes of local (Egypt) time for a UTC minute count.
pub fn utc_to_egypt(utc_min: i64) -> i64 {
    utc_min + egypt_offset_min(utc_min)
}

/// Width of the text `format_minutes` writes.
pub const FORMAT_LEN: usize = 16;

fn put(buf: &mut [u8; FORMAT_LEN], at: usize, width: usize, v: u32) {
    let mut v = v;
    for i in (0..width).rev() {
        buf[at + i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
}

/// Write `min` (minutes since the Unix epoch) as `YYYY-MM-DD HH:MM` into `buf` and return it.
///
/// No zone is applied: pass UTC for UTC, or `utc_to_egypt(min)` for Cairo. Written for the
/// 1970-01-01..2100 range the clock can hold; a time before it would print a nonsense year.
pub fn format_minutes(min: i64, buf: &mut [u8; FORMAT_LEN]) -> &str {
    let (y, mo, d) = civil_from_days(min.div_euclid(1440) as i32);
    let m = min.rem_euclid(1440) as u32;
    put(buf, 0, 4, y as u32);
    buf[4] = b'-';
    put(buf, 5, 2, mo);
    buf[7] = b'-';
    put(buf, 8, 2, d);
    buf[10] = b' ';
    put(buf, 11, 2, m / 60);
    buf[13] = b':';
    put(buf, 14, 2, m % 60);
    core::str::from_utf8(buf).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_day_numbers() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 1, 1), 20454);
        assert_eq!(days_from_civil(2045, 12, 31), 20454 + 7304);
        assert_eq!(civil_from_days(20454), (2026, 1, 1));
    }

    #[test]
    fn round_trip_every_day_to_2100() {
        for d in 0..50000 {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn weekdays() {
        assert_eq!(weekday(days_from_civil(2026, 10, 25)), 0); // Sunday
        assert_eq!(weekday(days_from_civil(2026, 10, 29)), 4); // Thursday
        assert_eq!(weekday(days_from_civil(2026, 4, 24)), 5); // Friday
    }

    fn utc(y: i32, m: u32, d: u32, hh: i64, mm: i64) -> i64 {
        days_from_civil(y, m, d) as i64 * 1440 + hh * 60 + mm
    }

    #[test]
    fn dst_2026() {
        // Starts Fri 24 Apr 2026 00:00 local (= 22:00 UTC on the 23rd).
        assert_eq!(egypt_offset_min(utc(2026, 4, 23, 21, 59)), 120);
        assert_eq!(egypt_offset_min(utc(2026, 4, 23, 22, 0)), 180);
        // Ends Thu 29 Oct 2026 24:00 local (= 21:00 UTC on the 29th).
        assert_eq!(egypt_offset_min(utc(2026, 10, 29, 20, 59)), 180);
        assert_eq!(egypt_offset_min(utc(2026, 10, 29, 21, 0)), 120);
        assert_eq!(egypt_offset_min(utc(2026, 7, 1, 0, 0)), 180);
        assert_eq!(egypt_offset_min(utc(2026, 1, 1, 0, 0)), 120);
    }

    #[test]
    fn formats_a_minute_count_as_a_date_and_time() {
        let mut b = [0u8; FORMAT_LEN];
        assert_eq!(format_minutes(0, &mut b), "1970-01-01 00:00");
        assert_eq!(format_minutes(utc(2026, 11, 4, 23, 9), &mut b), "2026-11-04 23:09");
        // Zero padding on every field, and the last minute of a day.
        assert_eq!(format_minutes(utc(2026, 1, 5, 0, 0), &mut b), "2026-01-05 00:00");
        assert_eq!(format_minutes(utc(2026, 1, 5, 0, 0) - 1, &mut b), "2026-01-04 23:59");
        // A leap day, and the end of the range the clock can hold.
        assert_eq!(format_minutes(utc(2024, 2, 29, 12, 0), &mut b), "2024-02-29 12:00");
        assert_eq!(format_minutes(utc(2099, 12, 31, 23, 59), &mut b), "2099-12-31 23:59");
    }

    #[test]
    fn cairo_is_two_or_three_hours_ahead_of_the_same_minute() {
        let mut b = [0u8; FORMAT_LEN];
        let winter = utc(2026, 11, 4, 23, 9);
        assert_eq!(format_minutes(winter, &mut b), "2026-11-04 23:09");
        assert_eq!(format_minutes(utc_to_egypt(winter), &mut b), "2026-11-05 01:09");
        let summer = utc(2026, 7, 1, 23, 9);
        assert_eq!(format_minutes(utc_to_egypt(summer), &mut b), "2026-07-02 02:09");
    }

    #[test]
    fn dst_2027() {
        // Last Friday of April 2027 is the 30th; last Thursday of October 2027 is the 28th.
        assert_eq!(egypt_offset_min(utc(2027, 4, 29, 21, 59)), 120);
        assert_eq!(egypt_offset_min(utc(2027, 4, 29, 22, 0)), 180);
        assert_eq!(egypt_offset_min(utc(2027, 10, 28, 20, 59)), 180);
        assert_eq!(egypt_offset_min(utc(2027, 10, 28, 21, 0)), 120);
    }
}
