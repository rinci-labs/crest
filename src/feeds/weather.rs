//! Weather (optional, off by default) from Open-Meteo (open-meteo.com), with no account and
//! no API key. The only things sent are the city name the user typed (once, to look up its
//! coordinates) and then that city's center coordinates. The device location is never read.

use std::thread;

use windows::Data::Json::JsonObject;
use windows::core::{HSTRING, Result};

use crate::system::http::{self, encode};
use crate::system::services::{Event, Notifier};
use crate::shell::store;

const FILE: &str = "city.txt";
const USER_AGENT: &str = concat!("Crest/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rinci-labs/crest)");

#[derive(Clone, Debug, PartialEq)]
pub struct City {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

impl City {
    /// The saved city, if one was ever set.
    pub fn load() -> Option<City> {
        store::load(FILE).first().and_then(|line| City::parse(line))
    }

    pub fn save(&self) {
        store::save(FILE, [format!("{};{:.4};{:.4}", self.name.replace(';', ","), self.lat, self.lon)]);
    }

    fn parse(line: &str) -> Option<City> {
        let mut parts = line.split(';');
        let name = parts.next()?.trim().to_string();
        let lat = parts.next()?.trim().parse().ok()?;
        let lon = parts.next()?.trim().parse().ok()?;
        (!name.is_empty()).then_some(City { name, lat, lon })
    }
}

#[derive(Clone, Debug)]
pub struct Weather {
    /// °C
    pub temp: f64,
    /// WMO weather code.
    pub code: u32,
}

/// Short description for a WMO weather code.
pub fn describe(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 | 80..=82 => "Rain",
        71..=77 | 85 | 86 => "Snow",
        95..=99 => "Thunderstorm",
        _ => "",
    }
}

/// Look up a city's coordinates on a separate thread; the result arrives as [`Event::City`].
pub fn find_city(name: String, notify: Notifier) {
    spawn("city lookup", Event::City(None), notify, move || {
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
            encode(&name)
        );
        let body = get(&url)?;
        Ok(Event::City(parse_city(&body)))
    });
}

/// Fetch the current weather on a separate thread; the result arrives as [`Event::Weather`].
pub fn fetch(city: &City, notify: Notifier) {
    let (lat, lon) = (city.lat, city.lon);
    spawn("weather", Event::Weather(None), notify, move || {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat:.2}&longitude={lon:.2}&current=temperature_2m,weather_code&timezone=auto"
        );
        Ok(Event::Weather(parse_weather(&get(&url)?)))
    });
}

/// Run `work` on a named thread and send its event, or `failed` if it errors.
fn spawn(name: &str, failed: Event, notify: Notifier, work: impl FnOnce() -> Result<Event> + Send + 'static) {
    let spawned = thread::Builder::new().name(name.into()).spawn(move || match work() {
        Ok(event) => notify.send(event),
        Err(e) => {
            error_log!("failed to fetch weather: {e}");
            notify.send(failed);
        }
    });
    if let Err(e) = spawned {
        error_log!("failed to create weather thread: {e}");
    }
}

fn get(url: &str) -> Result<String> {
    http::get(url, USER_AGENT)
}

fn parse_city(body: &str) -> Option<City> {
    let root = JsonObject::Parse(&HSTRING::from(body)).ok()?;
    let first = root.GetNamedArray(&HSTRING::from("results")).ok()?.GetObjectAt(0).ok()?;
    let text = |key: &str| first.GetNamedString(&HSTRING::from(key)).map(|s| s.to_string()).unwrap_or_default();
    let number = |key: &str| first.GetNamedNumber(&HSTRING::from(key)).ok();
    Some(City { name: text("name"), lat: number("latitude")?, lon: number("longitude")? }).filter(|c| !c.name.is_empty())
}

fn parse_weather(body: &str) -> Option<Weather> {
    let root = JsonObject::Parse(&HSTRING::from(body)).ok()?;
    let current = root.GetNamedObject(&HSTRING::from("current")).ok()?;
    let temp = current.GetNamedNumber(&HSTRING::from("temperature_2m")).ok()?;
    let code = current.GetNamedNumber(&HSTRING::from("weather_code")).ok()? as u32;
    Some(Weather { temp, code })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_meteo_answers() {
        let city = parse_city(r#"{"results":[{"id":1642911,"name":"Jakarta","latitude":-6.21462,"longitude":106.84513,"country":"Indonesia"}]}"#).unwrap();
        assert_eq!(city.name, "Jakarta");
        assert!((city.lat + 6.21462).abs() < 1e-9);
        assert!(parse_city(r#"{"generationtime_ms":0.5}"#).is_none(), "city not found");

        let weather = parse_weather(r#"{"current":{"time":"2026-10-19T19:00","interval":900,"temperature_2m":28.4,"weather_code":3}}"#).unwrap();
        assert_eq!((weather.temp, weather.code, describe(weather.code)), (28.4, 3, "Overcast"));
    }

    /// Real request to Open-Meteo. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn asks_open_meteo() {
        let body = get("https://geocoding-api.open-meteo.com/v1/search?name=Jakarta&count=1&language=en&format=json").unwrap();
        let city = parse_city(&body).expect("Jakarta found");
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={:.2}&longitude={:.2}&current=temperature_2m,weather_code&timezone=auto",
            city.lat, city.lon
        );
        let weather = parse_weather(&get(&url).unwrap()).expect("weather parsed");
        println!("{} {:.4},{:.4}: {:.1}° {}", city.name, city.lat, city.lon, weather.temp, describe(weather.code));
        assert!((-60.0..60.0).contains(&weather.temp));
    }

    #[test]
    fn city_round_trip() {
        let city = City { name: "Yogyakarta".into(), lat: -7.80139, lon: 110.36472 };
        let line = format!("{};{:.4};{:.4}", city.name, city.lat, city.lon);
        assert_eq!(City::parse(&line).unwrap().name, "Yogyakarta");
        assert!(City::parse("no coordinates").is_none());
    }
}
