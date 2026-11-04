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
use esp_hal::gpio::{Event, Input, InputConfig, Level, Output, OutputConfig, Pull, WakeupConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_println as _;
use lamp_core::ds3231;
use lamp_core::schedule::{self, Action, PrayerTable, Rule, When};
use lamp_core::settings::{MAX_RULES, Settings};
use wakecue::display::{PanelBus, first_frame};

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// A wake is a reset, so this function is the whole program: read the clock, run what is
/// due, arm the next event, sleep. Nothing is remembered in RAM because nothing survives.
#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let delay = Delay::new();

    // The display owns GPIO0, 1, 2, 8 and 9 (reset, busy, data/command, data, clock), so
    // this firmware never drives the onboard LED: it shares GPIO8 with the SPI data line.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default().with_frequency(Rate::from_mhz(4)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO9)
    .with_mosi(peripherals.GPIO8);
    let dc = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO0, Level::High, OutputConfig::default());
    let busy = Input::new(peripherals.GPIO1, InputConfig::default());

    // Bring-up only: every boot redraws the same frame, which costs a full refresh
    // (~2 s). Once the pages exist, the panel is only redrawn when something changes.
    first_frame(PanelBus::new(spi, dc), busy, rst);

    let mut sqw = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Up),
    );
    // The TTP223 drives its output both ways and idles low, so this pin takes no pull-up.
    // The weak pull-down only matters if its jumper comes loose: a floating line must not
    // read as a finger.
    let mut pad = Input::new(
        peripherals.GPIO4,
        InputConfig::default().with_pull(Pull::Down),
    );

    let mut bus = I2c::new(peripherals.I2C0, I2cConfig::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);

    // The clock's own note about itself. Set means it lost power and cannot know how much
    // time went by, so nothing in its registers can be trusted for scheduling.
    let mut status = [0u8; 1];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut status)
        .expect("read status");
    let osf = status[0] & ds3231::STATUS_OSF != 0;

    let mut t = [0u8; 7];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t)
        .expect("read time");
    let now_min = ds3231::parse_time_registers(t).div_euclid(60);
    info!("boot: osf {}, now {} min", osf, now_min);

    let mut target = now_min + 1;
    if osf {
        // There is no Wi-Fi and no settings page in this crate yet, so the time cannot be
        // fetched. Keep waking once a minute rather than parking the lamp on a clock it
        // does not trust; this branch becomes the "set the time" page.
        info!("osf set: time untrusted, arming a one-minute wake");
    } else {
        // TODO: the generated prayer table, and settings from flash once the pages exist.
        let settings = Settings::default();
        let mut rules = [Rule {
            days: 0,
            when: When::Local(0),
            action: Action::LampOff,
        }; MAX_RULES];
        let count = settings.rules(&mut rules);
        let prayers = PrayerTable {
            first_day: 0,
            data: &[],
        };

        // Anything whose time has arrived is this boot's job. The alarm fired, so the event
        // is at or before now; after running it, look again from the following minute.
        let mut from = now_min;
        if let Some(due) = schedule::next_event(from, &rules[..count], &prayers)
            && due.utc_min <= now_min
        {
            info!("due: {:?}", defmt::Debug2Format(&due.action));
            from = due.utc_min + 1;
        }

        match schedule::next_event(from, &rules[..count], &prayers) {
            Some(next) => {
                info!(
                    "next: {:?} at {} min ({} from now)",
                    defmt::Debug2Format(&next.action),
                    next.utc_min,
                    next.utc_min - now_min
                );
                target = next.utc_min;
            }
            None => info!("nothing scheduled; waking in a minute"),
        }
    }

    // A1F is the alarm that woke this boot; while it is set the line stays low, which would
    // end the next sleep immediately. OSF is deliberately left alone: only a boot that
    // wrote a fresh time is allowed to clear it.
    let mut st = [0u8; 1];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
        .expect("status");
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_STATUS, st[0] & !ds3231::STATUS_A1F],
    )
    .expect("clear a1f");

    let a = ds3231::alarm1_registers(target);
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_ALARM1, a[0], a[1], a[2], a[3]],
    )
    .expect("alarm1");
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_CONTROL, ds3231::CONTROL_ALARM1_ONLY],
    )
    .expect("control");

    // A wake pin already sitting at its wake level ends the sleep at the moment it starts,
    // which would spin the chip through resets. Wait for both to clear first.
    let mut waited = false;
    while !sqw.is_high() || pad.is_high() {
        if !waited {
            info!("waiting for the wake levels to clear");
            waited = true;
        }
        delay.delay_millis(200);
    }

    sqw.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("arm GPIO3 as a wake source");
    sqw.listen(Event::LowLevel);

    // Same setup, opposite level: the pad's module is fixed active high.
    pad.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("arm GPIO4 as a wake source");
    pad.listen(Event::HighLevel);

    info!("sleeping until {} min", target);
    let mut lp = LowPower::new(peripherals.LPWR);
    lp.sleep_deep(RtcSleepConfig::deep());
}
