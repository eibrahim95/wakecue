//! The sun, and the five prayer times that follow from where it is.
//!
//! This is the arithmetic `prayer-gen` used to run on a PC to write ten years of Cairo times. The
//! lamp runs it itself now, for the place and the method in the stored block, so the table and its
//! 36 KB are gone and the lamp computes for whatever place its owner picks.
//!
//! `core` has no trigonometry, so `libm` supplies it: the same source on the PC and on the C3, which
//! is what lets the old table still be evidence — `tests/cairo_table.rs` checks every day of it
//! against this code.
//!
//! A day here is a **UTC** day and a time is minutes after 00:00 UTC of that day. Which local date a
//! time belongs to is the scheduler's question, not this module's.

use crate::schedule::{Prayer, PrayerTimes};
use libm::{acos, asin, atan, cos, fabs, floor, sin, tan};

const PI: f64 = core::f64::consts::PI;

fn rad(d: f64) -> f64 {
    d * PI / 180.0
}

fn deg(r: f64) -> f64 {
    r * 180.0 / PI
}

/// `f64::rem_euclid`, which `core` does not have.
fn rem_euclid(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r < 0.0 { r + m } else { r }
}

/// (declination in degrees, equation of time in minutes) at Julian day `jd`, from NOAA's formulae.
fn sun(jd: f64) -> (f64, f64) {
    let t = (jd - 2451545.0) / 36525.0;
    let l0 = rem_euclid(280.46646 + t * (36000.76983 + t * 0.0003032), 360.0);
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let mr = rad(m);
    let c = sin(mr) * (1.914602 - t * (0.004817 + 0.000014 * t))
        + sin(2.0 * mr) * (0.019993 - 0.000101 * t)
        + sin(3.0 * mr) * 0.000289;
    let omega = 125.04 - 1934.136 * t;
    let lambda = l0 + c - 0.00569 - 0.00478 * sin(rad(omega));
    let eps0 = 23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let eps = eps0 + 0.00256 * cos(rad(omega));
    let decl = deg(asin(sin(rad(eps)) * sin(rad(lambda))));
    let y = tan(rad(eps) / 2.0);
    let y2 = y * y;
    let l0r = rad(l0);
    let eot = 4.0 * deg(
        y2 * sin(2.0 * l0r) - 2.0 * e * sin(mr) + 4.0 * e * y2 * sin(mr) * cos(2.0 * l0r)
            - 0.5 * y2 * y2 * sin(4.0 * l0r) - 1.25 * e * e * sin(2.0 * mr),
    );
    (decl, eot)
}

/// The sun altitude (degrees, negative below the horizon) at which `kind` happens.
#[derive(Clone, Copy)]
enum Event {
    /// The sun reaches `-angle` in the morning.
    Dawn(f64),
    /// The sun crosses the horizon in the evening; refraction and the disc's radius make it
    /// -0.833 degrees, not 0.
    Sunset,
    /// The sun reaches `-angle` in the evening.
    Night(f64),
    /// An object's shadow is `shadow` times its height plus the shadow it cast at noon.
    Asr(f64),
    Noon,
}

/// Minutes after 00:00 UTC of unix day `day`, unrounded, or `None` where the sun never gets that
/// far.
fn minutes(day: i32, lat: f64, lon: f64, ev: Event) -> Option<f64> {
    let jd0 = day as f64 + 2440587.5; // 00:00 UTC
    // A first guess near the hour the event usually keeps, then four refinements: the equations
    // are evaluated where the event actually falls, not at midnight.
    let mut t = match ev {
        Event::Dawn(_) => 180.0,
        Event::Noon => 600.0,
        Event::Asr(_) => 780.0,
        Event::Sunset => 900.0,
        Event::Night(_) => 960.0,
    };
    for _ in 0..4 {
        let (decl, eot) = sun(jd0 + t / 1440.0);
        let noon = 720.0 - 4.0 * lon - eot;
        let alt = match ev {
            Event::Noon => {
                t = noon;
                continue;
            }
            Event::Dawn(a) | Event::Night(a) => -a,
            Event::Sunset => -0.833,
            Event::Asr(shadow) => deg(atan(1.0 / (shadow + tan(fabs(rad(lat) - rad(decl)))))),
        };
        let cos_h = (sin(rad(alt)) - sin(rad(lat)) * sin(rad(decl)))
            / (cos(rad(lat)) * cos(rad(decl)));
        if !(-1.0..=1.0).contains(&cos_h) {
            return None;
        }
        let h = 4.0 * deg(acos(cos_h));
        t = match ev {
            Event::Dawn(_) => noon - h,
            _ => noon + h,
        };
    }
    Some(t)
}

/// How a place derives its times from the sun, in the units the stored block keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Method {
    /// How far below the horizon the sun sits at Fajr, in tenths of a degree (195 = 19.5°).
    pub fajr_angle_d10: u16,
    /// Isha as an angle below the horizon in tenths, or `None` for `isha_minutes` after Maghrib
    /// (Umm al-Qura's 90 minutes, for example).
    pub isha_angle_d10: Option<u16>,
    pub isha_minutes: u16,
    /// 1 for the standard (Shafi'i) shadow, 2 for Hanafi.
    pub asr_shadow: u8,
}

impl Method {
    fn fajr_angle(self) -> f64 {
        self.fajr_angle_d10 as f64 / 10.0
    }

    fn isha_angle(self) -> Option<f64> {
        self.isha_angle_d10.map(|a| a as f64 / 10.0)
    }
}

