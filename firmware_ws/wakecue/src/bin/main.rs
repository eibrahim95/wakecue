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
use esp_storage::FlashStorage;
use lamp_core::calendar::{FORMAT_LEN, format_minutes, utc_to_egypt};
use lamp_core::buttons::{self, KeyReader};
use lamp_core::ds3231;
use lamp_core::schedule::{self, Action, Prayer, PrayerTable, Rule, When};
use lamp_core::menu::{Menu, Screen};
use lamp_core::settings::{
    MAX_RULES, READ_LEN, Sector, Settings, Stored, SECTOR_A, SECTOR_B, SECTOR_LEN, STORED_LEN,
};
use wakecue::display::{Face, PanelBus, Standby};

/// Bench switch. `true` runs the whole boot — clock, schedule, alarm, face — and then stays
/// awake instead of sleeping, so the USB port stays up and the log keeps running. A sleeping
/// board takes both away, which makes every boot a race. Set to `false` for the real lamp.
const DEV_MODE: bool = false;

/// How long an alarm nobody answers keeps flashing before the lamp gives up and goes back to
/// sleep. A real ring should ring for longer than this; the panel is a stand-in, not a clock radio.
const ALARM_MS: u32 = 60_000;

/// How long each of the alarm's two frames stays up, so the panel changes about once a second.
const ALARM_BLINK_MS: u32 = 1_000;

/// How often the buttons are read while an alarm is flashing. The debouncer wants two matching
/// reads, so polling faster than the frame means a press is answered without holding it.
const ALARM_POLL_MS: u32 = 50;

/// How long the fingers must be still before the panel is drawn. Nothing is sampled while the
/// glass refreshes, so a run of presses is applied to the menu first and drawn once at the end:
/// pressing Down five times moves five rows instead of one, and only the presses that land inside
/// a refresh are still lost.
const DRAW_QUIET_MS: u32 = 180;

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

/// A key as the log names it.
fn key_name(key: buttons::Key) -> &'static str {
    match key {
        buttons::Key::Up => "Up",
        buttons::Key::Down => "Down",
        buttons::Key::Ok => "OK",
        buttons::Key::Back => "Back",
    }
}

/// A byte buffer the flash ROM can take as it stands. Given a pointer it cannot align, it copies
/// the write through a 4 KB sector buffer on the stack, which is a frame this file refuses to have.
#[repr(align(4))]
struct Words<const N: usize>([u8; N]);

/// The block a sector holds, or nothing if it is blank, damaged or unreadable. Only the first
/// [`READ_LEN`] bytes are read rather than the whole 4 KB: that is the block plus room for a field a
/// later build appends, and the rest of the sector carries nothing.
fn read_block(flash: &mut FlashStorage<'static>, at: u32) -> Option<Stored> {
    let mut buf = Words::<READ_LEN>([0u8; READ_LEN]);
    flash.read_nor(at, &mut buf.0).ok()?;
    Stored::decode(&buf.0)
}

/// Store the settings in the sector that is not holding the newest good block, and update the two
/// sectors to match what is now on the flash. The previous block is still there if this fails, so a
/// brown-out during the erase costs the change, never the settings that were already stored.
fn store(
    flash: &mut FlashStorage<'static>,
    sectors: &mut [Option<Stored>; 2],
    settings: Settings,
) -> bool {
    let (sector, seq) = Stored::save_target(sectors[0].as_ref(), sectors[1].as_ref());
    let block = Stored { seq, settings };
    let at = sector.addr();
    let slot = if sector == Sector::A { 0 } else { 1 };
    let mut buf = Words::<STORED_LEN>([0u8; STORED_LEN]);
    buf.0.copy_from_slice(&block.encode());

    // Whatever happens below, this sector can no longer be spoken for: an erase that fails halfway
    // or a write that lands short leaves bytes nobody should believe.
    sectors[slot] = None;
    if let Err(e) = flash.erase(at, at + SECTOR_LEN as u32) {
        info!("settings: erase at {=u32:#06x} failed: {:?}", at, defmt::Debug2Format(&e));
        return false;
    }
    if let Err(e) = flash.write_nor(at, &buf.0) {
        info!("settings: write at {=u32:#06x} failed: {:?}", at, defmt::Debug2Format(&e));
        return false;
    }
    sectors[slot] = Some(block);
    info!(
        "settings: sequence {} stored in sector {} ({=u32:#06x})",
        seq,
        if slot == 0 { "A" } else { "B" },
        at
    );
    true
}

