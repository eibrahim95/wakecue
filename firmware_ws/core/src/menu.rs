//! The settings menu as a state machine: keys in, screen and settings out. Drawing is the display code's job.
use crate::buttons::Key;
use crate::schedule::Prayer;
use crate::settings::{Dst, Settings, UserAlarm, FRIDAY, FRI_SAT, MAX_USER_ALARMS, SUN_TO_THU};
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
    Save,
    Delete,
}

const FIELDS: [Field; 5] = [Field::Hour, Field::Minute, Field::Days, Field::Save, Field::Delete];

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
    /// 0 = Prayers, 1 = My alarms, 2 = Time zone.
    Top { cursor: u8 },
    Prayers { cursor: u8 },
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
        let mut out = Outcome { redraw: true, save: false };
        self.screen = match self.screen {
            Screen::Home => match key {
                Key::Ok => Screen::Top { cursor: 0 },
                _ => return Outcome::default(),
            },
            Screen::Top { cursor } => match key {
                Key::Up | Key::Down => Screen::Top { cursor: step(cursor, 3, key) },
                Key::Back => Screen::Home,
                // Back is the way out of the menu, so there is no Exit row to walk to.
                Key::Ok => match cursor {
                    0 => Screen::Prayers { cursor: 0 },
                    1 => Screen::Alarms { cursor: 0 },
                    _ => Screen::Zone {
                        field: 0,
                        minutes: self.settings.utc_offset_min,
                        dst: self.settings.dst,
                    },
                },
            },
            Screen::Prayers { cursor } => match key {
                Key::Up | Key::Down => Screen::Prayers { cursor: step(cursor, 5, key) },
                Key::Ok => {
                    self.settings.toggle_prayer(Prayer::ALL[cursor as usize]);
                    out.save = true;
                    Screen::Prayers { cursor }
                }
                Key::Back => Screen::Top { cursor: 0 },
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
                Key::Back => Screen::Top { cursor: 2 },
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
                        Screen::Top { cursor: 2 }
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
        Outcome { redraw: true, save: false }
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
    fn toggling_a_prayer_saves_immediately() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Ok);
        m.press(Key::Down); // Dhuhr
        let o = m.press(Key::Ok);
        assert!(o.save && o.redraw);
        assert!(m.settings.prayer_on(Prayer::Dhuhr));
        assert!(m.settings.prayer_on(Prayer::Fajr));
        m.press(Key::Ok);
        assert!(!m.settings.prayer_on(Prayer::Dhuhr));
    }

    #[test]
    fn cursors_wrap() {
        let mut m = menu();
        m.press(Key::Ok);
        m.press(Key::Up);
        assert_eq!(m.screen, Screen::Top { cursor: 2 });
        m.press(Key::Down);
        assert_eq!(m.screen, Screen::Top { cursor: 0 });
    }

    #[test]
    fn the_top_screen_is_three_lists_and_back_is_the_way_out() {
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
        m.press(Key::Ok); // Hour -> Minute -> Days -> Save
        m.press(Key::Down); // Save -> Delete
        assert!(matches!(m.screen, Screen::Edit { field: 4, .. }));
        let o = m.press(Key::Ok);
        assert!(o.save);
        assert!(!m.settings.alarms[2].used);
    }

    fn open_zone(m: &mut Menu) {
        m.press(Key::Ok);
        m.press(Key::Up); // wraps to Time zone
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
        assert_eq!(m.screen, Screen::Top { cursor: 2 });
    }

    #[test]
    fn back_on_the_zone_screen_discards_the_draft() {
        let mut m = menu();
        open_zone(&mut m);
        m.press(Key::Down);
        let o = m.press(Key::Back);
        assert!(!o.save);
        assert_eq!(m.settings.utc_offset_min, 120);
        assert_eq!(m.screen, Screen::Top { cursor: 2 });
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
}
