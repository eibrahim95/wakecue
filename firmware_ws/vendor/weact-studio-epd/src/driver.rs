use core::iter;

#[cfg(not(feature = "blocking"))]
use display_interface::AsyncWriteOnlyDataCommand;
#[cfg(feature = "blocking")]
use display_interface::WriteOnlyDataCommand;

#[cfg(feature = "blocking")]
use embedded_hal::delay::DelayNs;
#[cfg(not(feature = "blocking"))]
use embedded_hal_async::{delay::DelayNs, digital::Wait};

use display_interface::DataFormat;
use embedded_hal::digital::{InputPin, OutputPin};

#[cfg(feature = "graphics")]
use crate::graphics::Display;
use crate::{
    color::{self, ColorType},
    command, flag, lut, Color, Result, TriColor,
};

/// Display driver for the WeAct Studio 2.9 inch B/W display.
pub type WeActStudio290BlackWhiteDriver<DI, BSY, RST, DELAY> =
    DisplayDriver<DI, BSY, RST, DELAY, 128, 128, 296, Color>;
/// Display driver for the WeAct Studio 2.9 inch Tri-Color display.
pub type WeActStudio290TriColorDriver<DI, BSY, RST, DELAY> =
    DisplayDriver<DI, BSY, RST, DELAY, 128, 128, 296, TriColor>;
/// Display driver for the WeAct Studio 2.13 inch B/W display.
pub type WeActStudio213BlackWhiteDriver<DI, BSY, RST, DELAY> =
    DisplayDriver<DI, BSY, RST, DELAY, 128, 122, 250, Color>;
/// Display driver for the WeAct Studio 2.13 inch Tri-Color display.
pub type WeActStudio213TriColorDriver<DI, BSY, RST, DELAY> =
    DisplayDriver<DI, BSY, RST, DELAY, 128, 122, 250, TriColor>;

/// The main driver struct that manages the communication with the display.
///
/// You probably want to use one of the display-specific type aliases instead.
pub struct DisplayDriver<
    DI,
    BSY,
    RST,
    DELAY,
    const WIDTH: u32,
    const VISIBLE_WIDTH: u32,
    const HEIGHT: u32,
    C,
> {
    _color: core::marker::PhantomData<C>,
    interface: DI,
    busy: BSY,
    reset: RST,
    delay: DELAY,
    // State
    using_partial_mode: bool,
    initial_full_refresh_done: bool,
}

impl<DI, BSY, RST, DELAY, const WIDTH: u32, const VISIBLE_WIDTH: u32, const HEIGHT: u32, C>
    DisplayDriver<DI, BSY, RST, DELAY, WIDTH, VISIBLE_WIDTH, HEIGHT, C>
{
    /// Tell the driver that the panel already holds a valid image, so the next
    /// [`Self::fast_refresh`] skips the full refresh it would otherwise force first.
    ///
    /// This is the deep-sleep path. While the chip was off the panel kept its RAM and its
    /// picture, and that first full refresh is a visible strobe. Use it after
    /// [`Self::wake_up`], which only resets the hardware: [`Self::init`] begins with a
    /// software reset, and that drops the RAM a fast refresh compares against.
    ///
    /// Added for wakecue, which wakes every minute to redraw a clock; see
    /// `firmware_ws/vendor/README.md`.
    pub fn assume_image_on_panel(&mut self) {
        self.initial_full_refresh_done = true;
    }
}

#[maybe_async_cfg::maybe(
    sync(
        feature = "blocking",
        keep_self,
        idents(
            AsyncWriteOnlyDataCommand(sync = "WriteOnlyDataCommand"),
            Wait(sync = "InputPin")
        )
    ),
    async(not(feature = "blocking"), keep_self)
)]
impl<DI, BSY, RST, DELAY, const WIDTH: u32, const VISIBLE_WIDTH: u32, const HEIGHT: u32, C>
    DisplayDriver<DI, BSY, RST, DELAY, WIDTH, VISIBLE_WIDTH, HEIGHT, C>
