//! What the lamp answers when a phone on its setup network asks for a web page.
//!
//! The firmware owns the sockets and the flash; this is only the decision, from a parsed request
//! to the bytes of a reply, so every route can be tried on the PC. It ties the three pure pieces
//! together: [`crate::http`] for the request and the reply head, [`crate::setup_page`] for the
//! page and the form, and [`crate::settings::Settings`] for what the form changes.
//!
//! The routes:
//! - the connectivity checks phones make on joining (`/generate_204`, `/hotspot-detect.html`, ...)
//!   get a redirect to the setup page, which is what makes the phone offer to "sign in";
//! - `GET /` is the setup page;
//! - `POST /save` applies the form, and only when every field is valid does it change the
//!   settings and say so, so the firmware writes flash only when `saved` is true;
//! - anything else is a plain error.

use crate::http::{self, Method, Request, Status};
use crate::settings::Settings;
use crate::setup_page::{self, FormError};

/// Where the redirects point: the lamp's own address on its own network.
pub const HOME: &str = "http://192.168.4.1/";

/// Room for the largest body this module writes (the page is held under 6000 bytes).
pub const BODY_MAX: usize = 6144;
/// Room for a reply head.
pub const HEAD_MAX: usize = 384;

/// A reply written into the caller's two buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// Bytes of the head buffer to send first.
    pub head: usize,
    /// Bytes of the body buffer to send after it.
    pub body: usize,
    /// The form was valid and `settings` now holds it: the caller stores them.
    pub saved: bool,
}

fn plain(
    head: &mut [u8; HEAD_MAX],
    body: &mut [u8; BODY_MAX],
    status: Status,
    text: &str,
    location: Option<&str>,
) -> Reply {
    let text = text.as_bytes();
    body[..text.len()].copy_from_slice(text);
    Reply {
        head: http::head(head, status, "text/plain; charset=utf-8", text.len(), location).unwrap_or(0),
        body: text.len(),
        saved: false,
    }
}

fn html(head: &mut [u8; HEAD_MAX], body: &mut [u8; BODY_MAX], len: Option<usize>) -> Reply {
    // The buffer is sized for the page, so a miss would be a bug; an empty 404 is the honest
    // answer to it rather than half a page.
    let Some(len) = len else {
        return plain(head, body, Status::NotFound, "Not found", None);
    };
    Reply {
        head: http::head(head, Status::Ok, "text/html; charset=utf-8", len, None).unwrap_or(0),
        body: len,
        saved: false,
    }
}

/// Answer one request. `settings` is changed only when the reply says `saved`.
pub fn respond(
    req: &Request,
    settings: &mut Settings,
    head: &mut [u8; HEAD_MAX],
    body: &mut [u8; BODY_MAX],
) -> Reply {
    match (req.method, req.path) {
        (Method::Get, path) if http::is_captive_probe(path) => {
            plain(head, body, Status::Found, "Wakecue setup", Some(HOME))
        }
        (Method::Get, "/") => {
            let len = setup_page::render(settings, body);
            html(head, body, len)
        }
        (Method::Post, "/save") => {
            // The form is applied to a copy by `apply` itself, which leaves it untouched on an
            // error; the place flag is the lamp's own bookkeeping, set only on success.
            match setup_page::apply(req.body, settings) {
                Ok(()) => {
                    settings.setup.place = true;
                    let len = setup_page::render_saved(body);
                    let mut reply = html(head, body, len);
                    reply.saved = true;
                    reply
                }
                Err(e) => {
                    let what = match e {
                        FormError::Place => "the place name",
                        FormError::Lat => "the latitude",
                        FormError::Lon => "the longitude",
                        FormError::Utc | FormError::Dst => "the time zone",
                        FormError::Ssid => "the Wi-Fi name",
                        FormError::Psk => "the Wi-Fi password (8 to 63 characters)",
                        FormError::Malformed => "the form",
                    };
                    let mut text = [0u8; 96];
                    let n = join(&mut text, &["Could not read ", what, ". Go back and check it."]);
                    let text = core::str::from_utf8(&text[..n]).unwrap_or("Could not read the form.");
                    plain(head, body, Status::BadRequest, text, None)
                }
            }
        }
        (Method::Get | Method::Post, _) => plain(head, body, Status::NotFound, "Not found", None),
        (Method::Other, _) => plain(head, body, Status::MethodNotAllowed, "Method not allowed", None),
    }
}

