//! The e-paper panel: the tiny bit of SPI glue the driver needs, and the face it shows.
//!
//! The panel is the only device on the bus, so its chip-select is wired to ground and
//! there is no pin to drive for it. That is why this speaks the driver's data/command
//! trait directly instead of going through a SPI-device wrapper.

use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_10X20};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;
use lamp_core::calendar::{
    COUNTDOWN_LEN, DATE_LEN, HHMM12_LEN, format_countdown, format_date, format_hhmm12, meridiem,
    utc_to_egypt,
};
use lamp_core::menu::{Field, Menu, Screen, day_name};
use lamp_core::schedule::Prayer;
use lamp_core::settings::MAX_USER_ALARMS;
use weact_studio_epd::graphics::{Display213BlackWhite, DisplayRotation};
use weact_studio_epd::{Color, WeActStudio213BlackWhiteDriver};

/// Which way up the panel sits in the lamp. The driver's own buffer is 128 wide by 250 tall —
/// the glass turned on its short edge, six of those columns invisible behind the border — so
/// without a rotation the face comes out portrait. Ninety degrees makes the drawing area
/// 250 wide by 128 tall, matching the window in the base; if the face lands upside down, this
/// is the line to change to `Rotate270`.
const ROTATION: DisplayRotation = DisplayRotation::Rotate90;

/// What the standby face says. The boot has the prayer table and the settings, so it decides this;
/// the face only lays it out.
#[derive(Clone, Copy)]
pub struct Standby {
    /// The clock's minute count, UTC.
    pub now_min: i64,
    /// The next prayer and its time in UTC minutes, or `None` when the clock cannot be trusted.
    pub next: Option<(i64, Prayer)>,
    /// Today's five prayers as local minutes after midnight, `None` where the table ends.
    pub today: [Option<i64>; 5],
    /// Which prayers ring, a bit per prayer in [`Prayer::ALL`] order.
    pub prayers_on: u8,
    /// The lamp is about to go back down: draw the corner mark.
    pub sleeping: bool,
}

/// Width of the text [`hhmm12_short`] writes, `12:45p`.
const SHORT_LEN: usize = HHMM12_LEN + 1;

/// [`format_hhmm12`] with the meridiem cut to one letter, which is all a fifty-pixel column has
/// room for. The next-prayer line above spells it out in full.
fn hhmm12_short(min: i64, buf: &mut [u8; SHORT_LEN]) -> &str {
    let mut digits = [0u8; HHMM12_LEN];
    let s = format_hhmm12(min, &mut digits);
    let n = s.len();
    buf[..n].copy_from_slice(s.as_bytes());
    buf[n] = if meridiem(min) == "AM" { b'a' } else { b'p' };
    core::str::from_utf8(&buf[..n + 1]).unwrap_or("")
}

/// Write-only SPI with the data/command line: low sends a command, high sends data.
pub struct PanelBus<'d> {
    spi: Spi<'d, esp_hal::Blocking>,
    dc: Output<'d>,
}

impl<'d> PanelBus<'d> {
    pub fn new(spi: Spi<'d, esp_hal::Blocking>, dc: Output<'d>) -> Self {
        Self { spi, dc }
    }

    fn write(&mut self, data: DataFormat<'_>) -> Result<(), DisplayError> {
        match data {
            DataFormat::U8(bytes) => self.spi.write(bytes),
            _ => return Err(DisplayError::InvalidFormatError),
        }
        .map_err(|_| DisplayError::BusWriteError)
    }
}

impl WriteOnlyDataCommand for PanelBus<'_> {
    fn send_commands(&mut self, cmds: DataFormat<'_>) -> Result<(), DisplayError> {
        self.dc.set_low();
        self.write(cmds)
    }

    fn send_data(&mut self, buf: DataFormat<'_>) -> Result<(), DisplayError> {
        self.dc.set_high();
        self.write(buf)
    }
}

