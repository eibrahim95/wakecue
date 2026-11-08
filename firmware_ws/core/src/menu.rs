//! The settings menu as a state machine: keys in, screen and settings out. Drawing is the display code's job.
use crate::buttons::Key;
use crate::schedule::Prayer;
use crate::settings::{Dst, Settings, UserAlarm, FRIDAY, FRI_SAT, MAX_USER_ALARMS, MAX_VOLUME, SUN_TO_THU};
use crate::schedule::ALL_DAYS;

/// Day sets offered when editing an alarm, in the order Up/Down steps through them.
pub const DAY_PRESETS: [u8; 4] = [ALL_DAYS, SUN_TO_THU, FRI_SAT, FRIDAY];

/// The words a day set is shown as, matched to the presets above. Anything else is a set the
/// presets do not offer, which no screen should be able to produce.
pub fn day_name(days: u8) -> &'static str {
    match days {
        ALL_DAYS => "every day",
        SUN_TO_THU => "Sun-Thu",
        FRI_SAT => "Fri-Sat",
        FRIDAY => "Fri",
        _ => "custom",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Hour,
    Minute,
    Days,
    Volume,
    Save,
    Delete,
}

const FIELDS: [Field; 6] =
    [Field::Hour, Field::Minute, Field::Days, Field::Volume, Field::Save, Field::Delete];

/// What a prayer's screen edits, in the order Ok walks through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrayerField {
    On,
    Volume,
    Save,
}

const PRAYER_FIELDS: [PrayerField; 3] = [PrayerField::On, PrayerField::Volume, PrayerField::Save];

/// One step through the levels something can have of its own: Default (0), then 1 to the player's
/// top level, round and round. Up goes toward louder.
fn step_own_volume(v: u8, up: bool) -> u8 {
    let n = MAX_VOLUME + 1;
    (v + if up { 1 } else { n - 1 }) % n
}

/// One step through the general volume, which has no Default and stops at both ends: a level you
/// are turning up should not jump to the quietest when it reaches the top.
fn step_general_volume(v: u8, up: bool) -> u8 {
    if up { (v + 1).min(MAX_VOLUME) } else { v.saturating_sub(1).max(1) }
}

pub const VOLUME_LEN: usize = 12;

/// A volume as the screens show it: the level, or "Default (22)" for something that follows the
/// general level, so the owner can see what Default is without leaving the row.
pub fn format_volume(own: u8, general: u8, buf: &mut [u8; VOLUME_LEN]) -> &str {
    fn digits(v: u8, buf: &mut [u8], at: usize) -> usize {
        if v >= 10 {
            buf[at] = b'0' + v / 10;
            buf[at + 1] = b'0' + v % 10;
            at + 2
        } else {
            buf[at] = b'0' + v;
            at + 1
        }
    }
    let mut n = 0;
    if own == 0 {
        for &c in b"Default (" {
            buf[n] = c;
            n += 1;
        }
        n = digits(general.min(MAX_VOLUME), buf, n);
        buf[n] = b')';
        n += 1;
    } else {
        n = digits(own.min(MAX_VOLUME), buf, 0);
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// What the time-zone screen edits, in the order Ok walks through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneField {
    Hour,
    Minute,
    Summer,
    Save,
}

const ZONE_FIELDS: [ZoneField; 4] = [ZoneField::Hour, ZoneField::Minute, ZoneField::Summer, ZoneField::Save];

/// The offsets the zone screen can reach, in minutes. The settings block accepts exactly these
/// bounds, so anything the screen builds can be stored.
pub const OFFSET_MIN: i16 = -12 * 60;
pub const OFFSET_MAX: i16 = 14 * 60;

/// An offset as whole hours (toward zero) and the minutes past them: -210 is -3 and 30.
///
/// The sign lives on the hours, so an offset between -0:59 and 0 would lose it. No zone is there,
/// and the block can only hold one if the settings tool put it there.
pub fn offset_parts(min: i16) -> (i16, u8) {
    (min / 60, (min % 60).unsigned_abs() as u8)
}

fn offset_of(hours: i16, minutes: u8) -> i16 {
    let m = minutes as i16;
    (if hours < 0 { hours * 60 - m } else { hours * 60 + m }).clamp(OFFSET_MIN, OFFSET_MAX)
}

/// The summer-time rules in the order Up/Down steps through them.
const DST_RULES: [Dst; 4] = [Dst::None, Dst::Egypt, Dst::Europe, Dst::Us];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The standby face: time, next alarm, prayer times. Ok opens the menu.
    Home,
    /// 0 = Prayers, 1 = My alarms, 2 = Volume, 3 = Time zone, 4 = Phone setup.
    Top { cursor: u8 },
    Prayers { cursor: u8 },
    /// One prayer being edited: the field under the cursor and the draft of whether it rings and
    /// at what level (0 = Default), which reach the settings only on Save.
    PrayerEdit { prayer: u8, field: u8, on: bool, volume: u8 },
    /// The general volume being edited, which reaches the settings only on Ok.
    Volume { level: u8 },
    Alarms { cursor: u8 },
    Edit { slot: u8, field: u8, draft: UserAlarm },
    /// The time zone being edited: the field under the cursor and the draft, which reaches the
    /// settings only on Save.
    Zone { field: u8, minutes: i16, dst: Dst },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub redraw: bool,
    /// The settings changed and must be written to flash (and the next alarm re-programmed).
    pub save: bool,
    /// The owner asked for the phone setup: the caller leaves the menu and raises the lamp's own
    /// network, which is a different program from the one that reads buttons.
    pub setup: bool,
}

