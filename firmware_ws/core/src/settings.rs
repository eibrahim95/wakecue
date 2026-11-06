//! What the user can change, and how it survives losing power.
//!
//! One block of [`STORED_LEN`] bytes, written whole into one of two 4 KB sectors. A save goes to
//! the sector that is *not* holding the newest good block, so an erase interrupted by a brown-out
//! cannot lose what was there: the other sector still holds the previous block. Loading reads
//! both, keeps the valid one with the higher sequence number, and falls back to
//! [`Settings::default`] when neither is usable.
//!
//! The layout is append-only: never move, shrink or reorder a field. New settings are carved out of
//! `reserved` or added past the old end, and the length byte grows with them, so a build that does
//! not know a field skips it instead of throwing the whole block away. A field added later must read
//! both 0x00 and 0xFF as "not set, use the default", because that is what older builds wrote there.
//!
//! Byte map, little endian. Header 12 bytes, payload 200, block 212:
//!
//! ```text
//!   off  size  field
//!     0     4  magic, "WKS1"
//!     4     1  version, bumped only when a field moves
//!     5     1  payload length, grows when a field is added
//!     6     2  sequence, one more per save
//!     8     4  crc32 of the payload
//!    12     1  setup flags: bit0 network, bit1 place, bit2 clock
//!    13    33  ssid, NUL padded
//!    46    64  psk, NUL padded; empty means an open network
//!   110     2  utc offset in minutes, without summer time
//!   112     1  summer time rule
//!   113     2  latitude, hundredths of a degree
//!   115     2  longitude, hundredths of a degree
//!   117    16  place name, NUL padded
//!   133     2  Fajr angle, tenths of a degree
//!   135     1  Isha kind: 0 an angle, 1 minutes after Maghrib
//!   136     2  Isha value, tenths of a degree or minutes
//!   138     1  Asr shadow: 1 standard, 2 Hanafi
//!   139     1  high latitude rule
//!   140     1  prayers that ring, a bit per prayer
//!   141     5  minutes to shift each prayer by
//!   146     1  Fajr ramp, minutes
//!   147     1  adhan volume
//!   148     1  adhan fade-in, seconds
//!   149    40  eight alarms, 5 bytes each
//!   189     1  brightness cap, percent
//!   190     1  night brightness, percent
//!   191     1  face flags: bit0 24-hour clock
//!   192     1  the lamp is on
//!   193     1  snooze flags: 1 = snoozing
//!   194     2  snooze until, minutes after midnight
//!   196    16  reserved, zero
//! ```
use crate::schedule::{Action, Prayer, Rule, When, ALL_DAYS};

pub const MAX_USER_ALARMS: usize = 8;
/// Fajr gives two rules (ramp and sound), the other prayers one each, plus the user alarms.
pub const MAX_RULES: usize = 2 + 4 + MAX_USER_ALARMS;

/// The block lives in two sectors of the 24 KB `nvs` partition. Nothing in this build uses NVS and
/// espflash puts the application at 0x10000, so these sectors are ours alone.
pub const SECTOR_A: u32 = 0x9000;
pub const SECTOR_B: u32 = 0xA000;
pub const SECTOR_LEN: usize = 4096;

pub const HEADER_LEN: usize = 12;
pub const PAYLOAD_LEN: usize = 200;
pub const STORED_LEN: usize = HEADER_LEN + PAYLOAD_LEN;

/// How much of a sector a boot reads. The block, plus room for a field a later build appends, so
/// the read stays two small buffers instead of two whole 4 KB sectors.
pub const READ_LEN: usize = 512;

const MAGIC: [u8; 4] = *b"WKS1";
const VERSION: u8 = 1;

pub const SUN_TO_THU: u8 = 0b0001_1111;
pub const FRI_SAT: u8 = 0b0110_0000;
pub const FRIDAY: u8 = 0b0010_0000;

/// The tones an alarm can pick, by the byte stored with it. Index 0 is the adhan.
pub const SOUNDS: [&str; 3] = ["adhan", "chime", "beep"];

