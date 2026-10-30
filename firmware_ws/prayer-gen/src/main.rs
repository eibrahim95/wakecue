//! Writes a table of the five daily prayers: per day five little-endian u16, minutes after 00:00 UTC,
//! in the order Fajr, Dhuhr, Asr, Maghrib, Isha.
//! usage: prayer-gen <lat> <lon> <first_year> <years> <out.bin> [fajr_angle=19.5] [isha_angle=17.5] [asr_shadow=1]
//! The defaults are the Egyptian General Authority of Survey method with the standard (shadow = 1) Asr.
use lamp_core::calendar::{civil_from_days, days_from_civil};
use std::f64::consts::PI;

fn rad(d: f64) -> f64 { d * PI / 180.0 }
fn deg(r: f64) -> f64 { r * 180.0 / PI }

/// (declination in degrees, equation of time in minutes) at Julian day `jd` (NOAA's formulae).
fn sun(jd: f64) -> (f64, f64) {
    let t = (jd - 2451545.0) / 36525.0;
    let l0 = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let mr = rad(m);
    let c = mr.sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + (2.0 * mr).sin() * (0.019993 - 0.000101 * t)
        + (3.0 * mr).sin() * 0.000289;
    let omega = 125.04 - 1934.136 * t;
    let lambda = l0 + c - 0.00569 - 0.00478 * rad(omega).sin();
    let eps0 = 23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let eps = eps0 + 0.00256 * rad(omega).cos();
    let decl = deg((rad(eps).sin() * rad(lambda).sin()).asin());
    let y = (rad(eps) / 2.0).tan().powi(2);
    let l0r = rad(l0);
    let eot = 4.0 * deg(
        y * (2.0 * l0r).sin() - 2.0 * e * mr.sin() + 4.0 * e * y * mr.sin() * (2.0 * l0r).cos()
            - 0.5 * y * y * (4.0 * l0r).sin() - 1.25 * e * e * (2.0 * mr).sin(),
    );
    (decl, eot)
}

/// Sun altitude (degrees, negative = below the horizon) at which `kind` happens.
#[derive(Clone, Copy)]
enum Event {
    /// Sun reaches `-angle` below the horizon in the morning.
    Dawn(f64),
    /// Sun crosses the horizon (refraction and the disc radius: -0.833 degrees) in the evening.
    Sunset,
    /// Sun reaches `-angle` below the horizon in the evening.
    Night(f64),
    /// An object's shadow is `shadow` times its height plus its noon shadow.
    Asr(f64),
    Noon,
}

/// Minutes after 00:00 UTC of unix day `day`, unrounded. None where the sun never gets that far.
fn minutes(day: i32, lat: f64, lon: f64, ev: Event) -> Option<f64> {
    let jd0 = day as f64 + 2440587.5; // 00:00 UTC
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
            Event::Noon => { t = noon; continue; }
            Event::Dawn(a) | Event::Night(a) => -a,
            Event::Sunset => -0.833,
            Event::Asr(shadow) => deg((1.0 / (shadow + (rad(lat) - rad(decl)).abs().tan())).atan()),
        };
        let cos_h = (rad(alt).sin() - rad(lat).sin() * rad(decl).sin())
            / (rad(lat).cos() * rad(decl).cos());
        if !(-1.0..=1.0).contains(&cos_h) { return None; }
        let h = 4.0 * deg(cos_h.acos());
        t = match ev {
            Event::Dawn(_) => noon - h,
            _ => noon + h,
        };
    }
    Some(t)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: prayer-gen <lat> <lon> <first_year> <years> <out.bin> [fajr_angle=19.5] [isha_angle=17.5] [asr_shadow=1]");
        std::process::exit(2);
    }
    let lat: f64 = a[1].parse().expect("lat");
    let lon: f64 = a[2].parse().expect("lon");
    let year: i32 = a[3].parse().expect("first_year");
    let years: i32 = a[4].parse().expect("years");
    let fajr: f64 = a.get(6).map_or(19.5, |s| s.parse().expect("fajr_angle"));
    let isha: f64 = a.get(7).map_or(17.5, |s| s.parse().expect("isha_angle"));
    let shadow: f64 = a.get(8).map_or(1.0, |s| s.parse().expect("asr_shadow"));
    let events = [Event::Dawn(fajr), Event::Noon, Event::Asr(shadow), Event::Sunset, Event::Night(isha)];

    let first = days_from_civil(year, 1, 1);
    let last = days_from_civil(year + years, 1, 1);
    let mut out = Vec::with_capacity(((last - first) * 10) as usize);
    for day in first..last {
        for ev in events {
            let m = minutes(day, lat, lon, ev).unwrap_or_else(|| {
                let (y, mo, d) = civil_from_days(day);
                panic!("a prayer does not occur at this latitude on {y}-{mo:02}-{d:02}")
            });
            // Round up to the next whole minute so nothing rings before the astronomical time.
            out.extend_from_slice(&(m.ceil() as u16).to_le_bytes());
        }
    }
    std::fs::write(&a[5], &out).expect("write");
    let (ly, lm, ld) = civil_from_days(last - 1);
    println!(
        "wrote {}: {} days ({year}-01-01 .. {ly}-{lm:02}-{ld:02}), {} bytes, first_day={first}",
        a[5], last - first, out.len()
    );
}
