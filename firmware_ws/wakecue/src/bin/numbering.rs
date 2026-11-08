//! Bench: does the MP3 board find a file by the number in its name, or by its position?
//!
//! The card holds /mp3/0001.mp3 to 0021.mp3 (the adhan) and /mp3/0101.mp3, a four-second 880 Hz
//! beep that cannot be mistaken for an adhan. There is no 0022 to 0100, so the two meanings of a
//! number give different answers:
//!
//!   A  command 0x03, "track 22":   plays the beep if tracks are counted by position (the beep is
//!                                   the 22nd file), nothing if they are counted by file name.
//!   B  command 0x12, "file 101":   plays the beep if the board finds /mp3/0101.mp3 by name.
//!   C  command 0x12, "file 1":     the control: plays adhan 1, so a silent B is not "0x12 is
//!                                   unsupported" but "0x12 does not find 0101".
//!
//! After each play command the board is asked its state (0x42) and which track it is on (0x4B),
//! so the result does not rest on ears alone. The log announces each test; say what you heard
//! after each one. Wiring and serial port are the same as `sound`.

#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::uart::{Config, Uart};
use esp_println as _;
use lamp_core::dfplayer::{
    CMD_PLAY_MP3_FOLDER, CMD_PLAY_TRACK, CMD_QUERY_STATUS, CMD_QUERY_TF_FILES, CMD_QUERY_TF_TRACK,
    CMD_STOP, CMD_VOLUME, frame,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// 0 to 30 on the board. Low: the board starts at full volume.
const VOLUME: u16 = 20;

type Port<'d> = Uart<'d, esp_hal::Blocking>;

fn send(uart: &mut Port<'_>, what: &str, cmd: u8, param: u16) {
    let f = frame(cmd, param);
    info!("tx {=str}: {=[u8]:x}", what, &f[..]);
    uart.write(&f).expect("uart write");
    uart.flush().expect("uart flush");
}

/// Read for `ms` milliseconds, then log everything that arrived as one block and decode each
/// whole reply frame found in it (7E FF 06 CMD FB PH PL CK CK EF).
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
        info!("rx: {=u32} read errors", errors);
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

/// Play something, then ask what the board thinks it is doing, then let it play on.
fn trial(uart: &mut Port<'_>, delay: &Delay, name: &str, cmd: u8, param: u16) {
    info!("==== {=str} ====", name);
    send(uart, "play", cmd, param);
    // An error reply, if the board has one, arrives within a moment of the command.
    listen(uart, delay, 1000);
    send(uart, "status", CMD_QUERY_STATUS, 0);
    listen(uart, delay, 300);
    send(uart, "current track", CMD_QUERY_TF_TRACK, 0);
    listen(uart, delay, 300);
    listen(uart, delay, 3000);
    send(uart, "stop", CMD_STOP, 0);
    listen(uart, delay, 1500);
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

    info!("numbering: UART1 at 9600 baud, RX GPIO20, TX GPIO21, volume {=u16}", VOLUME);

    loop {
        info!("waiting 3 s for the board to start");
        listen(&mut uart, &delay, 3000);

        send(&mut uart, "volume", CMD_VOLUME, VOLUME);
        listen(&mut uart, &delay, 500);

        send(&mut uart, "files on the card", CMD_QUERY_TF_FILES, 0);
        listen(&mut uart, &delay, 600);

        trial(&mut uart, &delay, "A: command 0x03, track 22 (position?)", CMD_PLAY_TRACK, 22);
        trial(&mut uart, &delay, "B: command 0x12, file 101 (by name?)", CMD_PLAY_MP3_FOLDER, 101);
        trial(&mut uart, &delay, "C: command 0x12, file 1 (control)", CMD_PLAY_MP3_FOLDER, 1);

        info!("round done, again in 5 s");
        delay.delay_millis(5000);
    }
}
