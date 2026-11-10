#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

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
use lamp_core::astro::Place;
use lamp_core::buttons::{self, KeyReader};
use lamp_core::calendar::{FORMAT_LEN, Zone, format_minutes};
use lamp_core::dfplayer::fade_volume;
use lamp_core::ds3231;
use lamp_core::menu::{Menu, Screen};
use lamp_core::ramp::ramp_pixel;
use lamp_core::schedule::{self, Action, Prayer, PrayerTimes, Rule, When};
use lamp_core::settings::{
    MAX_RULES, READ_LEN, SECTOR_A, SECTOR_B, SECTOR_LEN, STORED_LEN, Sector, Settings, Stored,
};
use wakecue::clock;
use wakecue::display::{Face, NextUp, PanelBus, Standby};
use wakecue::player::Player;
use wakecue::ring::{self, LedRing};

// The phone setup needs a heap and the radio, which the other binaries in this crate do not
// link, so it belongs to this binary and not to the library.
#[path = "../radio.rs"]
mod radio;
#[path = "../setup.rs"]
mod setup;
#[path = "../station.rs"]
mod station;

/// How long an alarm nobody answers keeps flashing before the lamp gives up and goes back to
/// sleep. A real ring should ring for longer than this; the panel is a stand-in, not a clock radio.
const ALARM_MS: u32 = 60_000;

