/// Brightness 0..=255 of the ring: grows with the square of the time so the eye sees a smooth ramp.
pub fn ramp_duty(elapsed_s: u32, total_s: u32) -> u8 {
    if total_s == 0 || elapsed_s >= total_s { return 255; }
    let e = elapsed_s as u64;
    let t = total_s as u64;
    (255 * e * e / (t * t)) as u8
}

/// Colour at a point in the ramp, before the brightness cap: deep red -> amber -> warm white (255, 147, 41).
pub fn ramp_colour(elapsed_s: u32, total_s: u32) -> [u8; 3] {
    const STOPS: [(u32, [u8; 3]); 4] = [
        (0, [42, 7, 0]),
        (35, [201, 66, 14]),
        (60, [240, 138, 42]),
        (100, [255, 147, 41]),
    ];
    let p = if total_s == 0 { 100 } else { (elapsed_s.min(total_s) as u64 * 100 / total_s as u64) as u32 };
    for w in STOPS.windows(2) {
        let (p0, c0) = w[0];
        let (p1, c1) = w[1];
        if p <= p1 {
            let f = p - p0;
            let span = p1 - p0;
            let mut out = [0u8; 3];
            for i in 0..3 {
                let a = c0[i] as i32;
                let b = c1[i] as i32;
                out[i] = (a + (b - a) * f as i32 / span as i32) as u8;
            }
            return out;
        }
    }
    STOPS[3].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_is_monotonic_and_bounded() {
        let mut prev = 0;
        for s in 0..=900 {
            let d = ramp_duty(s, 900);
            assert!(d >= prev);
            prev = d;
        }
        assert_eq!(ramp_duty(0, 900), 0);
        assert_eq!(ramp_duty(900, 900), 255);
        assert_eq!(ramp_duty(450, 900), 63);
        assert_eq!(ramp_duty(5, 0), 255);
    }

    #[test]
    fn colour_ends_at_warm_white() {
        assert_eq!(ramp_colour(900, 900), [255, 147, 41]);
        assert_eq!(ramp_colour(0, 900), [42, 7, 0]);
        assert_eq!(ramp_colour(2000, 900), [255, 147, 41]);
    }
}