/// The panel, held open for as long as the lamp is awake so the face can be redrawn without
/// reinitialising the driver. The first draw of a cold boot is a two-second full refresh; every
/// later draw is a fast refresh, which flips only the pixels that changed and does not strobe.
///
/// A sleeping panel keeps its last image with no power, so the lamp wakes on each new minute and
/// draws again; `show` is what makes the face right, `finish` is what lets the panel sleep.
pub struct Face {
    driver:
        WeActStudio213BlackWhiteDriver<PanelBus<'static>, Input<'static>, Output<'static>, Delay>,
    frame: Display213BlackWhite,
    /// Whether the panel's RAM still matches the glass, so a fast refresh is safe.
    fast: bool,
    /// False when the panel never answered; drawing is then a no-op.
    ready: bool,
    /// The glass is still working on the frame that was last pushed.
    pending: bool,
}

impl Face {
    /// Take the panel over. `woken` says this boot came out of sleep rather than from power-on: a
    /// wake needs only a hardware reset, because the panel kept both its picture and the RAM a
    /// fast refresh compares against. A cold boot has nothing to keep and must initialise, and a
    /// software reset is exactly what throws that RAM away — hence the two paths.
    pub fn new(
        bus: PanelBus<'static>,
        busy: Input<'static>,
        rst: Output<'static>,
        woken: bool,
    ) -> Self {
        let mut driver = WeActStudio213BlackWhiteDriver::new(bus, busy, rst, Delay::new());
        let fast = woken && driver.wake_up().is_ok();
        let ready = if fast {
            driver.assume_image_on_panel();
            true
        } else {
            match driver.init() {
                Ok(()) => true,
                Err(e) => {
                    defmt::info!("panel did not answer: {:?}", defmt::Debug2Format(&e));
                    false
                }
            }
        };
        Self {
            driver,
            frame: {
                let mut frame = Display213BlackWhite::new();
                frame.set_rotation(ROTATION);
                frame
            },
            fast: fast && ready,
            ready,
            pending: false,
        }
    }

