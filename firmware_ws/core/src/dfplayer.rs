/// Every command is a 10-byte frame: 7E FF 06 CMD 00 PH PL CK_H CK_L EF
pub fn frame(cmd: u8, param: u16) -> [u8; 10] {
    let [ph, pl] = param.to_be_bytes();
    let sum = 0xFFu16 + 0x06 + cmd as u16 + ph as u16 + pl as u16;
    let [ch, cl] = 0u16.wrapping_sub(sum).to_be_bytes();
    [0x7E, 0xFF, 0x06, cmd, 0x00, ph, pl, ch, cl, 0xEF]
}

pub const CMD_PLAY_TRACK: u8 = 0x03;
pub const CMD_VOLUME: u8 = 0x06;
pub const CMD_PLAY_FOLDER_TRACK: u8 = 0x0F;
pub const CMD_STOP: u8 = 0x16;
/// Play `/mp3/NNNN.mp3` by the number in its name. Not in the V1.0 datasheet's table; whether the
/// HW-247A answers it is what `bin/numbering.rs` finds out.
pub const CMD_PLAY_MP3_FOLDER: u8 = 0x12;
/// Query: the player's state (device in the high byte, playing, paused or stopped in the low).
pub const CMD_QUERY_STATUS: u8 = 0x42;
/// Query: which track of the microSD card is playing now.
pub const CMD_QUERY_TF_TRACK: u8 = 0x4B;
/// Query: how many files the microSD card holds. Measured on the HW-247A board on 8 Nov 2026 with
/// 21 files on the card: 0x48 answered 21, while 0x47 and 0x49 answered 0. The datasheet's
/// command table (V1.0, section 3.2) says 0x47 is the TF card and 0x48 the U-disk; for this
/// board it is the other way round, so the table is not to be trusted for these queries.
pub const CMD_QUERY_TF_FILES: u8 = 0x48;

/// The player's own message that a track has finished, with the track's position as the value.
/// It arrives twice, a few milliseconds apart.
pub const REPLY_TF_FINISHED: u8 = 0x3D;

/// The level to play at `elapsed_ms` into a sound that climbs to `target` over `fade_ms`: from 1
/// (the quietest the player has that is not silence) in even steps to the target, and the target
/// from then on. With no fade it is the target at once. Never above the player's top level (30),
/// and never silent, so a sound that starts is a sound that can be heard.
pub fn fade_volume(target: u8, fade_ms: u32, elapsed_ms: u32) -> u8 {
    let target = target.clamp(1, 30) as u32;
    if fade_ms == 0 || elapsed_ms >= fade_ms {
        return target as u8;
    }
    (1 + (target - 1) * elapsed_ms / fade_ms) as u8
}

/// One reply from the player: the command it answers or announces, and its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub cmd: u8,
    pub param: u16,
}

