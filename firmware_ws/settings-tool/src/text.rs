//! The `key = value` file the settings tool edits: human units going in, block fields coming out.
//!
//! A file is a *patch*: only the lines it holds are applied, everything else keeps the value it had
//! in the block on the lamp. That is what makes "change the password and put it back" safe.
use lamp_core::schedule::{Prayer, ALL_DAYS};
use lamp_core::settings::{
    Dst, HighLat, Isha, Settings, Setup, UserAlarm, FRI_SAT, FRIDAY, MAX_SOUND, MAX_USER_ALARMS, MAX_VOLUME,
    SUN_TO_THU,
};

pub const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

const HEADER: &str = "\
# The lamp's settings, one per line. Edit what you need and leave the rest alone: a line that is
# not here keeps the value already in the lamp. `#` starts a comment only at the start of a line,
# so a password may contain one.
#
#   ssid, psk     the network the lamp joins; an empty ssid means it is not set up yet
#   place         the name on the menu, at most 15 characters
#   lat, lon      where the sky is watched, in degrees; 30.0444 31.2357 is Cairo
#   utc           the zone's offset without summer time, e.g. +2:00 or -3:30
#   dst           none | egypt | europe | us
#   fajr_angle    degrees below the horizon, e.g. 19.5 for the Egyptian authority
#   isha_angle    the same for Isha, or use isha_min for a fixed time after Maghrib
#   asr           shafi | hanafi
#   high_lat      none | middle | seventh | angle
#   prayers       all | none | a list like fajr,dhuhr
#   offset_x      minutes to shift a prayer, so the lamp matches the local mosque
#   ramp          minutes the light takes to come up before any alarm
#   volume        the general volume, 0 to 30 on the player's own scale
#   volume_X      one prayer's own volume (volume_fajr ...): default, or 1 to 30
#   fade          seconds the adhan takes to reach its volume
#   adhan         the adhan every prayer plays unless it has its own: 1 to 99 (0001.mp3 and up)
#   alarm_sound   the sound every alarm plays unless it has its own: 1 to 99 (0101.mp3 and up)
#   sound_X       one prayer's own adhan (sound_fajr ...): default, or 1 to 99
#   brightness    percent at full, and night percent
#   clock         12 or 24
#   lamp          on | off
#   snooze        off, or a time like 06:30
#   alarmN        empty, or `06:30 * on default default` (time, days, on/off, sound, volume)
#                 days: * | sun-thu | fri-sat | fri | sun,mon,...
#                 sound: default (the default alarm sound) or 1 to 99; left out means default
#                 volume: default (the general volume) or 1 to 30; left out means default
#   setup         wifi, place, clock: which first-boot questions are answered
\n";

/// One non-comment line of the file, with the line number kept for error messages.
pub struct Line {
    pub no: usize,
    pub key: String,
    pub value: String,
}

pub fn parse(src: &str) -> Result<Vec<Line>, String> {
    let mut out = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let no = i + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let Some((k, v)) = text.split_once('=') else {
            return Err(format!("line {no}: expected `key = value`, found `{text}`"));
        };
        out.push(Line { no, key: k.trim().to_ascii_lowercase(), value: v.trim().to_string() });
    }
    Ok(out)
}

pub fn apply(s: &mut Settings, lines: &[Line]) -> Result<(), String> {
    let setup_line = lines.iter().find(|l| l.key == "setup");
    if let Some(l) = setup_line {
        s.setup = parse_setup(&l.value).map_err(|e| at(l, &e))?;
    }
    for l in lines.iter().filter(|l| l.key != "setup") {
        apply_one(s, l)?;
    }
    // A hand-written file usually names the network and the place without spelling out which
    // questions the setup screen considers answered. A dumped file always carries `setup`, so this
    // only fills the gap for a file someone typed.
    if setup_line.is_none() {
        if lines.iter().any(|l| l.key == "ssid") {
            s.setup.wifi = !s.ssid().is_empty();
        }
        if lines.iter().any(|l| l.key == "lat" || l.key == "lon") {
            s.setup.place = true;
        }
    }
    Ok(())
}

