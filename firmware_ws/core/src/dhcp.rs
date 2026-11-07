//! A DHCP server small enough for the lamp's setup network: packets in, packets out, no sockets.
//!
//! The lamp raises its own Wi-Fi network for the first boot and has to hand the phone an address
//! on it, because nothing else is there to do so. This is only the packet logic, so it runs on the
//! PC under test; the firmware owns the UDP socket on port 67 and passes each datagram in.
//!
//! It serves one /24 and a handful of clients: the pool is `first_host ..= first_host + 7` in the
//! server's own last octet, one address per MAC, kept in RAM. A restart forgets the leases, and a
//! client that comes back asking for its old address is given it if nobody else holds it.
//! It answers DISCOVER with OFFER and REQUEST with ACK or NAK; everything else is ignored.

/// UDP port the server listens on.
pub const SERVER_PORT: u16 = 67;
/// UDP port the clients listen on, and where every reply goes (as a broadcast: the client has no
/// address yet, and the reply has to reach it anyway).
pub const CLIENT_PORT: u16 = 68;
/// Room for the longest reply this server writes.
pub const REPLY_MAX: usize = 300;

/// Clients the pool holds.
pub const SLOTS: usize = 8;

const FIXED_LEN: usize = 240;
const COOKIE: [u8; 4] = [99, 130, 83, 99];

const OPT_PAD: u8 = 0;
const OPT_NETMASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_RENEWAL: u8 = 58;
const OPT_REBINDING: u8 = 59;
const OPT_END: u8 = 255;

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;

/// What the server answered, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Offer,
    Ack,
    Nak,
}

/// A reply written into the caller's buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// How many bytes of the buffer are the datagram.
    pub len: usize,
    pub kind: Kind,
    /// The address on offer or granted; all zeros for a refusal.
    pub ip: [u8; 4],
    /// The client's hardware address, for the log.
    pub mac: [u8; 6],
}

pub struct Server {
    /// The lamp's own address: also the router and the DNS server it announces.
    server: [u8; 4],
    first_host: u8,
    lease_s: u32,
    macs: [Option<[u8; 6]>; SLOTS],
    /// Where the next client goes once every slot is taken, so a full pool recycles the oldest.
    next: usize,
}

/// The fields of a request this server reads.
struct Request {
    msg_type: u8,
    xid: [u8; 4],
    flags: [u8; 2],
    ciaddr: [u8; 4],
    mac: [u8; 6],
    requested: Option<[u8; 4]>,
    server_id: Option<[u8; 4]>,
}

fn parse(req: &[u8]) -> Option<Request> {
    if req.len() < FIXED_LEN || req[0] != 1 || req[1] != 1 || req[2] != 6 || req[236..240] != COOKIE {
        return None;
    }
    let mut r = Request {
        msg_type: 0,
        xid: req[4..8].try_into().ok()?,
        flags: req[10..12].try_into().ok()?,
        ciaddr: req[12..16].try_into().ok()?,
        mac: req[28..34].try_into().ok()?,
        requested: None,
        server_id: None,
    };
    let mut at = FIXED_LEN;
    while at < req.len() {
        let tag = req[at];
        at += 1;
        match tag {
            OPT_PAD => continue,
            OPT_END => break,
            _ => {}
        }
        let len = *req.get(at)? as usize;
        let data = req.get(at + 1..at + 1 + len)?;
        match (tag, len) {
            (OPT_MSG_TYPE, 1) => r.msg_type = data[0],
            (OPT_REQUESTED_IP, 4) => r.requested = data.try_into().ok(),
            (OPT_SERVER_ID, 4) => r.server_id = data.try_into().ok(),
            _ => {}
        }
        at += 1 + len;
    }
    Some(r)
}

impl Server {
    /// A server at `server` handing out `server`'s first three octets plus `first_host ..`, with
    /// leases of `lease_s` seconds.
    pub const fn new(server: [u8; 4], first_host: u8, lease_s: u32) -> Server {
        Server { server, first_host, lease_s, macs: [None; SLOTS], next: 0 }
    }

    fn ip_of(&self, slot: usize) -> [u8; 4] {
        [self.server[0], self.server[1], self.server[2], self.first_host + slot as u8]
    }

    /// The slot an address belongs to, if it is in the pool at all.
    fn slot_of(&self, ip: [u8; 4]) -> Option<usize> {
        if ip[..3] != self.server[..3] {
            return None;
        }
        let slot = ip[3].checked_sub(self.first_host)? as usize;
        (slot < SLOTS).then_some(slot)
    }