    /// Draw the standby face and push it to the glass: the time on the wall, the date, what rings
    /// next and how long that is, and today's five prayers with a filled dot on the ones that
    /// ring.
    ///
    /// Every mark stays inside x 10..240 and y 6..115 of the 250 by 128 drawing area. The glass is
    /// 250 by 122 behind a 128-row buffer and which six rows or columns are hidden has not been
    /// measured, so the layout keeps a six-pixel margin all round: whichever edge it turns out to
    /// be, it cannot eat a glyph. The menu still draws its border at the very edge, and that is now
    /// the thing that answers the question.
    pub fn show_clock(&mut self, s: Standby) {
        if !self.ready {
            return;
        }
        self.settle();

        // The clock keeps UTC; the face shows the time on the wall.
        let local = utc_to_egypt(s.now_min);
        let mut time_buf = [0u8; HHMM12_LEN];
        let mut date_buf = [0u8; DATE_LEN];
        let time = format_hhmm12(local, &mut time_buf);
        let date = format_date(local, &mut date_buf);

        let frame = &mut self.frame;
        frame.clear(Color::White);
        let big = MonoTextStyle::new(&FONT_10X20, Color::Black);
        let small = MonoTextStyle::new(&FONT_6X10, Color::Black);
        let rule = PrimitiveStyle::with_stroke(Color::Black, 1);

        let _ = Text::new(time, Point::new(14, 44), big).draw(frame);
        // AM/PM rides beside the big time in the small font, both bottom-aligned on y = 44;
        // FONT_10X20 advances ten pixels a character, so the two never touch.
        let ampm_x = 14 + time.len() as i32 * 10 + 6;
        let _ = Text::new(meridiem(local), Point::new(ampm_x, 44), small).draw(frame);
        let _ = Text::new(date, Point::new(236 - date.len() as i32 * 6, 15), small).draw(frame);

        // The corner mark says the lamp is asleep. It is part of the frame, not a thing drawn and
        // erased: every minute tick redraws the whole face anyway, so the tick keeps it, and only a
        // wake by hand draws a face without it.
        if s.sleeping {
            let _ = Text::new("z z z", Point::new(196, 28), small).draw(frame);
        }

        match s.next {
            Some((at, prayer)) => {
                let local_at = utc_to_egypt(at);
                let mut at_buf = [0u8; HHMM12_LEN];
                let at_text = format_hhmm12(local_at, &mut at_buf);
                // A prayer shown at 10:43 reading "Fajr 4:44" has to say it is tomorrow's.
                let mut x = 14;
                let _ = Text::new("Next:", Point::new(x, 72), small).draw(frame);
                x += 5 * 6 + 6;
                if local_at.div_euclid(1440) != local.div_euclid(1440) {
                    let _ = Text::new("tomorrow", Point::new(x, 72), small).draw(frame);
                    x += 8 * 6 + 6;
                }
                // FONT_6X10 advances six pixels per character, so each word follows the last.
                let name = prayer.name();
                let _ = Text::new(name, Point::new(x, 72), small).draw(frame);
                x += name.len() as i32 * 6 + 6;
                let _ = Text::new(at_text, Point::new(x, 72), small).draw(frame);
                x += at_text.len() as i32 * 6 + 6;
                let _ = Text::new(meridiem(local_at), Point::new(x, 72), small).draw(frame);

                // How long is left, held against the right edge: a name plus `tomorrow` can use
                // most of the line, so the countdown gets its own end rather than following on.
                let mut cd_buf = [0u8; COUNTDOWN_LEN];
                let cd = format_countdown(at - s.now_min, &mut cd_buf);
                let _ = Text::new(cd, Point::new(236 - cd.len() as i32 * 6, 72), small).draw(frame);
                defmt::info!("panel: face drawn, {} next {} {} {}", time, name, at_text, cd);
            }
            None => {
                let _ = Text::new("time not set", Point::new(14, 72), small).draw(frame);
                defmt::info!("panel: face drawn, {} with no trusted time", time);
            }
        }

        // Today's five: the name, a dot that is filled when that prayer rings, and its time under
        // the name. Fifty pixels a column is what limits the time to a one-letter meridiem.
        if s.today.iter().any(|t| t.is_some()) {
            let _ = Line::new(Point::new(10, 88), Point::new(240, 88))
                .into_styled(rule)
                .draw(frame);
            for (i, p) in Prayer::ALL.iter().enumerate() {
                let cx = 24 + i as i32 * 48;
                let name = p.name();
                let w = name.len() as i32 * 6;
                let _ = Text::new(name, Point::new(cx - w / 2, 98), small).draw(frame);

                let dot = Circle::new(Point::new(cx + w / 2 + 3, 92), 5);
                let ring = s.prayers_on & (1 << p.index()) != 0;
                let style = if ring {
                    PrimitiveStyle::with_fill(Color::Black)
                } else {
                    PrimitiveStyle::with_stroke(Color::Black, 1)
                };
                let _ = dot.into_styled(style).draw(frame);

                let mut t_buf = [0u8; SHORT_LEN];
                let t = match s.today[i] {
                    Some(min) => hhmm12_short(min, &mut t_buf),
                    None => "--:--",
                };
                let _ = Text::new(t, Point::new(cx - t.len() as i32 * 3, 111), small).draw(frame);
            }
        }

        self.push();
    }