/// How long a prayer's adhan may play before the lamp gives up on it: longer than any of the
/// shipped files, so a recording plays to its end unless somebody answers it first.
const PLAY_MAX_MS: u32 = 420_000;

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
    /// Bench switch. `true` runs the whole boot — clock, schedule, alarm, face — and then stays
    /// awake instead of sleeping, so the USB port stays up and the log keeps running. A sleeping
    /// board takes both away, which makes every boot a race. Set to `false` for the real lamp.
    let DEV_MODE: bool = env!("DEV_MODE") == "true";
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let delay = Delay::new();

    // The stored settings, read before anything asks a question they answer. Nothing else in this
    // build uses flash, so the two sectors inside the unused `nvs` partition are the lamp's alone.
    let mut flash = FlashStorage::new(peripherals.FLASH);
    let mut sectors = [
        read_block(&mut flash, SECTOR_A),
        read_block(&mut flash, SECTOR_B),
    ];
    let stored = Stored::newest(sectors[0], sectors[1]);
    let settings = stored.map_or_else(Settings::default, |s| s.settings);
    // The five times are computed from the stored place on every boot: nothing about them is in
    // the image, so the place the owner picks is the place the lamp computes for.
    let prayers = Place::new(settings.lat_e2, settings.lon_e2, settings.method());
    // The clock on the wall comes from the same block: the offset and summer-time rule the owner
    // picked, not Egypt's.
    let mut zone = settings.zone();
    // Nothing stored at all means this lamp has never been set up.
    let first_boot = stored.is_none();
    // The radio's parts wait here until the clock sync or the phone setup needs them; whichever
    // comes first starts it, once.
    let mut radio = radio::Lazy::new(radio::Parts {
        wifi: peripherals.WIFI,
        timer: peripherals.TIMG0,
        interrupt: peripherals.FROM_CPU_INTR0,
    });
    // The MP3 board's serial link. Nothing is sent until something is to be played.
    let mut player = Player::new(peripherals.UART1, peripherals.GPIO20, peripherals.GPIO21);
    // The LED ring. It holds the colour it was last given through the sleep, so each wake sets it
    // to match the minute: a step of the light ramp, or dark.
    let mut led = LedRing::new(peripherals.RMT, peripherals.GPIO10);

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

    let mut i2c = I2c::new(peripherals.I2C0, I2cConfig::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);

    let mut clock_status = [0u8; 1];
    i2c.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut clock_status)
        .expect("read status");
    let osf = clock_status[0] & ds3231::STATUS_OSF != 0;
    let a1f = clock_status[0] & ds3231::STATUS_A1F != 0;

    let now_min = clock::now_minutes(&mut i2c);
    // A wake is a wake from sleep; a power-on reset leaves the hardware with no cause to report.
    let woken = !esp_hal::rtc_cntl::wakeup_cause().is_empty();
    let by_hand = woken && !a1f;
    info!(
        "boot: osf {}, woken {}, by hand {}, now {} UTC = {} local, dev mode {}",
        osf,
        woken,
        by_hand,
        Stamp(now_min),
        Stamp(zone.to_local(now_min)),
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

    // What the face's next-up line names, decided below; `None` means the clock cannot be trusted,
    // so the face says that instead of naming anything.
    let mut face_next = FaceNext::NONE;
    // Today's five prayers as local minutes, for the strip along the bottom of the face.
    let mut today: [Option<i64>; 5] = [None; 5];
    // The event this boot has to answer for, if one came due: the prayer's sound or a user alarm.
    // The light ramp is not an alarm, and nothing is wired to ring yet.
    let mut alarm: Option<(i64, Action)> = None;
    // The panel is taken over before the clock sync, which can take half a minute with nothing on
    // the glass otherwise. It is only drawn on when there is something to say.
    let mut face = Face::new(PanelBus::new(spi, dc), busy, rst, woken);
    if osf {
        // The clock lost power, so its time cannot be believed. With a network stored, ask it for
        // the time: the finished lamp does this once, after the setup page, and never again unless
        // the clock loses power. Any failure just falls through to waking once a minute, so a wrong
        // password cannot keep the lamp awake or stop the boot.
        if settings.ssid().is_empty() {
            info!("osf set: no network stored, the time cannot be fetched");
        } else {
            info!("osf set: joining {} for the time", settings.ssid());
            face.show_message("Syncing time...", settings.ssid());
            if let Some(unix) = station::sync(radio.get(), settings.ssid(), settings.psk()) {
                let regs = ds3231::time_registers(unix);
                let mut frame = [0u8; 8];
                frame[0] = ds3231::REG_TIME;
                frame[1..].copy_from_slice(&regs);
                i2c.write(ds3231::I2C_ADDR, &frame).expect("write time");
                // A fresh time is the sign-off: only now may the power-loss flag go, and the stale
                // alarm flag with it, so nothing claims to be due from before the loss.
                let mut st = [0u8; 1];
                i2c.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
                    .expect("status");
                i2c.write(
                    ds3231::I2C_ADDR,
                    &[
                        ds3231::REG_STATUS,
                        st[0] & !(ds3231::STATUS_A1F | ds3231::STATUS_OSF),
                    ],
                )
                .expect("clear flags");
                let mut done = settings;
                done.setup.wifi = true;
                done.setup.clock = true;
                if !store(&mut flash, &mut sectors, done) {
                    info!("settings: the setup flags could not be written");
                }
                info!("clock set from the network to {} UTC, resetting", Stamp(unix.div_euclid(60)));
                // A clean boot with a clock that can be believed, instead of patching this one up.
                esp_hal::system::software_reset();
            }
            info!("osf set: no time from the network");
        }
        info!("osf set: time untrusted, arming a one-minute wake");
    } else {
        let mut rules = [Rule {
            days: 0,
            when: When::Local(0),
            action: Action::LampOff,
        }; MAX_RULES];
        let count = settings.rules(&mut rules);
        // The face names the next prayer of the five, whatever the switches say: it is a clock,
        // and the schedule below only answers "what will the lamp act on".
        face_next = FaceNext::at(now_min, &settings, &prayers);
        today = schedule::local_times(now_min, &prayers, zone);

        // Anything whose time has arrived is this boot's job. The alarm fired, so the event
        // is at or before now; after running it, look again from the following minute.
        let mut from = now_min;
        if let Some(due) = schedule::next_event(from, &rules[..count], &prayers, zone)
            && due.utc_min <= now_min
        {
            info!(
                "due: {:?} at {} UTC = {} local",
                defmt::Debug2Format(&due.action),
                Stamp(due.utc_min),
                Stamp(zone.to_local(due.utc_min))
            );
            from = due.utc_min + 1;
            if matches!(due.action, Action::Play { .. } | Action::Alarm { .. }) {
                alarm = Some((due.utc_min, due.action));
            }
        }

        // The light. A ramp is a start and a finish with nothing at the minutes between, so every
        // wake works out how far in it is. A wake that is due for a sound leaves the ring to the
        // alarm below, which lights it fully.
        if alarm.is_none() {
            match schedule::active_ramp(now_min, &rules[..count], &prayers, zone) {
                Some((elapsed_s, total_s)) => {
                    let px = ramp_pixel(elapsed_s, total_s, ring::CAP);
                    info!(
                        "ring: ramp {} of {} s, ({=u8}, {=u8}, {=u8})",
                        elapsed_s, total_s, px[0], px[1], px[2]
                    );
                    led.show(px);
                }
                None => led.off(),
            }
        }

        match schedule::next_event(from, &rules[..count], &prayers, zone) {
            Some(next) => {
                info!(
                    "next: {:?} at {} UTC = {} local ({} min from now)",
                    defmt::Debug2Format(&next.action),
                    Stamp(next.utc_min),
                    Stamp(zone.to_local(next.utc_min)),
                    next.utc_min - now_min
                );
            }
            None => info!("nothing scheduled; waking in a minute"),
        }
    }

    // The panel is the lamp's face, so it is drawn once the boot knows what it is looking at:
    // a face that woke before the clock was read could only show yesterday's minute.
    // A lamp with nothing stored has never been set up: raise the setup network at once. It can be
    // skipped with Back or a touch, which writes the defaults down so it does not ask again.
    if first_boot {
        info!("first boot: nothing stored, raising the setup network (Back or touch skips it)");
        setup::run(face, radio.take(), i2c, pad, flash, sectors, settings, true);
    }
    // Asleep unless a hand woke the lamp: the corner mark is what the lamp shows while it is
    // down, and it comes off for as long as somebody is standing in front of it.
    // A boot that woke for an alarm goes straight to the alarm: drawing the clock first would put
    // a two-second full refresh on the glass at 4 a.m. only to paint over it.
    if alarm.is_none() {
        face.show_clock(Standby {
            now_min,
            next_adhan: face_next.adhan,
            next_enabled_adhan: face_next.enabled_adhan,
            next_alarm: face_next.alarm,
            today,
            prayers_on: settings.prayers_on,
            sleeping: !by_hand,
            zone,
        });
    }

    // The alarm is the whole reason this boot exists, so it takes the panel until somebody answers
    // it or it gives up. Whatever happens, the clock is drawn again afterwards: the glass must
    // never be left showing a ring that has ended.
    if let Some((at, action)) = alarm {
        // What rings, and for how long: a prayer plays its adhan once and ends with it; an alarm's
        // sound repeats until it is answered or the minute is up. Both climb to their volume over
        // the same fade.
        let fade_in_s = settings.adhan_fade_in_s as u16;
        let (title, ring) = match action {
            Action::Play { track, file, volume, fade_in_s } => (
                Prayer::ALL[(track.max(1) as usize - 1).min(4)].name(),
                Some(Ring { file, volume, fade_in_s, max_ms: PLAY_MAX_MS, repeat: false }),
            ),
            Action::Alarm { file, volume } => (
                "Alarm",
                Some(Ring { file, volume, fade_in_s, max_ms: ALARM_MS, repeat: true }),
            ),
            _ => ("Alarm", None),
        };
        info!(
            "alarm: {} due at {}, flashing for up to {} s",
            title,
            Stamp(at),
            ALARM_MS / 1000
        );
        // The end of the ramp: warm white at the cap, held while the sound plays.
        led.show(ramp_pixel(1, 1, ring::CAP));
        let answered = run_alarm(&mut face, &mut i2c, &mut pad, &mut player, ring, title, at, zone);
        led.off();
        info!(
            "alarm: {}",
            if answered {
                "answered"
            } else {
                "ended, nobody answered"
            }
        );
        let later = clock::now_minutes(&mut i2c);
        draw_clock(&mut face, later, &settings, &prayers, osf, !by_hand);
    }

    // The lamp redraws every minute, so the alarm is always the next minute; what is due is found
    // by the check above on whichever boot lands on it. Armed after the alarm window rather than
    // before it, because an alarm can hold the panel past the minute: programming a minute that
    // has already gone by leaves the clock's alarm matching nothing, and the lamp would sleep for
    // good.
    let after = clock::now_minutes(&mut i2c);
    let mut wakes_at = clock::arm_next_minute(&mut i2c, after);

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
            info!(
                "awake: a hand woke the lamp, staying {} s",
                buttons::IDLE_SLEEP_MS / 1000
            );
        }
        i2c.write(buttons::PCF8574_ADDR, &[buttons::RELEASE_ALL])
            .expect("release expander pins");
        let mut reader = KeyReader::default();
        let mut menu = Menu::new(settings);
        let mut previewing = false;
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
            i2c.read(buttons::PCF8574_ADDR, &mut port)
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
                // A sound being chosen plays while it is chosen, and stops with the next press.
                if outcome.stop_sound && previewing {
                    player.stop();
                    previewing = false;
                }
                if let Some(pv) = outcome.preview {
                    info!("preview: file {} at volume {}", pv.file, pv.volume);
                    player.set_volume(pv.volume);
                    player.play_file(pv.file);
                    previewing = true;
                }
                if outcome.setup {
                    // The owner asked for the phone setup. It is a different program, and it
                    // never returns: Back or a touch resets the chip into this one again.
                    setup::run(face, radio.take(), i2c, pad, flash, sectors, menu.settings, false);
                }
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
                let now = clock::now_minutes(&mut i2c);
                info!(
                    "now {} UTC, sqw {}",
                    Stamp(now),
                    if sqw.is_high() { "high" } else { "low" }
                );
                if menu.screen == Screen::Home && now != shown_min && face.is_idle() {
                    shown_min = now;
                    draw_clock(&mut face, now, &menu.settings, &prayers, osf, false);
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
                        let now = clock::now_minutes(&mut i2c);
                        shown_min = now;
                        // Fresh from the settings that were just changed: an alarm set from the
                        // menu has to be able to become the next thing the face names.
                        draw_clock(&mut face, now, &menu.settings, &prayers, osf, false);
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
        if previewing {
            player.stop();
        }

        // The window may have crossed a minute, and the alarm was armed before it. Redraw with
        // the corner mark on, so the last thing on the glass is the lamp going back to sleep.
        let later = clock::now_minutes(&mut i2c);
        draw_clock(&mut face, later, &menu.settings, &prayers, osf, true);
        zone = menu.settings.zone();
        wakes_at = clock::arm_next_minute(&mut i2c, later);
    }

    // The panel holds the last image with no power, so this is the last thing before the chip
    // goes down.
    face.finish();

    // A wake pin already sitting at its wake level ends the sleep at the moment it starts,
    // which would spin the chip through resets. Wait for both to clear first. GPIO3 is shared by
    // the clock's SQW and the expander's INT, and INT stays latched low after a button press
    // until the port is read — so read it, or a press during this wait would hold the lamp awake.
    //
    // One low on GPIO3 is not a thing to wait out: the clock's own alarm. The refresh is finished
    // after the alarm is armed, so the next minute can arrive before the lamp gets here, and then
    // A1F holds the line low until somebody clears it — which nothing in this loop would, so the
    // lamp used to spin here for good with the old minute on the glass. That low is the wake the
    // lamp was about to sleep for, so sleep on it: the chip comes straight back up, and that boot
    // is an ordinary minute tick that clears A1F and draws the new minute.
    let mut waited = false;
    while !sqw.is_high() || pad.is_high() {
        if !pad.is_high() {
            let mut st = [0u8; 1];
            if i2c
                .write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
                .is_ok()
                && st[0] & ds3231::STATUS_A1F != 0
            {
                info!("the clock's alarm fired before sleep; sleeping on it to redraw");
                break;
            }
        }
        if !waited {
            info!("waiting for GPIO3 and GPIO4 to sit clear");
            waited = true;
        }
        let mut port = [0u8; 1];
        let _ = i2c.read(buttons::PCF8574_ADDR, &mut port);
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
        "sleeping until {} UTC = {} local",
        Stamp(wakes_at),
        Stamp(zone.to_local(wakes_at))
    );
    let mut lp = LowPower::new(peripherals.LPWR);
    lp.sleep_deep(RtcSleepConfig::deep());
}

