//! The calculation methods the lamp can follow: one named authority each.
//!
//! A prayer time depends on a convention, and the conventions differ by country: the sun angle that
//! counts as dawn and as nightfall, and sometimes a fixed delay after sunset. Egypt follows the
//! Egyptian General Authority of Survey (Fajr 19.5°), Saudi Arabia follows Umm al-Qura University
//! (Fajr 18.5°, Isha 90 minutes after Maghrib), Turkey follows Diyanet, and so on. Applying one
//! country's convention in another puts a prayer minutes out: Makkah with the Egyptian 19.5° has its
//! Fajr at 5:04 where Umm al-Qura's 18.5° and Google have it at 5:09.
//!
//! The numbers and the ids are those of the AlAdhan calculator's list (`api.aladhan.com/v1/methods`,
//! read on 8 Nov 2026), which names the authority of each; the ids are kept so the two can be
//! compared. Methods that need something the lamp's block cannot hold are not here: Tehran's and
//! the Shia Jafari method put Maghrib when the sun is a few degrees below the horizon, Portugal's
//! and the Moonsighting Committee's need rules of their own. [`UNSUPPORTED`] lists them so a place
//! that follows one can say so instead of being given a method that is wrong for it.
//!
//! A few methods also move individual prayers by a fixed number of minutes, as the authority's own
//! timetable does (Turkey's Maghrib is seven minutes after sunset, for example). Those are
//! [`Preset::shift`], measured from what AlAdhan outputs for each method on four dates over a year
//! and ignored where it is within the rounding (under half a minute), and they are written into the
//! settings' per-prayer shifts.
//!
//! Not modelled: Umm al-Qura lengthens Isha to 120 minutes during Ramadan, which needs the Hijri
//! calendar the lamp does not have, and no high-latitude rule is applied in summer, so at about 48
//! degrees north and beyond (Paris, London in June) the Fajr and Isha an app shows can be several
//! minutes away from the lamp's.

use crate::settings::{Isha, Settings};

/// How a method ends the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IshaRule {
    /// The sun this many tenths of a degree below the horizon.
    Angle(u16),
    /// This many minutes after Maghrib.
    Minutes(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    /// AlAdhan's id for it.
    pub id: u8,
    /// The authority, in words short enough for a form.
    pub name: &'static str,
    /// The sun's depression at Fajr, in tenths of a degree.
    pub fajr_d10: u16,
    pub isha: IshaRule,
    /// Minutes the authority adds to each prayer's calculated time, in prayer order (Fajr, Dhuhr,
    /// Asr, Maghrib, Isha). All zero for most methods.
    pub shift: [i8; 5],
}

impl Preset {
    /// Write this method into `s`: the angles, the Isha rule, the authority's own minute shifts and
    /// which method it is. The Asr shadow is the owner's own choice and is left as it is. A shift
    /// the owner had set by hand is replaced: the method's are the starting point.
    pub fn apply(&self, s: &mut Settings) {
        s.fajr_angle_d10 = self.fajr_d10;
        match self.isha {
            IshaRule::Angle(d10) => {
                s.isha = Isha::Angle;
                s.isha_value = d10;
            }
            IshaRule::Minutes(m) => {
                s.isha = Isha::Minutes;
                s.isha_value = m;
            }
        }
        s.offset_min = self.shift;
        s.method_id = self.id;
    }

    /// True when `s` holds this method's angles and Isha rule.
    pub fn same_numbers(&self, s: &Settings) -> bool {
        let isha = match self.isha {
            IshaRule::Angle(d10) => s.isha == Isha::Angle && s.isha_value == d10,
            IshaRule::Minutes(m) => s.isha == Isha::Minutes && s.isha_value == m,
        };
        s.fajr_angle_d10 == self.fajr_d10 && isha
    }

    /// True when `s` holds exactly this method, shifts included.
    pub fn matches(&self, s: &Settings) -> bool {
        self.same_numbers(s) && s.offset_min == self.shift
    }
}

const fn p(id: u8, name: &'static str, fajr_d10: u16, isha: IshaRule, shift: [i8; 5]) -> Preset {
    Preset { id, name, fajr_d10, isha, shift }
}

