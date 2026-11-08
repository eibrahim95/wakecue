//! Just enough HTTP/1.1 for the lamp's setup page: bytes in, a parsed request out, and the head of
//! a response written into a buffer. No sockets and no allocation, so it runs on the PC under test.
//!
//! The lamp serves one phone at a time on its own network, so the server is as small as it can be:
//! one request per connection, every response says `Connection: close`, and nothing is pipelined.
//! The firmware reads from the TCP socket into a buffer and calls [`parse`] on whatever has arrived
//! so far; `Incomplete` means read more, `Bad` means answer 400 and hang up. It sends a response as
//! [`head`] followed by the body, which keeps the head here and the page's bytes with the page.
//!
//! What it refuses is as deliberate as what it accepts: chunked bodies, other HTTP versions, headers
//! that are not UTF-8 and any line over 512 bytes are `Bad`, because the one client this serves is a
//! phone's browser posting a short form.

/// The longest request line or header line accepted, not counting its CRLF.
pub const MAX_LINE: usize = 512;

/// The methods the setup page tells apart; anything else is `Other` and gets a 405 from the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Other,
}

/// A complete request, borrowed from the buffer it was parsed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request<'a> {
    pub method: Method,
    /// The path without its query string: `/save` for `/save?a=1`.
    pub path: &'a str,
    /// What follows the `?`, or empty when there is none.
    pub query: &'a str,
    /// The `Host` header, for telling a request for the lamp from one a phone aimed at another name.
    pub host: Option<&'a str>,
    /// Exactly `Content-Length` bytes; empty when the header is absent.
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed<'a> {
    /// The head or the body has not all arrived: read more and try again.
    Incomplete,
    /// Not a request this server will answer: reply 400 and close.
    Bad,
    Complete(Request<'a>),
}

/// The statuses the setup page answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Found,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    PayloadTooLarge,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Status::Ok => "200 OK",
            Status::Found => "302 Found",
            Status::BadRequest => "400 Bad Request",
            Status::NotFound => "404 Not Found",
            Status::MethodNotAllowed => "405 Method Not Allowed",
            Status::PayloadTooLarge => "413 Payload Too Large",
        }
    }
}

/// True if any line of `buf` is longer than [`MAX_LINE`]. A last line with no CRLF yet counts as
/// far as it has got: waiting for its end would let a client fill the buffer with one endless line.
fn line_too_long(buf: &[u8]) -> bool {
    let mut start = 0;
    for i in 0..buf.len() {
        if buf[i] == b'\n' && i > start && buf[i - 1] == b'\r' {
            if i - 1 - start > MAX_LINE {
                return true;
            }
            start = i + 1;
        }
    }
    let tail = &buf[start..];
    // A trailing CR may be the first half of the CRLF that ends the line.
    let content = if tail.last() == Some(&b'\r') { tail.len() - 1 } else { tail.len() };
    content > MAX_LINE
}

fn find_blank_line(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Parse a decimal `Content-Length`: digits only, no sign, no spaces, and it must fit a `usize`.
fn content_length(value: &str) -> Option<usize> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Parse what has arrived of one request.
///
/// `Incomplete` until the blank line that ends the head has arrived and, after it, as many body
/// bytes as `Content-Length` promised. Bytes past the body are ignored: there is one request per
/// connection.
pub fn parse(buf: &[u8]) -> Parsed<'_> {
    let end = match find_blank_line(buf) {
        Some(end) => end,
        None => {
            // No end of head yet. A line already too long will not get shorter.
            return if line_too_long(buf) { Parsed::Bad } else { Parsed::Incomplete };
        }
    };
    if line_too_long(&buf[..end + 2]) {
        return Parsed::Bad;
    }
    let Ok(head) = core::str::from_utf8(&buf[..end]) else {
        return Parsed::Bad;
    };

    let mut lines = head.split("\r\n");
    let Some(request_line) = lines.next() else {
        return Parsed::Bad;
    };
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Parsed::Bad;
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Parsed::Bad;
    }
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Parsed::Bad;
    }
    // Only the origin form: a path from the root. A phone talking to the lamp never sends the
    // absolute form (that is for proxies), and `*` is for OPTIONS, which is not served.
    if !target.starts_with('/') || target.bytes().any(|b| b.is_ascii_control()) {
        return Parsed::Bad;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let method = match method {
        "GET" => Method::Get,
        "POST" => Method::Post,
        _ => Method::Other,
    };

    let mut host = None;
    let mut length: Option<usize> = None;
    for line in lines {
        // A bare CR or LF inside a line is a smuggling attempt or a broken client, not a header.
        if line.contains(['\r', '\n']) {
            return Parsed::Bad;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Parsed::Bad;
        };
        // An empty name or a space in it also catches obsolete line folding, which starts with one.
        if name.is_empty() || name.bytes().any(|b| b == b' ' || b == b'\t' || b.is_ascii_control()) {
            return Parsed::Bad;
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("content-length") {
            let Some(n) = content_length(value) else {
                return Parsed::Bad;
            };
            if length.is_some_and(|seen| seen != n) {
                return Parsed::Bad;
            }
            length = Some(n);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Parsed::Bad;
        } else if name.eq_ignore_ascii_case("host") && host.is_none() {
            host = Some(value);
        }
    }

    let body_start = end + 4;
    let wanted = length.unwrap_or(0);
    let Some(body) = body_start
        .checked_add(wanted)
        .and_then(|body_end| buf.get(body_start..body_end))
    else {
        return Parsed::Incomplete;
    };
    Parsed::Complete(Request { method, path, query, host, body })
}

/// A cursor over the response buffer that gives up instead of overflowing.
struct Writer<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl Writer<'_> {
    fn put(&mut self, s: &str) -> Option<()> {
        let end = self.len.checked_add(s.len())?;
        self.out.get_mut(self.len..end)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Some(())
    }

    fn number(&mut self, mut n: usize) -> Option<()> {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.put(core::str::from_utf8(&digits[at..]).ok()?)
    }
}

