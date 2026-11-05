//! The settings menu as a state machine: keys in, screen and settings out. Drawing is the display code's job.
use crate::buttons::Key;
use crate::schedule::Prayer;
use crate::settings::{Settings, UserAlarm, FRIDAY, FRI_SAT, MAX_USER_ALARMS, SUN_TO_THU};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The standby face: time, next alarm, prayer times. Ok opens the menu.
    Home,
    /// 0 = Prayers, 1 = My alarms, 2 = Exit.
    Top { cursor: u8 },
    Prayers { cursor: u8 },
    Alarms { cursor: u8 },
    Edit { slot: u8, field: u8, draft: UserAlarm },
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
                Key::Ok => match cursor {
                    0 => Screen::Prayers { cursor: 0 },
                    1 => Screen::Alarms { cursor: 0 },
                    _ => Screen::Home,
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
        assert_eq!(m.settings.alarms[0], UserAlarm { used: true, enabled: true, days: FRI_SAT, minute: 6 * 60 + 30 });
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
        m.settings.alarms[2] = UserAlarm { used: true, enabled: true, days: ALL_DAYS, minute: 300 };
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
}