/// Finds replies in the byte stream coming back from the player, one byte at a time.
///
/// A reply is ten bytes, `7E FF 06 CMD FB PH PL CK CK EF`, and bytes arrive at 9600 baud, so one
/// can straddle two reads. The parser keeps the last ten bytes and looks for a frame in that
/// window after every byte. It never restarts on seeing `7E`, because `7E` can be data (the board's
/// start-up complaint carries one), and a frame that does not check out is simply not a frame: the
/// bytes slide past and the next one is found.
#[derive(Clone, Copy, Debug, Default)]
pub struct Parser {
    window: [u8; 10],
    filled: usize,
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    pub fn push(&mut self, byte: u8) -> Option<Reply> {
        if self.filled < 10 {
            self.window[self.filled] = byte;
            self.filled += 1;
        } else {
            self.window.copy_within(1.., 0);
            self.window[9] = byte;
        }
        if self.filled < 10 {
            return None;
        }
        let w = &self.window;
        if w[0] != 0x7E || w[1] != 0xFF || w[2] != 0x06 || w[9] != 0xEF {
            return None;
        }
        let sum = w[1..7].iter().fold(0u16, |a, &b| a + b as u16);
        if 0u16.wrapping_sub(sum) != u16::from_be_bytes([w[7], w[8]]) {
            return None;
        }
        let reply = Reply { cmd: w[3], param: u16::from_be_bytes([w[5], w[6]]) };
        // The same bytes must not match again as the window slides.
        self.filled = 0;
        Some(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_track_1_matches_published_frame() {
        assert_eq!(
            frame(CMD_PLAY_TRACK, 1),
            [0x7E, 0xFF, 0x06, 0x03, 0x00, 0x00, 0x01, 0xFE, 0xF7, 0xEF]
        );
    }

    #[test]
    fn play_mp3_folder_track_1_frame() {
        // Worked out by hand: FF + 06 + 12 + 00 + 00 + 01 = 0x118, and 0x10000 - 0x118 = 0xFEE8.
        assert_eq!(
            frame(CMD_PLAY_MP3_FOLDER, 1),
            [0x7E, 0xFF, 0x06, 0x12, 0x00, 0x00, 0x01, 0xFE, 0xE8, 0xEF]
        );
    }

    fn feed(p: &mut Parser, bytes: &[u8]) -> ([Reply; 4], usize) {
        let mut out = [Reply { cmd: 0, param: 0 }; 4];
        let mut n = 0;
        for &b in bytes {
            if let Some(r) = p.push(b) {
                out[n] = r;
                n += 1;
            }
        }
        (out, n)
    }

    #[test]
    fn the_cards_file_count_reply_from_the_bench_is_read() {
        // What the board answered to command 0x48 with 22 files on the card.
        let mut p = Parser::new();
        let (r, n) = feed(&mut p, &[0x7E, 0xFF, 0x06, 0x48, 0x00, 0x00, 0x16, 0xFE, 0x9D, 0xEF]);
        assert_eq!(&r[..n], [Reply { cmd: 0x48, param: 22 }]);
    }

    #[test]
    fn the_finished_message_arrives_twice_and_is_read_twice() {
        let mut p = Parser::new();
        let one = [0x7E, 0xFF, 0x06, 0x3D, 0x00, 0x00, 0x16, 0xFE, 0xA8, 0xEF];
        let mut two = [0u8; 20];
        two[..10].copy_from_slice(&one);
        two[10..].copy_from_slice(&one);
        let (r, n) = feed(&mut p, &two);
        assert_eq!(&r[..n], [Reply { cmd: REPLY_TF_FINISHED, param: 22 }; 2]);
    }

    #[test]
    fn the_tail_of_a_frame_and_a_frame_that_straddles_two_reads_are_both_handled() {
        // The board's start-up complaint arrived with its first two bytes missing: nothing to read.
        let mut p = Parser::new();
        let (_, n) = feed(&mut p, &[0x06, 0x40, 0x00, 0x00, 0x7E, 0xFE, 0x3D, 0xEF]);
        assert_eq!(n, 0);
        // A real reply cut in two reads is found as soon as its last byte lands.
        let (_, n) = feed(&mut p, &[0x7E, 0xFF, 0x06, 0x42, 0x00]);
        assert_eq!(n, 0);
        let (r, n) = feed(&mut p, &[0x02, 0x01, 0xFE, 0xB6, 0xEF]);
        assert_eq!(&r[..n], [Reply { cmd: 0x42, param: 0x0201 }]);
    }

    #[test]
    fn a_frame_whose_data_holds_the_start_byte_is_still_read() {
        // The complaint with its head intact: value 0x007E, so 7E appears inside the frame.
        let mut p = Parser::new();
        let (r, n) = feed(&mut p, &[0x7E, 0xFF, 0x06, 0x40, 0x00, 0x00, 0x7E, 0xFE, 0x3D, 0xEF]);
        assert_eq!(&r[..n], [Reply { cmd: 0x40, param: 0x7E }]);
    }

    #[test]
    fn noise_in_front_of_a_frame_and_a_bad_checksum_do_not_confuse_it() {
        let mut p = Parser::new();
        let good = [0x7E, 0xFF, 0x06, 0x48, 0x00, 0x00, 0x16, 0xFE, 0x9D, 0xEF];
        let mut stream = [0u8; 15];
        stream[..5].copy_from_slice(&[0x00, 0x7E, 0x7E, 0xFF, 0x13]);
        stream[5..].copy_from_slice(&good);
        let (r, n) = feed(&mut p, &stream);
        assert_eq!(&r[..n], [Reply { cmd: 0x48, param: 22 }]);
        let mut bad = good;
        bad[8] = 0x9E; // the checksum is off by one
        let (_, n) = feed(&mut p, &bad);
        assert_eq!(n, 0);
    }

    #[test]
    fn a_fade_climbs_from_the_quietest_to_the_target_and_stays_there() {
        assert_eq!(fade_volume(22, 30_000, 0), 1);
        assert_eq!(fade_volume(22, 30_000, 15_000), 11);
        assert_eq!(fade_volume(22, 30_000, 29_999), 21);
        assert_eq!(fade_volume(22, 30_000, 30_000), 22);
        assert_eq!(fade_volume(22, 30_000, 90_000), 22);
        // No fade is the target at once; a target of 0 is never silence; nothing passes 30.
        assert_eq!(fade_volume(22, 0, 0), 22);
        assert_eq!(fade_volume(0, 0, 0), 1);
        assert_eq!(fade_volume(99, 10_000, 20_000), 30);
        // It never goes down on the way up.
        let mut last = 0;
        for t in (0..=30_000).step_by(250) {
            let v = fade_volume(30, 30_000, t);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn volume_30_matches_published_frame() {
        assert_eq!(
            frame(CMD_VOLUME, 30),
            [0x7E, 0xFF, 0x06, 0x06, 0x00, 0x00, 0x1E, 0xFE, 0xD7, 0xEF]
        );
    }
}