where
    DI: AsyncWriteOnlyDataCommand,
    BSY: InputPin + Wait,
    RST: OutputPin,
    DELAY: DelayNs,
    C: ColorType,
{
    const RESET_DELAY_MS: u32 = 50;

    /// How long the busy line may stay high before the wait gives up, in milliseconds. A full
    /// refresh of this glass takes about two seconds; anything past a few more than that is a panel
    /// that is not going to answer.
    ///
    /// Local patch, to go with the timeout in `wait_until_idle`.
    const BUSY_TIMEOUT_MS: u32 = 5_000;

    /// How long to wait for the busy line to rise before deciding a refresh never started.
    const BUSY_START_MS: u32 = 50;

    /// Create a new display driver.
    ///
    /// Use [`Self::init`] to initialize the display.
    pub fn new(interface: DI, busy: BSY, reset: RST, delay: DELAY) -> Self {
        Self {
            _color: core::marker::PhantomData,
            interface,
            busy,
            reset,
            delay,
            using_partial_mode: false,
            initial_full_refresh_done: false,
        }
    }

    /// Initialize the display
    pub async fn init(&mut self) -> Result<()> {
        self.hw_reset().await;
        self.command(command::SW_RESET).await?;
        self.delay.delay_ms(10).await;
        self.wait_until_idle().await?;
        self.command_with_data(
            command::DRIVER_CONTROL,
            &[(HEIGHT - 1) as u8, ((HEIGHT - 1) >> 8) as u8, 0x00],
        )
        .await?;
        self.command_with_data(command::DATA_ENTRY_MODE, &[flag::DATA_ENTRY_INCRY_INCRX])
            .await?;
        self.command_with_data(
            command::BORDER_WAVEFORM_CONTROL,
            &[flag::BORDER_WAVEFORM_FOLLOW_LUT | flag::BORDER_WAVEFORM_LUT1],
        )
        .await?;
        self.command_with_data(command::DISPLAY_UPDATE_CONTROL, &[0x00, 0x80])
            .await?;
        self.command_with_data(command::TEMP_CONTROL, &[flag::INTERNAL_TEMP_SENSOR])
            .await?;
        self.use_full_frame().await?;
        self.wait_until_idle().await?;
        Ok(())
    }

    /// Perform a hardware reset of the display.
    pub async fn hw_reset(&mut self) {
        self.reset.set_low().unwrap();
        self.delay.delay_ms(Self::RESET_DELAY_MS).await;
        self.reset.set_high().unwrap();
        self.delay.delay_ms(Self::RESET_DELAY_MS).await;
    }

    /// Write to the B/W buffer.
    pub async fn write_bw_buffer(&mut self, buffer: &[u8]) -> Result<()> {
        self.use_full_frame().await?;
        self.command_with_data(command::WRITE_BW_DATA, buffer)
            .await?;
        Ok(())
    }

    /// Write to the red buffer.
    ///
    /// On B/W displays this buffer is used for fast refreshes.
    pub async fn write_red_buffer(&mut self, buffer: &[u8]) -> Result<()> {
        self.use_full_frame().await?;
        self.command_with_data(command::WRITE_RED_DATA, buffer)
            .await?;
        Ok(())
    }

    /// Write to the B/W buffer at the given position.
    ///
    /// `x`, and `width` must be multiples of 8.
    pub async fn write_partial_bw_buffer(
        &mut self,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.use_partial_frame(x, y, width, height).await?;
        self.command_with_data(command::WRITE_BW_DATA, buffer)
            .await?;
        Ok(())
    }

    /// Write to the red buffer at the given position.
    ///
    /// `x`, and `width` must be multiples of 8.
    ///
    /// On B/W displays this buffer is used for fast refreshes.
    pub async fn write_partial_red_buffer(
        &mut self,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.use_partial_frame(x, y, width, height).await?;
        self.command_with_data(command::WRITE_RED_DATA, buffer)
            .await?;
        Ok(())
    }

    /// Make the whole black and white frame on the display driver white.
    pub async fn clear_bw_buffer(&mut self) -> Result<()> {
        self.use_full_frame().await?;

        // TODO: allow non-white background color
        let color = color::Color::White.byte_value().0;

        self.command(command::WRITE_BW_DATA).await?;
        self.data_x_times(color, WIDTH / 8 * HEIGHT).await?;
        Ok(())
    }

    /// Make the whole red frame on the display driver white.
    ///
    /// On B/W displays this buffer is used for fast refreshes.
    pub async fn clear_red_buffer(&mut self) -> Result<()> {
        self.use_full_frame().await?;

        // TODO: allow non-white background color
        let color = color::Color::White.byte_value().1;

        self.command(command::WRITE_RED_DATA).await?;
        self.data_x_times(color, WIDTH / 8 * HEIGHT).await?;
        Ok(())
    }

    /// Start a full refresh of the display.
    pub async fn full_refresh(&mut self) -> Result<()> {
        self.initial_full_refresh_done = true;
        self.using_partial_mode = false;

        self.command_with_data(command::UPDATE_DISPLAY_CTRL2, &[flag::DISPLAY_MODE_1])
            .await?;
        self.command(command::MASTER_ACTIVATE).await?;
        self.wait_until_idle().await?;
        Ok(())
    }

    /// Put the device into deep-sleep mode.
    /// You will need to call [`Self::wake_up`] before you can draw to the screen again.
    pub async fn sleep(&mut self) -> Result<()> {
        // We can't use send_with_data, because the data function will also wait_until_idle,
        // but after sending the deep sleep command, busy will not be cleared,
        // maybe as a feature to signal the device won't be able to process further instuctions until woken again.
        self.interface
            .send_commands(DataFormat::U8(&[command::DEEP_SLEEP]))
            .await?;
        self.interface
            .send_data(DataFormat::U8(&[flag::DEEP_SLEEP_MODE_1]))
            .await?;
        Ok(())
    }

    /// Wake the device up from deep-sleep mode.
    pub async fn wake_up(&mut self) -> Result<()> {
        // HW reset seems to be enough in deep sleep mode 1, no need to call init again
        self.hw_reset().await;
        Ok(())
    }

    async fn use_full_frame(&mut self) -> Result<()> {
        self.use_partial_frame(0, 0, WIDTH, HEIGHT).await?;
        Ok(())
    }

    async fn use_partial_frame(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<()> {
        // TODO: make sure positions are byte-aligned
        self.set_ram_area(x, y, x + width - 1, y + height - 1)
            .await?;
        self.set_ram_counter(x, y).await?;
        Ok(())
    }

    async fn set_ram_area(
        &mut self,
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    ) -> Result<()> {
        assert!(start_x < end_x);
        assert!(start_y < end_y);

        self.command_with_data(
            command::SET_RAMXPOS,
            &[(start_x >> 3) as u8, (end_x >> 3) as u8],
        )
        .await?;

        self.command_with_data(
            command::SET_RAMYPOS,
            &[
                start_y as u8,
                (start_y >> 8) as u8,
                end_y as u8,
                (end_y >> 8) as u8,
            ],
        )
        .await?;
        Ok(())
    }

    async fn set_ram_counter(&mut self, x: u32, y: u32) -> Result<()> {
        // x is positioned in bytes, so the last 3 bits which show the position inside a byte in the ram
        // aren't relevant
        self.command_with_data(command::SET_RAMX_COUNTER, &[(x >> 3) as u8])
            .await?;

        // 2 Databytes: A[7:0] & 0..A[8]
        self.command_with_data(command::SET_RAMY_COUNTER, &[y as u8, (y >> 8) as u8])
            .await?;
        Ok(())
    }

    /// Send a command to the display.
    async fn command(&mut self, command: u8) -> Result<()> {
        self.interface
            .send_commands(DataFormat::U8(&[command]))
            .await?;
        Ok(())
    }

    /// Send an array of bytes to the display.
    async fn data(&mut self, data: &[u8]) -> Result<()> {
        self.interface.send_data(DataFormat::U8(data)).await?;
        self.wait_until_idle().await?;
        Ok(())
    }

    /// Waits until the device is not busy any more.
    ///
    /// Local patch: this used to wait for ever. The busy line is the only thing that can tell a
    /// panel from a dead one, and a panel that stops answering must not take the lamp down with
    /// it, so the wait gives up after [`Self::BUSY_TIMEOUT_MS`] and says so. A full refresh of
    /// this glass takes about two seconds, so the limit is generous.
    async fn wait_until_idle(&mut self) -> Result<()> {
        #[cfg(feature = "blocking")]
        {
            let mut waited_ms = 0u32;
            while self.busy.is_high().unwrap_or(true) {
                if waited_ms >= Self::BUSY_TIMEOUT_MS {
                    return Err(display_interface::DisplayError::BusWriteError);
                }
                self.delay.delay_ms(1);
                waited_ms += 1;
            }
        }

        #[cfg(not(feature = "blocking"))]
        let _ = self.busy.wait_for_low().await;
        Ok(())
    }

    /// Waits, briefly, for the busy line to rise after a command that starts a refresh.
    ///
    /// Local patch, for [`Self::fast_update_finish`]: without it the wait that follows can run a
    /// millisecond too early, see an idle panel, and write the next frame into a panel that has
    /// already begun. Giving up after a few milliseconds is right — a refresh that never starts is
    /// not one to wait for.
    async fn wait_until_busy(&mut self) {
        #[cfg(feature = "blocking")]
        {
            let mut waited_ms = 0u32;
            while self.busy.is_low().unwrap_or(false) && waited_ms < Self::BUSY_START_MS {
                self.delay.delay_ms(1);
                waited_ms += 1;
            }
        }

        #[cfg(not(feature = "blocking"))]
        let _ = self.busy.wait_for_high().await;
    }

    /// Sending a command and the data belonging to it.
    async fn command_with_data(&mut self, command: u8, data: &[u8]) -> Result<()> {
        self.command(command).await?;
        self.data(data).await?;
        Ok(())
    }

    /// Send a byte to the display mutiple times.
    async fn data_x_times(&mut self, data: u8, repetitions: u32) -> Result<()> {
        let mut iter = iter::repeat(data).take(repetitions as usize);
        self.interface
            .send_data(DataFormat::U8Iter(&mut iter))
            .await?;
        Ok(())
    }
}

/// Functions available only for B/W displays
#[maybe_async_cfg::maybe(
    sync(
        feature = "blocking",
        keep_self,
        idents(
            AsyncWriteOnlyDataCommand(sync = "WriteOnlyDataCommand"),
            Wait(sync = "InputPin")
        )
    ),
    async(not(feature = "blocking"), keep_self)
)]
impl<DI, BSY, RST, DELAY, const WIDTH: u32, const VISIBLE_WIDTH: u32, const HEIGHT: u32>
    DisplayDriver<DI, BSY, RST, DELAY, WIDTH, VISIBLE_WIDTH, HEIGHT, Color>
