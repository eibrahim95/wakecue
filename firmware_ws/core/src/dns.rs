//! A DNS responder for the lamp's setup network: packets in, packets out, no sockets.
//!
//! A phone that joins `Wakecue-XXXX` has nowhere to resolve names, and the lamp has no internet to
//! forward them to. So it answers every question itself, always with its own address: whatever
//! name the phone's connectivity probe or the owner's browser asks for, the answer is the lamp, and
//! the plain-HTTP request that follows lands on the setup page. That is what makes the phone raise
//! the "sign in to network" prompt without anyone typing an address.
//!
//! This is only the packet logic, so it runs on the PC under test; the firmware owns the UDP
//! socket on port 53 and passes each datagram in.
//!
//! What it answers:
//! - An `A` or `ANY` question in class `IN` gets one `A` record holding the lamp's address, with a
//!   short TTL so the name is forgotten soon after the owner leaves the setup network.
//! - Any other question (`AAAA`, `HTTPS`, `TXT`, another class) gets an empty, successful answer.
//!   An error here would make phones retry or give up; an empty answer makes them fall back to `A`.
//! - Anything that is not a plain query (a response, another opcode, no question or several, a
//!   compressed or truncated question) gets no answer at all.

/// UDP port the server listens on.
pub const PORT: u16 = 53;
/// Room for the longest reply this server writes (a 255-byte name, its question and one record).
pub const REPLY_MAX: usize = 512;

/// Seconds a phone may remember the answer.
const TTL_S: u32 = 30;

const HEADER_LEN: usize = 12;
const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
/// The longest name on the wire, length bytes and the final zero included.
const NAME_MAX: usize = 255;

/// Where a well-formed question ends, and what it asks.
struct Question {
    /// Offset just past the class field.
    end: usize,
    qtype: u16,
    qclass: u16,
}

/// Read the single question of a standard query, or `None` if the packet is anything else.
fn parse(query: &[u8]) -> Option<Question> {
    if query.len() < HEADER_LEN {
        return None;
    }
    // QR must be 0 (a query) and the opcode 0 (standard query); the other flag bits do not matter.
    if query[2] & 0xF8 != 0 {
        return None;
    }
    if u16::from_be_bytes([query[4], query[5]]) != 1 {
        return None;
    }
    let mut at = HEADER_LEN;
    let mut name_len = 1; // the final zero
    loop {
        let len = *query.get(at)? as usize;
        at += 1;
        if len == 0 {
            break;
        }
        // The top two bits are a compression pointer (or a reserved form): a question has no
        // business using either, and following a pointer is how a reader gets walked in circles.
        if len & 0xC0 != 0 {
            return None;
        }
        name_len += 1 + len;
        if name_len > NAME_MAX {
            return None;
        }
        query.get(at..at + len)?;
        at += len;
    }
    let tail = query.get(at..at + 4)?;
    Some(Question {
        end: at + 4,
        qtype: u16::from_be_bytes([tail[0], tail[1]]),
        qclass: u16::from_be_bytes([tail[2], tail[3]]),
    })
}

