use crate::calendar::{Zone, weekday};

/// Bit 0 = Sunday .. bit 6 = Saturday, by the local date of the event.
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
    /// Minutes after midnight on the wall of the stored zone (summer time handled).
    Local(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The lamp comes up over `ramp_min` minutes, finishing at the rule's time. It is the light
    /// half of any alarm, a prayer's or the lamp's own.
    Sunrise { ramp_min: u16 },
    /// A prayer's sound at the prayer's own time. `track` is which prayer it is, 1 to 5, and says
    /// what to call it; `file` is the number in the card's file name that plays.
    Play { track: u16, file: u16, volume: u8, fade_in_s: u16 },
    LampOn,
    LampOff,
    /// A user alarm. `file` is the number in the card's file name and `volume` the level, both
    /// already worked out (the alarm's own choice or the default one), so whoever plays it need
    /// not know about "Default".
    Alarm { file: u16, volume: u8 },
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

/// Where the schedule gets a day's prayer times.
///
/// The lamp computes them from the place it is told it sits at ([`crate::astro::Place`]); the tests
/// hand them in, which is how a rule can be tried on a day that suits it.
///
/// The day is a UTC day and the answer is minutes after its 00:00 UTC, so it may fall outside a
/// single day for a place whose clocks and longitude disagree.
pub trait PrayerTimes {
    fn utc_min(&self, prayer: Prayer, day: i32) -> Option<i64>;
}

/// The instant (UTC minutes) when `rule` reaches its time on the UTC day `day`, if it does.
fn target_min(rule: &Rule, day: i32, prayers: &impl PrayerTimes, zone: Zone) -> Option<i64> {
    let base = day as i64 * 1440;
    match rule.when {
        When::Prayer(p, off) => Some(base + prayers.utc_min(p, day)? + off as i64),
        // The offset can change on this very day, so the zone converts the wall time itself.
        When::Local(m) => Some(zone.to_utc(base + m as i64)),
    }
}

/// The next of the five prayers at or after `from_min`, with its time in UTC minutes.
///
/// Unlike [`next_event`] this ignores the rules and the switches: it answers "which prayer comes
/// next", which is what the face shows. A prayer is reported at its own time, not at the minute
/// its ramp starts.
pub fn next_prayer(from_min: i64, prayers: &impl PrayerTimes) -> Option<(i64, Prayer)> {
    next_prayer_in(from_min, prayers, 0b1_1111)
}

/// Like [`next_prayer`], but only among the prayers whose bit is set in `mask` (a bit per prayer in
/// [`Prayer::ALL`] order, as `Settings::prayers_on`). `None` when the mask is empty.
pub fn next_prayer_in(
    from_min: i64,
    prayers: &impl PrayerTimes,
    mask: u8,
) -> Option<(i64, Prayer)> {
    let first = from_min.div_euclid(1440) as i32;
    // Five prayers a day are well inside two days of lookahead, and a place with no times gets the
    // same answer either way: none.
    for day in first..first + 2 {
        let base = day as i64 * 1440;
        let mut best: Option<(i64, Prayer)> = None;
        for p in Prayer::ALL {
            if mask & (1 << p.index()) == 0 {
                continue;
            }
            let Some(m) = prayers.utc_min(p, day) else { continue };
            let at = base + m;
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
/// The day to ask for is the local date, not the UTC one: at 23:30 UTC the lamp is already in
/// tomorrow's date and its strip should say so. `None` per prayer where the sun never gets there.
pub fn local_times(now_min: i64, prayers: &impl PrayerTimes, zone: Zone) -> [Option<i64>; 5] {
    let day = zone.to_local(now_min).div_euclid(1440) as i32;
    core::array::from_fn(|i| {
        let m = prayers.utc_min(Prayer::ALL[i], day)?;
        // The offset in force at the prayer's own instant, so the two days a year the clocks
        // change keep the right hour on either side of the switch.
        let at = day as i64 * 1440 + m;
        Some(m + zone.offset_min(at))
    })
}

/// The earliest event whose start is at or after `from_min`. Looks three days ahead.
/// The firmware asks with `now + 1` after running what is due, so an event is never programmed twice.
pub fn next_event(from_min: i64, rules: &[Rule], prayers: &impl PrayerTimes, zone: Zone) -> Option<Event> {
    let first = from_min.div_euclid(1440) as i32 - 1;
    let mut best: Option<Event> = None;
    for day in first..first + 5 {
        for rule in rules {
            let Some(target) = target_min(rule, day, prayers, zone) else { continue };
            let start = match rule.action {
                Action::Sunrise { ramp_min } => target - ramp_min as i64,
                _ => target,
            };
            if start < from_min { continue; }
            // Weekday of the local date of the target.
            let local = zone.to_local(target);
            let wd = weekday(local.div_euclid(1440) as i32);
            if rule.days & (1 << wd) == 0 { continue; }
            if best.is_none_or(|b| start < b.utc_min) {
                best = Some(Event { utc_min: start, action: rule.action });
            }
        }
    }
    best
}

/// How far into a light ramp `now_min` is, if one is under way: seconds elapsed and seconds in all.
///
/// The ramp is a start-and-finish pair: [`next_event`] reports only the minute it starts, so a lamp
/// that wakes every minute asks this instead, on every wake, and sets the light to match. A ramp is
/// under way from its start up to, but not at, the minute its rule is due (that minute belongs to
/// the sound). With two ramps overlapping, the one that finishes first wins.
pub fn active_ramp(now_min: i64, rules: &[Rule], prayers: &impl PrayerTimes, zone: Zone) -> Option<(u32, u32)> {
    let first = now_min.div_euclid(1440) as i32 - 1;
    let mut best: Option<(i64, (u32, u32))> = None;
    for day in first..first + 3 {
        for rule in rules {
            let Action::Sunrise { ramp_min } = rule.action else { continue };
            if ramp_min == 0 { continue; }
            let Some(target) = target_min(rule, day, prayers, zone) else { continue };
            let start = target - ramp_min as i64;
            if now_min < start || now_min >= target { continue; }
            let local = zone.to_local(target);
            let wd = weekday(local.div_euclid(1440) as i32);
            if rule.days & (1 << wd) == 0 { continue; }
            if best.is_none_or(|(t, _)| target < t) {
                best = Some((target, (((now_min - start) * 60) as u32, ramp_min as u32 * 60)));
            }
        }
    }
    best.map(|(_, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::days_from_civil;
    use crate::settings::Dst;

    /// The old table file's shape, kept here as the test source of times: the lamp computes its own
    /// now, but a day with Fajr at 03:10 is still the clearest way to test a rule.
    struct Fake<'a> {
        first_day: i32,
        data: &'a [u8],
    }

    const STRIDE: usize = 10;

    impl PrayerTimes for Fake<'_> {
        fn utc_min(&self, prayer: Prayer, day: i32) -> Option<i64> {
            let i = usize::try_from(day.checked_sub(self.first_day)?).ok()?;
            let at = i * STRIDE + prayer.index() * 2;
            let b = self.data.get(at..at + 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]) as i64)
        }
    }

    /// A table where every prayer on every day is `fajr` minutes plus 100 per prayer index.
    fn table(bytes: &mut [u8; 50], first_day: i32, fajr: [u16; 5]) -> Fake<'_> {
        for (d, f) in fajr.iter().enumerate() {
            for p in 0..5 {
                let at = d * STRIDE + p * 2;
                bytes[at..at + 2].copy_from_slice(&(f + 100 * p as u16).to_le_bytes());
            }
        }
        Fake { first_day, data: bytes }
    }

    const RULES: [Rule; 3] = [
        Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Sunrise { ramp_min: 15 } },
        Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Play { track: 1, file: 1, volume: 20, fade_in_s: 60 } },
        Rule { days: 0b0001_1111, when: When::Local(7 * 60 + 30), action: Action::Alarm { file: 101, volume: 22 } },
    ];

    #[test]
    fn the_strip_reads_the_local_day_at_its_own_offset() {
        let mut b = [0u8; 50];
        let t = table(&mut b, days_from_civil(2026, 1, 1), [217, 217, 217, 217, 217]);
        // 10:00 UTC in winter: Cairo is +2, so 03:37 UTC reads as 05:37 and the rest follow.
        let now = days_from_civil(2026, 1, 1) as i64 * 1440 + 600;
        assert_eq!(
            local_times(now, &t, Zone::CAIRO),
            [Some(337), Some(437), Some(537), Some(637), Some(737)]
        );

        // 23:30 UTC on 31 December is already 1 January in Cairo, so the strip is that day's.
        let new_year_eve = days_from_civil(2025, 12, 31) as i64 * 1440 + 23 * 60 + 30;
        assert_eq!(local_times(new_year_eve, &t, Zone::CAIRO)[0], Some(337));

        // A day the table does not reach has nothing to show.
        let far = days_from_civil(2030, 1, 1) as i64 * 1440;
        assert_eq!(local_times(far, &t, Zone::CAIRO), [None; 5]);
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
    fn next_prayer_without_times_is_none() {
        let empty = Fake { first_day: 0, data: &[] };
        assert_eq!(next_prayer(20454 * 1440, &empty), None);
    }

    #[test]
    fn ramp_starts_fifteen_minutes_before_fajr_then_sound_then_alarm() {
        // Sunday 25 Oct 2026, Fajr 03:10 UTC every day in this test table.
        let d = days_from_civil(2026, 10, 25);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]);
        let base = d as i64 * 1440;

        let e = next_event(base, &RULES, &t, Zone::CAIRO).unwrap();
        assert_eq!(e, Event { utc_min: base + 190 - 15, action: Action::Sunrise { ramp_min: 15 } });

        let e = next_event(e.utc_min + 1, &RULES, &t, Zone::CAIRO).unwrap();
        assert_eq!(e.utc_min, base + 190);
        assert!(matches!(e.action, Action::Play { .. }));

        // 07:30 Egypt on Sunday 25 Oct is still summer time (UTC+3) = 04:30 UTC.
        let e = next_event(e.utc_min + 1, &RULES, &t, Zone::CAIRO).unwrap();
        assert_eq!(e, Event { utc_min: base + 4 * 60 + 30, action: Action::Alarm { file: 101, volume: 22 } });
    }

    #[test]
    fn local_alarm_follows_the_clock_across_the_end_of_summer_time() {
        // Fri 30 Oct 2026 is winter time: 07:30 local = 05:30 UTC. Thursday 29 Oct is still summer: 04:30 UTC.
        let rules = [Rule { days: ALL_DAYS, when: When::Local(7 * 60 + 30), action: Action::Alarm { file: 101, volume: 22 } }];
        let thu = days_from_civil(2026, 10, 29);
        let t = Fake { first_day: 0, data: &[] };
        let e = next_event(thu as i64 * 1440, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e.utc_min, thu as i64 * 1440 + 4 * 60 + 30);
        let e = next_event(e.utc_min + 1, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e.utc_min, (thu as i64 + 1) * 1440 + 5 * 60 + 30);
    }

    #[test]
    fn day_mask_skips_friday_and_saturday() {
        // Sun-Thu only: after Thursday's alarm the next is Sunday's, three days later.
        let rules = [Rule { days: 0b0001_1111, when: When::Local(7 * 60 + 30), action: Action::Alarm { file: 101, volume: 22 } }];
        let thu = days_from_civil(2026, 1, 8); // winter
        assert_eq!(weekday(thu), 4);
        let t = Fake { first_day: 0, data: &[] };
        let e = next_event(thu as i64 * 1440 + 5 * 60, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e.utc_min, thu as i64 * 1440 + 5 * 60 + 30);
        let e = next_event(e.utc_min + 1, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e.utc_min, (thu as i64 + 3) * 1440 + 5 * 60 + 30);
    }

    #[test]
    fn each_prayer_reads_its_own_column() {
        let d = days_from_civil(2026, 1, 8);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]); // Dhuhr 290, Asr 390, Maghrib 490, Isha 590 UTC
        let rules = [
            Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Asr, 0), action: Action::Alarm { file: 101, volume: 22 } },
            Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Isha, -10), action: Action::LampOn },
        ];
        let base = d as i64 * 1440;
        let e = next_event(base, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e, Event { utc_min: base + 390, action: Action::Alarm { file: 101, volume: 22 } });
        let e = next_event(e.utc_min + 1, &rules, &t, Zone::CAIRO).unwrap();
        assert_eq!(e, Event { utc_min: base + 580, action: Action::LampOn });
    }

    #[test]
    fn no_events_when_there_are_no_prayer_times_and_only_fajr_rules() {
        let rules = [Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Alarm { file: 101, volume: 22 } }];
        let t = Fake { first_day: 0, data: &[] };
        assert_eq!(next_event(20454 * 1440, &rules, &t, Zone::CAIRO), None);
    }

    fn alarm_at(h: u16, m: u16) -> [Rule; 1] {
        [Rule { days: ALL_DAYS, when: When::Local(h * 60 + m), action: Action::Alarm { file: 101, volume: 22 } }]
    }

    #[test]
    fn a_local_alarm_rings_at_the_wall_time_of_the_stored_zone() {
        // 07:00 in New York in July (UTC-4) is 11:00 UTC, in January (UTC-5) 12:00 UTC.
        let ny = Zone { std_min: -300, dst: Dst::Us };
        let none = Fake { first_day: 0, data: &[] };
        let rules = alarm_at(7, 0);
        let jul = days_from_civil(2026, 7, 1) as i64 * 1440;
        assert_eq!(next_event(jul, &rules, &none, ny).unwrap().utc_min, jul + 11 * 60);
        let jan = days_from_civil(2026, 1, 8) as i64 * 1440;
        assert_eq!(next_event(jan, &rules, &none, ny).unwrap().utc_min, jan + 12 * 60);
        // The same minute count in Cairo is a different alarm.
        let cairo = next_event(jan, &rules, &none, Zone::CAIRO).unwrap();
        assert_eq!(cairo.utc_min, jan + 5 * 60);
    }

    #[test]
    fn a_local_alarm_follows_the_clock_across_us_and_europe_changes() {
        let none = Fake { first_day: 0, data: &[] };
        let rules = alarm_at(7, 0);
        // New York, Sat 7 Mar 2026 is winter (12:00 UTC); Mon 9 Mar is summer (11:00 UTC).
        let ny = Zone { std_min: -300, dst: Dst::Us };
        let sat = days_from_civil(2026, 3, 7) as i64 * 1440;
        let e = next_event(sat, &rules, &none, ny).unwrap();
        assert_eq!(e.utc_min, sat + 12 * 60);
        let e = next_event(e.utc_min + 1, &rules, &none, ny).unwrap();
        assert_eq!(e.utc_min, sat + 1440 + 11 * 60); // Sun 8: the switch is at 07:00 UTC, so 07:00 is EDT
        let e = next_event(e.utc_min + 1, &rules, &none, ny).unwrap();
        assert_eq!(e.utc_min, sat + 2 * 1440 + 11 * 60);
        // London, Sat 28 Mar is winter (07:00 UTC), Sun 29 is summer (06:00 UTC).
        let london = Zone { std_min: 0, dst: Dst::Europe };
        let sat = days_from_civil(2026, 3, 28) as i64 * 1440;
        let e = next_event(sat, &rules, &none, london).unwrap();
        assert_eq!(e.utc_min, sat + 7 * 60);
        let e = next_event(e.utc_min + 1, &rules, &none, london).unwrap();
        assert_eq!(e.utc_min, sat + 1440 + 6 * 60);
    }

    #[test]
    fn an_alarm_in_the_hour_the_clocks_skip_rings_at_the_jump_not_never() {
        // London, Sun 29 Mar 2026: 01:30 does not exist. It rings at the jump, 01:00 UTC.
        let none = Fake { first_day: 0, data: &[] };
        let london = Zone { std_min: 0, dst: Dst::Europe };
        let sun = days_from_civil(2026, 3, 29) as i64 * 1440;
        let e = next_event(sun, &alarm_at(1, 30), &none, london).unwrap();
        assert_eq!(e.utc_min, sun + 60);
        // Once, not twice: the next one is tomorrow's.
        let e = next_event(e.utc_min + 1, &alarm_at(1, 30), &none, london).unwrap();
        assert_eq!(e.utc_min, sun + 1440 + 30); // 01:30 BST = 00:30 UTC
    }

    #[test]
    fn an_alarm_in_the_repeated_hour_rings_once() {
        // London, Sun 25 Oct 2026: 01:30 happens at 00:30 UTC and again at 01:30 UTC. Once.
        let none = Fake { first_day: 0, data: &[] };
        let london = Zone { std_min: 0, dst: Dst::Europe };
        let sun = days_from_civil(2026, 10, 25) as i64 * 1440;
        let e = next_event(sun, &alarm_at(1, 30), &none, london).unwrap();
        assert_eq!(e.utc_min, sun + 30);
        let e = next_event(e.utc_min + 1, &alarm_at(1, 30), &none, london).unwrap();
        assert_eq!(e.utc_min, sun + 1440 + 90);
    }

    #[test]
    fn the_strip_reads_the_stored_zone_not_cairo() {
        let mut b = [0u8; 50];
        let t = table(&mut b, days_from_civil(2026, 1, 1), [217, 217, 217, 217, 217]);
        let now = days_from_civil(2026, 1, 1) as i64 * 1440 + 600;
        // Fajr 03:37 UTC reads 03:37 in London in winter and 12:07 in Tokyo.
        let london = Zone { std_min: 0, dst: Dst::Europe };
        assert_eq!(local_times(now, &t, london)[0], Some(217));
        let tokyo = Zone { std_min: 540, dst: Dst::None };
        assert_eq!(local_times(now, &t, tokyo)[0], Some(217 + 540));
        // 23:30 UTC on 31 December is already 1 January in Tokyo, but still the 31st in London.
        let eve = days_from_civil(2025, 12, 31) as i64 * 1440 + 23 * 60 + 30;
        assert_eq!(local_times(eve, &t, tokyo)[0], Some(217 + 540));
        assert_eq!(local_times(eve, &t, london)[0], None);
    }
    #[test]
    fn a_ramp_is_active_from_its_start_to_the_minute_before_its_rule() {
        // Fajr 03:10 UTC with a 15-minute ramp: it starts 02:55 and the sound takes over at 03:10.
        let d = days_from_civil(2026, 10, 25);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]);
        let base = d as i64 * 1440;
        let z = Zone::CAIRO;

        assert_eq!(active_ramp(base + 174, &RULES, &t, z), None);
        assert_eq!(active_ramp(base + 175, &RULES, &t, z), Some((0, 900)));
        assert_eq!(active_ramp(base + 182, &RULES, &t, z), Some((420, 900)));
        assert_eq!(active_ramp(base + 189, &RULES, &t, z), Some((840, 900)));
        assert_eq!(active_ramp(base + 190, &RULES, &t, z), None);
    }

    #[test]
    fn a_ramp_that_crosses_midnight_is_found_from_either_side() {
        // Fajr at 00:05 UTC on the next day, so the ramp starts at 23:50 the day before.
        let d = days_from_civil(2026, 10, 25);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [5; 5]);
        let base = d as i64 * 1440;
        let z = Zone::CAIRO;
        // Day d's own Fajr is 00:05; its ramp ran 23:50 of the day before.
        assert_eq!(active_ramp(base - 10, &RULES, &t, z), Some((0, 900)));
        assert_eq!(active_ramp(base - 1, &RULES, &t, z), Some((540, 900)));
        assert_eq!(active_ramp(base + 2, &RULES, &t, z), Some((720, 900)));
        assert_eq!(active_ramp(base + 5, &RULES, &t, z), None);
    }

    #[test]
    fn only_a_sunrise_rule_makes_a_ramp() {
        let d = days_from_civil(2026, 10, 25);
        let mut b = [0u8; 50];
        let t = table(&mut b, d, [190; 5]);
        let sounds_only = [RULES[1], RULES[2]];
        assert_eq!(active_ramp(d as i64 * 1440 + 180, &sounds_only, &t, Zone::CAIRO), None);
        // A zero-minute ramp is no ramp.
        let zero = [Rule { days: ALL_DAYS, when: When::Prayer(Prayer::Fajr, 0), action: Action::Sunrise { ramp_min: 0 } }];
        assert_eq!(active_ramp(d as i64 * 1440 + 190, &zero, &t, Zone::CAIRO), None);
    }
}