pub struct Menu {
    pub screen: Screen,
    pub settings: Settings,
}

fn step(cursor: u8, len: u8, key: Key) -> u8 {
    match key {
        Key::Up => (cursor + len - 1) % len,
        Key::Down => (cursor + 1) % len,
        _ => cursor,
    }
}

impl Menu {
    pub fn new(settings: Settings) -> Menu {
        Menu { screen: Screen::Home, settings }
    }

    pub fn press(&mut self, key: Key) -> Outcome {
        let mut out = Outcome { redraw: true, ..Outcome::default() };
        self.screen = match self.screen {
            Screen::Home => match key {
                Key::Ok => Screen::Top { cursor: 0 },
                _ => return Outcome::default(),
            },
            Screen::Top { cursor } => match key {
                Key::Up | Key::Down => Screen::Top { cursor: step(cursor, 5, key) },
                Key::Back => Screen::Home,
                // Back is the way out of the menu, so there is no Exit row to walk to.
                Key::Ok => match cursor {
                    0 => Screen::Prayers { cursor: 0 },
                    1 => Screen::Alarms { cursor: 0 },
                    2 => Screen::Volume { level: self.settings.volume.clamp(1, MAX_VOLUME) },
                    4 => {
                        out.setup = true;
                        Screen::Top { cursor }
                    }
                    _ => Screen::Zone {
                        field: 0,
                        minutes: self.settings.utc_offset_min,
                        dst: self.settings.dst,
                    },
                },
            },
            Screen::Prayers { cursor } => match key {
                Key::Up | Key::Down => Screen::Prayers { cursor: step(cursor, 5, key) },
                Key::Ok => Screen::PrayerEdit {
                    prayer: cursor,
                    field: 0,
                    on: self.settings.prayer_on(Prayer::ALL[cursor as usize]),
                    volume: self.settings.prayer_volume[cursor as usize],
                },
                Key::Back => Screen::Top { cursor: 0 },
            },
            Screen::Volume { level } => match key {
                Key::Up | Key::Down => Screen::Volume { level: step_general_volume(level, key == Key::Up) },
                Key::Ok => {
                    self.settings.volume = level;
                    out.save = true;
                    Screen::Top { cursor: 2 }
                }
                // Back cancels: nothing is written.
                Key::Back => Screen::Top { cursor: 2 },
            },
            Screen::PrayerEdit { prayer, field, on, volume } => match key {
                // Back cancels: nothing is written.
                Key::Back => Screen::Prayers { cursor: prayer },
                Key::Up | Key::Down => {
                    let up = key == Key::Up;
                    match PRAYER_FIELDS[field as usize] {
                        PrayerField::On => Screen::PrayerEdit { prayer, field, on: !on, volume },
                        PrayerField::Volume => Screen::PrayerEdit {
                            prayer,
                            field,
                            on,
                            volume: step_own_volume(volume, up),
                        },
                        // On Save, Up/Down move between the fields.
                        PrayerField::Save => Screen::PrayerEdit {
                            prayer,
                            field: step(field, PRAYER_FIELDS.len() as u8, key),
                            on,
                            volume,
                        },
                    }
                }
                Key::Ok => match PRAYER_FIELDS[field as usize] {
                    PrayerField::Save => {
                        let p = Prayer::ALL[prayer as usize];
                        if self.settings.prayer_on(p) != on {
                            self.settings.toggle_prayer(p);
                        }
                        self.settings.prayer_volume[prayer as usize] = volume;
                        out.save = true;
                        Screen::Prayers { cursor: prayer }
                    }
                    _ => Screen::PrayerEdit { prayer, field: field + 1, on, volume },
                },
            },
            Screen::Alarms { cursor } => match key {
                Key::Up | Key::Down => Screen::Alarms { cursor: step(cursor, MAX_USER_ALARMS as u8, key) },
                Key::Back => Screen::Top { cursor: 1 },
                Key::Ok => {
                    let a = self.settings.alarms[cursor as usize];
                    let draft = if a.used { a } else { UserAlarm { used: true, enabled: true, days: SUN_TO_THU, ..a } };
                    Screen::Edit { slot: cursor, field: 0, draft }
                }
            },
            Screen::Zone { field, minutes, dst } => match key {
                // Back cancels: nothing is written.
                Key::Back => Screen::Top { cursor: 3 },
                Key::Up | Key::Down => {
                    let up = key == Key::Up;
                    let (h, m) = offset_parts(minutes);
                    match ZONE_FIELDS[field as usize] {
                        ZoneField::Hour => {
                            let span = (OFFSET_MAX - OFFSET_MIN) / 60 + 1;
                            let h = (h - OFFSET_MIN / 60 + if up { 1 } else { span - 1 }) % span + OFFSET_MIN / 60;
                            Screen::Zone { field, minutes: offset_of(h, m), dst }
                        }
                        ZoneField::Minute => {
                            let m = (m + if up { 15 } else { 45 }) % 60;
                            Screen::Zone { field, minutes: offset_of(h, m), dst }
                        }
                        ZoneField::Summer => {
                            let i = DST_RULES.iter().position(|d| *d == dst).unwrap_or(0);
                            let n = DST_RULES.len();
                            Screen::Zone { field, minutes, dst: DST_RULES[(i + if up { 1 } else { n - 1 }) % n] }
                        }
                        // On Save, Up/Down move between the fields.
                        ZoneField::Save => {
                            Screen::Zone { field: step(field, ZONE_FIELDS.len() as u8, key), minutes, dst }
                        }
                    }
                }
                Key::Ok => match ZONE_FIELDS[field as usize] {
                    ZoneField::Save => {
                        self.settings.utc_offset_min = minutes;
                        self.settings.dst = dst;
                        out.save = true;
                        Screen::Top { cursor: 3 }
                    }
                    _ => Screen::Zone { field: field + 1, minutes, dst },
                },
            },
            Screen::Edit { slot, field, mut draft } => match key {
                // Back cancels: nothing is written.
                Key::Back => Screen::Alarms { cursor: slot },
                Key::Up | Key::Down => {
                    let up = key == Key::Up;
                    match FIELDS[field as usize] {
                        Field::Hour => {
                            let h = (draft.minute / 60 + if up { 1 } else { 23 }) % 24;
                            draft.minute = h * 60 + draft.minute % 60;
                        }
                        Field::Minute => {
                            let m = (draft.minute % 60 + if up { 1 } else { 59 }) % 60;
                            draft.minute = draft.minute / 60 * 60 + m;
                        }
                        Field::Days => {
                            let i = DAY_PRESETS.iter().position(|d| *d == draft.days).unwrap_or(0);
                            let n = DAY_PRESETS.len();
                            draft.days = DAY_PRESETS[(i + if up { 1 } else { n - 1 }) % n];
                        }
                        Field::Volume => draft.volume = step_own_volume(draft.volume, up),
                        // On Save and Delete, Up/Down move between the fields.
                        _ => return self.press_move(slot, field, draft, key),
                    }
                    Screen::Edit { slot, field, draft }
                }
                Key::Ok => match FIELDS[field as usize] {
                    Field::Save => {
                        self.settings.alarms[slot as usize] = draft;
                        out.save = true;
                        Screen::Alarms { cursor: slot }
                    }
                    Field::Delete => {
                        self.settings.alarms[slot as usize] = UserAlarm::EMPTY;
                        out.save = true;
                        Screen::Alarms { cursor: slot }
                    }
                    _ => Screen::Edit { slot, field: field + 1, draft },
                },
            },
        };
        out
    }