/// Concatenate `parts` into `out` and return the length; what does not fit is dropped.
fn join(out: &mut [u8], parts: &[&str]) -> usize {
    let mut n = 0;
    for p in parts {
        let b = p.as_bytes();
        let take = b.len().min(out.len() - n);
        out[n..n + take].copy_from_slice(&b[..take]);
        n += take;
    }
    n
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::http::Parsed;
    use crate::settings::Dst;
    use std::format;

    /// Run a raw request through the parser and the router.
    fn ask(raw: &str, settings: &mut Settings) -> (Reply, [u8; HEAD_MAX], [u8; BODY_MAX]) {
        let mut head = [0u8; HEAD_MAX];
        let mut body = [0u8; BODY_MAX];
        let Parsed::Complete(req) = http::parse(raw.as_bytes()) else { panic!("did not parse: {raw}") };
        let r = respond(&req, settings, &mut head, &mut body);
        (r, head, body)
    }

    fn text(b: &[u8]) -> &str {
        core::str::from_utf8(b).unwrap()
    }

    fn post(form: &str) -> std::string::String {
        format!(
            "POST /save HTTP/1.1\r\nHost: 192.168.4.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{form}",
            form.len()
        )
    }

    #[test]
    fn the_connectivity_checks_are_sent_to_the_setup_page() {
        let mut s = Settings::default();
        for path in ["/generate_204", "/hotspot-detect.html", "/connecttest.txt", "/ncsi.txt"] {
            let (r, head, _) = ask(&format!("GET {path} HTTP/1.1\r\nHost: example.com\r\n\r\n"), &mut s);
            let head = text(&head[..r.head]);
            assert!(head.starts_with("HTTP/1.1 302"), "{path}: {head}");
            assert!(head.contains("Location: http://192.168.4.1/\r\n"), "{path}: {head}");
            assert!(!r.saved);
        }
    }

    #[test]
    fn the_front_page_is_the_setup_form_filled_from_the_settings() {
        let mut s = Settings::default();
        s.set_place("Cairo");
        // Whatever name the phone typed, the lamp answers: every name resolves to it.
        let (r, head, body) = ask("GET / HTTP/1.1\r\nHost: anything.example\r\n\r\n", &mut s);
        let head = text(&head[..r.head]);
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(head.contains("text/html"));
        assert!(head.contains(&format!("Content-Length: {}\r\n", r.body)), "{head}");
        let page = text(&body[..r.body]);
        assert!(page.contains("Wakecue setup"));
        assert!(page.contains("Cairo"));
        assert!(page.contains("action=/save"));
        assert!(!r.saved);
    }

    #[test]
    fn a_valid_form_changes_the_settings_and_says_so() {
        let mut s = Settings::default();
        let (r, head, body) = ask(&post("place=London&lat=51.5074&lon=-0.1278&utc=0&dst=europe"), &mut s);
        assert!(r.saved);
        assert!(text(&head[..r.head]).starts_with("HTTP/1.1 200"));
        assert!(text(&body[..r.body]).contains("Saved"));
        assert_eq!(s.place_name(), "London");
        assert_eq!((s.lat_e2, s.lon_e2), (5151, -13));
        assert_eq!((s.utc_offset_min, s.dst), (0, Dst::Europe));
        // The lamp's own bookkeeping: the place was chosen by the owner now.
        assert!(s.setup.place);
        // And the Wi-Fi flag is the firmware's to set, once the network has really been joined.
        assert!(!s.setup.wifi);
    }

    #[test]
    fn a_bad_form_changes_nothing_and_is_refused() {
        let mut s = Settings::default();
        let before = s;
        for form in [
            "lat=95",
            "lon=abc",
            "utc=9999",
            "dst=mars",
            "place=%FF",
            "lat=1&lon=999",
            "psk=short",
        ] {
            let (r, head, body) = ask(&post(form), &mut s);
            assert!(!r.saved, "{form}");
            assert!(text(&head[..r.head]).starts_with("HTTP/1.1 400"), "{form}");
            assert!(text(&body[..r.body]).starts_with("Could not read"), "{form}");
            assert_eq!(s, before, "{form}");
        }
    }

    #[test]
    fn the_wifi_name_and_password_are_stored_and_the_password_is_never_sent_back() {
        let mut s = Settings::default();
        let (r, _, _) = ask(&post("ssid=Home+Net&psk=correct+horse"), &mut s);
        assert!(r.saved);
        assert_eq!((s.ssid(), s.psk()), ("Home Net", "correct horse"));
        // The network is open: nothing the lamp serves may carry the saved password.
        let (r, _, body) = ask("GET / HTTP/1.1\r\nHost: x\r\n\r\n", &mut s);
        let page = text(&body[..r.body]);
        assert!(page.contains("Home Net"));
        assert!(!page.contains("correct horse"));
        // An empty password field keeps the saved one.
        ask(&post("ssid=Home+Net&psk="), &mut s);
        assert_eq!(s.psk(), "correct horse");
    }

    #[test]
    fn everything_else_is_an_error() {
        let mut s = Settings::default();
        let (r, head, _) = ask("GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n", &mut s);
        assert!(text(&head[..r.head]).starts_with("HTTP/1.1 404"));
        let (r, head, _) = ask("GET /save HTTP/1.1\r\nHost: x\r\n\r\n", &mut s);
        assert!(text(&head[..r.head]).starts_with("HTTP/1.1 404"));
        let (r, head, _) = ask("POST /elsewhere HTTP/1.1\r\nContent-Length: 0\r\n\r\n", &mut s);
        assert!(text(&head[..r.head]).starts_with("HTTP/1.1 404"));
        let (r, head, _) = ask("DELETE / HTTP/1.1\r\nHost: x\r\n\r\n", &mut s);
        assert!(text(&head[..r.head]).starts_with("HTTP/1.1 405"));
        assert!(!r.saved);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn what_the_form_wrote_is_what_the_page_shows_next_time() {
        let mut s = Settings::default();
        ask(&post("place=Kolkata&lat=22.5726&lon=88.3639&utc=330&dst=none"), &mut s);
        let (r, _, body) = ask("GET / HTTP/1.1\r\nHost: x\r\n\r\n", &mut s);
        let page = text(&body[..r.body]);
        assert!(page.contains("Kolkata"));
        assert!(page.contains("22.57"));
    }
}