where
    DI: AsyncWriteOnlyDataCommand,
    BSY: InputPin + Wait,
    RST: OutputPin,
    DELAY: DelayNs,
{
    /// Start a fast refresh of the display using the current in-screen buffers.
    ///
    /// If the display hasn't done a [`Self::full_refresh`] yet, it will do that first.
    pub async fn fast_refresh(&mut self) -> Result<()> {
        self.fast_refresh_start().await?;
        self.wait_until_busy().await;
        self.wait_until_idle().await
    }

    /// The first half of [`Self::fast_refresh`]: everything up to the command that starts the
    /// glass working, and nothing that waits for it.
    ///
    /// Local patch, split out so a caller can keep doing its own work — reading buttons, in this
    /// lamp — while the panel takes the better part of a second to finish.
    async fn fast_refresh_start(&mut self) -> Result<()> {
        if !self.initial_full_refresh_done {
            // There a bug here which causes the new image to overwrite the existing image which then
            // fades out over several updates.
            self.full_refresh().await?;
        }

        if !self.using_partial_mode {
            self.command_with_data(command::WRITE_LUT, &lut::LUT_PARTIAL_UPDATE)
                .await?;
            self.using_partial_mode = true;
        }
        self.command_with_data(command::UPDATE_DISPLAY_CTRL2, &[flag::UNDOCUMENTED])
            .await?;
        self.command(command::MASTER_ACTIVATE).await?;
        Ok(())
    }

    /// Update the screen with the provided full frame buffer using a full refresh.
    pub async fn full_update_from_buffer(&mut self, buffer: &[u8]) -> Result<()> {
        self.write_red_buffer(buffer).await?;
        self.write_bw_buffer(buffer).await?;
        self.full_refresh().await?;
        self.write_red_buffer(buffer).await?;
        self.write_bw_buffer(buffer).await?;
        Ok(())
    }

    /// Update the screen with the provided full frame buffer using a fast refresh.
    pub async fn fast_update_from_buffer(&mut self, buffer: &[u8]) -> Result<()> {
        self.fast_update_start(buffer).await?;
        self.fast_update_finish(buffer).await
    }

    /// Write the frame and start the refresh, without waiting for the glass to finish.
    ///
    /// Local patch. [`Self::fast_update_from_buffer`] holds the caller for the whole refresh —
    /// most of a second on this panel — which is exactly what a caller with buttons to read cannot
    /// afford. Between this and [`Self::fast_update_finish`] no other command may be sent: the
    /// panel is busy and would not hear it.
    pub async fn fast_update_start(&mut self, buffer: &[u8]) -> Result<()> {
        self.write_bw_buffer(buffer).await?;
        self.fast_refresh_start().await
    }

    /// Wait for the refresh started above, then leave both RAMs holding the frame just drawn, so
    /// the next update diffs against what is actually on the glass.
    pub async fn fast_update_finish(&mut self, buffer: &[u8]) -> Result<()> {
        // The busy line takes a moment to rise after the command; waiting for it first is what
        // stops the trailing writes racing the start of the refresh.
        self.wait_until_busy().await;
        self.wait_until_idle().await?;
        self.write_red_buffer(buffer).await?;
        self.write_bw_buffer(buffer).await?;
        Ok(())
    }

    /// Whether the glass is still working on a refresh.
    pub fn is_busy(&mut self) -> bool {
        self.busy.is_high().unwrap_or(true)
    }

    /// Update the screen with the provided partial frame buffer at the given position using a fast refresh.
    ///
    /// `x`, and `width` must be multiples of 8.
    pub async fn fast_partial_update_from_buffer(
        &mut self,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.write_partial_bw_buffer(buffer, x, y, width, height)
            .await?;
        self.fast_refresh().await?;
        self.write_partial_red_buffer(buffer, x, y, width, height)
            .await?;
        self.write_partial_bw_buffer(buffer, x, y, width, height)
            .await?;
        Ok(())
    }

    /// Update the screen with the provided [`Display`] using a full refresh.
    #[cfg_attr(docsrs, doc(cfg(feature = "graphics")))]
    #[cfg(feature = "graphics")]
    pub async fn full_update<const BUFFER_SIZE: usize>(
        &mut self,
        display: &Display<WIDTH, HEIGHT, BUFFER_SIZE, Color>,
    ) -> Result<()> {
        self.full_update_from_buffer(display.buffer()).await
    }

    /// Update the screen with the provided [`Display`] using a fast refresh.
    #[cfg_attr(docsrs, doc(cfg(feature = "graphics")))]
    #[cfg(feature = "graphics")]
    pub async fn fast_update<const BUFFER_SIZE: usize>(
        &mut self,
        display: &Display<WIDTH, HEIGHT, BUFFER_SIZE, Color>,
    ) -> Result<()> {
        self.fast_update_from_buffer(display.buffer()).await
    }

    /// Update the screen with the provided partial [`Display`] at the given position using a fast refresh.
    ///
    /// `x` and the display width `W` must be multiples of 8.
    #[cfg_attr(docsrs, doc(cfg(feature = "graphics")))]
    #[cfg(feature = "graphics")]
    pub async fn fast_partial_update<const W: u32, const H: u32, const BUFFER_SIZE: usize>(
        &mut self,
        display: &Display<W, H, BUFFER_SIZE, Color>,
        x: u32,
        y: u32,
    ) -> Result<()> {
        self.fast_partial_update_from_buffer(display.buffer(), x, y, W, H)
            .await
    }
}

