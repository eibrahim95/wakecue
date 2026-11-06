use crate::calendar::{egypt_offset_min, utc_to_egypt, weekday};

/// Bit 0 = Sunday .. bit 6 = Saturday, by the Egypt date of the event.
pub const ALL_DAYS: u8 = 0b0111_1111;

/// The five daily prayers, in table order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prayer {
    Fajr,
    Dhuhr,
    Asr,
    Maghrib,
    Isha,
}

impl Prayer {
    pub const ALL: [Prayer; 5] = [Prayer::Fajr, Prayer::Dhuhr, Prayer::Asr, Prayer::Maghrib, Prayer::Isha];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The name as it reads on the panel and in the settings page.
    pub fn name(self) -> &'static str {
        match self {
            Prayer::Fajr => "Fajr",
            Prayer::Dhuhr => "Dhuhr",
            Prayer::Asr => "Asr",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isha => "Isha",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// A prayer plus this many minutes (negative = before).
    Prayer(Prayer, i16),
    /// Minutes after midnight, Egypt local time (summer time handled).
    Local(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Ring ramps for `ramp_min` minutes and finishes at the rule's time.
    Sunrise { ramp_min: u16 },
    Play { track: u16, volume: u8, fade_in_s: u16 },
    LampOn,
    LampOff,
    Alarm,
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub days: u8,
    pub when: When,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Minutes since the Unix epoch, UTC, when the action must start.
    pub utc_min: i64,
    pub action: Action,
}

/// Prayer table: per day five little-endian u16 (Fajr, Dhuhr, Asr, Maghrib, Isha), minutes after 00:00 UTC.
pub struct PrayerTable<'a> {
    pub first_day: i32,
    pub data: &'a [u8],
}

/// Bytes per day in a prayer table.
pub const TABLE_STRIDE: usize = 10;

/// First four bytes of a table file, from `prayer-gen`.
pub const BLOB_MAGIC: &[u8; 4] = b"WCP1";
/// Magic, then the first day as an i32, before the per-day times.
pub const BLOB_HEADER: usize = 8;

impl<'a> PrayerTable<'a> {
    /// Read a table file from `prayer-gen`: magic, first day (i32 little-endian), then the days.
    ///
    /// The day range travels with the times, so regenerating over another range cannot leave the
    /// firmware looking up the right prayer on the wrong day.
    pub fn from_blob(blob: &'a [u8]) -> Option<PrayerTable<'a>> {
        if blob.len() < BLOB_HEADER || !blob.starts_with(BLOB_MAGIC) {
            return None;
        }
        let first_day = i32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
        let data = &blob[BLOB_HEADER..];
        if data.is_empty() || data.len() % TABLE_STRIDE != 0 {
            return None;
        }
        Some(PrayerTable { first_day, data })
    }

    pub fn utc_min(&self, prayer: Prayer, day: i32) -> Option<u16> {
        let i = usize::try_from(day.checked_sub(self.first_day)?).ok()?;
        let at = i * TABLE_STRIDE + prayer.index() * 2;
        let b = self.data.get(at..at + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
}

/// The instant (UTC minutes) when `rule` reaches its time on the UTC day `day`, if it does.
fn target_min(rule: &Rule, day: i32, prayers: &PrayerTable) -> Option<i64> {
    let base = day as i64 * 1440;
    match rule.when {
        When::Prayer(p, off) => Some(base + prayers.utc_min(p, day)? as i64 + off as i64),
        When::Local(m) => {
            // The Egypt offset can change on this very day, so test the day's local midnight.
            let off = egypt_offset_min(base + m as i64 - 120);
            Some(base + m as i64 - off)
        }
    }
}

/// The next of the five prayers at or after `from_min`, with its time in UTC minutes.
///
/// Unlike [`next_event`] this ignores the rules and the switches: it answers "which prayer comes
/// next", which is what the face shows. A prayer is reported at its own time, not at the minute
/// its ramp starts.
pub fn next_prayer(from_min: i64, prayers: &PrayerTable) -> Option<(i64, Prayer)> {
    let first = from_min.div_euclid(1440) as i32;
    // Five prayers a day are well inside two days of lookahead, but a table that ends gets the
    // same answer either way: none.
    for day in first..first + 2 {
        let base = day as i64 * 1440;
        let mut best: Option<(i64, Prayer)> = None;
        for p in Prayer::ALL {
            let Some(m) = prayers.utc_min(p, day) else { continue };
            let at = base + m as i64;
            if at < from_min {
                continue;
            }
            if best.is_none_or(|(b, _)| at < b) {
                best = Some((at, p));
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

/// The five prayer times of the local day `now_min` falls on, as local minutes after midnight.
///
/// A table entry is one local date's prayers in UTC minutes, so the day to read is the local date,
/// not the UTC one: at 23:30 UTC the lamp is already in tomorrow's date and its strip should say
/// so. `None` per prayer where the table does not reach.
pub fn local_times(now_min: i64, prayers: &PrayerTable) -> [Option<i64>; 5] {
    let day = utc_to_egypt(now_min).div_euclid(1440) as i32;
    core::array::from_fn(|i| {
        let m = prayers.utc_min(Prayer::ALL[i], day)? as i64;
        // The offset in force at the prayer's own hour, so the two days a year the clocks change
        // keep the right hour on either side of the switch.
        Some(m + egypt_offset_min(day as i64 * 1440 + m - 120))
    })
}

/// The earliest event whose start is at or after `from_min`. Looks three days ahead.
/// The firmware asks with `now + 1` after running what is due, so an event is never programmed twice.
pub fn next_event(from_min: i64, rules: &[Rule], prayers: &PrayerTable) -> Option<Event> {
    let first = from_min.div_euclid(1440) as i32 - 1;
    let mut best: Option<Event> = None;
    for day in first..first + 5 {
        for rule in rules {
            let Some(target) = target_min(rule, day, prayers) else { continue };
            let start = match rule.action {
                Action::Sunrise { ramp_min } => target - ramp_min as i64,
                _ => target,
            };
            if start < from_min { continue; }
            // Weekday of the Egypt date of the target.
            let local = target + egypt_offset_min(target);
            let wd = weekday(local.div_euclid(1440) as i32);
            if rule.days & (1 << wd) == 0 { continue; }
            if best.map_or(true, |b| start < b.utc_min) {
                best = Some(Event { utc_min: start, action: rule.action });
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::days_from_civil;

    /// A table where every prayer on every day is `fajr` minutes plus 100 per prayer index.
    fn table(bytes: &mut [u8; 50], first_day: i32, fajr: [u16; 5]) -> PrayerTable<'_> {
        for (d, f) in fajr.iter().enumerate() {
            for p in 0..5 {
                let at = d * TABLE_STRIDE + p * 2;
                bytes[at..at + 2].copy_from_slice(&(f + 100 * p as u16).to_le_bytes());
            }
        }
        PrayerTable { first_day, data: bytes }
    }

    const RULES: [Rule; 3] = [
        Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Sunrise { ramp_min: 15 } },
        Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Play { track: 1, volume: 20, fade_in_s: 60 } },
        Rule { days: 0b0001_1111, when: When::Local(7 * 60 + 30), action: Action::Alarm },
    ];

    #[test]
    fn the_strip_reads_the_local_day_at_its_own_offset() {
        let mut b = [0u8; 50];
        let t = table(&mut b, days_from_civil(2026, 1, 1), [217, 217, 217, 217, 217]);
        // 10:00 UTC in winter: Cairo is +2, so 03:37 UTC reads as 05:37 and the rest follow.
        let now = days_from_civil(2026, 1, 1) as i64 * 1440 + 600;
        assert_eq!(
            local_times(now, &t),
            [Some(337), Some(437), Some(537), Some(637), Some(737)]
        );

        // 23:30 UTC on 31 December is already 1 January in Cairo, so the strip is that day's.
        let new_year_eve = days_from_civil(2025, 12, 31) as i64 * 1440 + 23 * 60 + 30;
        assert_eq!(local_times(new_year_eve, &t)[0], Some(337));

        // A day the table does not reach has nothing to show.
        let far = days_from_civil(2030, 1, 1) as i64 * 1440;
        assert_eq!(local_times(far, &t), [None; 5]);
    }

    #[test]
    fn table_lookup_and_bounds() {
        let mut b = [0u8; 50];
        let t = table(&mut b, 100, [5, 6, 7, 8, 300]);
        assert_eq!(t.utc_min(Prayer::Fajr, 100), Some(5));
        assert_eq!(t.utc_min(Prayer::Isha, 100), Some(405));
        assert_eq!(t.utc_min(Prayer::Maghrib, 104), Some(600));
        assert_eq!(t.utc_min(Prayer::Fajr, 105), None);
        assert_eq!(t.utc_min(Prayer::Fajr, 99), None);
    }

    #[test]
    fn blob_header_carries_the_first_day() {
        let mut blob = [0u8; BLOB_HEADER + 2 * TABLE_STRIDE];
        blob[..4].copy_from_slice(BLOB_MAGIC);
        blob[4..8].copy_from_slice(&100i32.to_le_bytes());
        for day in 0..2 {
            for p in 0..5 {
                let at = BLOB_HEADER + day * TABLE_STRIDE + p * 2;
                blob[at..at + 2].copy_from_slice(&(5 + 100 * p as u16).to_le_bytes());
            }
        }
        let t = PrayerTable::from_blob(&blob).unwrap();
        assert_eq!(t.first_day, 100);
        assert_eq!(t.utc_min(Prayer::Fajr, 100), Some(5));
        assert_eq!(t.utc_min(Prayer::Isha, 101), Some(405));
        assert_eq!(t.utc_min(Prayer::Fajr, 102), None);
    }

    #[test]
    fn a_blob_with_the_wrong_magic_or_length_is_refused() {
        assert!(PrayerTable::from_blob(&[]).is_none());
        // Header only, no days.
        let mut bare = [0u8; BLOB_HEADER];
        bare[..4].copy_from_slice(BLOB_MAGIC);
        assert!(PrayerTable::from_blob(&bare).is_none());
        // Right length, wrong magic.
        let mut wrong = [0u8; BLOB_HEADER + TABLE_STRIDE];
        wrong[..4].copy_from_slice(b"WCP0");
        assert!(PrayerTable::from_blob(&wrong).is_none());
        // One byte short of a whole day.
        let mut short = [0u8; BLOB_HEADER + TABLE_STRIDE - 1];
        short[..4].copy_from_slice(BLOB_MAGIC);
        assert!(PrayerTable::from_blob(&short).is_none());
    }

    #[test]
    fn prayer_names_match_the_table_order() {
        assert_eq!(Prayer::ALL.map(|p| p.name()), ["Fajr", "Dhuhr", "Asr", "Maghrib", "Isha"]);
    }

    #[test]
    fn next_prayer_is_the_clock_face_answer() {
        // `table` takes one Fajr per day and adds an hour per prayer after it, so day 100 is
        // Fajr 00:05, Dhuhr 01:45, Asr 03:25, Maghrib 05:05, Isha 06:45 (UTC minutes).
        let mut b = [0u8; 50];
        let t = table(&mut b, 100, [5, 6, 7, 8, 300]);
        let base = 100 * 1440;
        assert_eq!(next_prayer(base, &t), Some((base + 5, Prayer::Fajr)));
        assert_eq!(next_prayer(base + 6, &t), Some((base + 105, Prayer::Dhuhr)));
        assert_eq!(next_prayer(base + 300, &t), Some((base + 305, Prayer::Maghrib)));
        assert_eq!(next_prayer(base + 306, &t), Some((base + 405, Prayer::Isha)));
        // After Isha the answer is tomorrow's Fajr, which is what the face has to say out loud.
        assert_eq!(next_prayer(base + 406, &t), Some((base + 1440 + 6, Prayer::Fajr)));
    }

    #[test]
    fn next_prayer_skips_the_day_that_has_gone() {
        let mut b = [0u8; 50];
        let t = table(&mut b, 100, [5, 6, 7, 8, 300]);
        // One minute before midnight on day 100, the next prayer is day 101's Fajr at 00:06.
        assert_eq!(next_prayer(101 * 1440 - 1, &t), Some((101 * 1440 + 6, Prayer::Fajr)));
        // The table covers days 100 to 104; past its last day no prayer can be named.
        assert_eq!(next_prayer(105 * 1440, &t), None);
    }

    #[test]
    fn next_prayer_without_a_table_is_none() {
        let empty = PrayerTable { first_day: 0, data: &[] };
        assert_eq!(next_prayer(20454 * 1440, &empty), None);
    }

    #[test]
    fn ramp_starts_fifteen_minutes_before_fajr_then_sound_then_alarm() {
        // Sunday 25 Oct 2026, Fajr 03:10 UTC every day in this test table.
        let d = days_from_civil(2026, 10, 25);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]);
        let base = d as i64 * 1440;

        let e = next_event(base, &RULES, &t).unwrap();
        assert_eq!(e, Event { utc_min: base + 190 - 15, action: Action::Sunrise { ramp_min: 15 } });

        let e = next_event(e.utc_min + 1, &RULES, &t).unwrap();
        assert_eq!(e.utc_min, base + 190);
        assert!(matches!(e.action, Action::Play { .. }));

        // 07:30 Egypt on Sunday 25 Oct is still summer time (UTC+3) = 04:30 UTC.
        let e = next_event(e.utc_min + 1, &RULES, &t).unwrap();
        assert_eq!(e, Event { utc_min: base + 4 * 60 + 30, action: Action::Alarm });
    }

    #[test]
    fn local_alarm_follows_the_clock_across_the_end_of_summer_time() {
        // Fri 30 Oct 2026 is winter time: 07:30 local = 05:30 UTC. Thursday 29 Oct is still summer: 04:30 UTC.
        let rules = [Rule { days: ALL_DAYS, when: When::Local(7 * 60 + 30), action: Action::Alarm }];
        let thu = days_from_civil(2026, 10, 29);
        let t = PrayerTable { first_day: 0, data: &[] };
        let e = next_event(thu as i64 * 1440, &rules, &t).unwrap();
        assert_eq!(e.utc_min, thu as i64 * 1440 + 4 * 60 + 30);
        let e = next_event(e.utc_min + 1, &rules, &t).unwrap();
        assert_eq!(e.utc_min, (thu as i64 + 1) * 1440 + 5 * 60 + 30);
    }

    #[test]
    fn day_mask_skips_friday_and_saturday() {
        // Sun-Thu only: after Thursday's alarm the next is Sunday's, three days later.
        let rules = [Rule { days: 0b0001_1111, when: When::Local(7 * 60 + 30), action: Action::Alarm }];
        let thu = days_from_civil(2026, 1, 8); // winter
        assert_eq!(weekday(thu), 4);
        let t = PrayerTable { first_day: 0, data: &[] };
        let e = next_event(thu as i64 * 1440 + 5 * 60, &rules, &t).unwrap();
        assert_eq!(e.utc_min, thu as i64 * 1440 + 5 * 60 + 30);
        let e = next_event(e.utc_min + 1, &rules, &t).unwrap();
        assert_eq!(e.utc_min, (thu as i64 + 3) * 1440 + 5 * 60 + 30);
    }

    #[test]
    fn each_prayer_reads_its_own_column() {
        let d = days_from_civil(2026, 1, 8);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]); // Dhuhr 290, Asr 390, Maghrib 490, Isha 590 UTC
        let rules = [
            Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Asr, 0), action: Action::Alarm },
            Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Isha, -10), action: Action::LampOn },
        ];
        let base = d as i64 * 1440;
        let e = next_event(base, &rules, &t).unwrap();
        assert_eq!(e, Event { utc_min: base + 390, action: Action::Alarm });
        let e = next_event(e.utc_min + 1, &rules, &t).unwrap();
        assert_eq!(e, Event { utc_min: base + 580, action: Action::LampOn });
    }

    #[test]
    fn no_events_when_the_table_is_missing_and_only_fajr_rules() {
        let rules = [Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Alarm }];
        let t = PrayerTable { first_day: 0, data: &[] };
        assert_eq!(next_event(20454 * 1440, &rules, &t), None);
    }
}
