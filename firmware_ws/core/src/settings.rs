//! What the user can change: which prayers ring and up to eight own alarms. Stored in flash as a small block.
use crate::schedule::{Action, Prayer, Rule, When, ALL_DAYS};

pub const MAX_USER_ALARMS: usize = 8;
/// Fajr gives two rules (ramp and sound), the other prayers one each, plus the user alarms.
pub const MAX_RULES: usize = 2 + 4 + MAX_USER_ALARMS;
/// Bytes in the stored block.
pub const STORED_LEN: usize = 3 + MAX_USER_ALARMS * 4;
const VERSION: u8 = 1;

pub const SUN_TO_THU: u8 = 0b0001_1111;
pub const FRI_SAT: u8 = 0b0110_0000;
pub const FRIDAY: u8 = 0b0010_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserAlarm {
    pub used: bool,
    pub enabled: bool,
    /// Weekday mask by Egypt date, bit 0 = Sunday.
    pub days: u8,
    /// Minutes after midnight, Egypt local time.
    pub minute: u16,
}

impl UserAlarm {
    pub const EMPTY: UserAlarm = UserAlarm { used: false, enabled: false, days: ALL_DAYS, minute: 7 * 60 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Bit per prayer, in table order (bit 0 = Fajr).
    pub prayers_on: u8,
    pub alarms: [UserAlarm; MAX_USER_ALARMS],
}

impl Default for Settings {
    /// Fajr only, as before the menu existed.
    fn default() -> Self {
        Settings { prayers_on: 1, alarms: [UserAlarm::EMPTY; MAX_USER_ALARMS] }
    }
}

impl Settings {
    pub fn prayer_on(&self, p: Prayer) -> bool {
        self.prayers_on & (1 << p.index()) != 0
    }

    pub fn toggle_prayer(&mut self, p: Prayer) {
        self.prayers_on ^= 1 << p.index();
    }

    /// Fills `out` with the rules the schedule should run and returns how many. Prayer sounds are tracks
    /// 1..=5 (`/mp3/0001.mp3` is Fajr); Fajr also gets the 15-minute sunrise ramp.
    pub fn rules(&self, out: &mut [Rule; MAX_RULES]) -> usize {
        let mut n = 0;
        let mut push = |r: Rule| {
            out[n] = r;
            n += 1;
        };
        for p in Prayer::ALL {
            if !self.prayer_on(p) { continue; }
            if p == Prayer::Fajr {
                push(Rule { days: ALL_DAYS, when: When::Prayer(p, 0), action: Action::Sunrise { ramp_min: 15 } });
            }
            let fade_in_s = if p == Prayer::Fajr { 60 } else { 20 };
            push(Rule {
                days: ALL_DAYS,
                when: When::Prayer(p, 0),
                action: Action::Play { track: p.index() as u16 + 1, volume: 20, fade_in_s },
            });
        }
        for a in self.alarms.iter().filter(|a| a.used && a.enabled) {
            push(Rule { days: a.days, when: When::Local(a.minute), action: Action::Alarm });
        }
        n
    }

    pub fn to_bytes(&self) -> [u8; STORED_LEN] {
        let mut b = [0u8; STORED_LEN];
        b[0] = VERSION;
        b[1] = self.prayers_on;
        for (i, a) in self.alarms.iter().enumerate() {
            let at = 2 + i * 4;
            b[at] = a.used as u8 | (a.enabled as u8) << 1;
            b[at + 1] = a.days;
            b[at + 2..at + 4].copy_from_slice(&a.minute.to_le_bytes());
        }
        b[STORED_LEN - 1] = checksum(&b[..STORED_LEN - 1]);
        b
    }

    /// None if the block is blank (erased flash), from another version, damaged or out of range.
    pub fn from_bytes(b: &[u8]) -> Option<Settings> {
        if b.len() != STORED_LEN || b[0] != VERSION || b[STORED_LEN - 1] != checksum(&b[..STORED_LEN - 1]) {
            return None;
        }
        if b[1] & !0b1_1111 != 0 { return None; }
        let mut s = Settings { prayers_on: b[1], alarms: [UserAlarm::EMPTY; MAX_USER_ALARMS] };
        for (i, a) in s.alarms.iter_mut().enumerate() {
            let at = 2 + i * 4;
            let minute = u16::from_le_bytes([b[at + 2], b[at + 3]]);
            if b[at] & !0b11 != 0 || b[at + 1] & 0x80 != 0 || minute >= 1440 { return None; }
            *a = UserAlarm { used: b[at] & 1 != 0, enabled: b[at] & 2 != 0, days: b[at + 1], minute };
        }
        Some(s)
    }
}

/// Sum of the bytes plus a constant, so an all-0xFF or all-zero block never validates.
fn checksum(b: &[u8]) -> u8 {
    b.iter().fold(0x5Au8, |s, x| s.wrapping_add(*x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_fajr_only_with_ramp_and_sound() {
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        let n = Settings::default().rules(&mut out);
        assert_eq!(n, 2);
        assert!(matches!(out[0].action, Action::Sunrise { ramp_min: 15 }));
        assert!(matches!(out[1].action, Action::Play { track: 1, .. }));
    }

    #[test]
    fn all_prayers_and_all_alarms_fit_the_rule_buffer() {
        let mut s = Settings { prayers_on: 0b1_1111, ..Settings::default() };
        for a in s.alarms.iter_mut() {
            *a = UserAlarm { used: true, enabled: true, days: SUN_TO_THU, minute: 450 };
        }
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        assert_eq!(s.rules(&mut out), MAX_RULES);
        assert!(matches!(out[5].action, Action::Play { track: 5, .. })); // Isha is track 5
        assert!(matches!(out[6].action, Action::Alarm));
    }

    #[test]
    fn disabled_and_unused_alarms_make_no_rules() {
        let mut s = Settings::default();
        s.alarms[0] = UserAlarm { used: true, enabled: false, days: ALL_DAYS, minute: 400 };
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        assert_eq!(s.rules(&mut out), 2);
    }

    #[test]
    fn stored_block_round_trips_and_rejects_damage() {
        let mut s = Settings { prayers_on: 0b1_0101, ..Settings::default() };
        s.alarms[3] = UserAlarm { used: true, enabled: true, days: FRI_SAT, minute: 1439 };
        let b = s.to_bytes();
        assert_eq!(Settings::from_bytes(&b), Some(s));
        let mut bad = b;
        bad[5] ^= 1;
        assert_eq!(Settings::from_bytes(&bad), None);
        assert_eq!(Settings::from_bytes(&[0xFF; STORED_LEN]), None); // erased flash
        assert_eq!(Settings::from_bytes(&[0x00; STORED_LEN]), None);
        assert_eq!(Settings::from_bytes(&b[..10]), None);
    }
}
