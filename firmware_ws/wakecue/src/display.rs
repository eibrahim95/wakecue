//! The e-paper panel: the tiny bit of SPI glue the driver needs, and the face it shows.
//!
//! The panel is the only device on the bus, so its chip-select is wired to ground and
//! there is no pin to drive for it. That is why this speaks the driver's data/command
//! trait directly instead of going through a SPI-device wrapper.

use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_10X20};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle};
use embedded_graphics::text::Text;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;
use lamp_core::calendar::{DATE_LEN, HHMM_LEN, format_date, format_hhmm, utc_to_egypt};
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

/// Draw the standby face — Cairo time, the date, and what the lamp will do next — then put the
/// panel back to sleep. A sleeping panel holds its image with no power, which is why the lamp
/// can leave it alone; the cost is the two-second full refresh each boot.
///
/// `next` is the event the boot armed and a short word for it, or `None` when the clock's time
/// cannot be trusted, in which case the face says so instead of pointing at a time.
#[allow(
    clippy::large_stack_frames,
    reason = "the frame buffer is the point of this function"
)]
pub fn clock_face(
    bus: PanelBus<'static>,
    busy: Input<'static>,
    rst: Output<'static>,
    now_min: i64,
    next: Option<(i64, &str)>,
) {
    let mut driver = WeActStudio213BlackWhiteDriver::new(bus, busy, rst, Delay::new());

    if let Err(e) = driver.init() {
        defmt::info!("panel did not answer: {:?}", defmt::Debug2Format(&e));
        return;
    }

    // The clock keeps UTC; the face shows the time on the wall.
    let local = utc_to_egypt(now_min);
    let mut time_buf = [0u8; HHMM_LEN];
    let mut date_buf = [0u8; DATE_LEN];
    let time = format_hhmm(local, &mut time_buf);
    let date = format_date(local, &mut date_buf);

    let mut frame = Display213BlackWhite::new();
    frame.set_rotation(ROTATION);
    frame.clear(Color::White);
    let big = MonoTextStyle::new(&FONT_10X20, Color::Black);
    let small = MonoTextStyle::new(&FONT_6X10, Color::Black);
    let rule = PrimitiveStyle::with_stroke(Color::Black, 1);

    let _ = Text::new(time, Point::new(14, 46), big).draw(&mut frame);
    let _ = Line::new(Point::new(14, 60), Point::new(236, 60))
        .into_styled(rule)
        .draw(&mut frame);
    let _ = Text::new(date, Point::new(14, 84), small).draw(&mut frame);

    match next {
        Some((at, label)) => {
            let mut at_buf = [0u8; HHMM_LEN];
            let at = format_hhmm(utc_to_egypt(at), &mut at_buf);
            let _ = Text::new(label, Point::new(14, 108), small).draw(&mut frame);
            // FONT_6X10 advances six pixels per character, so the time follows the label.
            let after = 14 + label.len() as i32 * 6 + 6;
            let _ = Text::new(at, Point::new(after, 108), small).draw(&mut frame);
            defmt::info!("panel: face drawn, {} next {}", time, at);
        }
        None => {
            let _ = Text::new("time not set", Point::new(14, 108), small).draw(&mut frame);
            defmt::info!("panel: face drawn, {} with no trusted time", time);
        }
    }

    match driver.full_update(&frame) {
        Ok(()) => {}
        Err(e) => defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e)),
    }

    if driver.sleep().is_err() {
        defmt::info!("panel: could not be put to sleep");
    }
}