fn apply_one(s: &mut Settings, l: &Line) -> Result<(), String> {
    match l.key.as_str() {
        "ssid" => {
            if !s.set_ssid(&l.value) {
                return Err(at(l, "an ssid is at most 32 characters"));
            }
        }
        "psk" => {
            if !s.set_psk(&l.value) {
                return Err(at(l, "a password is at most 63 characters"));
            }
        }
        "place" => {
            if !s.set_place(&l.value) {
                return Err(at(l, "a place name is at most 15 characters"));
            }
        }
        "lat" => s.lat_e2 = degrees(l, 90.0)?,
        "lon" => s.lon_e2 = degrees(l, 180.0)?,
        "utc" => s.utc_offset_min = utc_minutes(l)?,
        "dst" => {
            s.dst = Dst::from_name(&l.value.to_ascii_lowercase())
                .ok_or_else(|| at(l, "none, egypt, europe or us"))?
        }
        "fajr_angle" => s.fajr_angle_d10 = tenths(l, 30.0)?,
        "isha_angle" => {
            s.isha = Isha::Angle;
            s.isha_value = tenths(l, 30.0)?;
        }
        "isha_min" => {
            s.isha = Isha::Minutes;
            s.isha_value = u16_of(l, 180)?;
        }
        "asr" => {
            s.asr_shadow = match l.value.to_ascii_lowercase().as_str() {
                "shafi" => 1,
                "hanafi" => 2,
                _ => return Err(at(l, "shafi or hanafi")),
            }
        }
        "high_lat" => {
            s.high_lat = HighLat::from_name(&l.value.to_ascii_lowercase())
                .ok_or_else(|| at(l, "none, middle, seventh or angle"))?
        }
        "prayers" => s.prayers_on = prayers(l)?,
        "offset_fajr" => s.offset_min[0] = i8_of(l, 120)?,
        "offset_dhuhr" => s.offset_min[1] = i8_of(l, 120)?,
        "offset_asr" => s.offset_min[2] = i8_of(l, 120)?,
        "offset_maghrib" => s.offset_min[3] = i8_of(l, 120)?,
        "offset_isha" => s.offset_min[4] = i8_of(l, 120)?,
        "ramp" => s.ramp_min = u8_of(l, 120)?,
        "volume" => s.volume = u8_of(l, MAX_VOLUME)?,
        "adhan" => s.default_adhan = sound_number(l, &l.value)?,
        "alarm_sound" => s.default_alarm = sound_number(l, &l.value)?,
        "sound_fajr" => s.prayer_sound[0] = own_sound(l, &l.value)?,
        "sound_dhuhr" => s.prayer_sound[1] = own_sound(l, &l.value)?,
        "sound_asr" => s.prayer_sound[2] = own_sound(l, &l.value)?,
        "sound_maghrib" => s.prayer_sound[3] = own_sound(l, &l.value)?,
        "sound_isha" => s.prayer_sound[4] = own_sound(l, &l.value)?,
        "volume_fajr" => s.prayer_volume[0] = own_volume(l, &l.value)?,
        "volume_dhuhr" => s.prayer_volume[1] = own_volume(l, &l.value)?,
        "volume_asr" => s.prayer_volume[2] = own_volume(l, &l.value)?,
        "volume_maghrib" => s.prayer_volume[3] = own_volume(l, &l.value)?,
        "volume_isha" => s.prayer_volume[4] = own_volume(l, &l.value)?,
        "fade" => s.adhan_fade_in_s = u8_of(l, 120)?,
        "brightness" => s.brightness_max = u8_of(l, 100)?,
        "night" => s.brightness_night = u8_of(l, 100)?,
        "clock" => {
            s.clock_24h = match l.value.as_str() {
                "12" => false,
                "24" => true,
                _ => return Err(at(l, "12 or 24")),
            }
        }
        "lamp" => {
            s.lamp_on = match l.value.to_ascii_lowercase().as_str() {
                "on" => true,
                "off" => false,
                _ => return Err(at(l, "on or off")),
            }
        }
        "snooze" => {
            s.snooze_minute = if l.value.eq_ignore_ascii_case("off") {
                None
            } else {
                Some(clock_minutes(l)?)
            }
        }
        k if k.starts_with("alarm") => alarm(s, l)?,
        other => return Err(at(l, &format!("there is no setting called `{other}`"))),
    }
    Ok(())
}

