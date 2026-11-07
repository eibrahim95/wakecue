#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Interface};
use lamp_core::ds3231;
use static_cell::StaticCell;
extern crate alloc;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, Stack};
use embassy_time::with_timeout;
use esp_hal::i2c::master::{Config, I2c};
use lamp_core::calendar::civil_from_days;
// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
const NTP_UNIX_OFFSET: i64 = 2_208_988_800;
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 2.0.0
    // generator parameters: -o esp32c3 -o wifi -o alloc -o unstable-hal -o embassy -o defmt -o esp-backtrace -o esp32c3-mini-1 -o agents -o zed

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

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("Embassy initialized!");
    let mut bus = I2c::new(peripherals.I2C0, Config::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);
    let mut found = 0u8;
    for addr in 0x08..=0x77 {
        let mut b = [0u8; 1];
        match bus.read(addr, &mut b) {
            Ok(()) => {
                found += 1;
                info!("found 0x{:02x}", addr);
            }
            Err(e) => {
                if addr == ds3231::I2C_ADDR {
                    info!("0x68 read: {:?}", defmt::Debug2Format(&e));
                }
            }
        }
    }
    info!("scan done, {} device(s)", found);

    loop {
        info!("Getting the time");
        let mut t = [0u8; 7];
        match bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t) {
            Ok(()) => info!("{}, {}", t, ds3231::parse_time_registers(t)),
            Err(e) => info!("time read: {:?}", defmt::Debug2Format(&e)),
        }
        Timer::after(Duration::from_secs(10)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}