/// Answer one datagram that arrived on port 53 with the lamp's address `ip`. Returns how many
/// bytes of `out` are the reply, or `None` to say nothing.
pub fn answer(query: &[u8], ip: [u8; 4], out: &mut [u8; REPLY_MAX]) -> Option<usize> {
    let q = parse(query)?;
    let wants_address = q.qclass == CLASS_IN && (q.qtype == TYPE_A || q.qtype == TYPE_ANY);

    out.fill(0);
    out[0..2].copy_from_slice(&query[0..2]); // the ID the client will match the reply by
    // QR, opcode 0, AA, TC clear, and the client's RD echoed back.
    out[2] = 0x80 | 0x04 | (query[2] & 0x01);
    // RA, and RCODE 0: success, even when there is nothing to say.
    out[3] = 0x80;
    out[4..6].copy_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out[6..8].copy_from_slice(&u16::from(wants_address).to_be_bytes()); // ANCOUNT
    // NSCOUNT and ARCOUNT stay 0: the query's EDNS record is not echoed.

    // The question goes back as asked, case and all: some resolvers randomise the case of the
    // name and compare it on the way back.
    let mut n = q.end;
    out[HEADER_LEN..n].copy_from_slice(&query[HEADER_LEN..n]);

    if wants_address {
        out[n..n + 2].copy_from_slice(&[0xC0, 0x0C]); // the name, by pointer to the question
        out[n + 2..n + 4].copy_from_slice(&TYPE_A.to_be_bytes());
        out[n + 4..n + 6].copy_from_slice(&CLASS_IN.to_be_bytes());
        out[n + 6..n + 10].copy_from_slice(&TTL_S.to_be_bytes());
        out[n + 10..n + 12].copy_from_slice(&4u16.to_be_bytes());
        out[n + 12..n + 16].copy_from_slice(&ip);
        n += 16;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAMP: [u8; 4] = [192, 168, 4, 1];

    /// A query: header with the given flags and counts, one question built from `labels`.
    fn query(id: u16, flags: u16, qdcount: u16, labels: &[&[u8]], qtype: u16, qclass: u16) -> ([u8; 600], usize) {
        let mut p = [0u8; 600];
        p[0..2].copy_from_slice(&id.to_be_bytes());
        p[2..4].copy_from_slice(&flags.to_be_bytes());
        p[4..6].copy_from_slice(&qdcount.to_be_bytes());
        let mut n = HEADER_LEN;
        for l in labels {
            p[n] = l.len() as u8;
            p[n + 1..n + 1 + l.len()].copy_from_slice(l);
            n += 1 + l.len();
        }
        p[n] = 0;
        n += 1;
        p[n..n + 2].copy_from_slice(&qtype.to_be_bytes());
        p[n + 2..n + 4].copy_from_slice(&qclass.to_be_bytes());
        (p, n + 4)
    }

    /// The usual shape: recursion desired, one question about `labels`.
    fn ask(labels: &[&[u8]], qtype: u16) -> ([u8; 600], usize) {
        query(0xBEEF, 0x0100, 1, labels, qtype, CLASS_IN)
    }

    fn reply(q: ([u8; 600], usize)) -> Option<([u8; REPLY_MAX], usize)> {
        let mut out = [0u8; REPLY_MAX];
        let n = answer(&q.0[..q.1], LAMP, &mut out)?;
        Some((out, n))
    }

    #[test]
    fn an_a_question_gets_the_lamp_in_one_record() {
        let q = ask(&[b"connectivitycheck", b"gstatic", b"com"], TYPE_A);
        let (out, n) = reply(q).unwrap();
        // 12 header + 1+17 + 1+7 + 1+3 + 1 name + 4 type and class, then 16 for the record.
        assert_eq!(n, q.1 + 16);
        let out = &out[..n];
        assert_eq!(out[0..2], [0xBE, 0xEF]); // ID
        assert_eq!(out[2], 0x85); // QR, AA, RD echoed
        assert_eq!(out[3], 0x80); // RA, NOERROR
        assert_eq!(out[4..12], [0, 1, 0, 1, 0, 0, 0, 0]); // 1 question, 1 answer, no more
        assert_eq!(out[12..q.1], q.0[12..q.1]); // the question, as asked
        assert_eq!(
            out[q.1..],
            [0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 192, 168, 4, 1]
        );
    }

    #[test]
    fn an_any_question_is_answered_the_same_way() {
        let q = ask(&[b"example", b"org"], TYPE_ANY);
        let (out, n) = reply(q).unwrap();
        assert_eq!(n, q.1 + 16);
        assert_eq!(out[4..8], [0, 1, 0, 1]);
        // The record is an A record even though the question was ANY.
        assert_eq!(out[q.1 + 2..q.1 + 4], [0, 1]);
        assert_eq!(out[n - 4..n], LAMP);
    }

    #[test]
    fn other_types_get_an_empty_success_so_clients_fall_back_to_a() {
        for qtype in [28, 65, 16, 15, 2, 12] {
            // AAAA, HTTPS, TXT, MX, NS, PTR
            let q = ask(&[b"example", b"com"], qtype);
            let (out, n) = reply(q).unwrap();
            assert_eq!(n, q.1, "type {qtype}"); // header and question only
            assert_eq!(out[3] & 0x0F, 0, "type {qtype}"); // NOERROR
            assert_eq!(out[4..8], [0, 1, 0, 0], "type {qtype}");
            assert_eq!(out[12..q.1], q.0[12..q.1]);
        }
    }

    #[test]
    fn a_class_other_than_in_is_not_given_an_address() {
        let q = query(1, 0x0100, 1, &[b"version", b"bind"], 16, 3); // TXT CH
        let (out, n) = reply(q).unwrap();
        assert_eq!(n, q.1);
        assert_eq!(out[4..8], [0, 1, 0, 0]);
        let q = query(1, 0x0100, 1, &[b"example", b"com"], TYPE_A, 3); // A CH
        assert_eq!(reply(q).unwrap().1, q.1);
    }

    #[test]
    fn the_name_comes_back_in_the_case_it_was_asked_in() {
        let q = ask(&[b"ExAmPlE", b"CoM"], TYPE_A);
        let (out, n) = reply(q).unwrap();
        assert_eq!(out[12..q.1], q.0[12..q.1]);
        assert_eq!(out[13..20], *b"ExAmPlE");
        assert_eq!(n, q.1 + 16);
    }

    #[test]
    fn the_id_and_the_recursion_bit_are_echoed() {
        for (id, rd) in [(0x0000, 1u16), (0xFFFF, 0), (0x1234, 1), (0x8001, 0)] {
            let q = query(id, rd << 8, 1, &[b"a"], TYPE_A, CLASS_IN);
            let (out, _) = reply(q).unwrap();
            assert_eq!(out[0..2], id.to_be_bytes());
            assert_eq!(out[2] & 0x01, rd as u8);
            assert_eq!(out[2] & 0xFE, 0x84); // QR and AA set, opcode 0, TC clear
        }
    }

    #[test]
    fn the_other_query_flags_are_ignored() {
        // Checking disabled, authentic data, a stray Z bit: still a plain query.
        let q = query(7, 0x0130, 1, &[b"a"], TYPE_A, CLASS_IN);
        assert!(reply(q).is_some());
    }

    #[test]
    fn a_trailing_edns_record_is_neither_an_error_nor_echoed() {
        let (mut p, n) = ask(&[b"example", b"com"], TYPE_A);
        p[10..12].copy_from_slice(&1u16.to_be_bytes()); // ARCOUNT 1
        // OPT: root name, type 41, UDP size 4096, no flags, no data.
        p[n..n + 11].copy_from_slice(&[0, 0, 41, 0x10, 0, 0, 0, 0, 0, 0, 0]);
        let mut out = [0u8; REPLY_MAX];
        let m = answer(&p[..n + 11], LAMP, &mut out).unwrap();
        assert_eq!(m, n + 16);
        assert_eq!(out[10..12], [0, 0]);
    }

    #[test]
    fn a_name_with_no_labels_is_a_question_about_the_root() {
        let q = ask(&[], TYPE_A);
        let (out, n) = reply(q).unwrap();
        assert_eq!(n, q.1 + 16);
        assert_eq!(out[12], 0);
    }

    #[test]
    fn responses_and_other_opcodes_get_no_answer() {
        // A response (QR set), then opcodes 1 (inverse), 2 (status), 4 (notify) and 5 (update).
        for flags in [0x8100u16, 0x0900, 0x1100, 0x2100, 0x2900] {
            let q = query(1, flags, 1, &[b"example", b"com"], TYPE_A, CLASS_IN);
            assert!(reply(q).is_none(), "flags {flags:#06x}");
        }
    }

    #[test]
    fn anything_but_one_question_gets_no_answer() {
        for count in [0, 2, 3, 0xFFFF] {
            let q = query(1, 0x0100, count, &[b"example", b"com"], TYPE_A, CLASS_IN);
            assert!(reply(q).is_none(), "qdcount {count}");
        }
    }

    #[test]
    fn a_compressed_or_reserved_label_in_the_question_gets_no_answer() {
        let (mut p, n) = ask(&[b"example", b"com"], TYPE_A);
        // A pointer back to the header where the first label should be.
        p[12] = 0xC0;
        p[13] = 0x00;
        assert!(answer(&p[..n], LAMP, &mut [0u8; REPLY_MAX]).is_none());
        // A pointer after the first label.
        let (mut p, n) = ask(&[b"example", b"com"], TYPE_A);
        p[20] = 0xC0;
        assert!(answer(&p[..n], LAMP, &mut [0u8; REPLY_MAX]).is_none());
        // The reserved forms 0x40 and 0x80 are not labels either.
        for first in [0x40, 0x80, 0xBF] {
            let (mut p, n) = ask(&[b"example", b"com"], TYPE_A);
            p[12] = first;
            assert!(answer(&p[..n], LAMP, &mut [0u8; REPLY_MAX]).is_none(), "{first:#x}");
        }
    }

    #[test]
    fn a_name_of_the_longest_length_is_answered_and_one_byte_more_is_not() {
        let l63 = [b'a'; 63];
        let l61 = [b'b'; 61];
        let l62 = [b'c'; 62];
        // 3 * 64 + 62 + 1 = 255 bytes on the wire: the most a name may take.
        let q = ask(&[&l63, &l63, &l63, &l61], TYPE_A);
        let (out, n) = reply(q).unwrap();
        assert_eq!(n, q.1 + 16);
        assert!(n <= REPLY_MAX);
        assert_eq!(out[12..q.1], q.0[12..q.1]);
        // One byte longer than that is not a name.
        let q = ask(&[&l63, &l63, &l63, &l62], TYPE_A);
        assert!(reply(q).is_none());
        // A single label of 63 bytes is fine; its 64-byte cousin is a reserved length.
        assert!(reply(ask(&[&l63], TYPE_A)).is_some());
        assert!(reply(ask(&[&[b'a'; 64]], TYPE_A)).is_none());
    }

    #[test]
    fn a_query_cut_anywhere_never_panics_and_only_a_whole_question_is_answered() {
        let q = ask(&[b"connectivitycheck", b"gstatic", b"com"], TYPE_A);
        let mut out = [0u8; REPLY_MAX];
        for cut in 0..=q.1 {
            let r = answer(&q.0[..cut], LAMP, &mut out);
            assert_eq!(r.is_some(), cut == q.1, "cut at {cut}");
        }
        // The same with an EDNS record behind it: anything past the question is optional.
        let (mut p, n) = ask(&[b"a", b"b"], TYPE_ANY);
        p[n..n + 11].copy_from_slice(&[0, 0, 41, 0x10, 0, 0, 0, 0, 0, 0, 0]);
        for cut in 0..=n + 11 {
            let r = answer(&p[..cut], LAMP, &mut out);
            assert_eq!(r.is_some(), cut >= n, "cut at {cut}");
        }
    }

    #[test]
    fn garbage_never_panics() {
        let mut out = [0u8; REPLY_MAX];
        assert!(answer(&[], LAMP, &mut out).is_none());
        assert!(answer(&[0xFF; 11], LAMP, &mut out).is_none());
        // Every byte value in every header position over an otherwise valid query.
        let q = ask(&[b"example", b"com"], TYPE_A);
        for at in 0..q.1 {
            for v in [0x00, 0x01, 0x3F, 0x40, 0x7F, 0x80, 0xC0, 0xFF] {
                let mut p = q.0;
                p[at] = v;
                let _ = answer(&p[..q.1], LAMP, &mut out);
            }
        }
    }

    #[test]
    fn the_reply_never_outgrows_its_buffer() {
        let l63 = [b'a'; 63];
        let l61 = [b'b'; 61];
        let q = ask(&[&l63, &l63, &l63, &l61], TYPE_A);
        let (_, n) = reply(q).unwrap();
        assert_eq!(n, 12 + 255 + 4 + 16);
        assert!(n < REPLY_MAX);
    }
}