fn alarm(s: &mut Settings, l: &Line) -> Result<(), String> {
    let slot = l
        .key
        .strip_prefix("alarm")
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| (1..=MAX_USER_ALARMS).contains(n))
        .ok_or_else(|| at(l, "the alarm is alarm1 up to alarm8"))?;
    if l.value.eq_ignore_ascii_case("empty") {
        s.alarms[slot - 1] = UserAlarm::EMPTY;
        return Ok(());
    }
    let mut parts = l.value.split_whitespace();
    let time = parts.next().ok_or_else(|| at(l, "a time first, like `06:30 * on`, or `empty`"))?;
    let days = parts.next().ok_or_else(|| at(l, "the days next: * , sun-thu, fri-sat, fri, or a list"))?;
    let state = parts.next().unwrap_or("on").to_ascii_lowercase();
    let sound = own_sound(l, parts.next().unwrap_or("default"))?;
    let volume = own_volume(l, parts.next().unwrap_or("default"))?;
    let minute = clock_minutes_str(l, time)?;
    let days = days_of(l, days)?;
    let enabled = match state.as_str() {
        "on" => true,
        "off" => false,
        _ => return Err(at(l, "on or off")),
    };
    s.alarms[slot - 1] = UserAlarm { used: true, enabled, days, minute, sound, volume };
    Ok(())
}

/// A volume of its own: `default` (stored as 0, so the general volume applies) or 1 to 30.
fn own_volume(l: &Line, word: &str) -> Result<u8, String> {
    if word.eq_ignore_ascii_case("default") {
        return Ok(0);
    }
    match word.parse::<u8>() {
        Ok(v) if (1..=MAX_VOLUME).contains(&v) => Ok(v),
        _ => Err(at(l, "default, or a volume from 1 to 30")),
    }
}

/// A sound of its own: `default` (stored as 0, so the default one plays) or 1 to 99.
fn own_sound(l: &Line, word: &str) -> Result<u8, String> {
    if word.eq_ignore_ascii_case("default") {
        return Ok(0);
    }
    sound_number(l, word)
}

/// A sound number, 1 to 99: the number in the card's file name (adhan 0001 up, alarm sounds 0101 up).
fn sound_number(l: &Line, word: &str) -> Result<u8, String> {
    match word.parse::<u8>() {
        Ok(v) if (1..=MAX_SOUND).contains(&v) => Ok(v),
        _ => Err(at(l, "a sound from 1 to 99 (or `default` where a default is allowed)")),
    }
}

fn volume_text(own: u8) -> String {
    if own == 0 { "default".into() } else { own.to_string() }
}

