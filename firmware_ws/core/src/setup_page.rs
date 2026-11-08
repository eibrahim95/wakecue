//! The setup page: its HTML, the form it posts, and how that form changes the settings.
//!
//! The lamp raises its own Wi-Fi network and a phone joins it ([`crate::dhcp`]); this is the one
//! page the phone then opens. It sets what the lamp cannot know about itself, the time zone, the
//! place and the home Wi-Fi network, and nothing else. Everything here is pure: the page is written into the
//! caller's buffer, and a posted form is applied to a [`Settings`] value, so the whole of it runs
//! on the PC under test. The socket and the flash write belong to the firmware.
//!
//! The lamp's own network is open, so the page never shows the stored Wi-Fi password: the password
//! box is always empty, and leaving it empty keeps the saved one.
//!
//! The page is self-contained (one inline style, one inline script, no external asset) because the
//! network it is served on has no internet behind it. The only thing the script does is fill the
//! time-zone box from the phone's own clock when the owner taps the button: it never changes
//! anything by itself.

use crate::settings::{Dst, Settings};

/// What was wrong with a posted form. The first problem found wins; nothing is applied then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    /// Not valid urlencoding: a bad `%` escape, or a value that is not UTF-8.
    Malformed,
    /// The place name is too long for the block (15 bytes) or has control characters in it.
    Place,
    /// Not a latitude between -90 and 90.
    Lat,
    /// Not a longitude between -180 and 180.
    Lon,
    /// Not a whole number of minutes between -720 and 840.
    Utc,
    /// Not one of the summer-time rules the lamp knows.
    Dst,
    /// The Wi-Fi network name is longer than 32 bytes or has control characters in it.
    Ssid,
    /// The Wi-Fi password is not 8 to 63 bytes, or has control characters in it.
    Psk,
}

/// The widest decoded value any field can have; a longer one is that field's error.
const VALUE_MAX: usize = 80;
/// The longest field name worth decoding. A longer name is some other field and is ignored.
const KEY_MAX: usize = 16;

/// Decode one urlencoded token (`+` is a space, `%XX` a byte) into `out`. `Ok(None)` means it
/// decodes to more than `out` holds.
fn decode<'a>(raw: &[u8], out: &'a mut [u8]) -> Result<Option<&'a [u8]>, FormError> {
    let mut n = 0;
    let mut i = 0;
    while i < raw.len() {
        let byte = match raw[i] {
            b'+' => b' ',
            b'%' => {
                let hi = raw.get(i + 1).and_then(|c| (*c as char).to_digit(16));
                let lo = raw.get(i + 2).and_then(|c| (*c as char).to_digit(16));
                let (Some(hi), Some(lo)) = (hi, lo) else {
                    return Err(FormError::Malformed);
                };
                i += 2;
                (hi * 16 + lo) as u8
            }
            other => other,
        };
        i += 1;
        if n == out.len() {
            return Ok(None);
        }
        out[n] = byte;
        n += 1;
    }
    Ok(Some(&out[..n]))
}

/// The value as text with the spaces at either end removed; a value that is not UTF-8 is malformed.
fn text(bytes: &[u8]) -> Result<&str, FormError> {
    core::str::from_utf8(bytes).map(str::trim).map_err(|_| FormError::Malformed)
}

/// A decimal number of degrees as the block's hundredths: `"30.0444"` is 3004.
///
/// Rounds half away from zero on the third decimal digit, which is the same as asking whether the
/// part past the hundredths is at least half a hundredth: 31.2357 is 3124, -0.1278 is -13. Digits
/// past the third change nothing, so any number of them is fine. A comma may stand for the point.
/// Returns `None` for anything that is not a plain signed decimal, or that is more than ten
/// digits long.
fn hundredths(text: &str) -> Option<i32> {
    let t = text.as_bytes();
    let (negative, digits) = match t.first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let mut whole: i32 = 0;
    let mut frac = [0u8; 3];
    let mut seen_point = false;
    let mut count = 0;
    let mut any = false;
    for &c in digits {
        match c {
            b'0'..=b'9' => {
                any = true;
                if seen_point {
                    if count < 3 {
                        frac[count] = c - b'0';
                    }
                    count += 1;
                } else {
                    whole = whole.checked_mul(10)?.checked_add((c - b'0') as i32)?;
                }
            }
            b'.' | b',' if !seen_point => seen_point = true,
            _ => return None,
        }
        if whole > 1_000_000 {
            return None;
        }
    }
    if !any {
        return None;
    }
    let mut value = whole * 100 + frac[0] as i32 * 10 + frac[1] as i32;
    if frac[2] >= 5 {
        value += 1;
    }
    Some(if negative { -value } else { value })
}

/// A whole number of minutes with an optional sign.
fn minutes(text: &str) -> Option<i16> {
    let t = text.as_bytes();
    let (negative, digits) = match t.first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || digits.len() > 5 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = digits.iter().fold(0i32, |a, c| a * 10 + (c - b'0') as i32);
    i16::try_from(if negative { -v } else { v }).ok()
}

/// The values a form carries, checked but not yet applied.
#[derive(Default)]
struct Staged {
    place: Option<([u8; VALUE_MAX], usize)>,
    lat: Option<i16>,
    lon: Option<i16>,
    utc: Option<i16>,
    dst: Option<Dst>,
    ssid: Option<([u8; VALUE_MAX], usize)>,
    psk: Option<([u8; VALUE_MAX], usize)>,
}

