//! The rail switch: GPIO5 drives the AO3400 gate (through 1 kOhm, with 100 kOhm to ground), and the
//! MOSFET joins the ring's and the MP3 board's grounds to the lamp's.
//!
//! The rail is switched on only by a wake that needs the ring or the player. The MP3 board pops its
//! speaker when its ground is joined, so a minute tick that plays nothing must not touch it. The pin
//! is also left alone until then because a pad held through the last sleep keeps the rail on, and
//! taking the pin over would drop it for a moment: a ramp wake takes it over still high.

use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::peripherals::GPIO5;
use esp_hal::time::Instant;

/// How long the MP3 board takes to start after its ground is joined, before it takes a command. A
/// command sent sooner than this is lost.
const START_MS: u64 = 1500;

pub struct Rail {
    pin: Option<GPIO5<'static>>,
    out: Option<Output<'static>>,
    on_at: Option<Instant>,
}

impl Rail {
    pub fn new(pin: GPIO5<'static>) -> Rail {
        Rail { pin: Some(pin), out: None, on_at: None }
    }

    /// Join the ring's and the player's grounds. Does nothing when this wake already did.
    pub fn on(&mut self) {
        if self.out.is_none()
            && let Some(pin) = self.pin.take()
        {
            self.out = Some(Output::new(pin, Level::High, OutputConfig::default()));
            self.on_at = Some(Instant::now());
        }
    }

    /// Wait out what is left of the MP3 board's start-up. Call it after `on` and before the first
    /// command of a wake; it returns at once when the board has been up long enough.
    pub fn settle(&mut self) {
        if let Some(t) = self.on_at {
            let up = t.elapsed().as_millis();
            if up < START_MS {
                Delay::new().delay_millis((START_MS - up) as u32);
            }
        }
    }

    /// Last thing before the sleep. `keep` holds the rail on through it, for a ring that shows a
    /// light ramp; otherwise the rail drops and the 100 kOhm holds the gate low while the pin floats.
    pub fn finish(&mut self, keep: bool) {
        let mut out = match self.out.take() {
            Some(out) => out,
            None => match self.pin.take() {
                Some(pin) => {
                    let level = if keep { Level::High } else { Level::Low };
                    Output::new(pin, level, OutputConfig::default())
                }
                None => return,
            },
        };
        if keep {
            out.set_high();
            out.set_pad_hold(true);
        } else {
            out.set_low();
        }
        self.out = Some(out);
    }
}