/// The time of `prayer` on UTC day `day`, in minutes after 00:00 UTC.
///
/// Rounded to the nearest minute, as the Egyptian General Authority of Survey rounds the times it
/// publishes: against that Authority's own timetable for Mansoura (760 times over five months) this
/// rule reproduces the published minute 94% of the time, where rounding up reproduces it 43% and
/// rounding down 57%, and the rest differ by one minute (see `tests/egas_mansoura.rs`). `None` means
/// the sun never reaches
/// the altitude that prayer needs, which is a summer at a high latitude: the block stores a
/// `high_lat` rule, and nothing reads it yet, so such a prayer simply has no time.
///
/// Takes degrees rather than the block's hundredths so the old table, which was written at
/// 30.0444 and 31.2357, can be reproduced exactly in the test.
pub fn utc_min(prayer: Prayer, day: i32, lat: f64, lon: f64, method: &Method) -> Option<i64> {
    let ev = match prayer {
        Prayer::Fajr => Event::Dawn(method.fajr_angle()),
        Prayer::Dhuhr => Event::Noon,
        Prayer::Asr => Event::Asr(f64::from(method.asr_shadow.max(1))),
        Prayer::Maghrib => Event::Sunset,
        Prayer::Isha => match method.isha_angle() {
            Some(a) => Event::Night(a),
            None => {
                let sunset = minutes(day, lat, lon, Event::Sunset)?;
                return Some(round(sunset + f64::from(method.isha_minutes)));
            }
        },
    };
    Some(round(minutes(day, lat, lon, ev)?))
}

/// The nearest whole minute, halves going up.
fn round(minutes: f64) -> i64 {
    floor(minutes + 0.5) as i64
}

/// A place and its method, ready to be the scheduler's source of times.
#[derive(Clone, Copy)]
pub struct Place {
    lat: f64,
    lon: f64,
    method: Method,
}

impl Place {
    /// From the block's hundredths of a degree: 3004, 3124 is Cairo.
    pub fn new(lat_e2: i16, lon_e2: i16, method: Method) -> Place {
        Place {
            lat: f64::from(lat_e2) / 100.0,
            lon: f64::from(lon_e2) / 100.0,
            method,
        }
    }
}

impl PrayerTimes for Place {
    fn utc_min(&self, prayer: Prayer, day: i32) -> Option<i64> {
        utc_min(prayer, day, self.lat, self.lon, &self.method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::days_from_civil;

    /// The Egyptian General Authority's method: 19.5 degrees for Fajr, 17.5 for Isha, standard Asr.
    const CAIRO: Method = Method {
        fajr_angle_d10: 195,
        isha_angle_d10: Some(175),
        isha_minutes: 0,
        asr_shadow: 1,
    };

    fn cairo(prayer: Prayer, day: i32) -> Option<i64> {
        utc_min(prayer, day, 30.0444, 31.2357, &CAIRO)
    }

    #[test]
    fn the_five_times_come_in_order_winter_and_summer() {
        for (m, d) in [(1, 15), (7, 15)] {
            let day = days_from_civil(2026, m, d);
            let t: [i64; 5] = Prayer::ALL.map(|p| cairo(p, day).expect("Cairo has all five"));
            assert!(t[0] < t[1], "Fajr before Dhuhr on {m}/{d}: {t:?}");
            assert!(t[1] < t[2], "Dhuhr before Asr on {m}/{d}: {t:?}");
            assert!(t[2] < t[3], "Asr before Maghrib on {m}/{d}: {t:?}");
            assert!(t[3] < t[4], "Maghrib before Isha on {m}/{d}: {t:?}");
        }
    }

    #[test]
    fn isha_by_minutes_is_so_many_after_maghrib() {
        let day = days_from_civil(2026, 11, 5);
        let by_minutes = Method { isha_angle_d10: None, isha_minutes: 90, ..CAIRO };
        let maghrib = utc_min(Prayer::Maghrib, day, 30.0444, 31.2357, &by_minutes).unwrap();
        let isha = utc_min(Prayer::Isha, day, 30.0444, 31.2357, &by_minutes).unwrap();
        assert_eq!(isha, maghrib + 90);
    }

    #[test]
    fn the_hanafi_asr_is_later_than_the_standard_one() {
        let day = days_from_civil(2026, 2, 10);
        let hanafi = Method { asr_shadow: 2, ..CAIRO };
        let standard = cairo(Prayer::Asr, day).unwrap();
        let later = utc_min(Prayer::Asr, day, 30.0444, 31.2357, &hanafi).unwrap();
        assert!(later > standard, "Hanafi {later} should be after Shafi'i {standard}");
    }

    #[test]
    fn a_summer_that_never_gets_dark_has_no_fajr_or_maghrib() {
        // Svalbard in June: the sun is up all night, so nothing crosses the horizon.
        let day = days_from_civil(2026, 6, 21);
        assert_eq!(utc_min(Prayer::Fajr, day, 78.2, 15.6, &CAIRO), None);
        assert_eq!(utc_min(Prayer::Maghrib, day, 78.2, 15.6, &CAIRO), None);
        // Dhuhr is still the sun's high point, so a place like that keeps a midday time.
        assert!(utc_min(Prayer::Dhuhr, day, 78.2, 15.6, &CAIRO).is_some());
    }

    #[test]
    fn a_stored_place_is_degrees_and_hundredths() {
        let day = days_from_civil(2026, 11, 5);
        let place = Place::new(3004, 3124, CAIRO);
        // Four thousandths of a degree is a fraction of a minute, so the minute comes out the same
        // as the exact coordinates the old table was written at.
        assert_eq!(place.utc_min(Prayer::Fajr, day), cairo(Prayer::Fajr, day));
    }
}
