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
    fn volume_30_matches_published_frame() {
        assert_eq!(
            frame(CMD_VOLUME, 30),
            [0x7E, 0xFF, 0x06, 0x06, 0x00, 0x00, 0x1E, 0xFE, 0xD7, 0xEF]
        );
    }
}
