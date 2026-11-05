#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::{Format, Formatter, info};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Event, Input, InputConfig, Level, Output, OutputConfig, Pull, WakeupConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_println as _;
use lamp_core::calendar::{FORMAT_LEN, format_minutes, utc_to_egypt};
use lamp_core::buttons::{self, KeyReader};
use lamp_core::ds3231;
use lamp_core::schedule::{self, Action, Prayer, PrayerTable, Rule, When};
use lamp_core::settings::{MAX_RULES, Settings};
use wakecue::display::{Face, PanelBus};

/// Bench switch. `true` runs the whole boot — clock, schedule, alarm, face — and then stays
/// awake instead of sleeping, so the USB port stays up and the log keeps running. A sleeping
/// board takes both away, which makes every boot a race. Set to `false` for the real lamp.
const DEV_MODE: bool = true;

/// The prayer table from `prayer-gen` (Cairo, ten years from 2026-01-01), built into the image.
/// `just prayers` regenerates it; the day range travels inside the file.
static PRAYERS: &[u8] = include_bytes!("../../prayers-cairo.bin");

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// A minute count printed as `YYYY-MM-DD HH:MM`, so a log line says when instead of making the
/// reader divide.
struct Stamp(i64);

impl Format for Stamp {
    fn format(&self, f: Formatter) {
        let mut buf = [0u8; FORMAT_LEN];
        defmt::write!(f, "{}", format_minutes(self.0, &mut buf));
    }
}

/// The clock's minute count, read fresh. Every boot and every awake window asks it at least once.
fn now_minutes(bus: &mut I2c<'static, esp_hal::Blocking>) -> i64 {
    let mut t = [0u8; 7];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t)
        .expect("read time");
    ds3231::parse_time_registers(t).div_euclid(60)
}

