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
    fn dst_2027() {
        // Last Friday of April 2027 is the 30th; last Thursday of October 2027 is the 28th.
        assert_eq!(egypt_offset_min(utc(2027, 4, 29, 21, 59)), 120);
        assert_eq!(egypt_offset_min(utc(2027, 4, 29, 22, 0)), 180);
        assert_eq!(egypt_offset_min(utc(2027, 10, 28, 20, 59)), 180);
        assert_eq!(egypt_offset_min(utc(2027, 10, 28, 21, 0)), 120);
    }
}
