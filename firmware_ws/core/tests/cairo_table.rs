//! The table the lamp used to ship with, kept as an oracle for the arithmetic.
//!
//! `prayer-gen` wrote ten years of Cairo times (2026-01-01 .. 2035-12-31) for the firmware to embed.
//! The lamp computes its own times now, so the file is no longer in the image — but it is the one
//! piece of evidence that the arithmetic running on the board agrees with the times the board has
//! been showing, and a check written by the generator that moved into the lamp would prove only
//! that the code equals itself. So the old table stays here and this test asks the new code for
//! every one of its 18,260 times.
//!
//! Written on 5 Nov 2026 by `prayer-gen 30.0444 31.2357 2026 10`, defaults otherwise (19.5° Fajr,
//! 17.5° Isha, standard Asr). That crate is gone; it is in the history before this commit.
//!
//! The generator rounded every time UP to the next minute. The lamp now rounds to the NEAREST minute,
//! because that is what the Egyptian Survey Authority's published timetable does
//! (`tests/egas_mansoura.rs`), so each time in this table is either the lamp's or one minute later.
//! Nothing else about the arithmetic may differ, which is what the test still pins down.

use lamp_core::astro::{self, Method};
use lamp_core::schedule::Prayer;

const MAGIC: &[u8; 4] = b"WCP1";
const HEADER: usize = 8;
const STRIDE: usize = 10;

static TABLE: &[u8] = include_bytes!("data/prayers-cairo.bin");

const CAIRO: Method = Method {
    fajr_angle_d10: 195,
    isha_angle_d10: Some(175),
    isha_minutes: 0,
    asr_shadow: 1,
};

#[test]
fn every_cairo_time_is_the_table_the_lamp_shipped_with_or_one_minute_earlier() {
    assert_eq!(&TABLE[..4], MAGIC, "not a prayer table");
    assert_eq!((TABLE.len() - HEADER) % STRIDE, 0, "a part of a day");

    let first = i32::from_le_bytes([TABLE[4], TABLE[5], TABLE[6], TABLE[7]]);
    let days = (TABLE.len() - HEADER) / STRIDE;
    assert_eq!(days, 3652, "ten years of days");

    for (i, day) in (first..first + days as i32).enumerate() {
        for prayer in Prayer::ALL {
            let at = HEADER + i * STRIDE + prayer.index() * 2;
            let want = u16::from_le_bytes([TABLE[at], TABLE[at + 1]]) as i64;
            let got = astro::utc_min(prayer, day, 30.0444, 31.2357, &CAIRO)
                .unwrap_or_else(|| panic!("day {day} {prayer:?}: the sun never gets there"));
            // The table rounds up and the lamp to the nearest, so the table is never earlier and
            // never more than a minute later.
            assert!(want - got == 0 || want - got == 1, "day {day} {prayer:?}: table {want}, lamp {got}");
        }
    }
}