/// Write the head of a response: status line, headers and the blank line. The caller sends the body
/// itself afterwards, `content_len` bytes of it. Returns the length written, or `None` if `out` is
/// too small, or if `content_type` or `location` holds a CR or LF, which would let a header carry
/// another one.
pub fn head(
    out: &mut [u8],
    status: Status,
    content_type: &str,
    content_len: usize,
    location: Option<&str>,
) -> Option<usize> {
    let clean = |s: &str| !s.contains(['\r', '\n']);
    if !clean(content_type) || !location.is_none_or(clean) {
        return None;
    }
    let mut w = Writer { out, len: 0 };
    w.put("HTTP/1.1 ")?;
    w.put(status.line())?;
    w.put("\r\nContent-Type: ")?;
    w.put(content_type)?;
    w.put("\r\nContent-Length: ")?;
    w.number(content_len)?;
    w.put("\r\nConnection: close\r\nCache-Control: no-store\r\n")?;
    if let Some(location) = location {
        w.put("Location: ")?;
        w.put(location)?;
        w.put("\r\n")?;
    }
    w.put("\r\n")?;
    Some(w.len)
}

/// Whether `path` is one of the addresses phones and laptops fetch to find out if a Wi-Fi network
/// has a login page in the way. Answering these with a redirect to the setup page, instead of the
/// "success" they expect, is what makes the phone offer to open it by itself.
pub fn is_captive_probe(path: &str) -> bool {
    matches!(
        path,
        "/generate_204"
            | "/gen_204"
            | "/hotspot-detect.html"
            | "/library/test/success.html"
            | "/connecttest.txt"
            | "/ncsi.txt"
            | "/redirect"
            | "/success.txt"
            | "/canonical.html"
            | "/check_network_status.txt"
            | "/mobile/status.php"
            | "/fwlink"
    ) || path.starts_with("/fwlink/")
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::format;

    fn complete(buf: &[u8]) -> Request<'_> {
        match parse(buf) {
            Parsed::Complete(r) => r,
            other => panic!("expected a complete request, got {other:?}"),
        }
    }

    #[test]
    fn a_get_without_a_query() {
        let r = complete(b"GET / HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.path, "/");
        assert_eq!(r.query, "");
        assert_eq!(r.host, Some("192.168.4.1"));
        assert!(r.body.is_empty());
    }

    #[test]
    fn a_get_with_a_query_splits_it_off_the_path() {
        let r = complete(b"GET /save?a=1&b=two HTTP/1.1\r\n\r\n");
        assert_eq!((r.path, r.query), ("/save", "a=1&b=two"));
        assert_eq!(r.host, None);
        // A question mark with nothing after it is an empty query, and a second one stays in it.
        let r = complete(b"GET /x? HTTP/1.1\r\n\r\n");
        assert_eq!((r.path, r.query), ("/x", ""));
        let r = complete(b"GET /x?a=?b HTTP/1.0\r\n\r\n");
        assert_eq!((r.path, r.query), ("/x", "a=?b"));
    }

    #[test]
    fn a_post_is_incomplete_until_every_body_byte_has_arrived() {
        let full = b"POST /save HTTP/1.1\r\nHost: lamp\r\nContent-Length: 11\r\n\r\nutc=2&dst=1";
        let head_len = full.len() - 11;
        // Every prefix short of the whole request is Incomplete, never Bad and never Complete.
        for cut in 0..full.len() {
            assert_eq!(parse(&full[..cut]), Parsed::Incomplete, "cut at {cut}");
        }
        assert!(head_len > 0);
        let r = complete(full);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/save");
        assert_eq!(r.body, b"utc=2&dst=1");
    }

    #[test]
    fn bytes_after_the_body_are_ignored() {
        let r = complete(b"POST /a HTTP/1.1\r\nContent-Length: 3\r\n\r\nabcGET /b HTTP/1.1\r\n\r\n");
        assert_eq!(r.body, b"abc");
    }

    #[test]
    fn a_post_without_a_length_has_an_empty_body() {
        let r = complete(b"POST /a HTTP/1.1\r\nHost: x\r\n\r\nignored");
        assert_eq!(r.method, Method::Post);
        assert!(r.body.is_empty());
        let r = complete(b"POST /a HTTP/1.1\r\nContent-Length: 0\r\n\r\n");
        assert!(r.body.is_empty());
    }

    #[test]
    fn header_names_are_case_insensitive_and_values_are_trimmed() {
        let r = complete(b"POST /a HTTP/1.1\r\nHOST:   lamp.local \t\r\ncontent-LENGTH:\t2 \r\n\r\nok");
        assert_eq!(r.host, Some("lamp.local"));
        assert_eq!(r.body, b"ok");
        // The first Host wins, and other headers are skipped without complaint.
        let r = complete(b"GET / HTTP/1.1\r\nUser-Agent: a:b:c\r\nHost: one\r\nHost: two\r\nAccept:\r\n\r\n");
        assert_eq!(r.host, Some("one"));
    }

    #[test]
    fn methods_other_than_get_and_post_are_other() {
        assert_eq!(complete(b"PUT / HTTP/1.1\r\n\r\n").method, Method::Other);
        assert_eq!(complete(b"HEAD / HTTP/1.1\r\n\r\n").method, Method::Other);
        assert_eq!(complete(b"DELETE /x HTTP/1.0\r\n\r\n").method, Method::Other);
    }

    #[test]
    fn a_bad_request_line_is_bad() {
        for line in [
            &b"GET /\r\n\r\n"[..],                       // no version
            b"GET / HTTP/1.1 extra\r\n\r\n",             // a fourth part
            b"GET  / HTTP/1.1\r\n\r\n",                  // two spaces
            b" GET / HTTP/1.1\r\n\r\n",                  // leading space
            b"get / HTTP/1.1\r\n\r\n",                   // lower case method
            b"GET index.html HTTP/1.1\r\n\r\n",          // not from the root
            b"GET http://lamp/ HTTP/1.1\r\n\r\n",        // absolute form
            b"OPTIONS * HTTP/1.1\r\n\r\n",               // the asterisk form
            b"GET /a\x01b HTTP/1.1\r\n\r\n",             // a control character in the target
            b"\r\n\r\n",                                 // nothing at all
            b"HTTP/1.1 200 OK\r\n\r\n",                  // a response, not a request
        ] {
            assert_eq!(parse(line), Parsed::Bad, "{:?}", core::str::from_utf8(line));
        }
    }

    #[test]
    fn other_http_versions_are_bad() {
        for v in ["HTTP/0.9", "HTTP/2", "HTTP/2.0", "HTTP/1.2", "HTTP/1", "http/1.1", "HTTP/1.1x"] {
            let mut buf = [0u8; 64];
            let n = copy(&mut buf, &["GET / ", v, "\r\n\r\n"]);
            assert_eq!(parse(&buf[..n]), Parsed::Bad, "{v}");
        }
        assert!(matches!(parse(b"GET / HTTP/1.0\r\n\r\n"), Parsed::Complete(_)));
    }

    #[test]
    fn a_bad_header_is_bad() {
        for req in [
            &b"GET / HTTP/1.1\r\nno colon here\r\n\r\n"[..],
            b"GET / HTTP/1.1\r\n: empty name\r\n\r\n",
            b"GET / HTTP/1.1\r\nBad Name: x\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: a\r\n folded: x\r\n\r\n", // obsolete line folding
            b"GET / HTTP/1.1\r\nHost: a\nb\r\n\r\n",            // a bare LF inside a line
            b"GET / HTTP/1.1\r\nHost: a\rb\r\n\r\n",            // a bare CR inside a line
            b"GET / HTTP/1.1\r\nHost: \xff\xfe\r\n\r\n",        // not UTF-8
            b"GET /\xff HTTP/1.1\r\n\r\n",                      // not UTF-8 in the path
        ] {
            assert_eq!(parse(req), Parsed::Bad, "{:?}", core::str::from_utf8(req));
        }
    }

    #[test]
    fn a_bad_content_length_is_bad() {
        for v in ["abc", "-1", "+1", "1 2", "0x10", "1.5", "", "99999999999999999999999999"] {
            let mut buf = [0u8; 96];
            let n = copy(&mut buf, &["POST / HTTP/1.1\r\nContent-Length: ", v, "\r\n\r\n"]);
            assert_eq!(parse(&buf[..n]), Parsed::Bad, "{v:?}");
        }
        // Two lengths that disagree are Bad; two that agree are one length.
        let two = b"POST / HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\nabc";
        assert_eq!(parse(two), Parsed::Bad);
        let same = b"POST / HTTP/1.1\r\nContent-Length: 2\r\ncontent-length: 2\r\n\r\nab";
        assert_eq!(complete(same).body, b"ab");
    }

    #[test]
    fn chunked_bodies_are_refused() {
        let req = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        assert_eq!(parse(req), Parsed::Bad);
        let req = b"POST / HTTP/1.1\r\ntransfer-encoding: identity\r\nContent-Length: 1\r\n\r\na";
        assert_eq!(parse(req), Parsed::Bad);
    }

    #[test]
    fn a_line_over_512_bytes_is_bad_and_one_of_exactly_512_is_not() {
        // The request line: "GET /" + filler + " HTTP/1.1" is MAX_LINE bytes at the limit.
        let mut buf = [b'a'; 700];
        let fixed = "GET / HTTP/1.1".len();
        for (extra, ok) in [(MAX_LINE - fixed, true), (MAX_LINE - fixed + 1, false)] {
            let n = copy_with_filler(&mut buf, "GET /", extra, " HTTP/1.1\r\n\r\n");
            assert_eq!(matches!(parse(&buf[..n]), Parsed::Complete(_)), ok, "request line, {extra} extra");
        }
        // A header line.
        let fixed = "X: ".len();
        for (extra, ok) in [(MAX_LINE - fixed, true), (MAX_LINE - fixed + 1, false)] {
            let mut head = [0u8; 700];
            let start = copy(&mut head, &["GET / HTTP/1.1\r\nX: "]);
            let n = start + fill(&mut head[start..], b'v', extra);
            head[n..n + 4].copy_from_slice(b"\r\n\r\n");
            assert_eq!(matches!(parse(&head[..n + 4]), Parsed::Complete(_)), ok, "header, {extra} extra");
        }
        // And one that never ends is Bad before its line break arrives, not Incomplete forever.
        let mut endless = [b'a'; 600];
        endless[..5].copy_from_slice(b"GET /");
        assert_eq!(parse(&endless), Parsed::Bad);
        assert_eq!(parse(&endless[..100]), Parsed::Incomplete);
    }

    #[test]
    fn a_long_body_line_is_not_a_long_header_line() {
        let mut buf = [b'x'; 1100];
        let head = b"POST / HTTP/1.1\r\nContent-Length: 1000\r\n\r\n";
        buf[..head.len()].copy_from_slice(head);
        assert_eq!(complete(&buf[..head.len() + 1000]).body.len(), 1000);
    }

    #[test]
    fn cutting_or_scrambling_the_input_never_panics() {
        let reqs: [&[u8]; 4] = [
            b"POST /save?x=1 HTTP/1.1\r\nHost: lamp\r\nContent-Length: 5\r\n\r\nhello",
            b"GET / HTTP/1.1\r\nHost: a\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n",
            b"\xff\xfe\r\n\r\n\r\n\r\n:\r\n",
        ];
        for req in reqs {
            for cut in 0..=req.len() {
                let _ = parse(&req[..cut]);
            }
        }
        // A cheap pseudo-random stream, with CR and LF over-represented so the line logic is hit.
        let mut state = 0x2545_f491u32;
        let mut noise = [0u8; 200];
        for round in 0..2000 {
            for b in noise.iter_mut() {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *b = match (state >> 24) % 8 {
                    0 => b'\r',
                    1 => b'\n',
                    2 => b':',
                    3 => b' ',
                    _ => (state >> 16) as u8,
                };
            }
            let len = 1 + round % noise.len();
            let _ = parse(&noise[..len]);
        }
    }

    /// Copy the pieces one after another into `buf`, returning how many bytes that took.
    fn copy(buf: &mut [u8], pieces: &[&str]) -> usize {
        let mut n = 0;
        for p in pieces {
            buf[n..n + p.len()].copy_from_slice(p.as_bytes());
            n += p.len();
        }
        n
    }

    fn fill(buf: &mut [u8], byte: u8, count: usize) -> usize {
        buf[..count].fill(byte);
        count
    }

    fn copy_with_filler(buf: &mut [u8], before: &str, filler: usize, after: &str) -> usize {
        let mut n = copy(buf, &[before]);
        n += fill(&mut buf[n..], b'a', filler);
        n + copy(&mut buf[n..], &[after])
    }

    fn head_text(status: Status, ct: &str, len: usize, loc: Option<&str>) -> ([u8; 256], usize) {
        let mut out = [0u8; 256];
        let n = head(&mut out, status, ct, len, loc).expect("fits");
        (out, n)
    }

    #[test]
    fn the_head_of_each_status_is_exact() {
        for (status, line) in [
            (Status::Ok, "200 OK"),
            (Status::Found, "302 Found"),
            (Status::BadRequest, "400 Bad Request"),
            (Status::NotFound, "404 Not Found"),
            (Status::MethodNotAllowed, "405 Method Not Allowed"),
            (Status::PayloadTooLarge, "413 Payload Too Large"),
        ] {
            let (out, n) = head_text(status, "text/html; charset=utf-8", 1234, None);
            let want = format!(
                "HTTP/1.1 {line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 1234\r\n\
                 Connection: close\r\nCache-Control: no-store\r\n\r\n"
            );
            assert_eq!(&out[..n], want.as_bytes(), "{line}");
        }
    }

    #[test]
    fn a_redirect_carries_its_location_before_the_blank_line() {
        let (out, n) = head_text(Status::Found, "text/plain", 0, Some("http://192.168.4.1/"));
        let want = "HTTP/1.1 302 Found\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\
                    Connection: close\r\nCache-Control: no-store\r\nLocation: http://192.168.4.1/\r\n\r\n";
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), want);
    }

    #[test]
    fn content_length_prints_zero_and_the_largest_size() {
        let (out, n) = head_text(Status::Ok, "t", 0, None);
        assert!(core::str::from_utf8(&out[..n]).unwrap().contains("Content-Length: 0\r\n"));
        let mut big = [0u8; 256];
        let n = head(&mut big, Status::Ok, "t", usize::MAX, None).unwrap();
        let want = format!("Content-Length: {}\r\n", usize::MAX);
        assert!(core::str::from_utf8(&big[..n]).unwrap().contains(want.as_str()));
    }

    #[test]
    fn a_buffer_that_is_too_small_gives_none_at_every_size_below_the_head() {
        let (full, n) = head_text(Status::Found, "text/html", 77, Some("/x"));
        for size in 0..n {
            let mut out = [0u8; 256];
            assert_eq!(head(&mut out[..size], Status::Found, "text/html", 77, Some("/x")), None, "{size}");
        }
        let mut exact = [0u8; 256];
        assert_eq!(head(&mut exact[..n], Status::Found, "text/html", 77, Some("/x")), Some(n));
        assert_eq!(&exact[..n], &full[..n]);
    }

    #[test]
    fn a_header_value_cannot_carry_another_header() {
        let mut out = [0u8; 256];
        assert_eq!(head(&mut out, Status::Ok, "text/html\r\nSet-Cookie: x", 0, None), None);
        assert_eq!(head(&mut out, Status::Found, "t", 0, Some("/a\r\nSet-Cookie: x")), None);
        assert_eq!(head(&mut out, Status::Found, "t", 0, Some("/a\nb")), None);
    }

    #[test]
    fn the_probe_addresses_phones_ask_are_recognised() {
        for path in [
            "/generate_204",
            "/gen_204",
            "/hotspot-detect.html",
            "/library/test/success.html",
            "/connecttest.txt",
            "/ncsi.txt",
            "/redirect",
            "/success.txt",
            "/canonical.html",
            "/check_network_status.txt",
            "/mobile/status.php",
            "/fwlink",
            "/fwlink/",
            "/fwlink/?LinkID=219472",
            "/fwlink/anything/else",
        ] {
            assert!(is_captive_probe(path), "{path}");
        }
    }

    #[test]
    fn ordinary_paths_are_not_probes() {
        for path in [
            "/", "/save", "/index.html", "/generate_2045", "/generate_204/x", "/gen_204x", "/redirect/x",
            "/fwlinks", "/fwlin", "/FWLINK", "/Generate_204", "", "generate_204",
        ] {
            assert!(!is_captive_probe(path), "{path}");
        }
    }
}