    /// The slot this client already holds, else a free one, else the oldest recycled.
    fn slot_for(&mut self, mac: [u8; 6]) -> usize {
        if let Some(i) = self.macs.iter().position(|m| *m == Some(mac)) {
            return i;
        }
        let i = match self.macs.iter().position(|m| m.is_none()) {
            Some(free) => free,
            None => {
                let taken = self.next;
                self.next = (self.next + 1) % SLOTS;
                taken
            }
        };
        self.macs[i] = Some(mac);
        i
    }

    /// Answer one datagram that arrived on port 67. `None` means say nothing: not a request this
    /// server understands, or one addressed to another server.
    pub fn handle(&mut self, req: &[u8], out: &mut [u8; REPLY_MAX]) -> Option<Reply> {
        let r = parse(req)?;
        match r.msg_type {
            DISCOVER => {
                let slot = self.slot_for(r.mac);
                let ip = self.ip_of(slot);
                Some(self.write(out, &r, Kind::Offer, ip))
            }
            REQUEST => {
                // A client that picked another server's offer is telling this one to stand down.
                if r.server_id.is_some_and(|s| s != self.server) {
                    return None;
                }
                // Selecting names the address in option 50; renewing carries it in ciaddr.
                let want = r.requested.or((r.ciaddr != [0; 4]).then_some(r.ciaddr))?;
                let grant = self.slot_of(want).filter(|&i| match self.macs[i] {
                    Some(owner) => owner == r.mac,
                    // Nobody holds it, and the client asked for it by name: this is a client
                    // coming back after the lamp restarted and forgot its leases.
                    None => true,
                });
                match grant {
                    Some(i) => {
                        self.macs[i] = Some(r.mac);
                        Some(self.write(out, &r, Kind::Ack, want))
                    }
                    None => Some(self.write(out, &r, Kind::Nak, [0; 4])),
                }
            }
            _ => None,
        }
    }

