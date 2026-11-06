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

fn put(buf: &mut [u8], at: usize, width: usize, v: u32) {
    let mut v = v;
    for i in (0..width).rev() {
        buf[at + i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
}

fn append(buf: &mut [u8], at: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        buf[*at] = b;
        *at += 1;
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

/// Width of the text `format_hhmm` writes.
pub const HHMM_LEN: usize = 5;

/// Write `min` as `HH:MM` into `buf`; no zone is applied.
pub fn format_hhmm(min: i64, buf: &mut [u8; HHMM_LEN]) -> &str {
    let m = min.rem_euclid(1440) as u32;
    put(buf, 0, 2, m / 60);
    buf[2] = b':';
    put(buf, 3, 2, m % 60);
    core::str::from_utf8(buf).unwrap_or("")
}

/// Width of the longest text `format_hhmm12` writes, `12:45`.
pub const HHMM12_LEN: usize = 5;

/// Write `min` as a 12-hour time, `4:05` or `12:45`, with no leading zero on the hour; no zone is
/// applied, and [`meridiem`] says which half of the day it is.
pub fn format_hhmm12(min: i64, buf: &mut [u8; HHMM12_LEN]) -> &str {
    let m = min.rem_euclid(1440) as u32;
    let hour = match (m / 60) % 12 {
        0 => 12,
        h => h,
    };
    let mut n = 0;
    let w = if hour < 10 { 1 } else { 2 };
    put(buf, n, w, hour);
    n += w;
    buf[n] = b':';
    n += 1;
    put(buf, n, 2, m % 60);
    n += 2;
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// `AM` or `PM` for a minute count. Noon is PM and midnight is AM, as a clock reads.
pub fn meridiem(min: i64) -> &'static str {
    if (min.rem_euclid(1440) / 60) < 12 {
        "AM"
    } else {
        "PM"
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Width of the longest text `format_date` writes, `Wed 30 Sep 2026`.
pub const DATE_LEN: usize = 15;

/// Write `min`'s date as `Thu 5 Nov 2026` into `buf`; no zone is applied.
pub fn format_date(min: i64, buf: &mut [u8; DATE_LEN]) -> &str {
    let day = min.div_euclid(1440) as i32;
    let (y, mo, d) = civil_from_days(day);
    let mut n = 0;
    append(buf, &mut n, WEEKDAYS[weekday(day) as usize]);
    append(buf, &mut n, " ");
    let w = if d < 10 { 1 } else { 2 };
    put(buf, n, w, d);
    n += w;
    append(buf, &mut n, " ");
    append(buf, &mut n, MONTHS[mo as usize - 1]);
    append(buf, &mut n, " ");
    put(buf, n, 4, y as u32);
    n += 4;
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// Width of the longest text `format_countdown` writes, `in 13h57m`.
pub const COUNTDOWN_LEN: usize = 13;

/// How long until a prayer, as the face says it: `in 25 min`, `in 13h57m`, or `now`.
///
/// Past an hour the form goes compact because the face's next-prayer line shares its width with the
/// prayer's name, a `tomorrow` and the meridiem.
pub fn format_countdown(mins: i64, buf: &mut [u8; COUNTDOWN_LEN]) -> &str {
    let m = mins.max(0) as u32;
    let mut n = 0;
    if m == 0 {
        append(buf, &mut n, "now");
    } else if m < 60 {
        append(buf, &mut n, "in ");
        let w = if m < 10 { 1 } else { 2 };
        put(buf, n, w, m);
        n += w;
        append(buf, &mut n, " min");
    } else {
        append(buf, &mut n, "in ");
        let h = m / 60;
        let w = if h < 10 { 1 } else { 2 };
        put(buf, n, w, h);
        n += w;
        buf[n] = b'h';
        n += 1;
        put(buf, n, 2, m % 60);
        n += 2;
        buf[n] = b'm';
        n += 1;
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_reads_like_a_clock() {
        let mut b = [0u8; COUNTDOWN_LEN];
        assert_eq!(format_countdown(0, &mut b), "now");
        assert_eq!(format_countdown(-3, &mut b), "now");
        assert_eq!(format_countdown(1, &mut b), "in 1 min");
        assert_eq!(format_countdown(25, &mut b), "in 25 min");
        assert_eq!(format_countdown(59, &mut b), "in 59 min");
        assert_eq!(format_countdown(60, &mut b), "in 1h00m");
        assert_eq!(format_countdown(837, &mut b), "in 13h57m");
    }

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
    fn formats_a_clock_time_and_a_spoken_date() {
        let mut hhmm = [0u8; HHMM_LEN];
        let mut date = [0u8; DATE_LEN];
        // Midnight and a past-the-hour time both zero-pad.
        assert_eq!(format_hhmm(utc(2026, 11, 4, 23, 16), &mut hhmm), "23:16");
        assert_eq!(format_hhmm(utc(2026, 11, 4, 0, 0), &mut hhmm), "00:00");
        assert_eq!(format_hhmm(utc(2026, 11, 4, 9, 5), &mut hhmm), "09:05");
        // One-digit and two-digit days, and the longest text the buffer holds.
        assert_eq!(format_date(utc(2026, 11, 4, 23, 16), &mut date), "Wed 4 Nov 2026");
        assert_eq!(format_date(utc(2026, 9, 30, 0, 0), &mut date), "Wed 30 Sep 2026");
        assert_eq!(format_date(utc(2024, 2, 29, 12, 0), &mut date), "Thu 29 Feb 2024");
        assert_eq!(format_date(utc(2026, 1, 4, 0, 0), &mut date), "Sun 4 Jan 2026");
    }

    #[test]
    fn the_date_follows_cairo_past_midnight() {
        let mut date = [0u8; DATE_LEN];
        let mut hhmm = [0u8; HHMM_LEN];
        let local = utc_to_egypt(utc(2026, 11, 4, 23, 16));
        assert_eq!(format_date(local, &mut date), "Thu 5 Nov 2026");
        assert_eq!(format_hhmm(local, &mut hhmm), "01:16");
    }

    #[test]
    fn twelve_hour_clock_reads_the_way_a_clock_does() {
        let mut b = [0u8; HHMM12_LEN];
        // Midnight and noon are both "12", and the hour loses its leading zero.
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 0, 0), &mut b), "12:00");
        assert_eq!(meridiem(utc(2026, 11, 5, 0, 0)), "AM");
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 0, 59), &mut b), "12:59");
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 12, 0), &mut b), "12:00");
        assert_eq!(meridiem(utc(2026, 11, 5, 12, 0)), "PM");
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 9, 5), &mut b), "9:05");
        assert_eq!(meridiem(utc(2026, 11, 5, 9, 5)), "AM");
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 23, 59), &mut b), "11:59");
        assert_eq!(meridiem(utc(2026, 11, 5, 23, 59)), "PM");
        assert_eq!(format_hhmm12(utc(2026, 11, 5, 13, 7), &mut b), "1:07");
        assert_eq!(meridiem(utc(2026, 11, 5, 13, 7)), "PM");
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
