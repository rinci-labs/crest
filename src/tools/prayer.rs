//! Daily prayer times, computed on this PC from the saved city's coordinates: no network, no
//! downloaded tables. The formulas are the ones PrayTimes.org publishes (sun declination and
//! equation of time from the US Naval Observatory's low-precision solar algorithm), with its
//! angle-based fallback for high latitudes where the sun never sinks far enough for Fajr or Isha.

use crate::tools::agenda::Date;

/// The five prayers, with sunrise between Fajr and Dhuhr (shown, but it has no azan).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prayer {
    Fajr,
    Sunrise,
    Dhuhr,
    Asr,
    Maghrib,
    Isha,
}

impl Prayer {
    pub const ALL: [Prayer; 6] = [Prayer::Fajr, Prayer::Sunrise, Prayer::Dhuhr, Prayer::Asr, Prayer::Maghrib, Prayer::Isha];

    pub fn label(self) -> &'static str {
        match self {
            Prayer::Fajr => "Fajr",
            Prayer::Sunrise => "Sunrise",
            Prayer::Dhuhr => "Dhuhr",
            Prayer::Asr => "Asr",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isha => "Isha",
        }
    }

    /// Sunrise marks the end of Fajr's time; there is no azan for it, so no notice either.
    pub fn has_azan(self) -> bool {
        self != Prayer::Sunrise
    }
}

/// Twilight angle for Isha, or a fixed delay after Maghrib (Umm al-Qura).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Isha {
    Angle(f64),
    Minutes(f64),
}

/// Calculation convention: the sun's depression angle for Fajr and Isha, as used by the
/// authority in each region.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Method {
    /// Kementerian Agama RI (Indonesia): 20° / 18°, plus 2 minutes of ihtiyat.
    #[default]
    Kemenag,
    /// Muslim World League: 18° / 17°.
    Mwl,
    /// Islamic Society of North America: 15° / 15°.
    Isna,
    /// Egyptian General Authority of Survey: 19.5° / 17.5°.
    Egypt,
    /// Umm al-Qura, Makkah: 18.5°, Isha 90 minutes after Maghrib.
    Makkah,
    /// University of Islamic Sciences, Karachi: 18° / 18°.
    Karachi,
}

impl Method {
    pub const ALL: [Method; 6] = [Method::Kemenag, Method::Mwl, Method::Isna, Method::Egypt, Method::Makkah, Method::Karachi];

    /// Short name for the panel.
    pub fn label(self) -> &'static str {
        match self {
            Method::Kemenag => "Kemenag",
            Method::Mwl => "MWL",
            Method::Isna => "ISNA",
            Method::Egypt => "Egypt",
            Method::Makkah => "Makkah",
            Method::Karachi => "Karachi",
        }
    }

    /// Full name for the tray menu.
    pub fn title(self) -> &'static str {
        match self {
            Method::Kemenag => "Kemenag (Indonesia)",
            Method::Mwl => "Muslim World League",
            Method::Isna => "ISNA (North America)",
            Method::Egypt => "Egyptian General Authority",
            Method::Makkah => "Umm al-Qura (Makkah)",
            Method::Karachi => "Karachi",
        }
    }

    /// Name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Method::Kemenag => "kemenag",
            Method::Mwl => "mwl",
            Method::Isna => "isna",
            Method::Egypt => "egypt",
            Method::Makkah => "makkah",
            Method::Karachi => "karachi",
        }
    }

    /// The next method, for a button that steps through them.
    pub fn next(self) -> Method {
        let at = Method::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Method::ALL[(at + 1) % Method::ALL.len()]
    }

    fn fajr(self) -> f64 {
        match self {
            Method::Kemenag => 20.0,
            Method::Mwl | Method::Karachi => 18.0,
            Method::Isna => 15.0,
            Method::Egypt => 19.5,
            Method::Makkah => 18.5,
        }
    }

    fn isha(self) -> Isha {
        match self {
            Method::Kemenag | Method::Karachi => Isha::Angle(18.0),
            Method::Mwl => Isha::Angle(17.0),
            Method::Isna => Isha::Angle(15.0),
            Method::Egypt => Isha::Angle(17.5),
            Method::Makkah => Isha::Minutes(90.0),
        }
    }

    /// Safety margin (ihtiyat) in minutes: later for every prayer, earlier for sunrise.
    fn ihtiyat(self) -> i64 {
        match self {
            Method::Kemenag => 2,
            _ => 0,
        }
    }
}

