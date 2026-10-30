use crate::calendar::days_from_civil;

pub const I2C_ADDR: u8 = 0x68;
pub const REG_TIME: u8 = 0x00;
pub const REG_ALARM1: u8 = 0x07;
pub const REG_CONTROL: u8 = 0x0E;
pub const REG_STATUS: u8 = 0x0F;

/// Control: INTCN = 1 (alarm drives INT), A1IE = 1, A2IE = 0, oscillator on, square wave off.
pub const CONTROL_ALARM1_ONLY: u8 = 0b0000_0101;
/// Control: INTCN = 1, A1IE = 1, A2IE = 1.
pub const CONTROL_ALARM1_AND_2: u8 = 0b0000_0111;
pub const STATUS_A1F: u8 = 0b0000_0001;
pub const STATUS_A2F: u8 = 0b0000_0010;

fn from_bcd(b: u8) -> u8 { (b >> 4) * 10 + (b & 0x0F) }
fn to_bcd(v: u8) -> u8 { ((v / 10) << 4) | (v % 10) }

/// Registers 0x00..=0x06: sec, min, hour (24 h), weekday, date, month (+century bit), year (00-99).
/// Returns Unix seconds, assuming the chip holds UTC and years 2000-2099.
pub fn parse_time_registers(t: [u8; 7]) -> i64 {
    let sec = from_bcd(t[0] & 0x7F) as i64;
    let min = from_bcd(t[1] & 0x7F) as i64;
    let hour = from_bcd(t[2] & 0x3F) as i64;
    let date = from_bcd(t[4] & 0x3F) as u32;
    let month = from_bcd(t[5] & 0x1F) as u32;
    let year = 2000 + from_bcd(t[6]) as i32;
    days_from_civil(year, month, date) as i64 * 86400 + hour * 3600 + min * 60 + sec
}

/// Inverse of `parse_time_registers`, used to set the clock. The weekday register is 1..=7 (Monday = 1 here).
pub fn time_registers(unix_s: i64) -> [u8; 7] {
    use crate::calendar::{civil_from_days, weekday};
    let day = unix_s.div_euclid(86400) as i32;
    let s = unix_s.rem_euclid(86400);
    let (y, m, d) = civil_from_days(day);
    let wd = match weekday(day) { 0 => 7, w => w };
    [
        to_bcd((s % 60) as u8),
        to_bcd((s / 60 % 60) as u8),
        to_bcd((s / 3600) as u8),
        wd,
        to_bcd(d as u8),
        to_bcd(m as u8),
        to_bcd((y - 2000) as u8),
    ]
}

/// Alarm 1 registers 0x07..=0x0A for "date + hour + minute + second 0" (all mask bits A1M1..A1M4 = 0,
/// DY/DT = 0 so the last register is a day of the month), from minutes since the Unix epoch in UTC.
pub fn alarm1_registers(utc_min: i64) -> [u8; 4] {
    use crate::calendar::civil_from_days;
    let day = utc_min.div_euclid(1440) as i32;
    let m = utc_min.rem_euclid(1440);
    let (_, _, d) = civil_from_days(day);
    [to_bcd(0), to_bcd((m % 60) as u8), to_bcd((m / 60) as u8), to_bcd(d as u8)]
}

/// Alarm 2 fires every minute when A2M2..A2M4 are all 1: registers 0x0B..=0x0D.
pub const ALARM2_EVERY_MINUTE: [u8; 3] = [0x80, 0x80, 0x80];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_known_instant() {
        // 2026-10-25 14:30:45 UTC, a Sunday
        let regs = [0x45, 0x30, 0x14, 0x07, 0x25, 0x10, 0x26];
        let expected = days_from_civil(2026, 10, 25) as i64 * 86400 + 14 * 3600 + 30 * 60 + 45;
        assert_eq!(parse_time_registers(regs), expected);
    }

    #[test]
    fn century_bit_and_stray_bits_are_ignored() {
        let regs = [0x45, 0x30, 0x14, 0x07, 0x25, 0x90, 0x26];
        let expected = days_from_civil(2026, 10, 25) as i64 * 86400 + 14 * 3600 + 30 * 60 + 45;
        assert_eq!(parse_time_registers(regs), expected);
    }

    #[test]
    fn time_registers_round_trip() {
        for s in [946_684_800i64, 1_792_938_645, 1_900_000_000, 4_000_000_000] {
            let t = time_registers(s);
            assert_eq!(parse_time_registers(t), s);
        }
        // Sunday is 7
        assert_eq!(time_registers(1_792_938_645)[3], 7);
    }

    #[test]
    fn alarm1_bytes() {
        let utc_min = days_from_civil(2026, 10, 9) as i64 * 1440 + 2 * 60 + 5;
        assert_eq!(alarm1_registers(utc_min), [0x00, 0x05, 0x02, 0x09]);
    }
}