use IshaRule::{Angle, Minutes};

const NONE: [i8; 5] = [0; 5];

/// Every method the lamp can follow, in the order the form lists them.
pub static PRESETS: [Preset; 19] = [
    p(5, "Egyptian General Authority of Survey", 195, Angle(175), NONE),
    p(4, "Umm al-Qura University, Makkah", 185, Minutes(90), NONE),
    p(3, "Muslim World League", 180, Angle(170), NONE),
    p(2, "Islamic Society of North America", 150, Angle(150), NONE),
    p(1, "University of Islamic Sciences, Karachi", 180, Angle(180), NONE),
    p(13, "Diyanet, Turkey", 180, Angle(170), [0, 5, 4, 7, 0]),
    p(8, "Gulf Region", 195, Minutes(90), NONE),
    p(16, "Dubai", 182, Angle(182), [0, 3, 0, 3, 0]),
    p(9, "Kuwait", 180, Angle(175), NONE),
    p(10, "Qatar", 180, Minutes(90), NONE),
    p(23, "Jordan, Ministry of Awqaf", 180, Angle(180), [0, 0, 0, 5, 0]),
    p(21, "Morocco", 190, Angle(170), [0, 5, 0, 5, 0]),
    p(19, "Algeria", 180, Angle(170), NONE),
    p(18, "Tunisia", 180, Angle(180), NONE),
    p(17, "JAKIM, Malaysia", 200, Angle(180), NONE),
    p(11, "MUIS, Singapore", 200, Angle(180), NONE),
    p(20, "Kemenag, Indonesia", 200, Angle(180), NONE),
    p(12, "Union of Islamic Organizations of France", 120, Angle(120), NONE),
    p(14, "Spiritual Administration of Muslims of Russia", 160, Angle(150), NONE),
];

/// AlAdhan ids of methods that exist but that the lamp cannot follow yet: 0 (Shia Ithna-Ashari,
/// Qum) and 7 (Tehran) set Maghrib by a sun angle, 22 (Portugal) and 15 (Moonsighting Committee)
/// have rules of their own.
pub const UNSUPPORTED: [u8; 4] = [0, 7, 15, 22];

/// The method everything falls back to: the Muslim World League, which is what a place with no
/// authority of its own is given. It is a default, not a claim that the place follows it.
pub const DEFAULT_ID: u8 = 3;

