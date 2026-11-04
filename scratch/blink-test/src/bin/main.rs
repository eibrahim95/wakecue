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
use esp_hal::dma::TransferDirection::Out;
use esp_hal::gpio::{DriveMode, Level, Output, OutputConfig};
use esp_hal::ledc::channel::ChannelIFace;
use esp_hal::ledc::timer::TimerIFace;
use esp_hal::ledc::{Ledc, LowSpeed, channel, timer};
use esp_hal::main;
use esp_hal::time::{Duration, Instant, Rate};
use esp_println as _;
use lamp_core::ramp::ramp_duty;
// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 2.0.0
    // generator parameters: -o esp32c3 -o esp32c3-mini-1 -o defmt -o esp-backtrace -o agents -o ci -o zed

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO2
    // - GPIO8
    // - GPIO9
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _gpio11 = peripherals.GPIO11;
    let _gpio12 = peripherals.GPIO12;
    let _gpio13 = peripherals.GPIO13;
    let _gpio14 = peripherals.GPIO14;
    let _gpio15 = peripherals.GPIO15;
    let _gpio16 = peripherals.GPIO16;
    let _gpio17 = peripherals.GPIO17;
    let led = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let mut controller = Ledc::new(peripherals.LEDC);
    controller.set_global_slow_clock(esp_hal::ledc::LSGlobalClkSource::APBClk);
    let mut lstimer0 = controller.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(24),
        })
        .unwrap();
    let mut channel10 = controller.channel(channel::Number::Channel0, led);
    channel10
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        })
        .unwrap();

    // loop {
    //     channel10.start_duty_fade(100, 0, 1000).unwrap();
    //     while channel10.is_duty_fade_running() {}
    //     channel10.start_duty_fade(0, 100, 1000).unwrap();
    //     while channel10.is_duty_fade_running() {}
    // }

    const TOTAL_S: u32 = 30;
    loop {
        for elapsed in 00..=TOTAL_S {
            let duty = ramp_duty(elapsed, TOTAL_S);
            let pct = (duty as u32 * 100 / 255) as u8;
            channel10.set_duty(100 - pct).unwrap();
            info!("Elapsed {} duty {}", elapsed, duty);

            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(1000) {}
        }
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}