/// Payload offsets: the `off` column above minus the 12-byte header.
mod po {
    pub const SETUP: usize = 0;
    pub const SSID: usize = 1;
    pub const SSID_LEN: usize = 33;
    pub const PSK: usize = 34;
    pub const PSK_LEN: usize = 64;
    pub const UTC_OFFSET: usize = 98;
    pub const DST: usize = 100;
    pub const LAT: usize = 101;
    pub const LON: usize = 103;
    pub const PLACE: usize = 105;
    pub const PLACE_LEN: usize = 16;
    pub const FAJR_ANGLE: usize = 121;
    pub const ISHA_KIND: usize = 123;
    pub const ISHA_VALUE: usize = 124;
    pub const ASR: usize = 126;
    pub const HIGH_LAT: usize = 127;
    pub const PRAYERS: usize = 128;
    pub const OFFSETS: usize = 129;
    pub const RAMP: usize = 134;
    pub const VOLUME: usize = 135;
    pub const FADE: usize = 136;
    pub const ALARMS: usize = 137;
    pub const ALARM_STRIDE: usize = 5;
    pub const BRIGHT_MAX: usize = 177;
    pub const BRIGHT_NIGHT: usize = 178;
    pub const FACE: usize = 179;
    pub const LAMP: usize = 180;
    pub const SNOOZE_FLAGS: usize = 181;
    pub const SNOOZE_MIN: usize = 182;
    pub const RESERVED: usize = 184;
    pub const RESERVED_LEN: usize = 16;

    const _: () = assert!(RESERVED + RESERVED_LEN == super::PAYLOAD_LEN);
}

/// How summer time is applied to the stored UTC offset. Only Egypt has its real rule so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dst {
    None = 0,
    Egypt = 1,
    Europe = 2,
    Us = 3,
}

impl Dst {
    pub const NAMES: [&'static str; 4] = ["none", "egypt", "europe", "us"];

    pub fn name(self) -> &'static str {
        Dst::NAMES[self as usize]
    }

    pub fn from_name(s: &str) -> Option<Dst> {
        Dst::NAMES.iter().position(|n| *n == s).and_then(|i| Dst::from_u8(i as u8))
    }

    fn from_u8(v: u8) -> Option<Dst> {
        match v {
            0 => Some(Dst::None),
            1 => Some(Dst::Egypt),
            2 => Some(Dst::Europe),
            3 => Some(Dst::Us),
            _ => None,
        }
    }
}

/// Isha is either a sun angle below the horizon or a fixed time after Maghrib, depending on the
/// authority the user follows (Umm al-Qura uses 90 minutes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isha {
    Angle = 0,
    Minutes = 1,
}

impl Isha {
    pub const NAMES: [&'static str; 2] = ["angle", "minutes"];

    pub fn name(self) -> &'static str {
        Isha::NAMES[self as usize]
    }

    pub fn from_name(s: &str) -> Option<Isha> {
        match s {
            "angle" => Some(Isha::Angle),
            "minutes" => Some(Isha::Minutes),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Option<Isha> {
        match v {
            0 => Some(Isha::Angle),
            1 => Some(Isha::Minutes),
            _ => None,
        }
    }
}

/// What to do when the sun never reaches the prayer's angle, which happens above about 48 degrees
/// of latitude in summer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighLat {
    None = 0,
    Middle = 1,
    Seventh = 2,
    Angle = 3,
}

impl HighLat {
    pub const NAMES: [&'static str; 4] = ["none", "middle", "seventh", "angle"];

    pub fn name(self) -> &'static str {
        HighLat::NAMES[self as usize]
    }

    pub fn from_name(s: &str) -> Option<HighLat> {
        HighLat::NAMES.iter().position(|n| *n == s).and_then(|i| HighLat::from_u8(i as u8))
    }

    fn from_u8(v: u8) -> Option<HighLat> {
        match v {
            0 => Some(HighLat::None),
            1 => Some(HighLat::Middle),
            2 => Some(HighLat::Seventh),
            3 => Some(HighLat::Angle),
            _ => None,
        }
    }
}

/// Which of the first-boot questions have been answered. The setup flow resumes from here after a
/// power cut instead of starting over, and a blank block means all three are false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    /// The network in the block was joined at least once.
    pub wifi: bool,
    /// The place was chosen by the user, not taken from the default.
    pub place: bool,
    /// The clock has held a real time since the last power loss.
    pub clock: bool,
}

impl Setup {
    pub const NONE: Setup = Setup { wifi: false, place: false, clock: false };

