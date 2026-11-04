//! The e-paper panel: the tiny bit of SPI glue the driver needs, and the first frame.
//!
//! The panel is the only device on the bus, so its chip-select is wired to ground and
//! there is no pin to drive for it. That is why this speaks the driver's data/command
//! trait directly instead of going through a SPI-device wrapper.

use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;
use weact_studio_epd::graphics::Display213BlackWhite;
use weact_studio_epd::{Color, WeActStudio213BlackWhiteDriver};

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

/// Draw one frame we wrote ourselves, then put the panel back to sleep. A sleeping
/// panel holds its image with no power, which is why the lamp can leave it alone.
#[allow(
    clippy::large_stack_frames,
    reason = "the frame buffer is the point of this function"
)]
pub fn first_frame(bus: PanelBus<'static>, busy: Input<'static>, rst: Output<'static>) {
    let mut driver = WeActStudio213BlackWhiteDriver::new(bus, busy, rst, Delay::new());

    if let Err(e) = driver.init() {
        defmt::info!("panel did not answer: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut frame = Display213BlackWhite::new();
    frame.clear(Color::White);
    let style = MonoTextStyle::new(&FONT_10X20, Color::Black);
    let _ = Text::new("wakecue", Point::new(6, 26), style).draw(&mut frame);
    let _ = Text::new("display works", Point::new(6, 54), style).draw(&mut frame);
    let _ = Text::new("250 x 122", Point::new(6, 82), style).draw(&mut frame);

    match driver.full_update(&frame) {
        Ok(()) => defmt::info!("panel: first frame drawn"),
        Err(e) => defmt::info!("panel: refresh failed: {:?}", defmt::Debug2Format(&e)),
    }

    if driver.sleep().is_err() {
        defmt::info!("panel: could not be put to sleep");
    }
}