/// The whole file, with every value spelled out, so it can be edited and handed back.
pub fn dump(s: &Settings) -> String {
    let mut o = String::from(HEADER);
    let mut flags = Vec::new();
    if s.setup.wifi {
        flags.push("wifi");
    }
    if s.setup.place {
        flags.push("place");
    }
    if s.setup.clock {
        flags.push("clock");
    }
    o.push_str(&format!("setup = {}\n\n", if flags.is_empty() { "none".into() } else { flags.join(", ") }));

    o.push_str(&format!("ssid = {}\n", s.ssid()));
    o.push_str(&format!("psk = {}\n", s.psk()));
    o.push_str(&format!("place = {}\n", s.place_name()));
    o.push_str(&format!("lat = {:.2}\n", s.lat_e2 as f64 / 100.0));
    o.push_str(&format!("lon = {:.2}\n", s.lon_e2 as f64 / 100.0));
    o.push_str(&format!("utc = {}\n", utc_text(s.utc_offset_min)));
    o.push_str(&format!("dst = {}\n\n", s.dst.name()));

    o.push_str(&format!("fajr_angle = {:.1}\n", s.fajr_angle_d10 as f64 / 10.0));
    match s.isha {
        Isha::Angle => o.push_str(&format!("isha_angle = {:.1}\n", s.isha_value as f64 / 10.0)),
        Isha::Minutes => o.push_str(&format!("isha_min = {}\n", s.isha_value)),
    }
    o.push_str(&format!("asr = {}\n", if s.asr_shadow == 2 { "hanafi" } else { "shafi" }));
    o.push_str(&format!("high_lat = {}\n", s.high_lat.name()));
    o.push_str(&format!("prayers = {}\n\n", prayers_text(s.prayers_on)));

    for (i, p) in Prayer::ALL.iter().enumerate() {
        o.push_str(&format!("offset_{} = {}\n", p.name().to_ascii_lowercase(), s.offset_min[i]));
    }
    o.push_str(&format!("ramp = {}\n", s.ramp_min));
    o.push_str(&format!("volume = {}\n", s.volume));
    for (i, p) in Prayer::ALL.iter().enumerate() {
        o.push_str(&format!("volume_{} = {}\n", p.name().to_ascii_lowercase(), volume_text(s.prayer_volume[i])));
    }
    o.push_str(&format!("fade = {}\n", s.adhan_fade_in_s));
    o.push_str(&format!("adhan = {}\n", s.default_adhan));
    o.push_str(&format!("alarm_sound = {}\n", s.default_alarm));
    for (i, p) in Prayer::ALL.iter().enumerate() {
        o.push_str(&format!("sound_{} = {}\n", p.name().to_ascii_lowercase(), volume_text(s.prayer_sound[i])));
    }
    o.push_str(&format!("brightness = {}\n", s.brightness_max));
    o.push_str(&format!("night = {}\n\n", s.brightness_night));

    o.push_str(&format!("clock = {}\n", if s.clock_24h { 24 } else { 12 }));
    o.push_str(&format!("lamp = {}\n", if s.lamp_on { "on" } else { "off" }));
    match s.snooze_minute {
        Some(m) => o.push_str(&format!("snooze = {:02}:{:02}\n\n", m / 60, m % 60)),
        None => o.push_str("snooze = off\n\n"),
    }

    for (i, a) in s.alarms.iter().enumerate() {
        o.push_str(&format!("alarm{} = {}\n", i + 1, alarm_text(a)));
    }
    o
}

fn alarm_text(a: &UserAlarm) -> String {
    if !a.used {
        return "empty".into();
    }
    let sound = volume_text(a.sound);
    format!(
        "{:02}:{:02} {} {} {} {}",
        a.minute / 60,
        a.minute % 60,
        days_text(a.days),
        if a.enabled { "on" } else { "off" },
        sound,
        volume_text(a.volume)
    )
}

fn days_text(days: u8) -> String {
    if days == ALL_DAYS {
        return "*".into();
    }
    let mut out: Vec<&str> = Vec::new();
    for (i, name) in DAY_NAMES.iter().enumerate() {
        if days & (1 << i) != 0 {
            out.push(name);
        }
    }
    if out.is_empty() {
        "*".into()
    } else {
        out.join(",")
    }
}

fn prayers_text(mask: u8) -> String {
    if mask == 0 {
        return "none".into();
    }
    if mask == 0b1_1111 {
        return "all".into();
    }
    let mut out: Vec<String> = Vec::new();
    for p in Prayer::ALL {
        if mask & (1 << p.index()) != 0 {
            out.push(p.name().to_ascii_lowercase());
        }
    }
    out.join(",")
}

