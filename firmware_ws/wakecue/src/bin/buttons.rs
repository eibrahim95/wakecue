//! Bench: read the four menu buttons on the PCF8574 and print what is pressed.
//!
//! Stays awake so the log can be read — this is a bench tool, not the lamp. The
//! four switches hang on P0..P3 and pull their pin low; the rest of the port is
//! released high so nothing else interferes.

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
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_println as _;
use lamp_core::buttons::{self, KeyReader};

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let delay = Delay::new();

    let mut bus = I2c::new(peripherals.I2C0, I2cConfig::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);

    // Write 0xFF first: that releases every expander pin to its weak pull-up, so a
    // button shows up as that one bit reading 0.
    bus.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
        .expect("release expander pins");
    info!("buttons: press one");

    let mut reader = KeyReader::default();
    let mut last_port = buttons::RELEASE_ALL;
    loop {
        let mut port = [0u8; 1];
        bus.read(buttons::PCF8574_ADDR, &mut port)
            .expect("read expander");

        // Only on change, so a held button is not a wall of identical lines.
        if port[0] != last_port {
            last_port = port[0];
            info!("port {}", port[0]);
        }

        if let Some(key) = reader.update(port[0]) {
            info!(
                "pressed {}",
                match key {
                    buttons::Key::Up => "Up",
                    buttons::Key::Down => "Down",
                    buttons::Key::Ok => "OK",
                    buttons::Key::Back => "Back",
                }
            );
        }

        delay.delay_millis(20);
    }
}
