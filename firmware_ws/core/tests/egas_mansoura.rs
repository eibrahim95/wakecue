//! The lamp against the timetable Egypt is told to follow.
//!
//! Dar al-Ifta (fatwa 3012) says the calendar the Egyptian General Authority of Survey publishes is
//! the one to follow. `data/egas-mansoura-2026.csv` is that Authority's own table for Mansoura,
//! read from its website on 8 Nov 2026: 152 days from five months of the year, so winter, summer
//! (when the clocks are an hour forward) and the shoulders are all in it.
//!
//! Before this test the lamp rounded every time up and a prayer could come a minute after the one
//! on the mosque's calendar. Measured against this table, rounding up reproduced the published
//! minute 43% of the time, rounding down 57% and rounding to the nearest 94%. What is left is a
//! minute here and there where the Authority's coordinates for the city differ from the list's.

use lamp_core::astro::{self, Method};
use lamp_core::calendar::{Zone, days_from_civil};
use lamp_core::schedule::Prayer;

static TABLE: &str = include_str!("data/egas-mansoura-2026.csv");

/// The Egyptian method: 19.5° at Fajr, 17.5° at Isha, the standard Asr.
const EGAS: Method = Method {
    fajr_angle_d10: 195,
    isha_angle_d10: Some(175),
    isha_minutes: 0,
    asr_shadow: 1,
};

/// Mansoura as the setup page's list has it (`core::cities`).
const LAT: f64 = 31.04;
const LON: f64 = 31.38;

fn rows() -> impl Iterator<Item = (i32, [i64; 5])> {
    TABLE.lines().filter(|l| !l.starts_with('#') && !l.starts_with("date")).map(|l| {
        let mut f = l.split(',');
        let mut date = f.next().unwrap().split('-').map(|n| n.parse::<u32>().unwrap());
        let (y, m, d) = (date.next().unwrap() as i32, date.next().unwrap(), date.next().unwrap());
        let mut t = [0i64; 5];
        for slot in &mut t {
            *slot = f.next().unwrap().parse().unwrap();
        }
        (days_from_civil(y, m, d), t)
    })
}

#[test]
fn the_fixture_is_what_it_says_it_is() {
    assert_eq!(rows().count(), 152);
    let (first, _) = rows().next().unwrap();
    assert_eq!(first, days_from_civil(2026, 1, 1));
}

#[test]
fn the_lamp_gives_the_authoritys_minute_nearly_always_and_never_more_than_a_minute_away() {
    let mut exact = 0;
    let mut total = 0;
    let mut per = [0usize; 5];
    for (day, published) in rows() {
        for (i, prayer) in Prayer::ALL.into_iter().enumerate() {
            let utc = astro::utc_min(prayer, day, LAT, LON, &EGAS).expect("the sun gets there");
            // The wall time on that day, in the zone Egypt keeps then.
            let local = Zone::CAIRO.to_local(day as i64 * 1440 + utc) - day as i64 * 1440;
            let gap = local - published[i];
            assert!(gap.abs() <= 1, "day {day} {prayer:?}: lamp {local}, published {}", published[i]);
            total += 1;
            if gap == 0 {
                exact += 1;
                per[i] += 1;
            }
        }
    }
    assert_eq!(total, 760);
    assert!(exact * 100 >= total * 90, "only {exact} of {total} times are the published minute");
    for (i, prayer) in Prayer::ALL.into_iter().enumerate() {
        assert!(per[i] * 100 >= 152 * 85, "{prayer:?}: only {} of 152 days match", per[i]);
    }
}
