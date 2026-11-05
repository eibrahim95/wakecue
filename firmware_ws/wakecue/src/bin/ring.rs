//! Bench: drive the 16-pixel WS2812 ring on GPIO10.
//!
//! Three wires: 5 V, GND and data on GPIO10. The firmware walks red, green and blue so a swapped
//! colour order or a dead pixel is obvious, then runs the Fajr ramp compressed into 30 seconds.
//!
//! WS2812 wants 5 V logic and gets 3.3 V here, which usually works. If the first pixel glitches or
//! the colours smear towards the far end of the ring, that is the first thing to fix: a diode in
//! the ring's supply to bring it nearer 3.3 V, or a level shifter.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::Level;
use esp_hal::rmt::{PulseCode, Rmt, TxChannelConfig, TxChannelCreator};
use esp_hal::time::Rate;
use esp_println as _;
use lamp_core::ramp::{ramp_colour, ramp_duty};

esp_bootloader_esp_idf::esp_app_desc!();

/// Pixels on the round module.
const LEDS: usize = 16;
/// Seconds of ramp; the lamp's own is fifteen minutes.
const RAMP_S: u32 = 30;
/// The bench's own brightness cap. Sixteen pixels at full white want most of an amp, more than a
/// USB port promises, and a quarter is enough to judge colour.
const CAP: u16 = 64;

// The RMT runs at 80 MHz, so one tick is 12.5 ns. A WS2812 bit is a high pulse and a low pulse:
// 0.40 us + 0.85 us for a zero, 0.80 us + 0.45 us for a one, and the frame ends with the line
// held low for 50 us or more.
const T0H: u16 = 32;
const T0L: u16 = 68;
const T1H: u16 = 64;
const T1L: u16 = 36;
const LATCH: u16 = 6000;

/// One pulse-code per bit, then the latch and the end marker.
fn encode(pixels: &[[u8; 3]; LEDS], out: &mut [PulseCode]) {
    let mut i = 0;
    for px in pixels {
        // A WS2812 takes green, then red, then blue, most significant bit first.
        for byte in [px[1], px[0], px[2]] {
            for bit in (0..8).rev() {
                out[i] = if byte & (1 << bit) != 0 {
                    PulseCode::new(Level::High, T1H, Level::Low, T1L)
                } else {
                    PulseCode::new(Level::High, T0H, Level::Low, T0L)
                };
                i += 1;
            }
        }
    }
    out[i] = PulseCode::new(Level::Low, LATCH, Level::Low, 1);
    out[i + 1] = PulseCode::end_marker();
}

/// A colour dimmed to `duty` of full, then to the bench cap.
fn scaled(c: [u8; 3], duty: u8) -> [u8; 3] {
    let k = duty as u16 * CAP / 255;
    [
        (c[0] as u16 * k / 255) as u8,
        (c[1] as u16 * k / 255) as u8,
        (c[2] as u16 * k / 255) as u8,
    ]
}

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let delay = Delay::new();

    let rmt = Rmt::new(peripherals.RMT, Rate::from_mhz(80)).expect("RMT clock");
    let mut channel = rmt
        .channel0
        .configure_tx(&TxChannelConfig::default().with_clk_divider(1))
        .expect("RMT channel")
        .with_pin(peripherals.GPIO10);

    let mut codes = [PulseCode::new(Level::High, T0H, Level::Low, T0L); LEDS * 24 + 2];
    info!(
        "ring: {} pixels on GPIO10, colours then a {} s ramp at a quarter brightness",
        LEDS, RAMP_S
    );

    loop {
        for (name, colour) in [
            ("red", [255, 0, 0]),
            ("green", [0, 255, 0]),
            ("blue", [0, 0, 255]),
        ] {
            let px = scaled(colour, 255);
            info!("ring: {} ({=u8}, {=u8}, {=u8})", name, px[0], px[1], px[2]);
            encode(&[px; LEDS], &mut codes);
            channel = channel
                .transmit(&codes)
                .expect("transmit")
                .wait()
                .expect("wait");
            delay.delay_millis(1500);
        }

        for elapsed in 0..RAMP_S {
            let px = scaled(ramp_colour(elapsed, RAMP_S), ramp_duty(elapsed, RAMP_S));
            if elapsed % 5 == 0 {
                info!("ramp {} s: ({=u8}, {=u8}, {=u8})", elapsed, px[0], px[1], px[2]);
            }
            encode(&[px; LEDS], &mut codes);
            channel = channel
                .transmit(&codes)
                .expect("transmit")
                .wait()
                .expect("wait");
            delay.delay_millis(1000);
        }

        // A second of darkness so the next cycle reads as a new one.
        encode(&[[0, 0, 0]; LEDS], &mut codes);
        channel = channel
            .transmit(&codes)
            .expect("transmit")
            .wait()
            .expect("wait");
        delay.delay_millis(2000);
    }
}