/// A minute count printed as `YYYY-MM-DD HH:MM`, so a log line says when instead of making the
/// reader divide.
struct Stamp(i64);

impl Format for Stamp {
    fn format(&self, f: Formatter) {
        let mut buf = [0u8; FORMAT_LEN];
        defmt::write!(f, "{}", format_minutes(self.0, &mut buf));
    }
}

/// The three answers the face is drawn from.
struct FaceNext {
    /// The next prayer, whether or not it rings: the strip highlights it.
    adhan: Option<(i64, Prayer)>,
    /// The next prayer that is switched on.
    enabled_adhan: Option<(i64, Prayer)>,
    /// The next thing that will ring, an enabled prayer or an alarm: the "Next:" line names it.
    alarm: Option<NextUp>,
}

impl FaceNext {
    const NONE: Self = Self { adhan: None, enabled_adhan: None, alarm: None };

    fn at(now_min: i64, settings: &Settings, prayers: &impl PrayerTimes) -> Self {
        let adhan = schedule::next_prayer(now_min, prayers);
        let enabled_adhan = schedule::next_prayer_in(now_min, prayers, settings.prayers_on);
        let alarm = match (enabled_adhan, settings.next_alarm(now_min)) {
            (Some((prayer_at, _)), Some((alarm_at, _))) if alarm_at < prayer_at => {
                Some(NextUp::Alarm(alarm_at))
            }
            (Some((prayer_at, prayer)), _) => Some(NextUp::Prayer(prayer_at, prayer)),
            (None, Some((alarm_at, _))) => Some(NextUp::Alarm(alarm_at)),
            (None, None) => None,
        };
        Self { adhan, enabled_adhan, alarm }
    }
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
        info!(
            "settings: erase at {=u32:#06x} failed: {:?}",
            at,
            defmt::Debug2Format(&e)
        );
        return false;
    }
    if let Err(e) = flash.write_nor(at, &buf.0) {
        info!(
            "settings: write at {=u32:#06x} failed: {:?}",
            at,
            defmt::Debug2Format(&e)
        );
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
    player: &mut Player,
    ring: Option<Ring>,
    title: &str,
    at: i64,
    zone: Zone,
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
    // The sound starts as the panel does, at the quietest level, and climbs from there.
    let max_ms = ring.as_ref().map_or(ALARM_MS, |r| r.max_ms);
    let mut level = 0u8;
    if let Some(r) = &ring {
        level = fade_volume(r.volume, r.fade_in_s as u32 * 1000, 0);
        info!("ring: file {} from volume {} up to {}", r.file, level, r.volume);
        player.set_volume(level);
        player.play_file(r.file);
    }
    loop {
        face.show_alarm(title, at, zone, inverted);
        inverted = !inverted;
        waited += ALARM_BLINK_MS;
        if let Some(r) = &ring {
            let now = fade_volume(r.volume, r.fade_in_s as u32 * 1000, waited);
            if now != level {
                level = now;
                player.set_volume(level);
            }
        }
        if waited >= max_ms {
            player.stop();
            return false;
        }
        let mut polled = 0;
        while polled < ALARM_BLINK_MS {
            bus.read(buttons::PCF8574_ADDR, &mut port)
                .expect("read expander");
            if reader.update(port[0]).is_some() || pad.is_high() {
                if ring.is_some() {
                    player.stop();
                }
                return true;
            }
            if let Some(r) = &ring
                && player.finished()
            {
                if r.repeat {
                    player.play_file(r.file);
                } else {
                    // An adhan that has played to its end is over; nothing is left to stop.
                    return false;
                }
            }
            delay.delay_millis(ALARM_POLL_MS);
            polled += ALARM_POLL_MS;
        }
    }
}

/// What the player is to play while an alarm holds the panel.
struct Ring {
    /// The number in the card's file name.
    file: u16,
    /// The level it climbs to.
    volume: u8,
    fade_in_s: u16,
    /// The longest it may go on.
    max_ms: u32,
    /// Play it again when it ends, until somebody answers.
    repeat: bool,
}

fn draw_clock(
    face: &mut Face,
    now: i64,
    settings: &Settings,
    prayers: &impl PrayerTimes,
    osf: bool,
    sleeping: bool,
) {
    let mut face_next = FaceNext::NONE;
    let zone = settings.zone();
    let today = schedule::local_times(now, prayers, zone);
    if !osf {
        face_next = FaceNext::at(now, settings, prayers);
    }
    face.show_clock(Standby {
        now_min: now,
        next_adhan: face_next.adhan,
        next_enabled_adhan: face_next.enabled_adhan,
        next_alarm: face_next.alarm,
        today,
        prayers_on: settings.prayers_on,
        sleeping,
        zone,
    });
}