    /// Draw the alarm: what is ringing and when it was due, on either a white or a black ground so
    /// the two alternate and the panel flashes. `inverted` picks which.
    ///
    /// This is a stand-in for the ring and the card, not a design in its own right: the caller
    /// flashes it until the alarm is answered, and the sound and the light will be driven from the
    /// same place.
    pub fn show_alarm(&mut self, title: &str, at: i64, inverted: bool) {
        if !self.ready {
            return;
        }
        self.settle();
        let local = utc_to_egypt(at);
        let mut time_buf = [0u8; HHMM12_LEN];
        let time = format_hhmm12(local, &mut time_buf);
        let ampm = meridiem(local);
        let (ground, ink) = if inverted {
            (Color::Black, Color::White)
        } else {
            (Color::White, Color::Black)
        };

        let frame = &mut self.frame;
        frame.clear(ground);
        let big = MonoTextStyle::new(&FONT_10X20, ink);
        let small = MonoTextStyle::new(&FONT_6X10, ink);

        // FONT_10X20 advances ten pixels a character, so the word centres as len times ten.
        let _ = Text::new(title, Point::new((250 - title.len() as i32 * 10) / 2, 52), big)
            .draw(frame);
        // The time and its meridiem centre as one run, or the two would drift apart.
        let run = time.len() as i32 * 10 + 6 + ampm.len() as i32 * 6;
        let x = (250 - run) / 2;
        let _ = Text::new(time, Point::new(x, 88), big).draw(frame);
        let _ = Text::new(ampm, Point::new(x + time.len() as i32 * 10 + 6, 88), small).draw(frame);

        self.push();
    }

    /// Draw the screen the menu is on. The clock face is [`Self::show_clock`]; everything else
    /// comes from the menu's own state, which is why this only has to know how to lay out rows.
    pub fn show_menu(&mut self, menu: &Menu) {
        if !self.ready {
            return;
        }
        self.settle();
        let frame = &mut self.frame;
        frame.clear(Color::White);
        let small = MonoTextStyle::new(&FONT_6X10, Color::Black);
        let _ = Rectangle::new(Point::new(0, 0), Size::new(250, 128))
            .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
            .draw(frame);

        match menu.screen {
            // Nothing to draw: the caller shows the clock face for Home.
            Screen::Home => {}
            Screen::Top { cursor } => {
                for (i, label) in ["Prayers", "My alarms"].into_iter().enumerate() {
                    let y = 44 + i as i32 * 24;
                    if i as u8 == cursor {
                        let _ = Text::new(">", Point::new(12, y), small).draw(frame);
                    }
                    let _ = Text::new(label, Point::new(28, y), small).draw(frame);
                }
            }
            Screen::Prayers { cursor } => {
                for (i, p) in Prayer::ALL.into_iter().enumerate() {
                    let y = 28 + i as i32 * 20;
                    if i as u8 == cursor {
                        let _ = Text::new(">", Point::new(12, y), small).draw(frame);
                    }
                    let _ = Text::new(p.name(), Point::new(28, y), small).draw(frame);
                    let on = if menu.settings.prayer_on(p) { "on" } else { "off" };
                    let _ = Text::new(on, Point::new(150, y), small).draw(frame);
                }
            }
            Screen::Alarms { cursor } => {
                // Five rows fit, so the list scrolls under the cursor.
                let last = MAX_USER_ALARMS as u8 - 5;
                let first = cursor.saturating_sub(2).min(last);
                for i in 0..5u8 {
                    let slot = first + i;
                    let y = 28 + i as i32 * 20;
                    if slot == cursor {
                        let _ = Text::new(">", Point::new(4, y), small).draw(frame);
                    }
                    // Every empty row looks the same, so the slot number is what says the cursor
                    // moved at all — and what tells one empty slot from the next.
                    let mut number = [0u8; 1];
                    number[0] = b'1' + slot;
                    let _ = Text::new(
                        core::str::from_utf8(&number).unwrap_or(""),
                        Point::new(16, y),
                        small,
                    )
                    .draw(frame);
                    let alarm = menu.settings.alarms[slot as usize];
                    if alarm.used {
                        let mut t_buf = [0u8; HHMM12_LEN];
                        let t = format_hhmm12(alarm.minute as i64, &mut t_buf);
                        let _ = Text::new(t, Point::new(34, y), small).draw(frame);
                        let _ = Text::new(
                            meridiem(alarm.minute as i64),
                            Point::new(34 + t.len() as i32 * 6 + 6, y),
                            small,
                        )
                        .draw(frame);
                        let _ = Text::new(day_name(alarm.days), Point::new(120, y), small).draw(frame);
                        if !alarm.enabled {
                            let _ = Text::new("off", Point::new(214, y), small).draw(frame);
                        }
                    } else {
                        let _ = Text::new("empty", Point::new(34, y), small).draw(frame);
                    }
                }
            }
            Screen::Edit { slot, field, draft } => {
                // The list is gone once a row is open, so the screen has to say which alarm it is
                // editing.
                let _ = Text::new("Alarm", Point::new(14, 18), small).draw(frame);
                let mut number = [0u8; 1];
                number[0] = b'1' + slot;
                let _ = Text::new(
                    core::str::from_utf8(&number).unwrap_or(""),
                    Point::new(14 + 5 * 6 + 6, 18),
                    small,
                )
                .draw(frame);
                let mut hour = [0u8; 2];
                let mut minute = [0u8; 2];
                let h12 = match (draft.minute / 60) % 12 {
                    0 => 12,
                    h => h,
                };
                let rows = [
                    ("Hour", two(h12 as u32, &mut hour)),
                    ("Minute", two((draft.minute % 60) as u32, &mut minute)),
                    ("Days", day_name(draft.days)),
                    ("Save", ""),
                    ("Delete", ""),
                ];
                for (i, (label, value)) in rows.into_iter().enumerate() {
                    let y = 36 + i as i32 * 18;
                    if i as u8 == field {
                        let _ = Text::new(">", Point::new(12, y), small).draw(frame);
                    }
                    let _ = Text::new(label, Point::new(28, y), small).draw(frame);
                    let _ = Text::new(value, Point::new(96, y), small).draw(frame);
                    // The hour row is the first field, and it needs saying whether it is
                    // morning or afternoon.
                    if i == Field::Hour as usize {
                        let _ = Text::new(
                            meridiem(draft.minute as i64),
                            Point::new(96 + value.len() as i32 * 6 + 6, y),
                            small,
                        )
                        .draw(frame);
                    }
                }
            }
        }

        self.push();
    }