fn utc_text(min: i16) -> String {
    let sign = if min < 0 { '-' } else { '+' };
    let a = min.unsigned_abs();
    format!("{sign}{:02}:{:02}", a / 60, a % 60)
}

fn parse_setup(v: &str) -> Result<Setup, String> {
    if v.is_empty() || v.eq_ignore_ascii_case("none") {
        return Ok(Setup::NONE);
    }
    let mut s = Setup::NONE;
    for part in v.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "wifi" => s.wifi = true,
            "place" => s.place = true,
            "clock" => s.clock = true,
            other => return Err(format!("`{other}` is not one of wifi, place, clock")),
        }
    }
    Ok(s)
}

fn prayers(l: &Line) -> Result<u8, String> {
    let v = l.value.trim();
    if v.eq_ignore_ascii_case("none") {
        return Ok(0);
    }
    if v.eq_ignore_ascii_case("all") {
        return Ok(0b1_1111);
    }
    let mut mask = 0u8;
    for name in v.split(',') {
        let p = Prayer::ALL
            .iter()
            .find(|p| p.name().eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| at(l, "all, none, or a list of prayers"))?;
        mask |= 1 << p.index();
    }
    Ok(mask)
}

fn days_of(l: &Line, v: &str) -> Result<u8, String> {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "*" | "everyday" => Ok(ALL_DAYS),
        "sun-thu" => Ok(SUN_TO_THU),
        "fri-sat" => Ok(FRI_SAT),
        "fri" => Ok(FRIDAY),
        _ => {
            let mut mask = 0u8;
            for d in v.split(',') {
                let i = DAY_NAMES
                    .iter()
                    .position(|n| *n == d.trim())
                    .ok_or_else(|| at(l, "days: * , sun-thu, fri-sat, fri, or sun,mon,..."))?;
                mask |= 1 << i;
            }
            Ok(mask)
        }
    }
}

fn clock_minutes(l: &Line) -> Result<u16, String> {
    clock_minutes_str(l, &l.value)
}

fn clock_minutes_str(l: &Line, v: &str) -> Result<u16, String> {
    let (h, m) = v.split_once(':').ok_or_else(|| at(l, "a time looks like 06:30"))?;
    let h: u16 = h.trim().parse().map_err(|_| at(l, "the hour is a number"))?;
    let m: u16 = m.trim().parse().map_err(|_| at(l, "the minute is a number"))?;
    if h > 23 || m > 59 {
        return Err(at(l, "that is not a time of day"));
    }
    Ok(h * 60 + m)
}

fn utc_minutes(l: &Line) -> Result<i16, String> {
    let v = l.value.trim();
    let (sign, rest) = match v.as_bytes().first() {
        Some(b'-') => (-1i16, &v[1..]),
        Some(b'+') => (1, &v[1..]),
        _ => (1, v),
    };
    let (h, m) = rest.split_once(':').ok_or_else(|| at(l, "an offset looks like +2:00"))?;
    let h: i16 = h.trim().parse().map_err(|_| at(l, "the hour is a number"))?;
    let m: i16 = m.trim().parse().map_err(|_| at(l, "the minute is a number"))?;
    if m > 59 || h > 14 {
        return Err(at(l, "an offset is at most 14 hours"));
    }
    Ok(sign * (h * 60 + m))
}

fn degrees(l: &Line, limit: f64) -> Result<i16, String> {
    let v: f64 = l.value.parse().map_err(|_| at(l, "a number of degrees"))?;
    if !v.is_finite() || v.abs() > limit {
        return Err(at(l, &format!("between -{limit} and {limit} degrees")));
    }
    Ok((v * 100.0).round() as i16)
}