/// Functions available only for tri-color displays
#[maybe_async_cfg::maybe(
    sync(
        feature = "blocking",
        keep_self,
        idents(
            AsyncWriteOnlyDataCommand(sync = "WriteOnlyDataCommand"),
            Wait(sync = "InputPin")
        )
    ),
    async(not(feature = "blocking"), keep_self)
)]
impl<DI, BSY, RST, DELAY, const WIDTH: u32, const VISIBLE_WIDTH: u32, const HEIGHT: u32>
    DisplayDriver<DI, BSY, RST, DELAY, WIDTH, VISIBLE_WIDTH, HEIGHT, TriColor>
where
    DI: AsyncWriteOnlyDataCommand,
    BSY: InputPin + Wait,
    RST: OutputPin,
    DELAY: DelayNs,
{
    /// Update the screen with the provided full frame buffers using a full refresh.
    pub async fn full_update_from_buffer(
        &mut self,
        bw_buffer: &[u8],
        red_buffer: &[u8],
    ) -> Result<()> {
        self.write_red_buffer(red_buffer).await?;
        self.write_bw_buffer(bw_buffer).await?;
        self.full_refresh().await?;
        Ok(())
    }

    /// Update the screen with the provided [`Display`] using a full refresh.
    #[cfg_attr(docsrs, doc(cfg(feature = "graphics")))]
    #[cfg(feature = "graphics")]
    pub async fn full_update<const BUFFER_SIZE: usize>(
        &mut self,
        display: &Display<WIDTH, HEIGHT, BUFFER_SIZE, TriColor>,
    ) -> Result<()> {
        self.full_update_from_buffer(display.bw_buffer(), display.red_buffer())
            .await
    }

    // TODO: check if partial updates with full refresh are supported
}