    /// Send whatever is in the frame buffer to the glass: the full refresh only for the first draw
    /// of a cold boot, the quiet one after that.
    ///
    /// A fast refresh is only *started* here. The panel takes the better part of a second over it,
    /// and the caller has buttons to read in the meantime; [`Self::settle`] finishes it before the
    /// buffer is next changed.
    fn push(&mut self) {
        self.settle();
        if !self.ready {
            return;
        }
        if self.fast {
            match self.driver.fast_update_start(self.frame.buffer()) {
                Ok(()) => self.pending = true,
                Err(e) => {
                    defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e));
                    self.ready = false;
                }
            }
        } else {
            // The first draw of a cold boot is a full refresh, and there is nothing else to do
            // while it strobes.
            match self.driver.full_update(&self.frame) {
                Ok(()) => self.fast = true,
                Err(e) => {
                    defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e));
                    self.ready = false;
                }
            }
        }
    }

    /// False while the glass is still catching up with the last frame. A caller that has keys to
    /// read asks this and comes back later, instead of waiting inside a refresh and missing them.
    pub fn is_idle(&mut self) -> bool {
        !self.ready || !self.driver.is_busy()
    }

    /// Finish a refresh that is still running, with the image it drew. It has to happen before the
    /// frame buffer is changed: the driver leaves that image in the panel's own RAMs, and the next
    /// update diffs against it.
    fn settle(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        if let Err(e) = self.driver.fast_update_finish(self.frame.buffer()) {
            defmt::info!("panel: could not finish a refresh: {:?}", defmt::Debug2Format(&e));
            self.ready = false;
        }
    }

    /// Send the panel into its deep sleep, which keeps the last image with no power at all.
    pub fn finish(mut self) {
        self.settle();
        if self.ready && self.driver.sleep().is_err() {
            defmt::info!("panel: could not be put to sleep");
        }
    }
}

/// A value with a leading zero, for a menu row; two bytes is the widest the rows need.
fn two(v: u32, buf: &mut [u8; 2]) -> &str {
    buf[0] = b'0' + (v / 10 % 10) as u8;
    buf[1] = b'0' + (v % 10) as u8;
    core::str::from_utf8(buf).unwrap_or("")
}