/// School for the Asr shadow length.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Madhab {
    /// Shafi'i, Maliki, Hanbali: shadow equal to the object's length.
    #[default]
    Standard,
    /// Hanafi: shadow twice the object's length.
    Hanafi,
}

impl Madhab {
    pub const ALL: [Madhab; 2] = [Madhab::Standard, Madhab::Hanafi];

    pub fn label(self) -> &'static str {
        match self {
            Madhab::Standard => "Standard",
            Madhab::Hanafi => "Hanafi",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Madhab::Standard => "standard",
            Madhab::Hanafi => "hanafi",
        }
    }

    fn shadow(self) -> f64 {
        match self {
            Madhab::Standard => 1.0,
            Madhab::Hanafi => 2.0,
        }
    }
}

/// Reminder choices before each azan, in minutes; 0 = only when the azan time arrives.
pub const LEADS: [u32; 5] = [0, 5, 10, 15, 30];
pub const DEFAULT_LEAD: u32 = 10;

/// Sun altitude at sunrise and sunset: refraction plus the sun's radius.
const HORIZON: f64 = 0.833;

/// Times for `date` at latitude `lat` and longitude `lon` (degrees), as seconds after midnight
/// UTC at the start of that date (negative or past a day when the zone is far from UTC).
/// `None` where the sun never reaches the angle, e.g. sunrise during the polar night.
pub fn utc_seconds(date: Date, lat: f64, lon: f64, method: Method, madhab: Madhab) -> [Option<i64>; 6] {
    let sun = Sun { jd: 2_440_587.5 + date.days() as f64 - lon / 360.0, lat };
    // First guesses in hours, refined once with the sun's position at those times.
    let guess = [5.0, 6.0, 12.0, 13.0, 18.0, 18.0, 18.0];
    let pass = |t: [f64; 7]| {
        let isha = match method.isha() {
            Isha::Angle(angle) => sun.angle_time(angle, t[6] / 24.0, false),
            Isha::Minutes(_) => f64::NAN,
        };
        [
            sun.angle_time(method.fajr(), t[0] / 24.0, true),
            sun.angle_time(HORIZON, t[1] / 24.0, true),
            sun.noon(t[2] / 24.0),
            sun.asr(madhab.shadow(), t[3] / 24.0),
            sun.angle_time(HORIZON, t[4] / 24.0, false),
            sun.angle_time(HORIZON, t[5] / 24.0, false),
            isha,
        ]
    };
    let first = pass(guess);
    let refine = first.map(|h| if h.is_nan() { 12.0 } else { h });
    let [mut fajr, sunrise, dhuhr, asr, sunset, maghrib, mut isha] = pass(refine);
    if !sunrise.is_nan() && !sunset.is_nan() {
        // High latitudes: a twilight that never ends, or lasts improbably long, is replaced by
        // a share of the night proportional to the angle.
        let night = (sunrise - sunset).rem_euclid(24.0);
        let portion = |angle: f64| angle / 60.0 * night;
        if fajr.is_nan() || (sunrise - fajr).rem_euclid(24.0) > portion(method.fajr()) {
            fajr = sunrise - portion(method.fajr());
        }
        match method.isha() {
            Isha::Angle(angle) if isha.is_nan() || (isha - sunset).rem_euclid(24.0) > portion(angle) => {
                isha = sunset + portion(angle);
            }
            Isha::Minutes(minutes) => isha = maghrib + minutes / 60.0,
            Isha::Angle(_) => {}
        }
    }
    let shift = -lon / 15.0;
    let margin = method.ihtiyat();
    let hours = [fajr, sunrise, dhuhr, asr, maghrib, isha];
    let mut out = [None; 6];
    for (i, h) in hours.into_iter().enumerate() {
        if h.is_nan() {
            continue;
        }
        // Whole minutes, like every published timetable, then the margin on top.
        let minute = ((h + shift) * 60.0).round() as i64;
        let margin = if Prayer::ALL[i] == Prayer::Sunrise { -margin } else { margin };
        out[i] = Some((minute + margin) * 60);
    }
    out
}