/// Apply a posted `application/x-www-form-urlencoded` body to `s`.
///
/// A field that is absent leaves its setting alone, so a form with only `utc` changes only the
/// zone; a field that is present and wrong is an error. Unknown fields are ignored, and when a
/// field comes twice the last one counts. All or nothing: on any error `s` is exactly as it was.
///
/// - `place`: at most 15 bytes of UTF-8 once the spaces at both ends are gone, no control
///   characters. An empty name is allowed and clears it.
/// - `lat`, `lon`: decimal degrees, rounded to the block's hundredths (see [`hundredths`] for the
///   rule) and then checked against -90..=90 and -180..=180, so 90.004 is fine and 90.005 is not.
/// - `utc`: whole minutes from UTC, -720..=840, any value in the range and not only quarter hours.
/// - `dst`: `none`, `egypt`, `europe` or `us`.
/// - `ssid`: at most 32 bytes, no control characters, and kept exactly as typed, spaces included,
///   because a network name may have them. Empty is allowed and clears the stored network.
/// - `psk`: absent or empty keeps the saved password. Otherwise 8 to 63 bytes (what WPA2 takes),
///   no control characters, kept exactly as typed.
pub fn apply(body: &[u8], s: &mut Settings) -> Result<(), FormError> {
    let mut staged = Staged::default();
    for pair in body.split(|b| *b == b'&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_key, raw_value) = match pair.iter().position(|b| *b == b'=') {
            Some(at) => (&pair[..at], &pair[at + 1..]),
            None => (pair, &pair[pair.len()..]),
        };
        let mut key_buf = [0u8; KEY_MAX];
        let Some(key) = decode(raw_key, &mut key_buf)? else { continue };
        // Only a field the lamp knows is decoded, so a long or odd field it ignores cannot fail
        // the whole form.
        let field = match key {
            b"place" | b"lat" | b"lon" | b"utc" | b"dst" | b"ssid" | b"psk" => key,
            _ => continue,
        };
        let error = match field {
            b"place" => FormError::Place,
            b"lat" => FormError::Lat,
            b"lon" => FormError::Lon,
            b"utc" => FormError::Utc,
            b"ssid" => FormError::Ssid,
            b"psk" => FormError::Psk,
            _ => FormError::Dst,
        };
        let mut value_buf = [0u8; VALUE_MAX];
        let Some(value) = decode(raw_value, &mut value_buf)? else { return Err(error) };
        // The Wi-Fi fields are kept as typed; everything else loses the spaces at either end.
        let value_text = if matches!(field, b"ssid" | b"psk") {
            core::str::from_utf8(value).map_err(|_| FormError::Malformed)?
        } else {
            text(value)?
        };
        match field {
            b"place" => {
                if value_text.len() > 15 || value_text.chars().any(char::is_control) {
                    return Err(error);
                }
                let mut name = [0u8; VALUE_MAX];
                name[..value_text.len()].copy_from_slice(value_text.as_bytes());
                staged.place = Some((name, value_text.len()));
            }
            b"lat" => {
                let v = hundredths(value_text).filter(|v| (-9000..=9000).contains(v));
                staged.lat = Some(v.ok_or(error)? as i16);
            }
            b"lon" => {
                let v = hundredths(value_text).filter(|v| (-18000..=18000).contains(v));
                staged.lon = Some(v.ok_or(error)? as i16);
            }
            b"utc" => {
                let v = minutes(value_text).filter(|v| (-720..=840).contains(v));
                staged.utc = Some(v.ok_or(error)?);
            }
            b"ssid" => {
                if value_text.len() > 32 || value_text.chars().any(char::is_control) {
                    return Err(error);
                }
                let mut name = [0u8; VALUE_MAX];
                name[..value_text.len()].copy_from_slice(value_text.as_bytes());
                staged.ssid = Some((name, value_text.len()));
            }
            b"psk" => {
                // An empty box is "keep what is saved", and clears an earlier copy of the field.
                staged.psk = None;
                if !value_text.is_empty() {
                    if !(8..=63).contains(&value_text.len()) || value_text.chars().any(char::is_control) {
                        return Err(error);
                    }
                    let mut word = [0u8; VALUE_MAX];
                    word[..value_text.len()].copy_from_slice(value_text.as_bytes());
                    staged.psk = Some((word, value_text.len()));
                }
            }
            _ => staged.dst = Some(Dst::from_name(value_text).ok_or(error)?),
        }
    }

    // Everything checked: now, and only now, the settings change.
    let mut next = *s;
    if let Some((name, len)) = staged.place {
        let ok = core::str::from_utf8(&name[..len]).is_ok_and(|n| next.set_place(n));
        if !ok {
            return Err(FormError::Place);
        }
    }
    if let Some((name, len)) = staged.ssid {
        let ok = core::str::from_utf8(&name[..len]).is_ok_and(|n| next.set_ssid(n));
        if !ok {
            return Err(FormError::Ssid);
        }
    }
    if let Some((word, len)) = staged.psk {
        let ok = core::str::from_utf8(&word[..len]).is_ok_and(|w| next.set_psk(w));
        if !ok {
            return Err(FormError::Psk);
        }
    }
    if let Some(v) = staged.lat {
        next.lat_e2 = v;
    }
    if let Some(v) = staged.lon {
        next.lon_e2 = v;
    }
    if let Some(v) = staged.utc {
        next.utc_offset_min = v;
    }
    if let Some(v) = staged.dst {
        next.dst = v;
    }
    *s = next;
    Ok(())
}

/// A buffer that is written to and remembers whether it ran out.
struct Page<'a> {
    out: &'a mut [u8],
    n: usize,
    fits: bool,
}

