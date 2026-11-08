//! Bench: talk to the MP3 board over its serial link.
//!
//! Wires: GPIO21 (TX) through 1 kOhm to the board's RX, and the board's TX through 1 kOhm to
//! GPIO20 (RX), plus a shared GND. Power and speaker are wired as in the stage 1 test.
//!
//! Each round the firmware waits for the board to start, logs whatever the board says on its own,
//! sets a low volume, asks how many files the card holds, plays tracks 1 to 3 for six seconds
//! each, then stops. Every byte that comes back is logged in hex: if nothing comes back, the
//! board's TX wire is the thing to check. If it plays but never answers, the RX wire is fine and
//! the TX wire is not.
//!
//! The serial port is UART1 on GPIO20/21, not the console: the log goes out over USB.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
// The crate carries the radio, which brings `alloc` into every binary and so needs a global
// allocator to link. This binary never allocates, so it never gets a heap.
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::uart::{Config, Uart};
use esp_println as _;
use lamp_core::dfplayer::{
    CMD_PLAY_TRACK, CMD_QUERY_TF_FILES, CMD_STOP, CMD_VOLUME, frame,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// 0 to 30 on the board. Low on purpose: the board starts at full volume, which is very loud.
const VOLUME: u16 = 20;
/// Seconds each track plays.
const TRACK_S: u32 = 6;

type Port<'d> = Uart<'d, esp_hal::Blocking>;

/// Send one frame and log it.
fn send(uart: &mut Port<'_>, what: &str, cmd: u8, param: u16) {
    let f = frame(cmd, param);
    info!("tx {=str}: {=[u8]:x}", what, &f[..]);
    uart.write(&f).expect("uart write");
    uart.flush().expect("uart flush");
}

/// Read for `ms` milliseconds, then log everything that arrived as one block. A reply frame is
/// ten bytes, 7E FF 06 CMD FB PH PL CK CK EF, so each one found in the block has its command and
/// parameter printed too. Bytes arrive one at a time at 9600 baud, which is why they are gathered
/// here and not logged as they come.
fn listen(uart: &mut Port<'_>, delay: &Delay, ms: u32) {
    let mut acc = [0u8; 64];
    let mut len = 0usize;
    let mut chunk = [0u8; 16];
    let mut errors = 0u32;
    for _ in 0..ms {
        while uart.read_ready() {
            match uart.read(&mut chunk) {
                Ok(n) if n > 0 => {
                    for &b in &chunk[..n] {
                        if len < acc.len() {
                            acc[len] = b;
                            len += 1;
                        }
                    }
                }
                Ok(_) => break,
                Err(_) => {
                    errors += 1;
                    break;
                }
            }
        }
        delay.delay_millis(1);
    }
    if errors > 0 {
        info!("rx: {=u32} read errors (framing, parity or overrun)", errors);
    }
    if len == 0 {
        info!("rx: nothing for {=u32} ms", ms);
        return;
    }
    info!("rx {=usize} bytes: {=[u8]:x}", len, &acc[..len]);
    let mut i = 0;
    while i + 10 <= len {
        if acc[i] == 0x7E && acc[i + 9] == 0xEF {
            info!(
                "   reply cmd {=u8:#04x} param {=u16}",
                acc[i + 3],
                u16::from_be_bytes([acc[i + 5], acc[i + 6]])
            );
            i += 10;
        } else {
            i += 1;
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let delay = Delay::new();

    let mut uart = Uart::new(peripherals.UART1, Config::default().with_baudrate(9600))
        .expect("uart config")
        .with_rx(peripherals.GPIO20)
        .with_tx(peripherals.GPIO21);

    info!("sound: UART1 at 9600 baud, RX GPIO20, TX GPIO21, volume {=u16}", VOLUME);

    loop {
        // The board takes one and a half to three seconds to read the card after power-up.
        info!("waiting 3 s for the board to start, listening for its own announcement");
        listen(&mut uart, &delay, 3000);

        send(&mut uart, "volume", CMD_VOLUME, VOLUME);
        listen(&mut uart, &delay, 500);

        // Measured: 0x48 is the microSD file count (21 on the bench card), 0x47 and 0x49 answer 0.
        send(&mut uart, "files on the card", CMD_QUERY_TF_FILES, 0);
        listen(&mut uart, &delay, 600);

        for track in 1..=3u16 {
            info!("playing track {=u16}", track);
            send(&mut uart, "play", CMD_PLAY_TRACK, track);
            listen(&mut uart, &delay, TRACK_S * 1000);
        }

        send(&mut uart, "stop", CMD_STOP, 0);
        listen(&mut uart, &delay, 500);

        info!("round done, again in 2 s");
        delay.delay_millis(2000);
    }
}
