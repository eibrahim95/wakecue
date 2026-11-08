//! The MP3 board, over its serial link: GPIO21 (TX) and GPIO20 (RX) on UART1 at 9600 baud, each
//! through 1 kOhm. Every call is blocking and short; a reply is waited for only where one is
//! needed, because the board answers nothing it is not asked.
//!
//! What it does and does not tell: it reports the file count (command 0x48 on this board, not what
//! the datasheet says) and a track's end (0x3D), and nothing about which file plays, so the lamp
//! keeps track of that itself. The card is numbered `/mp3/0001.mp3` and up for the adhan and
//! `/mp3/0101.mp3` and up for the alarm sounds.

use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::peripherals::{GPIO20, GPIO21, UART1};
use esp_hal::uart::{Config, Uart};
use lamp_core::dfplayer::{
    CMD_PLAY_MP3_FOLDER, CMD_QUERY_TF_FILES, CMD_STOP, CMD_VOLUME, Parser, REPLY_TF_FINISHED, Reply,
    frame,
};

/// How long to leave the board alone after each command.
const CMD_GAP_MS: u32 = 120;

pub struct Player {
    uart: Uart<'static, Blocking>,
    parser: Parser,
    delay: Delay,
}

impl Player {
    pub fn new(uart: UART1<'static>, rx: GPIO20<'static>, tx: GPIO21<'static>) -> Player {
        let uart = Uart::new(uart, Config::default().with_baudrate(9600))
            .expect("uart config")
            .with_rx(rx)
            .with_tx(tx);
        Player { uart, parser: Parser::new(), delay: Delay::new() }
    }

    fn send(&mut self, cmd: u8, param: u16) {
        // A write that fails has nowhere to be reported: a silent lamp is what the owner will see,
        // and the next command tries again.
        let f = frame(cmd, param);
        let _ = self.uart.write(&f);
        let _ = self.uart.flush();
        // The board is slow to take a command in: the bench programs always left half a second, and
        // a play sent a few milliseconds after a volume is the kind of thing it can drop. Leaving a
        // gap after every command costs a preview step about a quarter of a second.
        self.delay.delay_millis(CMD_GAP_MS);
    }

    /// Hand every reply that has arrived to `on_reply`. Never waits.
    fn poll(&mut self, mut on_reply: impl FnMut(Reply)) {
        let mut chunk = [0u8; 16];
        while self.uart.read_ready() {
            match self.uart.read(&mut chunk) {
                Ok(n) if n > 0 => {
                    for &b in &chunk[..n] {
                        if let Some(r) = self.parser.push(b) {
                            on_reply(r);
                        }
                    }
                }
                _ => break,
            }
        }
    }

    /// 0 to 30.
    pub fn set_volume(&mut self, level: u8) {
        self.send(CMD_VOLUME, level.min(30) as u16);
    }

    /// Play `/mp3/NNNN.mp3` by the number in its name.
    pub fn play_file(&mut self, file: u16) {
        self.send(CMD_PLAY_MP3_FOLDER, file);
    }

    pub fn stop(&mut self) {
        self.send(CMD_STOP, 0);
    }

    /// How many files the card holds, or `None` if the board does not answer: no card, a board
    /// that is still starting (it takes up to three seconds after power), or a wire. Asks up to
    /// three times, half a second each, so a lamp with no board costs a hand wake a second and a
    /// half and not more.
    pub fn file_count(&mut self) -> Option<u16> {
        for _ in 0..3 {
            self.send(CMD_QUERY_TF_FILES, 0);
            for _ in 0..500 {
                let mut got = None;
                self.poll(|r| {
                    if r.cmd == CMD_QUERY_TF_FILES {
                        got = Some(r.param);
                    }
                });
                if got.is_some() {
                    return got;
                }
                self.delay.delay_millis(1);
            }
        }
        None
    }

    /// Whether the board has said a track ended since the last call. Never waits.
    pub fn finished(&mut self) -> bool {
        let mut done = false;
        self.poll(|r| {
            if r.cmd == REPLY_TF_FINISHED {
                done = true;
            }
        });
        done
    }
}