    fn bits(self) -> u8 {
        self.wifi as u8 | (self.place as u8) << 1 | (self.clock as u8) << 2
    }

    fn from_bits(b: u8) -> Option<Setup> {
        if b & !0b111 != 0 {
            return None;
        }
        Some(Setup { wifi: b & 1 != 0, place: b & 2 != 0, clock: b & 4 != 0 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserAlarm {
    pub used: bool,
    pub enabled: bool,
    /// Weekday mask by local date, bit 0 = Sunday.
    pub days: u8,
    /// Minutes after midnight, local time.
    pub minute: u16,
    /// Index into [`SOUNDS`]. No screen sets this yet; it is carried so the block does not have to
    /// change when the ring picks a tone.
    pub sound: u8,
}

impl UserAlarm {
    pub const EMPTY: UserAlarm =
        UserAlarm { used: false, enabled: false, days: ALL_DAYS, minute: 7 * 60, sound: 0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub setup: Setup,
    pub ssid: [u8; 33],
    pub psk: [u8; 64],
    pub utc_offset_min: i16,
    pub dst: Dst,
    pub lat_e2: i16,
    pub lon_e2: i16,
    pub place: [u8; 16],
    pub fajr_angle_d10: u16,
    pub isha: Isha,
    pub isha_value: u16,
    pub asr_shadow: u8,
    pub high_lat: HighLat,
    /// Bit per prayer, in table order (bit 0 = Fajr).
    pub prayers_on: u8,
    /// Minutes each prayer is shifted by, so the lamp can be matched to the local mosque.
    pub offset_min: [i8; 5],
    pub fajr_ramp_min: u8,
    pub adhan_volume: u8,
    pub adhan_fade_in_s: u8,
    pub alarms: [UserAlarm; MAX_USER_ALARMS],
    pub brightness_max: u8,
    pub brightness_night: u8,
    pub clock_24h: bool,
    pub lamp_on: bool,
    pub snooze_minute: Option<u16>,
}

impl Default for Settings {
    /// Fajr only, and Cairo: the first lamp is for one house, and a first boot that shows plausible
    /// times is friendlier than one that shows zeroes. The setup flow replaces the place.
    fn default() -> Self {
        Settings {
            setup: Setup::NONE,
            ssid: [0; 33],
            psk: [0; 64],
            utc_offset_min: 120,
            dst: Dst::Egypt,
            lat_e2: 3004,
            lon_e2: 3124,
            place: label("Cairo"),
            fajr_angle_d10: 195,
            isha: Isha::Angle,
            isha_value: 175,
            asr_shadow: 1,
            high_lat: HighLat::None,
            prayers_on: 1,
            offset_min: [0; 5],
            fajr_ramp_min: 15,
            adhan_volume: 22,
            adhan_fade_in_s: 30,
            alarms: [UserAlarm::EMPTY; MAX_USER_ALARMS],
            brightness_max: 60,
            brightness_night: 5,
            clock_24h: false,
            lamp_on: false,
            snooze_minute: None,
        }
    }
}

impl Settings {
    pub fn ssid(&self) -> &str {
        str_of(&self.ssid)
    }

    pub fn psk(&self) -> &str {
        str_of(&self.psk)
    }

    pub fn place_name(&self) -> &str {
        str_of(&self.place)
    }

    /// False when the text does not fit, so the caller can say which line it came from.
    pub fn set_ssid(&mut self, s: &str) -> bool {
        set_str(&mut self.ssid, s)
    }

    pub fn set_psk(&mut self, s: &str) -> bool {
        set_str(&mut self.psk, s)
    }

    pub fn set_place(&mut self, s: &str) -> bool {
        set_str(&mut self.place, s)
    }

    pub fn prayer_on(&self, p: Prayer) -> bool {
        self.prayers_on & (1 << p.index()) != 0
    }

    pub fn toggle_prayer(&mut self, p: Prayer) {
        self.prayers_on ^= 1 << p.index();
    }

    /// Fills `out` with the rules the schedule should run and returns how many. Prayer sounds are
    /// tracks 1..=5 (`/mp3/0001.mp3` is Fajr); Fajr also gets the sunrise ramp before it.
    pub fn rules(&self, out: &mut [Rule; MAX_RULES]) -> usize {
        let mut n = 0;
        let mut push = |r: Rule| {
            out[n] = r;
            n += 1;
        };
        for p in Prayer::ALL {
            if !self.prayer_on(p) {
                continue;
            }
            let shift = self.offset_min[p.index()] as i16;
            if p == Prayer::Fajr {
                push(Rule {
                    days: ALL_DAYS,
                    when: When::Prayer(p, shift),
                    action: Action::Sunrise { ramp_min: self.fajr_ramp_min as u16 },
                });
            }
            let fade_in_s = if p == Prayer::Fajr { self.adhan_fade_in_s as u16 } else { 20 };
            push(Rule {
                days: ALL_DAYS,
                when: When::Prayer(p, shift),
                action: Action::Play {
                    track: p.index() as u16 + 1,
                    volume: self.adhan_volume,
                    fade_in_s,
                },
            });
        }
        for a in self.alarms.iter().filter(|a| a.used && a.enabled) {
            push(Rule { days: a.days, when: When::Local(a.minute), action: Action::Alarm });
        }
        n
    }
}

/// A block with the sequence number that says how old it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    pub seq: u16,
    pub settings: Settings,
}

impl Stored {
    pub fn encode(&self) -> [u8; STORED_LEN] {
        let mut b = [0u8; STORED_LEN];
        b[..4].copy_from_slice(&MAGIC);
        b[4] = VERSION;
        b[5] = PAYLOAD_LEN as u8;
        b[6..8].copy_from_slice(&self.seq.to_le_bytes());

        let s = &self.settings;
        let p = &mut b[HEADER_LEN..];
        p[po::SETUP] = s.setup.bits();
        p[po::SSID..po::SSID + po::SSID_LEN].copy_from_slice(&s.ssid);
        p[po::PSK..po::PSK + po::PSK_LEN].copy_from_slice(&s.psk);
        p[po::UTC_OFFSET..po::UTC_OFFSET + 2].copy_from_slice(&s.utc_offset_min.to_le_bytes());
        p[po::DST] = s.dst as u8;
        p[po::LAT..po::LAT + 2].copy_from_slice(&s.lat_e2.to_le_bytes());
        p[po::LON..po::LON + 2].copy_from_slice(&s.lon_e2.to_le_bytes());
        p[po::PLACE..po::PLACE + po::PLACE_LEN].copy_from_slice(&s.place);
        p[po::FAJR_ANGLE..po::FAJR_ANGLE + 2]
            .copy_from_slice(&s.fajr_angle_d10.to_le_bytes());
        p[po::ISHA_KIND] = s.isha as u8;
        p[po::ISHA_VALUE..po::ISHA_VALUE + 2].copy_from_slice(&s.isha_value.to_le_bytes());
        p[po::ASR] = s.asr_shadow;
        p[po::HIGH_LAT] = s.high_lat as u8;
        p[po::PRAYERS] = s.prayers_on;
        for (i, v) in s.offset_min.iter().enumerate() {
            p[po::OFFSETS + i] = *v as u8;
        }
        p[po::RAMP] = s.fajr_ramp_min;
        p[po::VOLUME] = s.adhan_volume;
        p[po::FADE] = s.adhan_fade_in_s;
        for (i, a) in s.alarms.iter().enumerate() {
            let at = po::ALARMS + i * po::ALARM_STRIDE;
            p[at] = a.used as u8 | (a.enabled as u8) << 1;
            p[at + 1] = a.days;
            p[at + 2..at + 4].copy_from_slice(&a.minute.to_le_bytes());
            p[at + 4] = a.sound;
        }
        p[po::BRIGHT_MAX] = s.brightness_max;
        p[po::BRIGHT_NIGHT] = s.brightness_night;
        p[po::FACE] = s.clock_24h as u8;
        p[po::LAMP] = s.lamp_on as u8;
        p[po::SNOOZE_FLAGS] = s.snooze_minute.is_some() as u8;
        p[po::SNOOZE_MIN..po::SNOOZE_MIN + 2]
            .copy_from_slice(&s.snooze_minute.unwrap_or(0).to_le_bytes());

        let crc = crc32(p);
        b[8..12].copy_from_slice(&crc.to_le_bytes());
        b
    }

    /// None when the bytes are blank, damaged, from a layout this build does not speak, or hold a
    /// field outside its range. Bytes past the length this build knows are skipped, which is what
    /// lets a block grow without stranding the settings already in it.
    pub fn decode(b: &[u8]) -> Option<Stored> {
        if b.len() < HEADER_LEN + PAYLOAD_LEN || b[..4] != MAGIC || b[4] != VERSION {
            return None;
        }
        let len = b[5] as usize;
        if len < PAYLOAD_LEN || HEADER_LEN + len > b.len() {
            return None;
        }
        let payload = &b[HEADER_LEN..HEADER_LEN + len];
        if crc32(payload) != u32::from_le_bytes([b[8], b[9], b[10], b[11]]) {
            return None;
        }
        let p = &payload[..PAYLOAD_LEN];

        let setup = Setup::from_bits(p[po::SETUP])?;
        let ssid = at::<{ po::SSID_LEN }>(p, po::SSID)?;
        let psk = at::<{ po::PSK_LEN }>(p, po::PSK)?;
        let place = at::<{ po::PLACE_LEN }>(p, po::PLACE)?;
        for s in [ssid.as_slice(), psk.as_slice(), place.as_slice()] {
            if !nul_terminated_utf8(s) {
                return None;
            }
        }

        let utc_offset_min = at_i16(p, po::UTC_OFFSET)?;
        if !(-720..=840).contains(&utc_offset_min) {
            return None;
        }
        let dst = Dst::from_u8(p[po::DST])?;
        let lat_e2 = at_i16(p, po::LAT)?;
        let lon_e2 = at_i16(p, po::LON)?;
        if !(-9000..=9000).contains(&lat_e2) || !(-18000..=18000).contains(&lon_e2) {
            return None;
        }

        let fajr_angle_d10 = at_u16(p, po::FAJR_ANGLE)?;
        let isha = Isha::from_u8(p[po::ISHA_KIND])?;
        let isha_value = at_u16(p, po::ISHA_VALUE)?;
        if fajr_angle_d10 > 300 {
            return None;
        }
        match isha {
            Isha::Angle if isha_value > 300 => return None,
            Isha::Minutes if isha_value > 180 => return None,
            _ => {}
        }
        let asr_shadow = p[po::ASR];
        if !(1..=2).contains(&asr_shadow) {
            return None;
        }
        let high_lat = HighLat::from_u8(p[po::HIGH_LAT])?;

        let prayers_on = p[po::PRAYERS];
        if prayers_on & !0b1_1111 != 0 {
            return None;
        }
        let mut offset_min = [0i8; 5];
        for (i, o) in offset_min.iter_mut().enumerate() {
            let v = p[po::OFFSETS + i] as i8;
            if !(-120..=120).contains(&v) {
                return None;
            }
            *o = v;
        }

        let fajr_ramp_min = p[po::RAMP];
        let adhan_volume = p[po::VOLUME];
        let adhan_fade_in_s = p[po::FADE];
        if fajr_ramp_min > 120 || adhan_volume > 100 || !(0..=120).contains(&adhan_fade_in_s) {
            return None;
        }

        let mut alarms = [UserAlarm::EMPTY; MAX_USER_ALARMS];
        for (i, a) in alarms.iter_mut().enumerate() {
            let at = po::ALARMS + i * po::ALARM_STRIDE;
            let flags = p[at];
            let minute = at_u16(p, at + 2)?;
            if flags & !0b11 != 0
                || p[at + 1] & 0x80 != 0
                || minute >= 1440
                || p[at + 4] as usize >= SOUNDS.len()
            {
                return None;
            }
            *a = UserAlarm {
                used: flags & 1 != 0,
                enabled: flags & 2 != 0,
                days: p[at + 1],
                minute,
                sound: p[at + 4],
            };
        }

        let brightness_max = p[po::BRIGHT_MAX];
        let brightness_night = p[po::BRIGHT_NIGHT];
        if brightness_max > 100 || brightness_night > 100 {
            return None;
        }
        if p[po::FACE] & !1 != 0 || p[po::LAMP] & !1 != 0 {
            return None;
        }
        let snooze_minute = match p[po::SNOOZE_FLAGS] {
            0 => None,
            1 => Some(at_u16(p, po::SNOOZE_MIN)?),
            _ => return None,
        };
        if snooze_minute.unwrap_or(0) >= 1440 {
            return None;
        }

        Some(Stored {
            seq: u16::from_le_bytes([b[6], b[7]]),
            settings: Settings {
                setup,
                ssid,
                psk,
                utc_offset_min,
                dst,
                lat_e2,
                lon_e2,
                place,
                fajr_angle_d10,
                isha,
                isha_value,
                asr_shadow,
                high_lat,
                prayers_on,
                offset_min,
                fajr_ramp_min,
                adhan_volume,
                adhan_fade_in_s,
                alarms,
                brightness_max,
                brightness_night,
                clock_24h: p[po::FACE] & 1 != 0,
                lamp_on: p[po::LAMP] & 1 != 0,
                snooze_minute,
            },
        })
    }

    /// The block to load: the valid one with the higher sequence number. On a tie, and only then,
    /// the first argument wins, which is how a sector written by the PC tool takes effect.
    pub fn newest(a: Option<Stored>, b: Option<Stored>) -> Option<Stored> {
        match (a, b) {
            (Some(x), Some(y)) => Some(if newer(x.seq, y.seq) { x } else { y }),
            (x, None) => x,
            (None, y) => y,
        }
    }

    /// Which sector a save overwrites, and the sequence number it writes there: never the sector
    /// holding the newest good block, so the previous settings survive a failed write.
    pub fn save_target(a: Option<&Stored>, b: Option<&Stored>) -> (Sector, u16) {
        match (a, b) {
            (None, None) => (Sector::A, 1),
            (Some(x), None) => (Sector::B, x.seq.wrapping_add(1)),
            (None, Some(y)) => (Sector::A, y.seq.wrapping_add(1)),
            (Some(x), Some(y)) if newer(x.seq, y.seq) => (Sector::B, x.seq.wrapping_add(1)),
            (Some(_), Some(y)) => (Sector::A, y.seq.wrapping_add(1)),
        }
    }
}

/// Which of the two sectors a save writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sector {
    A,
    B,
}

impl Sector {
    pub fn addr(self) -> u32 {
        match self {
            Sector::A => SECTOR_A,
            Sector::B => SECTOR_B,
        }
    }
}

/// True when `x` is the later of two sequence numbers, wrapping at 65535.
fn newer(x: u16, y: u16) -> bool {
    (x.wrapping_sub(y) as i16) >= 0
}

fn label(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    set_str(&mut out, s);
    out
}

fn set_str<const N: usize>(dst: &mut [u8; N], s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() > N - 1 {
        return false;
    }
    dst.fill(0);
    dst[..b.len()].copy_from_slice(b);
    true
}

fn str_of(b: &[u8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..end]).unwrap_or("")
}

fn at<const N: usize>(p: &[u8], off: usize) -> Option<[u8; N]> {
    p.get(off..off + N)?.try_into().ok()
}

fn at_u16(p: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(at::<2>(p, off)?))
}

fn at_i16(p: &[u8], off: usize) -> Option<i16> {
    Some(i16::from_le_bytes(at::<2>(p, off)?))
}

/// A stored string must end in a NUL we can see, and say something readable.
fn nul_terminated_utf8(b: &[u8]) -> bool {
    match b.iter().position(|&c| c == 0) {
        Some(end) => core::str::from_utf8(&b[..end]).is_ok(),
        None => false,
    }
}

/// CRC-32 (the zlib polynomial), so a block that lost a bit is never believed.
fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Settings {
        Settings { prayers_on: 0b1_0101, ..Settings::default() }
    }

