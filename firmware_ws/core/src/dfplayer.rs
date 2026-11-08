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

    #[test]
    fn volume_30_matches_published_frame() {
        assert_eq!(
            frame(CMD_VOLUME, 30),
            [0x7E, 0xFF, 0x06, 0x06, 0x00, 0x00, 0x1E, 0xFE, 0xD7, 0xEF]
        );
    }
}