/// The alarm, on the only output the lamp has so far: the panel, flashing what is ringing and when
/// it was due, until a key or a touch answers it or until it has gone on long enough. Returns
/// whether somebody answered.
///
/// The ring and the card are the missing half of this. They hang off this same call, with the same
/// title and the same deadline, which is why the caller passes a title rather than a screen.
fn run_alarm(
    face: &mut Face,
    bus: &mut I2c<'static, esp_hal::Blocking>,
    pad: &mut Input<'static>,
    title: &str,
    at: i64,
) -> bool {
    // The expander latches INT low on a press until its port is read, so start from a known
    // release rather than from whatever the wake left behind.
    bus.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
        .expect("release expander pins");
    // Both frames are drawn in turn, so pressing a key is answered without waiting for the rest of
    // the minute: the very next frame is the clock face.
    let delay = Delay::new();
    let mut reader = KeyReader::default();
    let mut port = [0u8; 1];
    let mut waited = 0u32;
    let mut inverted = false;
    loop {
        face.show_alarm(title, at, inverted);
        inverted = !inverted;
        waited += ALARM_BLINK_MS;
        if waited >= ALARM_MS {
            return false;
        }
        let mut polled = 0;
        while polled < ALARM_BLINK_MS {
            bus.read(buttons::PCF8574_ADDR, &mut port)
                .expect("read expander");
            if reader.update(port[0]).is_some() || pad.is_high() {
                return true;
            }
            delay.delay_millis(ALARM_POLL_MS);
            polled += ALARM_POLL_MS;
        }
    }
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

    // The stored settings, read before anything asks a question they answer. Nothing else in this
    // build uses flash, so the two sectors inside the unused `nvs` partition are the lamp's alone.
    let mut flash = FlashStorage::new(peripherals.FLASH);
    let mut sectors = [read_block(&mut flash, SECTOR_A), read_block(&mut flash, SECTOR_B)];
    let stored = Stored::newest(sectors[0], sectors[1]);
    let settings = stored.map_or_else(Settings::default, |s| s.settings);

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
    match stored {
        Some(s) => info!(
            "settings: sequence {} loaded, place {}, prayers {}",
            s.seq,
            s.settings.place_name(),
            s.settings.prayers_on
        ),
        None => info!("settings: nothing stored, defaults"),
    }

    // The prayer the face names, decided below; `None` means the clock cannot be trusted, so the
    // face says that instead of naming one.
    let mut face_next: Option<(i64, Prayer)> = None;
    // Today's five prayers as local minutes, for the strip along the bottom of the face.
    let mut today: [Option<i64>; 5] = [None; 5];
    // The event this boot has to answer for, if one came due: the prayer's sound or a user alarm.
    // The light ramp is not an alarm, and nothing is wired to ring yet.
    let mut alarm: Option<(i64, Action)> = None;
    if osf {
        // There is no Wi-Fi and no settings page in this crate yet, so the time cannot be
        // fetched. Keep waking once a minute rather than parking the lamp on a clock it
        // does not trust; this branch becomes the "set the time" page.
        info!("osf set: time untrusted, arming a one-minute wake");
    } else {
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
        today = schedule::local_times(now_min, &prayers);

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
            if matches!(due.action, Action::Play { .. } | Action::Alarm) {
                alarm = Some((due.utc_min, due.action));
            }
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
    // Asleep unless a hand woke the lamp: the corner mark is what the lamp shows while it is
    // down, and it comes off for as long as somebody is standing in front of it.
    // A boot that woke for an alarm goes straight to the alarm: drawing the clock first would put
    // a two-second full refresh on the glass at 4 a.m. only to paint over it.
    if alarm.is_none() {
        face.show_clock(Standby {
            now_min,
            next: face_next,
            today,
            prayers_on: settings.prayers_on,
            sleeping: !by_hand,
        });
    }

    // The alarm is the whole reason this boot exists, so it takes the panel until somebody answers
    // it or it gives up. Whatever happens, the clock is drawn again afterwards: the glass must
    // never be left showing a ring that has ended.
    if let Some((at, action)) = alarm {
        let title = match action {
            Action::Play { track, .. } => Prayer::ALL[(track.max(1) as usize - 1).min(4)].name(),
            _ => "Alarm",
        };
        info!(
            "alarm: {} due at {}, flashing for up to {} s",
            title,
            Stamp(at),
            ALARM_MS / 1000
        );
        let answered = run_alarm(&mut face, &mut bus, &mut pad, title, at);
        info!(
            "alarm: {}",
            if answered { "answered" } else { "gave up, nobody came" }
        );
        let later = now_minutes(&mut bus);
        face.show_clock(Standby {
            now_min: later,
            next: face_next,
            today,
            prayers_on: settings.prayers_on,
            sleeping: !by_hand,
        });
    }

    // The lamp redraws every minute, so the alarm is always the next minute; what is due is found
    // by the check above on whichever boot lands on it. Armed after the alarm window rather than
    // before it, because an alarm can hold the panel past the minute: programming a minute that
    // has already gone by leaves the clock's alarm matching nothing, and the lamp would sleep for
    // good.
    let after = now_minutes(&mut bus);
    let mut wakes_at = arm_next_minute(&mut bus, after);

    // A hand is the only way a person announces themselves — the pad, or any of the four buttons
    // sharing the clock's wake line through the expander's INT. It is worth staying awake for:
    // the face is readable, the menu works, and a quiet lamp goes back to sleep by itself. Any
    // press or touch restarts the clock, so a menu being used never times out mid-press. A press
    // held for a second is a different thing from a tap, and will toggle the lamp once there is a
    // ring to toggle.
    //
    // Dev mode opens the same window without needing a hand and never closes it, so the log stays
    // attached while the menu is driven from the bench. It is not a separate build: everything
    // above this line ran for real.
    if by_hand || DEV_MODE {
        if DEV_MODE {
            info!("awake: dev mode, the window stays open so the log keeps up");
        } else {
            info!("awake: a hand woke the lamp, staying {} s", buttons::IDLE_SLEEP_MS / 1000);
        }
        bus.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
            .expect("release expander pins");
        let mut reader = KeyReader::default();
        let mut menu = Menu::new(settings);
        let mut port = [0u8; 1];
        let mut last_port = buttons::RELEASE_ALL;
        let mut last_touch = pad.is_high();
        let mut held_ms = 0u32;
        let mut idle_ms = 0u32;
        let mut lamp_on = false;
        let mut shown_min = now_min;
        let mut ticks = 0u32;
        // The menu has changed and the glass is behind. See DRAW_QUIET_MS.
        let mut dirty = false;
        loop {
            let mut busy = false;
            bus.read(buttons::PCF8574_ADDR, &mut port)
                .expect("read expander");
            // The raw port as well as the decoded key: a port that stops changing means the
            // expander or the bus, a port that changes with no key means the decoding.
            if port[0] != last_port {
                last_port = port[0];
                info!("port {}", port[0]);
            }
            if let Some(key) = reader.update(port[0]) {
                // One line per press, with the screen on both sides of it: if the lamp ever stops
                // answering again, the last line names the press and the screen it died on.
                let before = menu.screen;
                let outcome = menu.press(key);
                info!(
                    "menu: {} on {:?} -> {:?}, save {}",
                    key_name(key),
                    defmt::Debug2Format(&before),
                    defmt::Debug2Format(&menu.screen),
                    outcome.save
                );
                if outcome.save {
                    // The menu has already changed its own copy. This is the only place a block is
                    // written, so the minute tick can never wear the flash out; a failed write
                    // leaves the change in RAM and says so, and the lamp still goes to sleep.
                    if !store(&mut flash, &mut sectors, menu.settings) {
                        info!("settings: change kept in RAM only");
                    }
                }
                if outcome.redraw {
                    // Not drawn here: the refresh would blind the loop for a third of a second
                    // and eat the next presses. It is drawn below, once the fingers stop.
                    dirty = true;
                }
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
            // Every five seconds, the clock's own answer, so a stopped or drifting clock shows up
            // while sitting at the bench rather than at 4 a.m. A new minute with the face up also
            // redraws it, the same fast-refresh path the lamp takes on every wake.
            ticks += 1;
            if ticks % 250 == 0 {
                let now = now_minutes(&mut bus);
                info!(
                    "now {} UTC, sqw {}",
                    Stamp(now),
                    if sqw.is_high() { "high" } else { "low" }
                );
                if menu.screen == Screen::Home && now != shown_min && face.is_idle() {
                    shown_min = now;
                    face.show_clock(Standby {
                        now_min: now,
                        next: face_next,
                        today,
                        prayers_on: menu.settings.prayers_on,
                        sleeping: false,
                    });
                }
            }
            idle_ms = if busy { 0 } else { idle_ms + 20 };
            // The one draw, now that nothing has been pressed for a moment and the glass is free.
            // Leaving the menu puts the clock back, read fresh in case the presses crossed a
            // minute. The refresh is only started: the next loop iteration reads the buttons again
            // while the panel works, so a press never waits behind a refresh it could have been
            // read before.
            if dirty && idle_ms >= DRAW_QUIET_MS && face.is_idle() {
                let started = esp_hal::time::Instant::now();
                match menu.screen {
                    Screen::Home => {
                        let now = now_minutes(&mut bus);
                        shown_min = now;
                        face.show_clock(Standby {
                            now_min: now,
                            next: face_next,
                            today,
                            prayers_on: menu.settings.prayers_on,
                            sleeping: false,
                        });
                    }
                    _ => face.show_menu(&menu),
                }
                info!("panel: pushed in {} ms", started.elapsed().as_millis());
                dirty = false;
            }
            // Dev mode keeps the window open for the log; a hand's window closes after the idle
            // time so the lamp goes back to saving its battery. Anything pending has been drawn by
            // now: the quiet time is far shorter than the idle time.
            if !DEV_MODE && idle_ms >= buttons::IDLE_SLEEP_MS {
                break;
            }
            delay.delay_millis(20);
        }
        info!("awake: idle, back to sleep");

        // The window may have crossed a minute, and the alarm was armed before it. Redraw with
        // the corner mark on, so the last thing on the glass is the lamp going back to sleep.
        let later = now_minutes(&mut bus);
        face.show_clock(Standby {
            now_min: later,
            next: face_next,
            today,
            prayers_on: menu.settings.prayers_on,
            sleeping: true,
        });
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
