//! The e-paper panel: the tiny bit of SPI glue the driver needs, and the face it shows.
//!
//! The panel is the only device on the bus, so its chip-select is wired to ground and
//! there is no pin to drive for it. That is why this speaks the driver's data/command
//! trait directly instead of going through a SPI-device wrapper.

use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_10X20};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;
use lamp_core::calendar::{
    DATE_LEN, HHMM12_LEN, format_date, format_hhmm12, meridiem, utc_to_egypt,
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
        }
    }

    /// Draw the standby face — Cairo time, the date, and the next prayer — and push it to the
    /// glass. `next` is the prayer and its time in UTC minutes, or `None` when the clock's time
    /// cannot be trusted, in which case the face says so instead of naming a prayer. `sleeping`
    /// puts the corner mark on, saying the lamp is about to go back down.
    pub fn show_clock(&mut self, now_min: i64, next: Option<(i64, Prayer)>, sleeping: bool) {
        if !self.ready {
            return;
        }

        // The clock keeps UTC; the face shows the time on the wall.
        let local = utc_to_egypt(now_min);
        let mut time_buf = [0u8; HHMM12_LEN];
        let mut date_buf = [0u8; DATE_LEN];
        let time = format_hhmm12(local, &mut time_buf);
        let date = format_date(local, &mut date_buf);

        let frame = &mut self.frame;
        frame.clear(Color::White);
        let big = MonoTextStyle::new(&FONT_10X20, Color::Black);
        let small = MonoTextStyle::new(&FONT_6X10, Color::Black);
        let rule = PrimitiveStyle::with_stroke(Color::Black, 1);

        let _ = Text::new(time, Point::new(14, 46), big).draw(frame);
        // AM/PM rides beside the big time in the small font, both bottom-aligned on y = 46; FONT_10X20
        // advances ten pixels a character, so the two never touch.
        let ampm_x = 14 + time.len() as i32 * 10 + 6;
        let _ = Text::new(meridiem(local), Point::new(ampm_x, 46), small).draw(frame);
        // Two borders, to measure what the glass will actually show. The drawing area is 250 by
        // 128, but the panel is 122 rows visible behind a 128-row buffer: a border at the very
        // edge and another six pixels in make it obvious which edge is the one that disappears.
        let _ = Rectangle::new(Point::new(0, 0), Size::new(250, 128))
            .into_styled(rule)
            .draw(frame);
        let _ = Rectangle::new(Point::new(6, 6), Size::new(238, 116))
            .into_styled(rule)
            .draw(frame);
        let _ = Line::new(Point::new(14, 60), Point::new(236, 60))
            .into_styled(rule)
            .draw(frame);
        let _ = Text::new(date, Point::new(14, 84), small).draw(frame);

        match next {
            Some((at, prayer)) => {
                let local_at = utc_to_egypt(at);
                let mut at_buf = [0u8; HHMM12_LEN];
                let at = format_hhmm12(local_at, &mut at_buf);
                let name = prayer.name();
                // A prayer shown at 10:43 reading "Fajr 4:44" has to say it is tomorrow's.
                let mut x = 14;
                if local_at.div_euclid(1440) != local.div_euclid(1440) {
                    let _ = Text::new("tomorrow", Point::new(x, 108), small).draw(frame);
                    x += 8 * 6 + 6;
                }
                // FONT_6X10 advances six pixels per character, so each word follows the last.
                let _ = Text::new(name, Point::new(x, 108), small).draw(frame);
                x += name.len() as i32 * 6 + 6;
                let _ = Text::new(at, Point::new(x, 108), small).draw(frame);
                x += at.len() as i32 * 6 + 6;
                let _ = Text::new(meridiem(local_at), Point::new(x, 108), small).draw(frame);
                defmt::info!("panel: face drawn, {} next {} {}", time, name, at);
            }
            None => {
                let _ = Text::new("time not set", Point::new(14, 108), small).draw(frame);
                defmt::info!("panel: face drawn, {} with no trusted time", time);
            }
        }

        // The corner mark says the lamp is asleep. It is part of the frame, not a thing drawn
        // and erased: every minute tick redraws the whole face anyway, so the tick keeps it, and
        // only a wake by hand draws a face without it.
        if sleeping {
            let _ = Text::new("z z z", Point::new(196, 30), small).draw(frame);
        }

        let refreshed = if self.fast {
            self.driver.fast_update(&self.frame)
        } else {
            self.driver.full_update(&self.frame)
        };
        if let Err(e) = refreshed {
            defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e));
        }
        // The glass now holds this image, so the next draw can take the diff against it.
        self.fast = true;
    }

    /// Draw the screen the menu is on. The clock face is [`Self::show_clock`]; everything else
    /// comes from the menu's own state, which is why this only has to know how to lay out rows.
    pub fn show_menu(&mut self, menu: &Menu) {
        if !self.ready {
            return;
        }
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
                for (i, label) in ["Prayers", "My alarms", "Exit"].into_iter().enumerate() {
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
                        let _ = Text::new(">", Point::new(12, y), small).draw(frame);
                    }
                    let alarm = menu.settings.alarms[slot as usize];
                    if alarm.used {
                        let mut t_buf = [0u8; HHMM12_LEN];
                        let t = format_hhmm12(alarm.minute as i64, &mut t_buf);
                        let _ = Text::new(t, Point::new(28, y), small).draw(frame);
                        let _ = Text::new(
                            meridiem(alarm.minute as i64),
                            Point::new(28 + t.len() as i32 * 6 + 6, y),
                            small,
                        )
                        .draw(frame);
                        let _ = Text::new(day_name(alarm.days), Point::new(120, y), small).draw(frame);
                        if !alarm.enabled {
                            let _ = Text::new("off", Point::new(210, y), small).draw(frame);
                        }
                    } else {
                        let _ = Text::new("empty", Point::new(28, y), small).draw(frame);
                    }
                }
            }
            Screen::Edit { field, draft, .. } => {
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
                    let y = 28 + i as i32 * 20;
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
    fn push(&mut self) {
        let refreshed = if self.fast {
            self.driver.fast_update(&self.frame)
        } else {
            self.driver.full_update(&self.frame)
        };
        if let Err(e) = refreshed {
            defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e));
        }
        // The glass now holds this image, so the next draw can take the diff against it.
        self.fast = true;
    }

    /// Send the panel into its deep sleep, which keeps the last image with no power at all.
    pub fn finish(mut self) {
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
