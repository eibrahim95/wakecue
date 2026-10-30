//! The four menu buttons on a PCF8574 and the touch pad's tap/hold decision.

/// PCF8574 address with A0..A2 high (the usual module default); 0x38 for the PCF8574A.
pub const PCF8574_ADDR: u8 = 0x20;
/// Writing 0xFF releases every pin to its weak pull-up so the buttons can pull them low.
pub const RELEASE_ALL: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Ok,
    Back,
}

/// P0 = Up, P1 = Down, P2 = Ok, P3 = Back. A pressed button reads 0.
pub fn decode(port: u8) -> Option<Key> {
    let pressed = !port & 0b1111;
    match pressed {
        0b0001 => Some(Key::Up),
        0b0010 => Some(Key::Down),
        0b0100 => Some(Key::Ok),
        0b1000 => Some(Key::Back),
        _ => None, // nothing, or several at once
    }
}

/// Reports a key once, when the same key has been read on two samples in a row.
#[derive(Default)]
pub struct KeyReader {
    last: Option<Key>,
    reported: bool,
}

impl KeyReader {
    pub fn update(&mut self, port: u8) -> Option<Key> {
        let now = decode(port);
        if now != self.last {
            self.last = now;
            self.reported = false;
            return None;
        }
        match now {
            Some(k) if !self.reported => {
                self.reported = true;
                Some(k)
            }
            _ => None,
        }
    }
}

/// How long the pad must be held, in milliseconds, for a hold instead of a tap.
pub const HOLD_MS: u32 = 1000;
/// Touches shorter than this are ignored as noise.
pub const MIN_MS: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pad {
    Tap,
    Hold,
}

pub fn classify_pad(held_ms: u32) -> Option<Pad> {
    match held_ms {
        0..MIN_MS => None,
        MIN_MS..HOLD_MS => Some(Pad::Tap),
        _ => Some(Pad::Hold),
    }
}

/// The lamp goes back to sleep after this long with no touch or button.
pub const IDLE_SLEEP_MS: u32 = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_each_button_and_ignores_chords() {
        assert_eq!(decode(0xFF), None);
        assert_eq!(decode(0xFE), Some(Key::Up));
        assert_eq!(decode(0xFD), Some(Key::Down));
        assert_eq!(decode(0xFB), Some(Key::Ok));
        assert_eq!(decode(0xF7), Some(Key::Back));
        assert_eq!(decode(0xFC), None); // Up and Down together
        assert_eq!(decode(0xEF), None); // an unused pin is not a button
    }

    #[test]
    fn a_key_is_reported_once_per_press_after_two_matching_reads() {
        let mut r = KeyReader::default();
        assert_eq!(r.update(0xFF), None);
        assert_eq!(r.update(0xFB), None); // first sight: could be a bounce
        assert_eq!(r.update(0xFB), Some(Key::Ok));
        assert_eq!(r.update(0xFB), None); // still held
        assert_eq!(r.update(0xFF), None);
        assert_eq!(r.update(0xFF), None);
        assert_eq!(r.update(0xFB), None);
        assert_eq!(r.update(0xFB), Some(Key::Ok)); // pressed again
    }

    #[test]
    fn a_bounce_is_not_reported() {
        let mut r = KeyReader::default();
        assert_eq!(r.update(0xFE), None);
        assert_eq!(r.update(0xFF), None);
        assert_eq!(r.update(0xFE), None);
    }

    #[test]
    fn pad_tap_and_hold() {
        assert_eq!(classify_pad(10), None);
        assert_eq!(classify_pad(200), Some(Pad::Tap));
        assert_eq!(classify_pad(999), Some(Pad::Tap));
        assert_eq!(classify_pad(1000), Some(Pad::Hold));
    }
}