impl<'a> Page<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Page { out, n: 0, fits: true }
    }

    fn raw(&mut self, s: &str) {
        let b = s.as_bytes();
        match self.out.get_mut(self.n..self.n + b.len()) {
            Some(room) => {
                room.copy_from_slice(b);
                self.n += b.len();
            }
            None => self.fits = false,
        }
    }

    /// `s` as HTML text or as the inside of a quoted attribute.
    fn escaped(&mut self, s: &str) {
        for c in s.chars() {
            let mut one = [0u8; 4];
            self.raw(match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&#39;",
                _ => c.encode_utf8(&mut one),
            });
        }
    }

    fn number(&mut self, v: i32) {
        let mut buf = [0u8; 11];
        let mut at = buf.len();
        let mut rest = v.unsigned_abs();
        loop {
            at -= 1;
            buf[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        if v < 0 {
            self.raw("-");
        }
        self.raw(core::str::from_utf8(&buf[at..]).unwrap_or(""));
    }

    /// Degrees from the block's hundredths, always with two decimals: `-0.13`, `30.04`.
    fn degrees(&mut self, e2: i16) {
        let a = (e2 as i32).abs();
        if e2 < 0 {
            self.raw("-");
        }
        self.number(a / 100);
        self.raw(".");
        self.number(a / 10 % 10);
        self.number(a % 10);
    }

    /// `UTC+5:30`.
    fn offset_label(&mut self, min: i32) {
        self.raw(if min < 0 { "UTC-" } else { "UTC+" });
        let a = min.abs();
        self.number(a / 60);
        self.raw(":");
        self.number(a / 10 % 6);
        self.number(a % 10);
    }

    fn option(&mut self, min: i32, selected: bool) {
        self.raw("<option value=");
        self.number(min);
        if selected {
            self.raw(" selected");
        }
        self.raw(">");
        self.offset_label(min);
    }

    fn done(self) -> Option<usize> {
        self.fits.then_some(self.n)
    }
}

/// Shared by both pages: light and dark from the phone's own setting, 16 px gutters, controls
/// that are easy to hit with a thumb.
const HEAD: &str = "<!doctype html><html lang=en><meta charset=utf-8>\
<meta name=viewport content=\"width=device-width,initial-scale=1\">\
<meta name=color-scheme content=\"light dark\"><title>Wakecue setup</title><style>\
:root{--b:#fff;--f:#111;--l:#bbb;--a:#0a6a4a}\
@media(prefers-color-scheme:dark){:root{--b:#141414;--f:#eee;--l:#555;--a:#2a9d73}}\
body{margin:0;padding:16px;background:var(--b);color:var(--f);font:16px/1.4 system-ui,sans-serif}\
main{max-width:30rem;margin:auto}h1{font-size:1.4rem}label{display:block;margin:1rem 0 .25rem}\
input,select,button{box-sizing:border-box;width:100%;min-height:48px;padding:8px 12px;font:inherit;\
color:inherit;background:var(--b);border:1px solid var(--l);border-radius:8px}\
button{margin-top:1rem;background:var(--a);color:#fff;border:0;font-weight:600}\
button.s{background:none;color:var(--f);border:1px solid var(--l);font-weight:400}\
small{display:block;margin-top:.25rem;opacity:.7}\
</style><body><main><h1>Wakecue setup</h1>";

/// Write the setup page for the settings `s` into `out` and return its length, or `None` if `out`
/// is too small. Every field is filled with what the lamp has now, so saving without touching
/// anything changes nothing.
pub fn render(s: &Settings, out: &mut [u8]) -> Option<usize> {
    let mut p = Page::new(out);
    p.raw(HEAD);
    p.raw("<form method=post action=/save><label for=place>Place name</label>\
<input id=place name=place maxlength=15 autocomplete=off value=\"");
    p.escaped(s.place_name());
    p.raw("\"><label for=lat>Latitude, in degrees</label>\
<input id=lat name=lat inputmode=decimal autocomplete=off value=\"");
    p.degrees(s.lat_e2);
    p.raw("\"><label for=lon>Longitude, in degrees</label>\
<input id=lon name=lon inputmode=decimal autocomplete=off value=\"");
    p.degrees(s.lon_e2);
    p.raw("\"><label for=utc>Time zone</label><select id=utc name=utc>");
    // A value from the settings tool that is not a quarter hour still has to show and survive a
    // save, so it gets an option of its own at the top.
    let off = s.utc_offset_min as i32;
    if off % 15 != 0 {
        p.option(off, true);
    }
    for min in (-720..=840).step_by(15) {
        p.option(min, min == off);
    }
    p.raw("</select><button type=button class=s onclick=\"utc.value=-new Date().getTimezoneOffset()\">\
Use my phone's offset</button><label for=dst>Summer time</label><select id=dst name=dst>");
    const LABELS: [&str; 4] = ["No summer time", "Egypt", "Europe and UK", "US and Canada"];
    for (name, label) in Dst::NAMES.iter().zip(LABELS) {
        p.raw("<option value=");
        p.raw(name);
        if *name == s.dst.name() {
            p.raw(" selected");
        }
        p.raw(">");
        p.raw(label);
    }
    p.raw("</select><label for=ssid>Home Wi-Fi network</label>\
<input id=ssid name=ssid maxlength=32 autocapitalize=off autocorrect=off spellcheck=false \
autocomplete=off value=\"");
    p.escaped(s.ssid());
    // The password is never written into the page: the network the page is served on is open, so
    // anybody who joins it could read whatever the page held.
    p.raw("\"><label for=psk>Wi-Fi password</label>\
<input id=psk name=psk type=password maxlength=63 autocapitalize=off autocorrect=off \
spellcheck=false autocomplete=new-password><small>Leave empty to keep the saved password.</small>\
<button>Save</button></form>");
    p.done()
}

/// The page shown after a save.
pub fn render_saved(out: &mut [u8]) -> Option<usize> {
    let mut p = Page::new(out);
    p.raw(HEAD);
    p.raw("<p>Saved. Press Back on the lamp: it will follow this zone and place, join your Wi-Fi and set its clock.</p>");
    p.done()
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn page(s: &Settings) -> String {
        let mut buf = [0u8; 8000];
        let n = render(s, &mut buf).expect("the page fits");
        String::from_utf8(buf[..n].to_vec()).expect("the page is UTF-8")
    }

    fn london() -> Settings {
        let mut s = Settings::default();
        s.set_place("London");
        s.lat_e2 = 5151;
        s.lon_e2 = -13;
        s.utc_offset_min = 0;
        s.dst = Dst::Europe;
        s
    }

    fn kolkata() -> Settings {
        let mut s = Settings::default();
        s.set_place("Kolkata");
        s.lat_e2 = 2257;
        s.lon_e2 = 8836;
        s.utc_offset_min = 330;
        s.dst = Dst::None;
        s
    }

    fn applied(body: &str, from: Settings) -> Result<Settings, FormError> {
        let mut s = from;
        apply(body.as_bytes(), &mut s).map(|()| s)
    }

    #[test]
    fn the_cairo_defaults_are_filled_in() {
        let html = page(&Settings::default());
        assert!(html.contains("<title>Wakecue setup</title>"));
        assert!(html.contains("<form method=post action=/save>"));
        assert!(html.contains("name=place maxlength=15 autocomplete=off value=\"Cairo\""));
        assert!(html.contains("name=lat inputmode=decimal autocomplete=off value=\"30.04\""));
        assert!(html.contains("name=lon inputmode=decimal autocomplete=off value=\"31.24\""));
        assert!(html.contains("<option value=120 selected>UTC+2:00"));
        assert!(html.contains("<option value=egypt selected>Egypt"));
        // Exactly one choice is made in each box.
        assert_eq!(html.matches(" selected").count(), 2);
    }

    #[test]
    fn london_kolkata_and_a_western_zone_select_their_own_options() {
        let html = page(&london());
        assert!(html.contains("value=\"London\""));
        assert!(html.contains("value=\"51.51\""));
        assert!(html.contains("value=\"-0.13\""));
        assert!(html.contains("<option value=0 selected>UTC+0:00"));
        assert!(html.contains("<option value=europe selected>Europe and UK"));

        let html = page(&kolkata());
        assert!(html.contains("<option value=330 selected>UTC+5:30"));
        assert!(html.contains("<option value=none selected>No summer time"));

        let mut s = Settings::default();
        s.utc_offset_min = -210;
        s.dst = Dst::Us;
        let html = page(&s);
        assert!(html.contains("<option value=-210 selected>UTC-3:30"));
        assert!(html.contains("<option value=us selected>US and Canada"));
        assert_eq!(html.matches(" selected").count(), 2);
    }

    #[test]
    fn the_zone_list_runs_from_minus_twelve_to_plus_fourteen_in_quarter_hours() {
        let html = page(&Settings::default());
        assert_eq!(html.matches("<option value=").count(), 105 + 4);
        assert!(html.contains("<option value=-720>UTC-12:00"));
        assert!(html.contains("<option value=-15>UTC-0:15"));
        assert!(html.contains("<option value=345>UTC+5:45"));
        assert!(html.contains("<option value=840>UTC+14:00"));
    }

    #[test]
    fn an_offset_that_is_not_a_quarter_hour_still_shows_and_survives_a_save() {
        let mut s = Settings::default();
        s.utc_offset_min = 125;
        let html = page(&s);
        assert!(html.contains("<option value=125 selected>UTC+2:05"));
        assert_eq!(html.matches(" selected").count(), 2);
        let mut buf = [0u8; 8000];
        let n = render(&s, &mut buf).unwrap();
        // The list is the 105 quarter hours plus the odd one.
        assert_eq!(String::from_utf8_lossy(&buf[..n]).matches("<option value=").count(), 106 + 4);
    }

    #[test]
    fn the_place_name_is_escaped() {
        let mut s = Settings::default();
        assert!(s.set_place("<b>\"A&B'"));
        let html = page(&s);
        assert!(html.contains("value=\"&lt;b&gt;&quot;A&amp;B&#39;\""));
        assert!(!html.contains("<b>"));
    }

    #[test]
    fn the_worst_case_page_fits_in_six_thousand_bytes() {
        // Fifteen bytes of the character that grows most, and an offset off the quarter hour.
        let mut s = Settings::default();
        assert!(s.set_place(&"\"".repeat(15)));
        assert!(s.set_ssid(&"\"".repeat(32)));
        assert!(s.set_psk(&"x".repeat(63)));
        s.utc_offset_min = -725;
        s.dst = Dst::Us;
        let mut buf = [0u8; 6000];
        let n = render(&s, &mut buf).expect("fits in 6000");
        assert!(n < 6000, "{n} bytes");
        let mut buf = [0u8; 6000];
        let n = render_saved(&mut buf).expect("fits");
        assert!(n < 1500, "{n} bytes");
    }

    #[test]
    fn a_buffer_too_small_gives_none_at_every_size_below_the_page() {
        let s = london();
        let mut big = [0u8; 8000];
        let full = render(&s, &mut big).unwrap();
        for size in [0, 1, 100, full / 2, full - 1] {
            let mut small = std::vec![0u8; size];
            assert_eq!(render(&s, &mut small), None, "size {size}");
        }
        let mut exact = std::vec![0u8; full];
        assert_eq!(render(&s, &mut exact), Some(full));
        let mut tiny = [0u8; 10];
        assert_eq!(render_saved(&mut tiny), None);
    }

    #[test]
    fn the_saved_page_says_so_and_carries_no_form() {
        let mut buf = [0u8; 2000];
        let n = render_saved(&mut buf).unwrap();
        let html = String::from_utf8(buf[..n].to_vec()).unwrap();
        assert!(html.contains("<title>Wakecue setup</title>"));
        assert!(html.contains("Saved. Press Back on the lamp"));
        assert!(!html.contains("<form"));
    }

    #[test]
    fn the_phone_offset_is_filled_only_on_a_tap() {
        let html = page(&Settings::default());
        // The script is the button's own handler, not code that runs when the page loads.
        assert!(html.contains("onclick=\"utc.value=-new Date().getTimezoneOffset()\""));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn a_whole_form_for_cairo_london_and_kolkata() {
        let s = applied("place=Cairo&lat=30.0444&lon=31.2357&utc=120&dst=egypt", Settings::default())
            .unwrap();
        assert_eq!((s.lat_e2, s.lon_e2, s.utc_offset_min, s.dst), (3004, 3124, 120, Dst::Egypt));

        let s = applied(
            "place=London&lat=51.5074&lon=-0.1278&utc=0&dst=europe",
            Settings::default(),
        )
        .unwrap();
        assert_eq!(s.place_name(), "London");
        assert_eq!((s.lat_e2, s.lon_e2, s.utc_offset_min, s.dst), (5151, -13, 0, Dst::Europe));

        let s = applied("place=Kolkata&lat=22.5726&lon=88.3639&utc=330&dst=none", Settings::default())
            .unwrap();
        assert_eq!((s.lat_e2, s.lon_e2, s.utc_offset_min, s.dst), (2257, 8836, 330, Dst::None));
    }

    #[test]
    fn percent_escapes_and_plus_signs_decode_in_the_place_name() {
        let s = applied("place=Nasr+City", Settings::default()).unwrap();
        assert_eq!(s.place_name(), "Nasr City");
        let s = applied("place=M%C3%BCnchen", Settings::default()).unwrap();
        assert_eq!(s.place_name(), "München");
        let s = applied("place=%41%62%2b%2B", Settings::default()).unwrap();
        assert_eq!(s.place_name(), "Ab++");
        // Spaces at either end go; the name inside keeps its own.
        let s = applied("place=+++Abu+Dhabi++", Settings::default()).unwrap();
        assert_eq!(s.place_name(), "Abu Dhabi");
    }

    #[test]
    fn degrees_round_to_hundredths_half_away_from_zero() {
        let lat = |text: &str| applied(&format!("lat={text}"), Settings::default()).map(|s| s.lat_e2);
        assert_eq!(lat("30.0444"), Ok(3004));
        assert_eq!(lat("31.2357"), Ok(3124)); // .2357: the third digit is 5, so up
        assert_eq!(lat("51.5074"), Ok(5151));
        assert_eq!(lat("30.004999"), Ok(3000)); // third digit 4: down, whatever follows
        assert_eq!(lat("30.005"), Ok(3001));
        assert_eq!(lat("-0.1278"), Ok(-13));
        assert_eq!(lat("-0.004"), Ok(0));
        assert_eq!(lat("-0.005"), Ok(-1));
        assert_eq!(lat("31"), Ok(3100));
        assert_eq!(lat(".5"), Ok(50));
        assert_eq!(lat("5."), Ok(500));
        assert_eq!(lat("+12.5"), Ok(1250));
        assert_eq!(lat("30,0444"), Ok(3004)); // a comma for the point
        assert_eq!(lat("%2030.04%20"), Ok(3004)); // a space at either end
        assert_eq!(lat("30.04+"), Ok(3004));
        assert_eq!(lat("12.123456789012345678"), Ok(1212));
    }

    #[test]
    fn the_range_is_checked_after_rounding() {
        let lat = |text: &str| applied(&format!("lat={text}"), Settings::default()).map(|s| s.lat_e2);
        assert_eq!(lat("90"), Ok(9000));
        assert_eq!(lat("90.004"), Ok(9000));
        assert_eq!(lat("90.005"), Err(FormError::Lat));
        assert_eq!(lat("-90"), Ok(-9000));
        assert_eq!(lat("-90.01"), Err(FormError::Lat));
        let lon = |text: &str| applied(&format!("lon={text}"), Settings::default()).map(|s| s.lon_e2);
        assert_eq!(lon("180"), Ok(18000));
        assert_eq!(lon("-180"), Ok(-18000));
        assert_eq!(lon("180.01"), Err(FormError::Lon));
        assert_eq!(lon("-180.006"), Err(FormError::Lon));
    }

    #[test]
    fn each_field_has_its_own_error() {
        let one = |body: &str| applied(body, Settings::default()).map(|_| ());
        for bad in ["", "abc", "-", "+", ".", "1.2.3", "1e2", "0x10", "--1", "1 2", "99999999999"] {
            assert_eq!(one(&format!("lat={bad}")), Err(FormError::Lat), "lat {bad:?}");
            assert_eq!(one(&format!("lon={bad}")), Err(FormError::Lon), "lon {bad:?}");
        }
        for bad in ["", "abc", "1.5", "841", "-721", "100000", "99999999", "+", "1 2"] {
            assert_eq!(one(&format!("utc={bad}")), Err(FormError::Utc), "utc {bad:?}");
        }
        for bad in ["", "Egypt", "summer", "europe2", "1"] {
            assert_eq!(one(&format!("dst={bad}")), Err(FormError::Dst), "dst {bad:?}");
        }
        assert_eq!(one("place=0123456789abcdef"), Err(FormError::Place)); // sixteen bytes
        assert_eq!(one("place=M%C3%BCnchen%C3%BC%C3%BC%C3%BC%C3%BC"), Err(FormError::Place)); // 16 bytes
        assert_eq!(one("place=M%C3%BCnchen%C3%BC%C3%BC%C3%BC"), Ok(())); // 14 bytes
        assert_eq!(one("place=A%00B"), Err(FormError::Place)); // a NUL would cut the name short
        assert_eq!(one("place=A%0AB"), Err(FormError::Place));
        assert_eq!(one(&format!("place={}", "a".repeat(200))), Err(FormError::Place));
        assert_eq!(one(&format!("utc={}", "1".repeat(200))), Err(FormError::Utc));
        // The limits themselves are fine.
        assert_eq!(one("place=0123456789abcde&utc=840&dst=us"), Ok(()));
        assert_eq!(one("utc=-720"), Ok(()));
        assert_eq!(one("utc=%2B330"), Ok(()));
        // Not a quarter hour, but in range, which the block accepts.
        assert_eq!(one("utc=125"), Ok(()));
    }

    #[test]
    fn broken_encoding_is_malformed() {
        let one = |body: &[u8]| {
            let mut s = Settings::default();
            apply(body, &mut s)
        };
        assert_eq!(one(b"place=%"), Err(FormError::Malformed));
        assert_eq!(one(b"place=%4"), Err(FormError::Malformed));
        assert_eq!(one(b"place=%zz"), Err(FormError::Malformed));
        assert_eq!(one(b"place=%G1"), Err(FormError::Malformed));
        assert_eq!(one(b"place=%FF"), Err(FormError::Malformed)); // not UTF-8
        assert_eq!(one(b"place=%C3"), Err(FormError::Malformed)); // half a character
        assert_eq!(one(b"place=\xff\xfe"), Err(FormError::Malformed));
        assert_eq!(one(b"lat=%FF"), Err(FormError::Malformed));
        assert_eq!(one(b"%zz=1"), Err(FormError::Malformed)); // a bad escape in a name
        // A field the lamp does not know is not decoded, so it cannot spoil the rest.
        assert_eq!(one(b"other=%zz&utc=60"), Ok(()));
        assert_eq!(one(b"a_name_much_longer_than_any_field=%zz"), Ok(()));
    }

    #[test]
    fn an_error_leaves_every_setting_untouched() {
        let before = kolkata();
        // The good fields come first and last, around the bad one.
        for body in [
            "place=Paris&lat=48.85&utc=60&dst=europe&lon=999",
            "place=Paris&lon=2.35&lat=nope&utc=60&dst=europe",
            "place=Paris&lat=48.85&lon=2.35&utc=9999&dst=europe",
            "place=Paris&lat=48.85&lon=2.35&utc=60&dst=winter",
            "lat=48.85&lon=2.35&utc=60&dst=europe&place=0123456789abcdef",
            "place=Paris&lat=48.85&lon=2.35&utc=60&dst=europe&x=%zz&place=%zz",
            "ssid=Cafe&psk=goodpassword&utc=60&lat=nope",
            "ssid=Cafe&psk=short&utc=60",
            "ssid=0123456789012345678901234567890123&psk=goodpassword&utc=60",
            "ssid=Cafe&psk=goodpassword%00&utc=60",
            "ssid=Ca%0Afe&psk=goodpassword&utc=60",
            "place=Paris&ssid=Cafe&psk=goodpassword&dst=winter",
        ] {
            let mut s = before;
            assert!(apply(body.as_bytes(), &mut s).is_err(), "{body}");
            assert_eq!(s, before, "{body}");
        }
    }

    #[test]
    fn a_partial_form_changes_only_what_it_carries() {
        let from = kolkata();
        let s = applied("utc=0&dst=europe", from).unwrap();
        assert_eq!((s.utc_offset_min, s.dst), (0, Dst::Europe));
        assert_eq!((s.lat_e2, s.lon_e2, s.place_name()), (2257, 8836, "Kolkata"));

        let s = applied("place=Pune", from).unwrap();
        assert_eq!(s.place_name(), "Pune");
        assert_eq!((s.lat_e2, s.lon_e2, s.utc_offset_min, s.dst), (2257, 8836, 330, Dst::None));

        let s = applied("lon=73.86", from).unwrap();
        assert_eq!((s.lon_e2, s.lat_e2), (7386, 2257));

        // An empty body, a lone ampersand and an unknown field change nothing.
        for body in ["", "&", "&&", "x=1", "x"] {
            assert_eq!(applied(body, from).unwrap(), from, "{body:?}");
        }
        // An empty place clears the name.
        assert_eq!(applied("place=", from).unwrap().place_name(), "");
        // Only the settings this page owns can change.
        let s = applied("place=A&lat=1&lon=2&utc=3&dst=none", from).unwrap();
        assert_eq!((s.alarms, s.prayers_on, s.offset_min), (from.alarms, from.prayers_on, from.offset_min));
        assert_eq!((s.setup, s.ssid, s.psk), (from.setup, from.ssid, from.psk));
    }

    #[test]
    fn the_last_copy_of_a_field_counts() {
        let s = applied("utc=60&utc=180&dst=us&dst=none", Settings::default()).unwrap();
        assert_eq!((s.utc_offset_min, s.dst), (180, Dst::None));
        // A bad first copy is still an error, even when a good one follows.
        assert_eq!(applied("utc=abc&utc=60", Settings::default()), Err(FormError::Utc));
    }

    #[test]
    fn a_name_without_an_equals_sign_is_an_empty_value() {
        assert_eq!(applied("lat", Settings::default()), Err(FormError::Lat));
        assert_eq!(applied("place", kolkata()).unwrap().place_name(), "");
    }

    /// The `value="..."` of the field called `name`, and the `selected` option of a select.
    fn field(html: &str, name: &str) -> String {
        let at = html.find(&format!("name={name} ")).expect(name);
        let from = html[at..].find("value=\"").unwrap() + at + 7;
        let to = html[from..].find('"').unwrap() + from;
        String::from(&html[from..to])
    }

    fn chosen(html: &str, select: &str) -> String {
        let at = html.find(&format!("name={select}>")).expect(select);
        let from = html[at..].find("<option value=").unwrap() + at;
        let end = html[from..].find("</select>").unwrap() + from;
        let list = &html[from..end];
        let sel = list.find(" selected").unwrap();
        let start = list[..sel].rfind("<option value=").unwrap() + 14;
        String::from(&list[start..sel])
    }

    /// Fill the form in the way a browser does and send it back.
    fn submit(s: &Settings) -> String {
        let html = page(s);
        let mut body = String::new();
        for name in ["place", "lat", "lon", "ssid"] {
            let value = field(&html, name)
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'");
            body.push_str(&format!("{name}={}&", encode(&value)));
        }
        // The password box comes back empty, as a browser would send it untouched.
        body.push_str(&format!("psk=&utc={}&dst={}", chosen(&html, "utc"), chosen(&html, "dst")));
        body
    }

    fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b == b'.' || b == b'-' {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    #[test]
    fn saving_the_page_as_it_was_given_changes_nothing() {
        let mut odd_name = Settings::default();
        odd_name.set_place("<R&D> 'x' \"y\"");
        let mut odd_zone = Settings::default();
        odd_zone.utc_offset_min = 125;
        let mut gap = Settings::default();
        gap.lat_e2 = -13;
        gap.lon_e2 = 0;
        gap.utc_offset_min = -210;
        gap.dst = Dst::Us;
        let mut accents = Settings::default();
        accents.set_place("München");
        let mut wifi = Settings::default();
        assert!(wifi.set_ssid("<Home> & \"Net\" 'x' "));
        assert!(wifi.set_psk("hunter22 xyz"));
        let mut wifi_accents = Settings::default();
        assert!(wifi_accents.set_ssid("café ☕"));
        assert!(wifi_accents.set_psk("pässword1"));
        for s in [
            Settings::default(),
            london(),
            kolkata(),
            odd_name,
            odd_zone,
            gap,
            accents,
            wifi,
            wifi_accents,
        ] {
            let body = submit(&s);
            assert_eq!(applied(&body, s), Ok(s), "{body}");
        }
    }

    fn wifi() -> Settings {
        let mut s = kolkata();
        assert!(s.set_ssid("Home Net"));
        assert!(s.set_psk("hunter22xyz"));
        s
    }

    #[test]
    fn the_page_shows_the_stored_network_but_never_the_stored_password() {
        let html = page(&wifi());
        assert!(html.contains(
            "name=ssid maxlength=32 autocapitalize=off autocorrect=off spellcheck=false \
autocomplete=off value=\"Home Net\""
        ));
        // The stored password is nowhere in the page, in any form.
        assert!(!html.contains("hunter22xyz"));
        assert!(!html.contains("hunter"));
        assert!(html.contains("name=psk type=password"));
        assert!(html.contains("autocomplete=new-password"));
        assert!(html.contains("<small>Leave empty to keep the saved password.</small>"));
        // The password input has no value attribute at all.
        let at = html.find("name=psk ").unwrap();
        let end = html[at..].find('>').unwrap() + at;
        assert!(!html[at..end].contains("value"));
        // Nor does the page for a network with no password carry anything under that name.
        let mut open = wifi();
        assert!(open.set_psk(""));
        assert!(!page(&open).contains("hunter"));
        assert!(page(&Settings::default()).contains("name=ssid maxlength=32"));
    }

    #[test]
    fn the_stored_network_name_is_escaped() {
        let mut s = Settings::default();
        assert!(s.set_ssid("<i>\"A&B'"));
        let html = page(&s);
        assert!(html.contains("value=\"&lt;i&gt;&quot;A&amp;B&#39;\""));
        assert!(!html.contains("<i>"));
    }

    #[test]
    fn the_wifi_fields_apply() {
        let s = applied("ssid=Home+Net&psk=hunter22xyz", Settings::default()).unwrap();
        assert_eq!((s.ssid(), s.psk()), ("Home Net", "hunter22xyz"));
        // Everything else is where it was.
        let d = Settings::default();
        assert_eq!((s.place_name(), s.lat_e2, s.utc_offset_min, s.dst), (d.place_name(), d.lat_e2, 120, d.dst));
        // Escapes and plus signs decode in both; the spaces are the user's.
        let s = applied("ssid=%20caf%C3%A9+%E2%98%95%20&psk=p%C3%A4ss+word%2B1", Settings::default()).unwrap();
        assert_eq!(s.ssid(), " café ☕ ");
        assert_eq!(s.psk(), "päss word+1");
        // With the rest of the form.
        let s = applied(
            "place=London&lat=51.5074&lon=-0.1278&utc=0&dst=europe&ssid=Cafe&psk=correct+horse",
            Settings::default(),
        )
        .unwrap();
        assert_eq!((s.place_name(), s.ssid(), s.psk(), s.utc_offset_min), ("London", "Cafe", "correct horse", 0));
    }

    #[test]
    fn an_absent_or_empty_password_keeps_the_saved_one() {
        let s = applied("ssid=Other", wifi()).unwrap();
        assert_eq!((s.ssid(), s.psk()), ("Other", "hunter22xyz"));
        let s = applied("ssid=Other&psk=", wifi()).unwrap();
        assert_eq!((s.ssid(), s.psk()), ("Other", "hunter22xyz"));
        let s = applied("psk=&utc=60", wifi()).unwrap();
        assert_eq!((s.ssid(), s.psk(), s.utc_offset_min), ("Home Net", "hunter22xyz", 60));
        assert_eq!(applied("psk", wifi()).unwrap(), wifi());
        // A password alone changes only the password.
        let s = applied("psk=anotherone", wifi()).unwrap();
        assert_eq!((s.ssid(), s.psk()), ("Home Net", "anotherone"));
        // The last copy counts, and an empty last copy means keep.
        let s = applied("psk=firstpassword&psk=", wifi()).unwrap();
        assert_eq!(s.psk(), "hunter22xyz");
        let s = applied("psk=&psk=secondpass", wifi()).unwrap();
        assert_eq!(s.psk(), "secondpass");
    }

    #[test]
    fn an_empty_network_name_clears_the_network_and_an_absent_one_leaves_it() {
        let s = applied("ssid=", wifi()).unwrap();
        assert_eq!(s.ssid(), "");
        assert_eq!(s.psk(), "hunter22xyz"); // only the name was asked about
        assert_eq!(applied("utc=60", wifi()).unwrap().ssid(), "Home Net");
        assert_eq!(applied("ssid", wifi()).unwrap().ssid(), "");
    }

    #[test]
    fn the_password_has_to_be_eight_to_sixty_three_bytes() {
        let psk = |p: &str| applied(&format!("psk={p}"), wifi()).map(|s| String::from(s.psk()));
        assert_eq!(psk("1234567"), Err(FormError::Psk));
        assert_eq!(psk("12345678"), Ok(String::from("12345678")));
        assert_eq!(psk(&"a".repeat(63)), Ok("a".repeat(63)));
        assert_eq!(psk(&"a".repeat(64)), Err(FormError::Psk));
        assert_eq!(psk(&"a".repeat(500)), Err(FormError::Psk)); // longer than any buffer
        assert_eq!(psk(&"%41".repeat(500)), Err(FormError::Psk));
        // Bytes, not characters: 32 two-byte letters are 64 bytes, 4 of them only 8.
        assert_eq!(psk(&"%C3%A4".repeat(32)), Err(FormError::Psk));
        assert_eq!(psk(&"%C3%A4".repeat(4)), Ok("ä".repeat(4)));
        assert_eq!(psk(&"%C3%A4".repeat(3)), Err(FormError::Psk));
        // Spaces count, and are kept.
        assert_eq!(psk("++++++++"), Ok(String::from("        ")));
        assert_eq!(psk("abc%00defgh"), Err(FormError::Psk));
        assert_eq!(psk("abc%0Adefgh"), Err(FormError::Psk));
        assert_eq!(psk("abc%09defgh"), Err(FormError::Psk));
        assert_eq!(psk("abc%7Fdefgh"), Err(FormError::Psk));
        assert_eq!(psk("%FFabcdefgh"), Err(FormError::Malformed));
    }

    #[test]
    fn the_network_name_has_to_fit_in_thirty_two_bytes() {
        let ssid = |n: &str| applied(&format!("ssid={n}"), wifi()).map(|s| String::from(s.ssid()));
        assert_eq!(ssid(&"a".repeat(32)), Ok("a".repeat(32)));
        assert_eq!(ssid(&"a".repeat(33)), Err(FormError::Ssid));
        assert_eq!(ssid(&"a".repeat(500)), Err(FormError::Ssid));
        assert_eq!(ssid(&"%C3%A9".repeat(16)), Ok("é".repeat(16))); // 32 bytes
        assert_eq!(ssid(&"%C3%A9".repeat(17)), Err(FormError::Ssid)); // 34 bytes
        assert_eq!(ssid("a%00b"), Err(FormError::Ssid));
        assert_eq!(ssid("a%0Db"), Err(FormError::Ssid));
        assert_eq!(ssid("a%C3"), Err(FormError::Malformed));
    }

    #[test]
    fn garbage_bodies_never_panic() {
        // Every length of a body that mixes the characters the parser treats specially.
        let alphabet = b"place=lat&lon%2B+-.,utc0123dstFfssidpsk";
        let mut seed = 0x2545_f491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for round in 0..4000 {
            let len = (next() % 60) as usize;
            let body: Vec<u8> = (0..len)
                .map(|_| if round % 7 == 0 { next() as u8 } else { alphabet[next() as usize % alphabet.len()] })
                .collect();
            let mut s = Settings::default();
            let before = s;
            if apply(&body, &mut s).is_err() {
                assert_eq!(s, before);
            }
        }
        // And the long ones, which are what overflow a fixed buffer.
        let mut s = Settings::default();
        let before = s;
        for key in ["place", "lat", "lon", "utc", "dst", "ssid", "psk", "zzz"] {
            let body = format!("{key}={}", "%41".repeat(5000));
            let _ = apply(body.as_bytes(), &mut s);
        }
        let _ = apply(&std::vec![b'&'; 10_000], &mut s);
        let _ = apply(&std::vec![b'='; 10_000], &mut s);
        assert_eq!(s, before);
    }
}