    fn press_move(&mut self, slot: u8, field: u8, draft: UserAlarm, key: Key) -> Outcome {
        self.screen = Screen::Edit { slot, field: step(field, FIELDS.len() as u8, key), draft };
        Outcome { redraw: true, ..Outcome::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu::new(Settings::default())
    }

    #[test]
    fn every_preset_has_words() {
        assert_eq!(DAY_PRESETS.map(day_name), ["every day", "Sun-Thu", "Fri-Sat", "Fri"]);
        assert_eq!(day_name(0b0100_0001), "custom");
    }

    #[test]
    fn back_walks_out_to_the_standby_face() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Ok); // Prayers
        assert!(matches!(m.screen, Screen::Prayers { .. }));
        m.press(Key::Back);
        m.press(Key::Back);
        assert_eq!(m.screen, Screen::Home);
        assert_eq!(m.press(Key::Back), Outcome::default()); // nothing to do on Home
    }

    #[test]
    fn a_prayer_is_switched_on_and_off_from_its_own_screen_and_saved_there() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Down); // Dhuhr
        m.press(Key::Ok); // opens its screen, on the On row
        assert!(matches!(m.screen, Screen::PrayerEdit { prayer: 1, field: 0, on: false, volume: 0 }));
        m.press(Key::Up); // off -> on
        assert!(!m.settings.prayer_on(Prayer::Dhuhr)); // nothing written before Save
        m.press(Key::Ok);
        m.press(Key::Ok); // past Volume, to Save
        let o = m.press(Key::Ok);
        assert!(o.save && o.redraw);
        assert!(m.settings.prayer_on(Prayer::Dhuhr));
        assert!(m.settings.prayer_on(Prayer::Fajr));
        assert_eq!(m.screen, Screen::Prayers { cursor: 1 });
        // And off again.
        m.press(Key::Ok);
        m.press(Key::Down);
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Ok);
        assert!(!m.settings.prayer_on(Prayer::Dhuhr));
    }

    #[test]
    fn a_prayers_volume_starts_on_default_walks_the_levels_and_saves_on_save() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Ok); // Fajr's screen
        m.press(Key::Ok); // On -> Volume
        assert!(matches!(m.screen, Screen::PrayerEdit { field: 1, volume: 0, .. }));
        m.press(Key::Up); // Default -> 1
        m.press(Key::Up); // 2
        assert!(matches!(m.screen, Screen::PrayerEdit { volume: 2, .. }));
        m.press(Key::Down);
        m.press(Key::Down); // back to Default
        m.press(Key::Down); // Default wraps to the top level
        assert!(matches!(m.screen, Screen::PrayerEdit { volume: 30, .. }));
        m.press(Key::Up); // and the top level wraps to Default
        assert!(matches!(m.screen, Screen::PrayerEdit { volume: 0, .. }));
        m.press(Key::Up);
        m.press(Key::Up);
        m.press(Key::Up); // 3
        assert_eq!(m.settings.prayer_volume[0], 0); // nothing written yet
        m.press(Key::Ok); // to Save
        assert!(m.press(Key::Ok).save);
        assert_eq!(m.settings.prayer_volume[0], 3);
        assert!(m.settings.prayer_on(Prayer::Fajr)); // untouched
    }

    #[test]
    fn back_on_a_prayers_screen_discards_the_draft() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Up); // Fajr off
        let o = m.press(Key::Back);
        assert!(!o.save);
        assert!(m.settings.prayer_on(Prayer::Fajr));
        assert_eq!(m.screen, Screen::Prayers { cursor: 0 });
    }

    #[test]
    fn the_general_volume_starts_from_the_stored_level_stops_at_both_ends_and_saves_on_ok() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Down);
        m.press(Key::Down); // Volume
        m.press(Key::Ok);
        assert_eq!(m.screen, Screen::Volume { level: 22 });
        for _ in 0..20 {
            m.press(Key::Up);
        }
        assert_eq!(m.screen, Screen::Volume { level: 30 }); // no wrap to the quietest
        for _ in 0..40 {
            m.press(Key::Down);
        }
        assert_eq!(m.screen, Screen::Volume { level: 1 }); // and no wrap to the loudest
        assert_eq!(m.settings.volume, 22);
        m.press(Key::Up);
        m.press(Key::Up);
        let o = m.press(Key::Ok);
        assert!(o.save);
        assert_eq!(m.settings.volume, 3);
        assert_eq!(m.screen, Screen::Top { cursor: 2 });
    }

    #[test]
    fn back_on_the_volume_screen_discards_the_level() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Down);
        m.press(Key::Down);
        m.press(Key::Ok);
        m.press(Key::Down);
        let o = m.press(Key::Back);
        assert!(!o.save);
        assert_eq!(m.settings.volume, 22);
        assert_eq!(m.screen, Screen::Top { cursor: 2 });
    }

    #[test]
    fn a_stored_level_the_player_cannot_play_opens_at_its_top_level() {
        let mut m = menu();
        m.settings.volume = 90; // a block from before the scale was pinned to the player's
        m.press(Key::Ok);
        m.press(Key::Down);
        m.press(Key::Down);
        m.press(Key::Ok);
        assert_eq!(m.screen, Screen::Volume { level: 30 });
    }

    #[test]
    fn volumes_read_as_a_level_or_as_default_with_the_level_it_means() {
        let mut buf = [0u8; VOLUME_LEN];
        assert_eq!(format_volume(0, 22, &mut buf), "Default (22)");
        assert_eq!(format_volume(0, 7, &mut buf), "Default (7)");
        assert_eq!(format_volume(5, 22, &mut buf), "5");
        assert_eq!(format_volume(30, 22, &mut buf), "30");
        assert_eq!(format_volume(0, 90, &mut buf), "Default (30)");
    }

    #[test]
    fn cursors_wrap() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Up);
        assert_eq!(m.screen, Screen::Top { cursor: 4 });
        m.press(Key::Down);
        assert_eq!(m.screen, Screen::Top { cursor: 0 });
    }

    #[test]
    fn the_top_screen_is_five_rows_and_back_is_the_way_out() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Down);
        assert_eq!(m.screen, Screen::Top { cursor: 1 });
        m.press(Key::Up);
        m.press(Key::Ok);
        assert!(matches!(m.screen, Screen::Prayers { .. })); // Fajr
        m.press(Key::Back);
        m.press(Key::Down);
        m.press(Key::Ok);
        assert!(matches!(m.screen, Screen::Alarms { .. }));
        m.press(Key::Back);
        assert_eq!(m.screen, Screen::Top { cursor: 1 });
    }

    fn open_new_alarm(m: &mut Menu) {
        m.press(Key::Ok);
        m.press(Key::Down); // My alarms
        m.press(Key::Ok);
        m.press(Key::Ok); // slot 0, new
    }

    #[test]
    fn adding_an_alarm_sets_hour_minute_days_and_saves() {
        let mut m = menu();
        open_new_alarm(&mut m);
        assert!(matches!(m.screen, Screen::Edit { slot: 0, field: 0, .. }));
        m.press(Key::Down); // 07:00 -> 06:00
        m.press(Key::Ok);
        for _ in 0..30 { m.press(Key::Up); }
        m.press(Key::Ok); // Days: Sun-Thu -> Fri+Sat
        m.press(Key::Up);
        assert!(!m.settings.alarms[0].used); // nothing written before Save
        m.press(Key::Ok); // to Volume
        m.press(Key::Ok); // to Save
        let o = m.press(Key::Ok);
        assert!(o.save);
        assert_eq!(
            m.settings.alarms[0],
            UserAlarm { used: true, enabled: true, days: FRI_SAT, minute: 6 * 60 + 30, ..UserAlarm::EMPTY }
        );
        assert_eq!(m.screen, Screen::Alarms { cursor: 0 });
    }

    #[test]
    fn an_alarms_volume_starts_on_default_and_is_saved_with_the_alarm() {
        let mut m = menu();
        open_new_alarm(&mut m);
        for _ in 0..3 {
            m.press(Key::Ok); // Hour -> Minute -> Days -> Volume
        }
        assert!(matches!(m.screen, Screen::Edit { field: 3, draft: UserAlarm { volume: 0, .. }, .. }));
        m.press(Key::Down); // Default wraps to the top level
        m.press(Key::Down);
        m.press(Key::Down); // 28
        m.press(Key::Ok); // to Save
        assert!(m.press(Key::Ok).save);
        assert_eq!(m.settings.alarms[0].volume, 28);
        assert_eq!(m.settings.alarms[1].volume, 0);
    }

    #[test]
    fn back_in_the_editor_discards_the_draft() {
        let mut m = menu();
        open_new_alarm(&mut m);
        m.press(Key::Up);
        let o = m.press(Key::Back);
        assert!(!o.save);
        assert!(!m.settings.alarms[0].used);
        assert_eq!(m.screen, Screen::Alarms { cursor: 0 });
    }

    #[test]
    fn hour_and_minute_wrap_around() {
        let mut m = menu();
        open_new_alarm(&mut m);
        for _ in 0..8 { m.press(Key::Up); } // 07 -> 15
        m.press(Key::Down);
        let Screen::Edit { draft, .. } = m.screen else { panic!() };
        assert_eq!(draft.minute, 14 * 60);
        for _ in 0..15 { m.press(Key::Down); } // 14 -> 23
        let Screen::Edit { draft, .. } = m.screen else { panic!() };
        assert_eq!(draft.minute / 60, 23);
        m.press(Key::Up);
        m.press(Key::Up);
        let Screen::Edit { draft, .. } = m.screen else { panic!() };
        assert_eq!(draft.minute / 60, 1);
    }

    #[test]
    fn delete_clears_the_slot() {
        let mut m = menu();
        m.settings.alarms[2] =
            UserAlarm { used: true, enabled: true, days: ALL_DAYS, minute: 300, ..UserAlarm::EMPTY };
        m.screen = Screen::Alarms { cursor: 2 };
        m.press(Key::Ok); // edit
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Ok); // Hour -> Minute -> Days -> Volume -> Save
        m.press(Key::Down); // Save -> Delete
        assert!(matches!(m.screen, Screen::Edit { field: 5, .. }));
        let o = m.press(Key::Ok);
        assert!(o.save);
        assert!(!m.settings.alarms[2].used);
    }

    fn open_zone(m: &mut Menu) {
        m.press(Key::Ok);
        m.press(Key::Down);
        m.press(Key::Down);
        m.press(Key::Down); // Time zone
        m.press(Key::Ok);
    }

    #[test]
    fn the_zone_screen_starts_from_the_stored_zone() {
        let mut m = menu();
        open_zone(&mut m);
        assert_eq!(m.screen, Screen::Zone { field: 0, minutes: 120, dst: Dst::Egypt });
    }

    #[test]
    fn the_zone_is_written_only_on_save() {
        let mut m = menu();
        open_zone(&mut m);
        m.press(Key::Down); // +2 -> +1
        m.press(Key::Ok);
        m.press(Key::Up); // minutes 00 -> 15
        m.press(Key::Ok);
        m.press(Key::Up); // egypt -> europe
        assert_eq!(m.settings.utc_offset_min, 120); // nothing written yet
        assert_eq!(m.settings.dst, Dst::Egypt);
        m.press(Key::Ok); // to Save
        let o = m.press(Key::Ok);
        assert!(o.save && o.redraw);
        assert_eq!(m.settings.utc_offset_min, 75);
        assert_eq!(m.settings.dst, Dst::Europe);
        assert_eq!(m.screen, Screen::Top { cursor: 3 });
    }

    #[test]
    fn back_on_the_zone_screen_discards_the_draft() {
        let mut m = menu();
        open_zone(&mut m);
        m.press(Key::Down);
        let o = m.press(Key::Back);
        assert!(!o.save);
        assert_eq!(m.settings.utc_offset_min, 120);
        assert_eq!(m.screen, Screen::Top { cursor: 3 });
    }

    #[test]
    fn the_hour_walks_the_whole_range_and_wraps() {
        let mut m = menu();
        open_zone(&mut m);
        for _ in 0..12 { m.press(Key::Up); } // +2 -> +14
        assert!(matches!(m.screen, Screen::Zone { minutes: 840, .. }));
        m.press(Key::Up); // wraps to -12
        assert!(matches!(m.screen, Screen::Zone { minutes: -720, .. }));
        m.press(Key::Down); // and back
        assert!(matches!(m.screen, Screen::Zone { minutes: 840, .. }));
    }

    #[test]
    fn half_hour_and_quarter_hour_zones_are_reachable_on_both_sides_of_utc() {
        let mut m = menu();
        open_zone(&mut m);
        // Kolkata, +5:30: three hours up, two steps of fifteen.
        for _ in 0..3 { m.press(Key::Up); }
        m.press(Key::Ok);
        for _ in 0..2 { m.press(Key::Up); }
        assert!(matches!(m.screen, Screen::Zone { minutes: 330, .. }));
        // Newfoundland, -3:30: the minutes count away from zero when the hours are negative.
        m.press(Key::Back);
        m.press(Key::Ok); // the top menu is still on Time zone
        for _ in 0..5 { m.press(Key::Down); } // +2 -> -3
        m.press(Key::Ok);
        for _ in 0..2 { m.press(Key::Up); }
        assert!(matches!(m.screen, Screen::Zone { minutes: -210, .. }));
        // Fifteen minutes the other way wraps to 45.
        m.press(Key::Down);
        m.press(Key::Down);
        m.press(Key::Down);
        assert!(matches!(m.screen, Screen::Zone { minutes: -225, .. }));
    }

    #[test]
    fn nothing_the_screen_builds_is_outside_what_the_block_accepts() {
        let mut m = menu();
        open_zone(&mut m);
        for _ in 0..12 { m.press(Key::Up); } // +14:00
        m.press(Key::Ok);
        m.press(Key::Up); // +14:15 would pass the limit
        assert!(matches!(m.screen, Screen::Zone { minutes: 840, .. }));
    }

    #[test]
    fn up_and_down_on_save_move_between_the_zone_fields() {
        let mut m = menu();
        open_zone(&mut m);
        for _ in 0..3 { m.press(Key::Ok); }
        assert!(matches!(m.screen, Screen::Zone { field: 3, .. }));
        m.press(Key::Up);
        assert!(matches!(m.screen, Screen::Zone { field: 2, .. }));
        m.press(Key::Ok);
        m.press(Key::Down); // from Save, round to the first field
        assert!(matches!(m.screen, Screen::Zone { field: 0, .. }));
    }

    #[test]
    fn offset_parts_round_trip() {
        for min in OFFSET_MIN..=OFFSET_MAX {
            // Offsets in quarter hours, leaving out the gap just west of UTC that no zone uses.
            if min % 15 != 0 || (-59..0).contains(&min) {
                continue;
            }
            let (h, m) = offset_parts(min);
            assert_eq!(offset_of(h, m), min);
        }
    }

    #[test]
    fn the_last_row_asks_for_the_phone_setup_and_nothing_else_does() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Up); // wraps to Phone setup
        assert_eq!(m.screen, Screen::Top { cursor: 4 });
        let o = m.press(Key::Ok);
        assert!(o.setup && !o.save);
        // The other rows open their screens and never ask.
        for (downs, _) in [(0, ()), (1, ()), (2, ()), (3, ())] {
            let mut m = menu();
            m.press(Key::Ok);
            for _ in 0..downs { m.press(Key::Down); }
            assert!(!m.press(Key::Ok).setup);
        }
        assert!(!Outcome::default().setup);
    }
}