    #[test]
    fn default_is_fajr_only_with_ramp_and_sound() {
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        let n = Settings::default().rules(&mut out);
        assert_eq!(n, 2);
        assert!(matches!(out[0].action, Action::Sunrise { ramp_min: 15 }));
        assert!(matches!(out[1].action, Action::Play { track: 1, .. }));
        assert_eq!(Settings::default().setup, Setup::NONE);
        assert_eq!(Settings::default().place_name(), "Cairo");
    }

    #[test]
    fn all_prayers_and_all_alarms_fit_the_rule_buffer() {
        let mut s = Settings { prayers_on: 0b1_1111, ..Settings::default() };
        for a in s.alarms.iter_mut() {
            *a = UserAlarm { used: true, enabled: true, days: SUN_TO_THU, ..UserAlarm::EMPTY };
        }
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        assert_eq!(s.rules(&mut out), MAX_RULES);
        assert!(matches!(out[5].action, Action::Play { track: 5, .. })); // Isha is track 5
        assert!(matches!(out[6].action, Action::Alarm));
    }

    #[test]
    fn disabled_and_unused_alarms_make_no_rules() {
        let mut s = Settings::default();
        s.alarms[0] = UserAlarm { used: true, enabled: false, minute: 400, ..UserAlarm::EMPTY };
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        assert_eq!(s.rules(&mut out), 2);
    }