    fn write(&self, out: &mut [u8; REPLY_MAX], r: &Request, kind: Kind, ip: [u8; 4]) -> Reply {
        out.fill(0);
        out[0] = 2; // BOOTREPLY
        out[1] = 1; // Ethernet
        out[2] = 6;
        out[4..8].copy_from_slice(&r.xid);
        out[10..12].copy_from_slice(&r.flags);
        out[16..20].copy_from_slice(&ip); // yiaddr
        if kind != Kind::Nak {
            out[20..24].copy_from_slice(&self.server); // siaddr
        }
        out[28..34].copy_from_slice(&r.mac);
        out[236..240].copy_from_slice(&COOKIE);

        let mut n = FIXED_LEN;
        let mut put = |tag: u8, data: &[u8]| {
            out[n] = tag;
            out[n + 1] = data.len() as u8;
            out[n + 2..n + 2 + data.len()].copy_from_slice(data);
            n += 2 + data.len();
        };
        put(
            OPT_MSG_TYPE,
            &[match kind {
                Kind::Offer => OFFER,
                Kind::Ack => ACK,
                Kind::Nak => NAK,
            }],
        );
        put(OPT_SERVER_ID, &self.server);
        if kind != Kind::Nak {
            put(OPT_LEASE, &self.lease_s.to_be_bytes());
            put(OPT_RENEWAL, &(self.lease_s / 2).to_be_bytes());
            put(OPT_REBINDING, &(self.lease_s / 8 * 7).to_be_bytes());
            put(OPT_NETMASK, &[255, 255, 255, 0]);
            put(OPT_ROUTER, &self.server);
            put(OPT_DNS, &self.server);
        }
        out[n] = OPT_END;
        Reply { len: n + 1, kind, ip, mac: r.mac }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAMP: [u8; 4] = [192, 168, 4, 1];
    const PHONE: [u8; 6] = [0xaa, 0xbb, 0xcc, 0x00, 0x00, 0x01];
    const LAPTOP: [u8; 6] = [0xaa, 0xbb, 0xcc, 0x00, 0x00, 0x02];

    fn server() -> Server {
        Server::new(LAMP, 2, 3600)
    }

    /// A client packet: the fixed header, the cookie and the options given.
    fn client(mac: [u8; 6], xid: u32, opts: &[(u8, &[u8])]) -> ([u8; 400], usize) {
        let mut p = [0u8; 400];
        p[0] = 1;
        p[1] = 1;
        p[2] = 6;
        p[4..8].copy_from_slice(&xid.to_be_bytes());
        p[10] = 0x80; // broadcast flag
        p[28..34].copy_from_slice(&mac);
        p[236..240].copy_from_slice(&COOKIE);
        let mut n = FIXED_LEN;
        for (tag, data) in opts {
            p[n] = *tag;
            p[n + 1] = data.len() as u8;
            p[n + 2..n + 2 + data.len()].copy_from_slice(data);
            n += 2 + data.len();
        }
        p[n] = OPT_END;
        (p, n + 1)
    }

    fn discover(mac: [u8; 6]) -> ([u8; 400], usize) {
        client(mac, 0x1234_5678, &[(OPT_MSG_TYPE, &[DISCOVER])])
    }

    fn request(mac: [u8; 6], ip: [u8; 4], server: Option<[u8; 4]>) -> ([u8; 400], usize) {
        match server {
            Some(s) => client(
                mac,
                0x1234_5678,
                &[(OPT_MSG_TYPE, &[REQUEST]), (OPT_REQUESTED_IP, &ip), (OPT_SERVER_ID, &s)],
            ),
            None => client(mac, 0x1234_5678, &[(OPT_MSG_TYPE, &[REQUEST]), (OPT_REQUESTED_IP, &ip)]),
        }
    }

    fn ask(s: &mut Server, (p, n): ([u8; 400], usize)) -> Option<(Reply, [u8; REPLY_MAX])> {
        let mut out = [0u8; REPLY_MAX];
        let r = s.handle(&p[..n], &mut out)?;
        Some((r, out))
    }

    /// The value of option `tag` in a reply.
    fn option(out: &[u8], tag: u8) -> Option<&[u8]> {
        let mut at = FIXED_LEN;
        while at < out.len() && out[at] != OPT_END {
            let len = out[at + 1] as usize;
            if out[at] == tag {
                return Some(&out[at + 2..at + 2 + len]);
            }
            at += 2 + len;
        }
        None
    }

    #[test]
    fn a_discover_gets_an_offer_from_the_pool() {
        let mut s = server();
        let (r, out) = ask(&mut s, discover(PHONE)).unwrap();
        assert_eq!(r.kind, Kind::Offer);
        assert_eq!(r.ip, [192, 168, 4, 2]);
        assert_eq!(r.mac, PHONE);
        let out = &out[..r.len];
        // A reply to the same transaction, for the same client, on the same broadcast flag.
        assert_eq!(out[0], 2);
        assert_eq!(out[4..8], 0x1234_5678u32.to_be_bytes());
        assert_eq!(out[10], 0x80);
        assert_eq!(out[16..20], [192, 168, 4, 2]);
        assert_eq!(out[20..24], LAMP);
        assert_eq!(out[28..34], PHONE);
        assert_eq!(out[236..240], COOKIE);
        assert_eq!(option(out, OPT_MSG_TYPE), Some(&[OFFER][..]));
        assert_eq!(option(out, OPT_SERVER_ID), Some(&LAMP[..]));
        assert_eq!(option(out, OPT_LEASE), Some(&3600u32.to_be_bytes()[..]));
        assert_eq!(option(out, OPT_RENEWAL), Some(&1800u32.to_be_bytes()[..]));
        assert_eq!(option(out, OPT_REBINDING), Some(&3150u32.to_be_bytes()[..]));
        assert_eq!(option(out, OPT_NETMASK), Some(&[255, 255, 255, 0][..]));
        assert_eq!(option(out, OPT_ROUTER), Some(&LAMP[..]));
        assert_eq!(option(out, OPT_DNS), Some(&LAMP[..]));
        assert_eq!(out[r.len - 1], OPT_END);
    }

    #[test]
    fn the_same_client_keeps_its_address_and_a_second_one_gets_the_next() {
        let mut s = server();
        let first = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        let other = ask(&mut s, discover(LAPTOP)).unwrap().0.ip;
        let again = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        assert_eq!(first, [192, 168, 4, 2]);
        assert_eq!(other, [192, 168, 4, 3]);
        assert_eq!(again, first);
    }

    #[test]
    fn a_request_for_the_offered_address_is_acknowledged() {
        let mut s = server();
        let offered = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        let (r, out) = ask(&mut s, request(PHONE, offered, Some(LAMP))).unwrap();
        assert_eq!((r.kind, r.ip), (Kind::Ack, offered));
        assert_eq!(option(&out[..r.len], OPT_MSG_TYPE), Some(&[ACK][..]));
        assert_eq!(option(&out[..r.len], OPT_LEASE), Some(&3600u32.to_be_bytes()[..]));
    }

    #[test]
    fn a_renewal_names_its_address_in_ciaddr() {
        let mut s = server();
        let ip = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        let (mut p, n) = client(PHONE, 7, &[(OPT_MSG_TYPE, &[REQUEST])]);
        p[12..16].copy_from_slice(&ip);
        let (r, _) = ask(&mut s, (p, n)).unwrap();
        assert_eq!((r.kind, r.ip), (Kind::Ack, ip));
    }

    #[test]
    fn an_address_someone_else_holds_is_refused() {
        let mut s = server();
        let phone_ip = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        let (r, out) = ask(&mut s, request(LAPTOP, phone_ip, None)).unwrap();
        assert_eq!((r.kind, r.ip), (Kind::Nak, [0; 4]));
        let out = &out[..r.len];
        assert_eq!(option(out, OPT_MSG_TYPE), Some(&[NAK][..]));
        assert_eq!(option(out, OPT_SERVER_ID), Some(&LAMP[..]));
        assert_eq!(option(out, OPT_LEASE), None);
    }

    #[test]
    fn an_address_outside_the_pool_is_refused() {
        let mut s = server();
        for ip in [[192, 168, 4, 1], [192, 168, 4, 10], [192, 168, 5, 2], [10, 0, 0, 5]] {
            let (r, _) = ask(&mut s, request(PHONE, ip, None)).unwrap();
            assert_eq!(r.kind, Kind::Nak, "{ip:?}");
        }
    }

    #[test]
    fn a_client_coming_back_after_a_restart_gets_its_old_address_if_free() {
        let mut s = server();
        let (r, _) = ask(&mut s, request(PHONE, [192, 168, 4, 5], None)).unwrap();
        assert_eq!((r.kind, r.ip), (Kind::Ack, [192, 168, 4, 5]));
        // Now it is held: another client cannot take it.
        let (r, _) = ask(&mut s, request(LAPTOP, [192, 168, 4, 5], None)).unwrap();
        assert_eq!(r.kind, Kind::Nak);
    }

    #[test]
    fn a_client_that_chose_another_server_is_left_alone() {
        let mut s = server();
        let ip = ask(&mut s, discover(PHONE)).unwrap().0.ip;
        assert!(ask(&mut s, request(PHONE, ip, Some([192, 168, 4, 99]))).is_none());
    }

    #[test]
    fn a_full_pool_recycles_the_oldest_client() {
        let mut s = server();
        for i in 0..SLOTS as u8 {
            let mac = [0xaa, 0, 0, 0, 0, i];
            assert_eq!(ask(&mut s, discover(mac)).unwrap().0.ip, [192, 168, 4, 2 + i]);
        }
        // The ninth takes the first one's address instead of failing.
        let ninth = ask(&mut s, discover([0xbb, 0, 0, 0, 0, 9])).unwrap().0.ip;
        assert_eq!(ninth, [192, 168, 4, 2]);
        // And the one it displaced is no longer entitled to it.
        let (r, _) = ask(&mut s, request([0xaa, 0, 0, 0, 0, 0], [192, 168, 4, 2], None)).unwrap();
        assert_eq!(r.kind, Kind::Nak);
    }

    #[test]
    fn things_that_are_not_requests_get_no_answer() {
        let mut s = server();
        let mut out = [0u8; REPLY_MAX];
        // Too short, a reply rather than a request, the wrong cookie, a type nobody asked about.
        assert!(s.handle(&[0u8; 20], &mut out).is_none());
        let (mut p, n) = discover(PHONE);
        p[0] = 2;
        assert!(s.handle(&p[..n], &mut out).is_none());
        let (mut p, n) = discover(PHONE);
        p[236] = 0;
        assert!(s.handle(&p[..n], &mut out).is_none());
        let (p, n) = client(PHONE, 1, &[(OPT_MSG_TYPE, &[7])]); // RELEASE
        assert!(s.handle(&p[..n], &mut out).is_none());
        let (p, n) = client(PHONE, 1, &[]); // no message type at all
        assert!(s.handle(&p[..n], &mut out).is_none());
    }

    #[test]
    fn a_truncated_option_list_does_not_panic() {
        let mut s = server();
        let mut out = [0u8; REPLY_MAX];
        let (p, n) = discover(PHONE);
        for cut in FIXED_LEN..n {
            let _ = s.handle(&p[..cut], &mut out);
        }
        // An option that claims more bytes than the packet has.
        let mut q = [0u8; 400];
        q[..FIXED_LEN].copy_from_slice(&p[..FIXED_LEN]);
        q[FIXED_LEN] = OPT_MSG_TYPE;
        q[FIXED_LEN + 1] = 200;
        assert!(s.handle(&q[..FIXED_LEN + 4], &mut out).is_none());
    }

    #[test]
    fn padding_between_options_is_skipped() {
        let mut s = server();
        let mut out = [0u8; REPLY_MAX];
        let (mut p, _) = discover(PHONE);
        p[FIXED_LEN] = OPT_PAD;
        p[FIXED_LEN + 1] = OPT_MSG_TYPE;
        p[FIXED_LEN + 2] = 1;
        p[FIXED_LEN + 3] = DISCOVER;
        p[FIXED_LEN + 4] = OPT_END;
        assert_eq!(s.handle(&p[..FIXED_LEN + 5], &mut out).unwrap().kind, Kind::Offer);
    }
}