/// The sun's position for one day at one latitude.
struct Sun {
    /// Julian date at local midnight.
    jd: f64,
    lat: f64,
}

impl Sun {
    /// Declination (degrees) and equation of time (hours) at day fraction `t`.
    fn position(&self, t: f64) -> (f64, f64) {
        let d = self.jd + t - 2_451_545.0;
        let g = (357.529 + 0.985_600_28 * d).rem_euclid(360.0);
        let q = (280.459 + 0.985_647_36 * d).rem_euclid(360.0);
        let l = (q + 1.915 * sin(g) + 0.020 * sin(2.0 * g)).rem_euclid(360.0);
        let e = 23.439 - 0.000_000_36 * d;
        let ra = (cos(e) * sin(l)).atan2(cos(l)).to_degrees() / 15.0;
        let equation = q / 15.0 - ra.rem_euclid(24.0);
        let declination = (sin(e) * sin(l)).asin().to_degrees();
        (declination, equation)
    }

    /// Solar noon (hours, local mean time).
    fn noon(&self, t: f64) -> f64 {
        (12.0 - self.position(t).1).rem_euclid(24.0)
    }

    /// When the sun is `angle` degrees below the horizon: before noon when `morning`.
    fn angle_time(&self, angle: f64, t: f64, morning: bool) -> f64 {
        let (decl, _) = self.position(t);
        let noon = self.noon(t);
        let cos_h = (-sin(angle) - sin(decl) * sin(self.lat)) / (cos(decl) * cos(self.lat));
        // Outside -1..1 the sun never gets there today; acos gives NaN, which is what we want.
        let h = cos_h.acos().to_degrees() / 15.0;
        if morning { noon - h } else { noon + h }
    }

    /// Asr: when a shadow is `shadow` times its object's length plus the noon shadow.
    fn asr(&self, shadow: f64, t: f64) -> f64 {
        let (decl, _) = self.position(t);
        let angle = -(1.0 / (shadow + tan((self.lat - decl).abs()))).atan().to_degrees();
        self.angle_time(angle, t, false)
    }
}

fn sin(deg: f64) -> f64 {
    deg.to_radians().sin()
}

fn cos(deg: f64) -> f64 {
    deg.to_radians().cos()
}

fn tan(deg: f64) -> f64 {
    deg.to_radians().tan()
}

/// One day's times as local minutes after midnight, with tomorrow's Fajr for the evening.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Day {
    /// Local date (days since 1970) these times are for.
    pub day: i64,
    pub minutes: [Option<u32>; 6],
    pub tomorrow_fajr: Option<u32>,
}

impl Day {
    /// The coming prayer with an azan at or after `now` (minutes), and minutes until it. After
    /// Isha that is tomorrow's Fajr.
    pub fn next(&self, now: u32) -> Option<(Prayer, u32, u32)> {
        let upcoming = Prayer::ALL
            .iter()
            .zip(self.minutes)
            .filter(|(p, _)| p.has_azan())
            .find_map(|(p, m)| m.filter(|m| *m > now).map(|m| (*p, m, m - now)));
        upcoming.or_else(|| self.tomorrow_fajr.map(|m| (Prayer::Fajr, m, m + 24 * 60 - now)))
    }

    /// A notice due exactly at `now`: the prayer and how many minutes ahead it is (0 = azan).
    /// The reminder `lead` minutes before comes first; with `lead` 0 only the azan itself.
    pub fn due(&self, now: u32, lead: u32) -> Option<(Prayer, u32)> {
        Prayer::ALL.iter().zip(self.minutes).filter(|(p, _)| p.has_azan()).find_map(|(p, m)| {
            let m = m?;
            if m == now {
                Some((*p, 0))
            } else if lead > 0 && m >= lead && m - lead == now {
                Some((*p, lead))
            } else {
                None
            }
        })
    }
}

