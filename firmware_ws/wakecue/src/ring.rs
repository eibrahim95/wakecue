//! The 16-pixel WS2812 ring on GPIO10, driven by the RMT peripheral.
//!
//! The pixels keep the colour they were last sent for as long as they have power, so the lamp sets
//! the ring and goes back to sleep: the glow carries on through the sleep, because the lamp keeps
//! the rail switch on through a sleep that is inside a ramp. Any other sleep drops the rail, and the
//! ring goes dark by itself.

use esp_hal::Blocking;
use esp_hal::gpio::Level;
use esp_hal::peripherals::{GPIO10, RMT};
use esp_hal::rmt::{Channel, PulseCode, Rmt, Tx, TxChannelConfig, TxChannelCreator};
use esp_hal::time::Rate;

/// Pixels on the round module.
pub const LEDS: usize = 16;

/// The brightest any channel goes, out of 255. Sixteen pixels at full white want most of an amp,
/// which is more than a USB port or the bench supply promises; a quarter is enough to judge the
/// colour. Raise it when the ring runs from the cell.
pub const CAP: u8 = 64;

// The RMT runs at 80 MHz, so one tick is 12.5 ns. A WS2812 bit is a high pulse and a low pulse:
// 0.40 us + 0.85 us for a zero, 0.80 us + 0.45 us for a one, and the frame ends with the line
// held low for 50 us or more.
const T0H: u16 = 32;
const T0L: u16 = 68;
const T1H: u16 = 64;
const T1L: u16 = 36;
const LATCH: u16 = 6000;

pub struct LedRing {
    /// Held in an `Option` because a transmit takes the channel and gives it back when it ends.
    channel: Option<Channel<'static, Blocking, Tx>>,
    codes: [PulseCode; LEDS * 24 + 2],
}

impl LedRing {
    pub fn new(rmt: RMT<'static>, pin: GPIO10<'static>) -> LedRing {
        let rmt = Rmt::new(rmt, Rate::from_mhz(80)).expect("RMT clock");
        let channel = rmt
            .channel0
            .configure_tx(&TxChannelConfig::default().with_clk_divider(1))
            .expect("RMT channel")
            .with_pin(pin);
        LedRing {
            channel: Some(channel),
            codes: [PulseCode::new(Level::High, T0H, Level::Low, T0L); LEDS * 24 + 2],
        }
    }

    /// Every pixel the same colour (red, green, blue). A failed transmit has nowhere to be
    /// reported and the next wake sends the colour again, so it only costs a minute of the glow.
    pub fn show(&mut self, colour: [u8; 3]) {
        let mut i = 0;
        for _ in 0..LEDS {
            // A WS2812 takes green, then red, then blue, most significant bit first.
            for byte in [colour[1], colour[0], colour[2]] {
                for bit in (0..8).rev() {
                    self.codes[i] = if byte & (1 << bit) != 0 {
                        PulseCode::new(Level::High, T1H, Level::Low, T1L)
                    } else {
                        PulseCode::new(Level::High, T0H, Level::Low, T0L)
                    };
                    i += 1;
                }
            }
        }
        self.codes[i] = PulseCode::new(Level::Low, LATCH, Level::Low, 1);
        self.codes[i + 1] = PulseCode::end_marker();

        if let Some(channel) = self.channel.take() {
            if let Ok(tx) = channel.transmit(&self.codes)
                && let Ok(channel) = tx.wait()
            {
                self.channel = Some(channel);
            }
            // A channel lost in a failed transmit stays lost: the ring is a nicety, and the
            // alarm must go on without it.
        }
    }

    pub fn off(&mut self) {
        self.show([0, 0, 0]);
    }
}