    #[test]
    fn a_prayer_shift_reaches_its_rule() {
        let mut s = Settings::default();
        s.offset_min[Prayer::Fajr.index()] = -30;
        s.fajr_ramp_min = 20;
        let mut out = [Rule { days: 0, when: When::Local(0), action: Action::Alarm }; MAX_RULES];
        assert_eq!(s.rules(&mut out), 2);
        assert!(matches!(out[0].when, When::Prayer(Prayer::Fajr, -30)));
        assert!(matches!(out[0].action, Action::Sunrise { ramp_min: 20 }));
        assert!(matches!(out[1].when, When::Prayer(Prayer::Fajr, -30)));
    }

    #[test]
    fn stored_block_round_trips_and_rejects_damage() {
        let mut s = sample();
        s.alarms[3] = UserAlarm {
            used: true,
            enabled: true,
            days: FRI_SAT,
            minute: 1439,
            sound: 2,
        };
        s.set_ssid("Home").then_some(()).unwrap();
        s.set_psk("hunter22").then_some(()).unwrap();
        s.set_place("Nasr City").then_some(()).unwrap();
        s.offset_min[Prayer::Dhuhr.index()] = 5;
        s.clock_24h = true;
        s.lamp_on = true;
        s.snooze_minute = Some(6 * 60 + 5);
        let st = Stored { seq: 7, settings: s };
        let b = st.encode();
        assert_eq!(Stored::decode(&b), Some(st));

        let mut bad = b;
        bad[20] ^= 1;
        assert_eq!(Stored::decode(&bad), None); // one flipped bit in the payload

        assert_eq!(Stored::decode(&[0xFF; STORED_LEN]), None); // erased flash
        assert_eq!(Stored::decode(&[0x00; STORED_LEN]), None);
        assert_eq!(Stored::decode(&b[..10]), None); // shorter than a header
        assert_eq!(Stored::decode(&b[..STORED_LEN - 1]), None); // the block is cut short
    }

