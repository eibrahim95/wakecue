//! The two NTP packets the lamp needs: one question, and reading the time out of the answer.
//!
//! It asks once, with a bare client packet, and takes the whole seconds of the server's transmit
//! time. The lamp's clock shows minutes, so the network's few hundred milliseconds of delay do not
//! matter, and the answer is not worth the extra code of measuring them.

/// UDP port of an NTP server.
pub const PORT: u16 = 123;

/// Seconds between the NTP epoch (1900-01-01) and the Unix one (1970-01-01).
pub const UNIX_OFFSET: i64 = 2_208_988_800;

/// 2025-01-01 00:00 UTC. A reply that claims to be earlier is not a time, it is a server that has
/// not set its own clock, and the DS3231 must not be written with it.
const EARLIEST_UNIX: i64 = 1_735_689_600;

/// A client request: leap indicator 0, version 3, mode 3, everything else zero.
pub fn request() -> [u8; 48] {
    let mut p = [0u8; 48];
    p[0] = 0x1B;
    p
}

/// Unix seconds from a server's reply, or `None` when it cannot be believed: too short, not a
/// server's answer, a stratum of 0 (the server's "go away" or "I do not know"), an unsynchronised
/// leap indicator, or a time before [`EARLIEST_UNIX`].
pub fn parse_reply(reply: &[u8]) -> Option<i64> {
    if reply.len() < 48 {
        return None;
    }
    let leap = reply[0] >> 6;
    let mode = reply[0] & 0b111;
    let stratum = reply[1];
    // Leap 3 is "clock not synchronised"; mode 4 is a server, 5 a broadcast we did not ask for.
    if leap == 3 || mode != 4 || stratum == 0 || stratum > 15 {
        return None;
    }
    let secs = u32::from_be_bytes([reply[40], reply[41], reply[42], reply[43]]);
    let unix = secs as i64 - UNIX_OFFSET;
    (unix >= EARLIEST_UNIX).then_some(unix)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply carrying `unix` seconds in its transmit timestamp.
    fn reply(unix: i64, first: u8, stratum: u8) -> [u8; 48] {
        let mut p = [0u8; 48];
        p[0] = first;
        p[1] = stratum;
        p[40..44].copy_from_slice(&((unix + UNIX_OFFSET) as u32).to_be_bytes());
        p
    }

    // 8 Nov 2026 12:00:00 UTC.
    const NOW: i64 = 1_794_139_200;

    #[test]
    fn the_request_is_a_version_3_client_packet() {
        let r = request();
        assert_eq!(r.len(), 48);
        assert_eq!(r[0], 0x1B);
        assert!(r[1..].iter().all(|b| *b == 0));
    }

    #[test]
    fn a_server_answer_gives_unix_seconds() {
        assert_eq!(parse_reply(&reply(NOW, 0x24, 2)), Some(NOW));
        // Version 4 server, stratum 1, fraction bytes ignored.
        let mut p = reply(NOW, 0x24, 1);
        p[44..48].copy_from_slice(&[0xFF; 4]);
        assert_eq!(parse_reply(&p), Some(NOW));
    }

    #[test]
    fn answers_that_cannot_be_believed_are_refused() {
        assert_eq!(parse_reply(&reply(NOW, 0x24, 0)), None); // stratum 0: kiss-o'-death
        assert_eq!(parse_reply(&reply(NOW, 0x24, 16)), None); // stratum 16: unsynchronised
        assert_eq!(parse_reply(&reply(NOW, 0xE4, 2)), None); // leap indicator 3
        assert_eq!(parse_reply(&reply(NOW, 0x1B, 2)), None); // mode 3: our own kind of packet
        assert_eq!(parse_reply(&reply(NOW, 0x25, 2)), None); // mode 5: a broadcast
        assert_eq!(parse_reply(&reply(0, 0x24, 2)), None); // the epoch: a server with no time
        assert_eq!(parse_reply(&reply(1_700_000_000, 0x24, 2)), None); // before 2025
        assert_eq!(parse_reply(&[0u8; 47]), None);
        assert_eq!(parse_reply(&[]), None);
    }

    #[test]
    fn the_unix_epoch_offset_is_right() {
        // 1970-01-01 is 70 years after 1900-01-01, 17 of them leap.
        assert_eq!(UNIX_OFFSET, (70 * 365 + 17) * 86_400);
    }
}