/// Clear the flag this boot owns and arm Alarm 1 for the next minute, which it returns.
///
/// The lamp redraws its clock every minute, so the next minute is always the next thing to do;
/// an event that is due is found on the boot that lands on it. Clearing `A1F` first matters: while
/// it is set the line stays low, and a wake pin sitting at its wake level ends a sleep as it starts.
fn arm_next_minute(bus: &mut I2c<'static, esp_hal::Blocking>, now_min: i64) -> i64 {
    let mut st = [0u8; 1];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
        .expect("status");
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_STATUS, st[0] & !ds3231::STATUS_A1F],
    )
    .expect("clear a1f");

    let a = ds3231::alarm1_registers(now_min + 1);
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
    now_min + 1
}

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
    // A1F separates the two things that can wake the lamp: the clock leaves it set, a hand on the
    // pad or on any button leaves it clear. Nothing needs to know which of the two, because both
    // mean the same thing — someone is standing here.
    let a1f = status[0] & ds3231::STATUS_A1F != 0;

    let now_min = now_minutes(&mut bus);
    // A wake is a wake from sleep; a power-on reset leaves the hardware with no cause to report.
    let woken = !esp_hal::rtc_cntl::wakeup_cause().is_empty();
    let by_hand = woken && !a1f;
    info!(
        "boot: osf {}, woken {}, by hand {}, now {} UTC = {} Cairo, dev mode {}",
        osf,
        woken,
        by_hand,
        Stamp(now_min),
        Stamp(utc_to_egypt(now_min)),
        DEV_MODE
    );

    // The prayer the face names, decided below; `None` means the clock cannot be trusted, so the
    // face says that instead of naming one.
    let mut face_next: Option<(i64, Prayer)> = None;
    if osf {
        // There is no Wi-Fi and no settings page in this crate yet, so the time cannot be
        // fetched. Keep waking once a minute rather than parking the lamp on a clock it
        // does not trust; this branch becomes the "set the time" page.
        info!("osf set: time untrusted, arming a one-minute wake");
    } else {
        // TODO: settings from flash once the menu and its pages exist.
        let settings = Settings::default();
        let mut rules = [Rule {
            days: 0,
            when: When::Local(0),
            action: Action::LampOff,
        }; MAX_RULES];
        let count = settings.rules(&mut rules);
        let prayers = PrayerTable::from_blob(PRAYERS).expect("prayer table");
        // The face names the next prayer of the five, whatever the switches say: it is a clock,
        // and the schedule below only answers "what will the lamp act on".
        face_next = schedule::next_prayer(now_min, &prayers);

        // Anything whose time has arrived is this boot's job. The alarm fired, so the event
        // is at or before now; after running it, look again from the following minute.
        let mut from = now_min;
        if let Some(due) = schedule::next_event(from, &rules[..count], &prayers)
            && due.utc_min <= now_min
        {
            info!(
                "due: {:?} at {} UTC = {} Cairo",
                defmt::Debug2Format(&due.action),
                Stamp(due.utc_min),
                Stamp(utc_to_egypt(due.utc_min))
            );
            from = due.utc_min + 1;
        }

        match schedule::next_event(from, &rules[..count], &prayers) {
            Some(next) => {
                info!(
                    "next: {:?} at {} UTC = {} Cairo ({} min from now)",
                    defmt::Debug2Format(&next.action),
                    Stamp(next.utc_min),
                    Stamp(utc_to_egypt(next.utc_min)),
                    next.utc_min - now_min
                );            }
            None => info!("nothing scheduled; waking in a minute"),
        }
    }

    // The panel is the lamp's face, so it is drawn once the boot knows what it is looking at:
    // a face that woke before the clock was read could only show yesterday's minute.
    let mut face = Face::new(PanelBus::new(spi, dc), busy, rst, woken);
    face.show(now_min, face_next);

    // The lamp redraws every minute, so the alarm is always the next minute; what is due is found
    // by the check above on whichever boot lands on it.
    let mut wakes_at = arm_next_minute(&mut bus, now_min);

    // The bench switch: from here on the only thing left is the sleep, so with dev mode on the
    // board stays awake and turns into a console — buttons, pad and the clock's own answer. It
    // is not a separate build: everything above this line ran for real.
    if DEV_MODE {
        info!("dev mode: staying awake so the log stays attached");
        bus.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
            .expect("release expander pins");
        let mut reader = KeyReader::default();
        let mut port = [0u8; 1];
        let mut last_port = buttons::RELEASE_ALL;
        let mut last_touch = pad.is_high();
        let mut shown_min = now_min;
        let mut ticks = 0u32;
        loop {
            bus.read(buttons::PCF8574_ADDR, &mut port)
                .expect("read expander");
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
            let touch = pad.is_high();
            if touch != last_touch {
                last_touch = touch;
                info!("pad {}", if touch { "touched" } else { "released" });
            }
            // Every five seconds, the clock's own answer, so a stopped or drifting clock shows
            // up while sitting at the bench rather than at 4 a.m. A new minute also redraws the
            // face, which is the fast-refresh path the lamp itself takes on every wake.
            ticks += 1;
            if ticks % 250 == 0 {
                let mut t = [0u8; 7];
                bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t)
                    .expect("read time");
                let now = ds3231::parse_time_registers(t).div_euclid(60);
                info!(
                    "now {} UTC, sqw {}",
                    Stamp(now),
                    if sqw.is_high() { "high" } else { "low" }
                );
                if now != shown_min {
                    shown_min = now;
                    face.show(now, face_next);
                }
            }
            delay.delay_millis(20);
        }
    }

    // A hand is the only way a person announces themselves — the pad, or any of the four buttons
    // sharing the clock's wake line through the expander's INT. It is worth staying awake for:
    // the face is readable, the buttons work, and a quiet lamp goes back to sleep by itself. Any
    // press or touch restarts the clock, so a menu being used never times out mid-press. A press
    // held for a second is a different thing from a tap, and will toggle the lamp once there is a
    // ring to toggle.
    if by_hand {
        info!("awake: a hand woke the lamp, staying {} s", buttons::IDLE_SLEEP_MS / 1000);
        bus.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
            .expect("release expander pins");
        let mut reader = KeyReader::default();
        let mut port = [0u8; 1];
        let mut last_touch = pad.is_high();
        let mut held_ms = 0u32;
        let mut idle_ms = 0u32;
        let mut lamp_on = false;
        while idle_ms < buttons::IDLE_SLEEP_MS {
            let mut busy = false;
            bus.read(buttons::PCF8574_ADDR, &mut port)
                .expect("read expander");
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
                busy = true;
            }
            // The pad is the thing that woke the lamp, so say what it is doing too.
            let touch = pad.is_high();
            if touch != last_touch {
                last_touch = touch;
                info!("pad {}", if touch { "touched" } else { "released" });
            }
            if touch {
                held_ms += 20;
                busy = true;
                if held_ms == buttons::HOLD_MS {
                    lamp_on = !lamp_on;
                    info!("hold: lamp {}", if lamp_on { "on" } else { "off" });
                }
            } else {
                held_ms = 0;
            }
            idle_ms = if busy { 0 } else { idle_ms + 20 };
            delay.delay_millis(20);
        }
        info!("awake: idle, back to sleep");

        // The window may have crossed a minute, and the alarm was armed before it.
        let later = now_minutes(&mut bus);
        if later != now_min {
            face.show(later, face_next);
        }
        wakes_at = arm_next_minute(&mut bus, later);
    }

    // The panel holds the last image with no power, so this is the last thing before the chip
    // goes down.
    face.finish();

    // A wake pin already sitting at its wake level ends the sleep at the moment it starts,
    // which would spin the chip through resets. Wait for both to clear first. GPIO3 is shared by
    // the clock's SQW and the expander's INT, and INT stays latched low after a button press
    // until the port is read — so read it, or a press during this wait would hold the lamp awake.
    let mut waited = false;
    while !sqw.is_high() || pad.is_high() {
        if !waited {
            info!("waiting for GPIO3 and GPIO4 to sit clear");
            waited = true;
        }
        let mut port = [0u8; 1];
        let _ = bus.read(buttons::PCF8574_ADDR, &mut port);
        delay.delay_millis(200);
    }

    sqw.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("arm GPIO3 as a wake source");
    sqw.listen(Event::LowLevel);

    // Same setup, opposite level: the pad's module is fixed active high.
    pad.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("arm GPIO4 as a wake source");
    pad.listen(Event::HighLevel);

    info!(
        "sleeping until {} UTC = {} Cairo",
        Stamp(wakes_at),
        Stamp(utc_to_egypt(wakes_at))
    );
    let mut lp = LowPower::new(peripherals.LPWR);
    lp.sleep_deep(RtcSleepConfig::deep());
}