pub fn by_id(id: u8) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The method `s` currently holds, if it is one of the presets.
///
/// The stored id decides when the numbers still agree with it, because several authorities share
/// numbers and the name the owner picked is the one to show. Without an id, or if the numbers were
/// changed since (by the PC tool), the first preset that holds exactly these numbers and shifts.
pub fn current(s: &Settings) -> Option<&'static Preset> {
    if let Some(p) = by_id(s.method_id).filter(|p| p.same_numbers(s)) {
        return Some(p);
    }
    PRESETS.iter().find(|p| p.matches(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_none_is_both_supported_and_listed_as_unsupported() {
        for (i, a) in PRESETS.iter().enumerate() {
            assert!(PRESETS[i + 1..].iter().all(|b| b.id != a.id), "duplicate id {}", a.id);
            assert!(!UNSUPPORTED.contains(&a.id), "{} is both", a.id);
            assert!(a.name.len() <= 48 && a.name.is_ascii());
        }
        assert!(by_id(DEFAULT_ID).is_some());
    }

    #[test]
    fn the_numbers_are_the_ones_the_authorities_publish() {
        // Read from api.aladhan.com/v1/methods on 8 Nov 2026.
        let egypt = by_id(5).unwrap();
        assert_eq!((egypt.fajr_d10, egypt.isha), (195, Angle(175)));
        let makkah = by_id(4).unwrap();
        assert_eq!((makkah.fajr_d10, makkah.isha), (185, Minutes(90)));
        let mwl = by_id(3).unwrap();
        assert_eq!((mwl.fajr_d10, mwl.isha), (180, Angle(170)));
        let isna = by_id(2).unwrap();
        assert_eq!((isna.fajr_d10, isna.isha), (150, Angle(150)));
        let karachi = by_id(1).unwrap();
        assert_eq!((karachi.fajr_d10, karachi.isha), (180, Angle(180)));
        let dubai = by_id(16).unwrap();
        assert_eq!((dubai.fajr_d10, dubai.isha), (182, Angle(182)));
        let jordan = by_id(23).unwrap();
        assert_eq!((jordan.fajr_d10, jordan.isha, jordan.shift), (180, Angle(180), [0, 0, 0, 5, 0]));
        // The four with built-in minute shifts, as AlAdhan outputs them; every other method has none.
        assert_eq!(by_id(13).unwrap().shift, [0, 5, 4, 7, 0]); // Turkey
        assert_eq!(by_id(16).unwrap().shift, [0, 3, 0, 3, 0]); // Dubai
        assert_eq!(by_id(21).unwrap().shift, [0, 5, 0, 5, 0]); // Morocco
        assert!(PRESETS.iter().filter(|p| ![13, 16, 21, 23].contains(&p.id)).all(|p| p.shift == NONE));
    }

    #[test]
    fn applying_a_method_writes_it_and_keeps_the_owners_own_choices() {
        let mut s = Settings::default();
        s.asr_shadow = 2;
        s.offset_min[0] = -3; // a shift the owner set by hand
        by_id(4).unwrap().apply(&mut s);
        assert_eq!((s.fajr_angle_d10, s.isha, s.isha_value), (185, Isha::Minutes, 90));
        assert_eq!(s.asr_shadow, 2);
        // The method's own shifts replace the owner's: they are the starting point.
        assert_eq!(s.offset_min, [0; 5]);
        assert_eq!((s.method_id, current(&s).map(|p| p.id)), (4, Some(4)));
        // Turkey sets its three shifts, and the next method puts them back.
        by_id(13).unwrap().apply(&mut s);
        assert_eq!(s.offset_min, [0, 5, 4, 7, 0]);
        by_id(5).unwrap().apply(&mut s);
        assert_eq!(s.offset_min, [0; 5]);
        assert_eq!(current(&s).map(|p| p.id), Some(5));
    }

    #[test]
    fn the_stored_id_names_the_authority_among_those_that_share_numbers() {
        // Algeria and the Muslim World League hold the same angles; the one the owner picked is
        // the one to show, and a method the numbers no longer agree with is not.
        let mut s = Settings::default();
        by_id(19).unwrap().apply(&mut s);
        assert_eq!(current(&s).map(|p| p.id), Some(19));
        s.method_id = 3;
        assert_eq!(current(&s).map(|p| p.id), Some(3));
        s.method_id = 4; // numbers say otherwise
        assert_eq!(current(&s).map(|p| p.id), Some(3));
        s.fajr_angle_d10 = 188; // matches nothing
        assert_eq!(current(&s), None);
    }

    #[test]
    fn the_default_settings_are_the_egyptian_method() {
        assert_eq!(current(&Settings::default()).map(|p| p.id), Some(5));
    }

    /// The arithmetic with a method's numbers reproduces the minute the Egyptian and Umm al-Qura
    /// authorities give in Makkah, which is what started this: Fajr 5:04 with the wrong method.
    #[test]
    fn makkah_fajr_follows_the_method_not_the_place() {
        use crate::astro::{self, Method};
        use crate::calendar::days_from_civil;
        use crate::schedule::Prayer;
        let day = days_from_civil(2026, 11, 8);
        let at = |s: &Settings| {
            let m: Method = s.method();
            let utc = astro::utc_min(Prayer::Fajr, day, 21.39, 39.86, &m).unwrap();
            (utc + 180) % 1440 // Saudi Arabia keeps UTC+3 all year
        };
        let mut s = Settings::default();
        assert_eq!(at(&s), 5 * 60 + 4, "Egyptian 19.5 degrees");
        by_id(4).unwrap().apply(&mut s);
        assert_eq!(at(&s), 5 * 60 + 9, "Umm al-Qura 18.5 degrees, as Google and AlAdhan show");
    }
}