fn tenths(l: &Line, limit: f64) -> Result<u16, String> {
    let v: f64 = l.value.parse().map_err(|_| at(l, "a number of degrees"))?;
    if !v.is_finite() || v < 0.0 || v > limit {
        return Err(at(l, &format!("between 0 and {limit} degrees")));
    }
    Ok((v * 10.0).round() as u16)
}

fn u8_of(l: &Line, max: u8) -> Result<u8, String> {
    let v: u8 = l.value.parse().map_err(|_| at(l, "a whole number"))?;
    if v > max {
        return Err(at(l, &format!("at most {max}")));
    }
    Ok(v)
}

fn u16_of(l: &Line, max: u16) -> Result<u16, String> {
    let v: u16 = l.value.parse().map_err(|_| at(l, "a whole number"))?;
    if v > max {
        return Err(at(l, &format!("at most {max}")));
    }
    Ok(v)
}

fn i8_of(l: &Line, limit: i8) -> Result<i8, String> {
    let v: i8 = l.value.parse().map_err(|_| at(l, "a whole number of minutes"))?;
    if !(-limit..=limit).contains(&v) {
        return Err(at(l, &format!("between -{limit} and {limit} minutes")));
    }
    Ok(v)
}

fn at(l: &Line, msg: &str) -> String {
    format!("line {}: {}: {msg}", l.no, l.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patched(base: &Settings, src: &str) -> Settings {
        let mut s = *base;
        apply(&mut s, &parse(src).unwrap()).unwrap();
        s
    }

    #[test]
    fn a_dump_parses_back_to_what_it_came_from() {
        let mut s = Settings::default();
        s.set_ssid("Home").then_some(()).unwrap();
        s.set_psk("a pass#with spaces").then_some(()).unwrap();
        s.set_place("Nasr City").then_some(()).unwrap();
        s.prayers_on = 0b1_0101;
        s.offset_min[1] = -5;
        s.alarms[2] = UserAlarm { used: true, enabled: false, days: 0b0010_0000, minute: 390, sound: 1, volume: 9 };
        s.volume = 18;
        s.prayer_volume = [0, 4, 0, 30, 0];
        s.snooze_minute = Some(6 * 60 + 5);
        s.lamp_on = true;
        s.clock_24h = true;
        let text = dump(&s);
        let back = patched(&Settings::default(), &text);
        assert_eq!(back, s);
    }

    #[test]
    fn a_patch_leaves_everything_it_does_not_mention_alone() {
        let mut base = Settings::default();
        base.alarms[0] = UserAlarm { used: true, enabled: true, days: ALL_DAYS, minute: 400, sound: 0, volume: 0 };
        base.brightness_max = 40;
        let s = patched(&base, "ssid = Home\npsk = secret\n");
        assert_eq!(s.ssid(), "Home");
        assert_eq!(s.alarms[0], base.alarms[0]);
        assert_eq!(s.brightness_max, 40);
        assert!(s.setup.wifi);
        assert!(!s.setup.place);
    }

    #[test]
    fn human_units_become_block_units() {
        let s = patched(
            &Settings::default(),
            "utc = +2:00\nlat = 30.0444\nlon = -31.2357\nfajr_angle = 19.5\nisha_min = 90\n",
        );
        assert_eq!(s.utc_offset_min, 120);
        assert_eq!(s.lat_e2, 3004);
        assert_eq!(s.lon_e2, -3124);
        assert_eq!(s.fajr_angle_d10, 195);
        assert_eq!(s.isha, Isha::Minutes);
        assert_eq!(s.isha_value, 90);
        assert!(s.setup.place);

        let s = patched(&Settings::default(), "utc = -3:30\nisha_angle = 17.0\n");
        assert_eq!(s.utc_offset_min, -210);
        assert_eq!(s.isha, Isha::Angle);
        assert_eq!(s.isha_value, 170);
    }

    #[test]
    fn an_alarm_line_reads_and_writes_in_one_shape() {
        let s = patched(&Settings::default(), "alarm1 = 06:30 fri-sat off 1\n");
        assert_eq!(
            s.alarms[0],
            UserAlarm { used: true, enabled: false, days: 0b0110_0000, minute: 390, sound: 1, volume: 0 }
        );
        let text = dump(&s);
        assert!(text.contains("alarm1 = 06:30 fri,sat off 1 default"), "{text}");
        assert!(text.contains("alarm2 = empty"), "{text}");
        assert_eq!(patched(&s, "alarm1 = empty\n").alarms[0], UserAlarm::EMPTY);
    }

    #[test]
    fn volumes_read_as_default_or_a_level_and_mistakes_say_what_is_allowed() {
        let s = patched(
            &Settings::default(),
            "volume = 15\nvolume_asr = 6\nvolume_isha = default\nalarm2 = 07:00 * on default 12\nalarm3 = 07:30 * on 2\n",
        );
        assert_eq!(s.volume, 15);
        assert_eq!(s.prayer_volume, [0, 0, 6, 0, 0]);
        assert_eq!(s.alarms[1].volume, 12);
        assert_eq!(s.alarms[2].volume, 0); // left out means default
        assert_eq!(s.alarms[2].sound, 2);
        assert_eq!(s.alarms[1].sound, 0);
        for bad in ["volume = 31\n", "volume_fajr = 0\n", "volume_fajr = loud\n", "alarm1 = 07:00 * on default 31\n", "alarm1 = 07:00 * on 100 default\n", "alarm1 = 07:00 * on chime\n"] {
            let e = apply(&mut Settings::default(), &parse(bad).unwrap()).unwrap_err();
            assert!(e.contains("volume") || e.contains("sound") || e.contains("at most") || e.contains("30"), "{bad}: {e}");
        }
    }

    #[test]
    fn sounds_read_as_default_or_a_number_and_write_back_the_same() {
        let s = patched(
            &Settings::default(),
            "adhan = 7\nalarm_sound = 2\nsound_dhuhr = 12\nsound_isha = default\nalarm1 = 06:00 * on 3 18\n",
        );
        assert_eq!((s.default_adhan, s.default_alarm), (7, 2));
        assert_eq!(s.prayer_sound, [0, 12, 0, 0, 0]);
        assert_eq!((s.alarms[0].sound, s.alarms[0].volume), (3, 18));
        let text = dump(&s);
        assert!(text.contains("adhan = 7"), "{text}");
        assert!(text.contains("sound_dhuhr = 12"), "{text}");
        assert!(text.contains("sound_fajr = default"), "{text}");
        assert!(text.contains("alarm1 = 06:00 * on 3 18"), "{text}");
        assert_eq!(patched(&Settings::default(), &text), s);
        // A default is a number, never `default`, and 0 is not a sound.
        for bad in ["adhan = default\n", "adhan = 0\n", "alarm_sound = 100\n", "sound_fajr = 0\n"] {
            assert!(apply(&mut Settings::default(), &parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn setup_is_explicit_when_the_file_says_so() {
        let s = patched(&Settings::default(), "setup = wifi, clock\n");
        assert_eq!(s.setup, Setup { wifi: true, place: false, clock: true });
        assert!(parse("setup = wifi\n").is_ok());
    }

    #[test]
    fn mistakes_name_the_line_and_the_key() {
        let e = apply(&mut Settings::default(), &parse("ssid = Home\ntimezon = +2:00\n").unwrap())
            .unwrap_err();
        assert!(e.starts_with("line 2: timezon:"), "{e}");
        let e = apply(&mut Settings::default(), &parse("lat = 100\n").unwrap()).unwrap_err();
        assert!(e.contains("line 1: lat"), "{e}");
        let e = apply(&mut Settings::default(), &parse("alarm9 = 06:30 *\n").unwrap()).unwrap_err();
        assert!(e.contains("alarm1 up to alarm8"), "{e}");
        assert!(parse("destination nowhere\n").is_err());
    }
}