    #[test]
    fn the_layout_stays_where_the_table_says() {
        let mut s = Settings::default();
        s.set_ssid("Hi").then_some(()).unwrap();
        s.set_psk("k").then_some(()).unwrap();
        s.utc_offset_min = -210;
        s.dst = Dst::Us;
        s.lat_e2 = 3004;
        s.lon_e2 = -3124;
        s.fajr_angle_d10 = 185;
        s.isha = Isha::Minutes;
        s.isha_value = 90;
        s.asr_shadow = 2;
        s.high_lat = HighLat::Seventh;
        s.offset_min = [1, -1, 2, -2, 0];
        s.fajr_ramp_min = 20;
        s.adhan_volume = 44;
        s.adhan_fade_in_s = 60;
        s.alarms[0] =
            UserAlarm { used: true, enabled: true, days: FRI_SAT, minute: 90, sound: 2 };
        s.brightness_max = 70;
        s.brightness_night = 6;
        s.clock_24h = true;
        s.lamp_on = true;
        s.snooze_minute = Some(390);
        let b = Stored { seq: 0, settings: s }.encode();

        assert_eq!(STORED_LEN, 212);
        assert_eq!(&b[0..4], b"WKS1");
        assert_eq!((b[4], b[5]), (1, 200));
        assert_eq!(&b[13..15], b"Hi");
        assert_eq!(b[46], b'k');
        assert_eq!(b[110..112], (-210i16).to_le_bytes());
        assert_eq!(b[112], 3);
        assert_eq!(b[113..115], 3004i16.to_le_bytes());
        assert_eq!(b[115..117], (-3124i16).to_le_bytes());
        assert_eq!(&b[117..122], b"Cairo");
        assert_eq!(b[133..135], 185u16.to_le_bytes());
        assert_eq!(b[135], 1);
        assert_eq!(b[136..138], 90u16.to_le_bytes());
        assert_eq!(b[138], 2);
        assert_eq!(b[139], 2);
        assert_eq!(b[141..146], [1, 0xFF, 2, 0xFE, 0]);
        assert_eq!((b[146], b[147], b[148]), (20, 44, 60));
        assert_eq!(b[149], 0b11);
        assert_eq!(b[150], FRI_SAT);
        assert_eq!(b[151..153], 90u16.to_le_bytes());
        assert_eq!(b[153], 2);
        assert_eq!((b[189], b[190], b[191], b[192]), (70, 6, 1, 1));
        assert_eq!(b[193], 1);
        assert_eq!(b[194..196], 390u16.to_le_bytes());
        assert!(b[196..212].iter().all(|&x| x == 0)); // reserved
    }

