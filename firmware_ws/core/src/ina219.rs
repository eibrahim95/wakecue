//! INA219 as a battery-voltage meter: read the cell on the bus-voltage input, then power the chip down.
//! The register map and bit fields are from TI's INA219 datasheet.

/// I²C address with A0 and A1 tied to GND (the usual module default).
pub const ADDR: u8 = 0x40;
pub const REG_CONFIG: u8 = 0x00;
pub const REG_BUS: u8 = 0x02;

/// 16 V range, gain /1, 12-bit bus and shunt conversions, bus voltage triggered (one conversion per write).
pub const CONFIG_BUS_TRIGGERED: u16 = 0x019A;
/// Same settings, MODE = power-down (6 µA typical).
pub const CONFIG_POWER_DOWN: u16 = 0x0198;

/// Bus-voltage register: bits 15..3 hold the voltage in 4 mV steps, bit 1 is CNVR (conversion ready), bit 0 is OVF.
pub fn bus_mv(reg: [u8; 2]) -> u32 {
    (u16::from_be_bytes(reg) >> 3) as u32 * 4
}

pub fn conversion_ready(reg: [u8; 2]) -> bool {
    reg[1] & 0b10 != 0
}

pub fn overflow(reg: [u8; 2]) -> bool {
    reg[1] & 0b01 != 0
}

/// Resting-voltage to charge for a generic 18650 (points every 100 mV, linear between). A rough guide only:
/// replace the table with the discharge curve measured on the real cell once the lamp runs.
const CURVE: [(u32, u8); 10] = [
    (3000, 0), (3300, 3), (3500, 10), (3600, 20), (3700, 35),
    (3800, 50), (3900, 63), (4000, 76), (4100, 88), (4200, 100),
];

pub fn cell_percent(mv: u32) -> u8 {
    if mv <= CURVE[0].0 { return 0; }
    for w in CURVE.windows(2) {
        let (v0, p0) = w[0];
        let (v1, p1) = w[1];
        if mv <= v1 {
            return (p0 as u32 + (mv - v0) * (p1 - p0) as u32 / (v1 - v0)) as u8;
        }
    }
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_register_decodes_in_4_mv_steps() {
        // 4.2 V = 1050 steps; the value sits in bits 15..3, CNVR in bit 1.
        let reg = ((1050u16 << 3) | 0b10).to_be_bytes();
        assert_eq!(bus_mv(reg), 4200);
        assert!(conversion_ready(reg));
        assert!(!overflow(reg));
    }

    #[test]
    fn config_words_match_the_datasheet_fields() {
        // BRNG 0 (16 V), PGA /1, BADC = SADC = 0011, MODE 010 (bus triggered) or 000 (power-down).
        assert_eq!(CONFIG_BUS_TRIGGERED, 0b0000_0001_1001_1010);
        assert_eq!(CONFIG_BUS_TRIGGERED & 0b111, 0b010);
        assert_eq!(CONFIG_POWER_DOWN & 0b111, 0b000);
        assert_eq!(CONFIG_BUS_TRIGGERED & !0b111, CONFIG_POWER_DOWN);
    }

    #[test]
    fn percent_is_monotonic_and_bounded() {
        let mut last = 0;
        for mv in (2800..=4300).step_by(10) {
            let p = cell_percent(mv);
            assert!(p >= last && p <= 100, "{mv} mV -> {p}");
            last = p;
        }
        assert_eq!(cell_percent(2900), 0);
        assert_eq!(cell_percent(4200), 100);
        assert_eq!(cell_percent(3750), 42);
    }
}