/// "05:12"
pub fn clock(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

/// "in 1 h 12 min", "in 8 min", "now".
pub fn countdown(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, 0) => "now".to_string(),
        (0, m) => format!("in {m} min"),
        (h, 0) => format!("in {h} h"),
        (h, m) => format!("in {h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local "HH:MM" for a fixed UTC offset in hours.
    fn local(seconds: Option<i64>, offset: i64) -> String {
        let minute = (seconds.expect("time exists") / 60 + offset * 60).rem_euclid(24 * 60) as u32;
        clock(minute)
    }

    fn day_at(date: Date, lat: f64, lon: f64, method: Method, madhab: Madhab, offset: i64) -> Vec<String> {
        utc_seconds(date, lat, lon, method, madhab).iter().map(|s| local(*s, offset)).collect()
    }

    /// Jakarta (6°10' S, 106°49' E), UTC+7, against the published Kemenag-based timetable for
    /// October 2026 (Subuh, Terbit, Dzuhur, Ashar, Maghrib, Isya), within a minute of rounding.
    #[test]
    fn jakarta_matches_the_published_timetable() {
        let published = [
            (1, ["04:23", "05:36", "11:45", "14:51", "17:49", "18:59"]),
            // Sunrise is not in the source for this day.
            (10, ["04:18", "", "11:42", "14:43", "17:48", "18:58"]),
        ];
        let minutes = |t: &str| t[..2].parse::<i32>().unwrap() * 60 + t[3..].parse::<i32>().unwrap();
        for (day, expected) in published {
            let times = day_at(Date::new(2026, 10, day), -6.1667, 106.8167, Method::Kemenag, Madhab::Standard, 7);
            for (got, want) in times.iter().zip(expected).filter(|(_, want)| !want.is_empty()) {
                assert!((minutes(got) - minutes(want)).abs() <= 1, "Oct {day}: got {times:?}, want {expected:?}");
            }
        }
    }

    #[test]
    fn times_run_in_order_and_methods_differ() {
        let date = Date::new(2026, 3, 20);
        for method in Method::ALL {
            for madhab in Madhab::ALL {
                let s = utc_seconds(date, 21.4225, 39.8262, method, madhab);
                let s: Vec<i64> = s.iter().map(|t| t.expect("every time exists in Makkah")).collect();
                assert!(s.windows(2).all(|w| w[0] < w[1]), "{method:?} {madhab:?}: {s:?}");
            }
        }
        let asr = |m| utc_seconds(date, 21.4225, 39.8262, Method::Mwl, m)[3].unwrap();
        assert!(asr(Madhab::Hanafi) > asr(Madhab::Standard), "Hanafi Asr is later");
        let makkah = utc_seconds(date, 21.4225, 39.8262, Method::Makkah, Madhab::Standard);
        assert_eq!(makkah[5].unwrap() - makkah[4].unwrap(), 90 * 60, "Isha 90 min after Maghrib");
    }

    /// London at midsummer: the sun never sinks 18° below the horizon, so Fajr and Isha come
    /// from the night-portion fallback instead of disappearing.
    #[test]
    fn high_latitudes_still_get_fajr_and_isha() {
        let s = utc_seconds(Date::new(2026, 6, 21), 51.5074, -0.1278, Method::Mwl, Madhab::Standard);
        assert!(s.iter().all(Option::is_some), "{s:?}");
        // Svalbard in the polar night has no sunrise at all.
        let polar = utc_seconds(Date::new(2026, 12, 21), 78.22, 15.65, Method::Mwl, Madhab::Standard);
        assert!(polar[1].is_none());
    }

    #[test]
    fn next_and_due() {
        let day = Day { day: 0, minutes: [Some(244), Some(320), Some(698), Some(890), Some(1067), Some(1137)], tomorrow_fajr: Some(243) };
        assert_eq!(day.next(300), Some((Prayer::Dhuhr, 698, 398)), "sunrise is skipped");
        assert_eq!(day.next(1200), Some((Prayer::Fajr, 243, 243 + 1440 - 1200)), "after Isha comes tomorrow's Fajr");
        assert_eq!(day.due(890 - 10, 10), Some((Prayer::Asr, 10)));
        assert_eq!(day.due(890, 10), Some((Prayer::Asr, 0)));
        assert_eq!(day.due(320, 10), None, "no notice for sunrise");
        assert_eq!(day.due(880, 0), None, "no early reminder when the lead is 0");
        assert_eq!(countdown(72), "in 1 h 12 min");
        assert_eq!(countdown(60), "in 1 h");
        assert_eq!(countdown(8), "in 8 min");
        assert_eq!(Method::Karachi.next(), Method::Kemenag);
    }
}