    #[test]
    fn a_longer_block_from_a_newer_build_still_loads() {
        let st = Stored { seq: 4, settings: sample() };
        let mut long = [0x11u8; HEADER_LEN + PAYLOAD_LEN + 8];
        long[..STORED_LEN].copy_from_slice(&st.encode());
        long[5] = (PAYLOAD_LEN + 8) as u8; // a field this build does not know
        let crc = crc32(&long[HEADER_LEN..]);
        long[8..12].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(Stored::decode(&long), Some(st));
    }

    #[test]
    fn out_of_range_fields_are_rejected() {
        let mut s = sample();
        let good = Stored { seq: 1, settings: s };
        s.brightness_max = 101;
        assert_eq!(Stored::decode(&Stored { seq: 1, settings: s }.encode()), None);
        s = good.settings;
        s.utc_offset_min = 900;
        assert_eq!(Stored::decode(&Stored { seq: 1, settings: s }.encode()), None);
        s = good.settings;
        s.alarms[2].minute = 1440;
        assert_eq!(Stored::decode(&Stored { seq: 1, settings: s }.encode()), None);
        s = good.settings;
        s.alarms[2].sound = 3;
        assert_eq!(Stored::decode(&Stored { seq: 1, settings: s }.encode()), None);
        s = good.settings;
        s.prayers_on = 0b10_0000;
        assert_eq!(Stored::decode(&Stored { seq: 1, settings: s }.encode()), None);
        assert_eq!(Stored::decode(&good.encode()), Some(good));
    }

    #[test]
    fn strings_round_trip_and_too_long_is_refused() {
        let mut s = Settings::default();
        assert!(s.set_ssid(&"a".repeat(32)));
        assert!(!s.set_ssid(&"a".repeat(33)));
        assert_eq!(s.ssid().len(), 32);
        assert!(s.set_place(&"b".repeat(15)));
        assert!(!s.set_place(&"b".repeat(16)));
        assert_eq!(s.place_name().len(), 15);
        assert!(s.set_psk(""));
        assert_eq!(s.psk(), "");
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn newest_takes_the_higher_sequence_and_wraps() {
        let a = Stored { seq: 9, settings: Settings::default() };
        let b = Stored { seq: 10, settings: sample() };
        assert_eq!(Stored::newest(Some(a), Some(b)), Some(b));
        assert_eq!(Stored::newest(Some(a), None), Some(a));
        assert_eq!(Stored::newest(None, Some(b)), Some(b));
        assert_eq!(Stored::newest(None, None), None);
        let old = Stored { seq: 65535, settings: Settings::default() };
        assert_eq!(Stored::newest(Some(a), Some(old)), Some(a)); // 9 is newer than 65535
        assert_eq!(Stored::newest(Some(a), Some(a)), Some(a)); // a tie goes to the first
    }

    #[test]
    fn a_save_never_overwrites_the_newest_block() {
        let a = Stored { seq: 3, settings: Settings::default() };
        let b = Stored { seq: 4, settings: sample() };
        assert_eq!(Stored::save_target(None, None), (Sector::A, 1));
        assert_eq!(Stored::save_target(Some(&a), None), (Sector::B, 4));
        assert_eq!(Stored::save_target(None, Some(&b)), (Sector::A, 5));
        assert_eq!(Stored::save_target(Some(&a), Some(&b)), (Sector::A, 5));
        assert_eq!(Stored::save_target(Some(&b), Some(&a)), (Sector::B, 5));
        assert_eq!(Sector::A.addr(), 0x9000);
        assert_eq!(Sector::B.addr(), 0xA000);
    }
}
