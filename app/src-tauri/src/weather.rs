//! Weather and air quality from Open-Meteo (free, no API key): place search for
//! the settings and a refresh every 15 minutes for the `{TEMP}` / `{AQI}`-style
//! design tokens.
//! docs/ARCHITECTURE.md, "Weather".

use std::sync::Arc;
use std::time::{Duration, Instant};

use deskbunny_core::layout::Weather;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::live::Live;

const REFRESH: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    /// Country and region, for telling places apart in the list.
    #[serde(default)]
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
}

// Coordinates are compared exactly; they only ever come from a search result.
impl Eq for Place {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

/// Which air quality index to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AqiScale {
    /// US EPA, 0-500.
    #[default]
    Us,
    /// European EEA, 0-100+.
    Eu,
    /// India's National AQI (CPCB), 0-500, computed here from pollutant
    /// averages (Open-Meteo does not publish it).
    In,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WeatherCfg {
    pub place: Option<Place>,
    pub units: Units,
    pub aqi: AqiScale,
}

/// The latest weather and how fetching it went.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherNow {
    pub weather: Option<Weather>,
    pub error: Option<String>,
    /// Seconds since the last successful update (filled in on read).
    pub age_secs: Option<u64>,
    #[serde(skip)]
    pub fetched: Option<Instant>,
    #[serde(skip)]
    pub for_cfg: Option<WeatherCfg>,
}

fn get(url: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut req = ureq::get(url).timeout(TIMEOUT);
    for (k, v) in query {
        req = req.query(k, v);
    }
    req.call()
        .map_err(|e| format!("Could not reach Open-Meteo: {e}"))?
        .into_json::<Value>()
        .map_err(|e| format!("Open-Meteo sent something unexpected: {e}"))
}

pub fn search(q: &str) -> Result<Vec<Place>, String> {
    let q = q.trim();
    if q.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let v = get(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[
            ("name", q),
            ("count", "8"),
            ("language", "en"),
            ("format", "json"),
        ],
    )?;
    Ok(parse_places(&v))
}

fn parse_places(v: &Value) -> Vec<Place> {
    let Some(list) = v.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|r| {
            let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let detail = [s("admin1"), s("country")]
                .into_iter()
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            Some(Place {
                name: s("name"),
                detail,
                lat: r.get("latitude")?.as_f64()?,
                lon: r.get("longitude")?.as_f64()?,
            })
        })
        .collect()
}

pub fn fetch(cfg: &WeatherCfg) -> Result<Weather, String> {
    let place = cfg.place.as_ref().ok_or("No place chosen.")?;
    let (lat, lon) = (place.lat.to_string(), place.lon.to_string());
    let mut q = vec![
        ("latitude", lat.as_str()),
        ("longitude", lon.as_str()),
        (
            "current",
            "temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m,is_day",
        ),
        ("daily", "temperature_2m_max,temperature_2m_min"),
        ("timezone", "auto"),
        ("forecast_days", "1"),
    ];
    if cfg.units == Units::Imperial {
        q.push(("temperature_unit", "fahrenheit"));
        q.push(("wind_speed_unit", "mph"));
    }
    let v = get("https://api.open-meteo.com/v1/forecast", &q)?;
    let mut w = parse_forecast(&v, place, cfg.units)?;
    // Air quality is a separate service; without it the weather still shows.
    let air = get(
        "https://air-quality-api.open-meteo.com/v1/air-quality",
        &[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("current", "us_aqi,european_aqi,pm2_5,pm10"),
            (
                "hourly",
                "pm10,pm2_5,carbon_monoxide,nitrogen_dioxide,sulphur_dioxide,ozone",
            ),
            ("past_days", "1"),
            ("forecast_days", "1"),
            ("timezone", "auto"),
        ],
    );
    match air {
        Ok(a) => apply_air(&mut w, &a, cfg.aqi),
        Err(e) => log::warn!("air quality: {e}"),
    }
    Ok(w)
}

// --- India's National AQI (CPCB) ------------------------------------------

/// CPCB breakpoints: concentration ranges for the AQI bands 0-50, 51-100,
/// 101-200, 201-300, 301-400, 401-500. The last band has no published top; it
/// ends where the index reaches 500 at these (commonly used) values.
const NAQI_BANDS: [(u16, u16); 6] = [
    (0, 50),
    (51, 100),
    (101, 200),
    (201, 300),
    (301, 400),
    (401, 500),
];
/// (Open-Meteo hourly key, hours averaged, µg/m³ -> unit factor, band tops)
const NAQI_POLLUTANTS: [(&str, usize, f64, [f64; 6]); 6] = [
    ("pm10", 24, 1.0, [50.0, 100.0, 250.0, 350.0, 430.0, 600.0]),
    ("pm2_5", 24, 1.0, [30.0, 60.0, 90.0, 120.0, 250.0, 380.0]),
    (
        "nitrogen_dioxide",
        24,
        1.0,
        [40.0, 80.0, 180.0, 280.0, 400.0, 800.0],
    ),
    (
        "sulphur_dioxide",
        24,
        1.0,
        [40.0, 80.0, 380.0, 800.0, 1600.0, 2100.0],
    ),
    // CO is in mg/m³ in the CPCB table.
    (
        "carbon_monoxide",
        8,
        0.001,
        [1.0, 2.0, 10.0, 17.0, 34.0, 50.0],
    ),
    ("ozone", 8, 1.0, [50.0, 100.0, 168.0, 208.0, 748.0, 1000.0]),
];

/// The CPCB sub-index for a concentration on one pollutant's breakpoints.
fn naqi_sub(c: f64, tops: &[f64; 6]) -> f64 {
    let mut lo = 0.0;
    for (i, &hi) in tops.iter().enumerate() {
        if c <= hi || i == tops.len() - 1 {
            let (ilo, ihi) = NAQI_BANDS[i];
            let frac = ((c - lo) / (hi - lo)).clamp(0.0, 1.0);
            return f64::from(ilo) + frac * f64::from(ihi - ilo);
        }
        lo = hi;
    }
    500.0
}

/// India's AQI from Open-Meteo's hourly series: each pollutant averaged over
/// its window (24 h, or 8 h for CO and O3) ending at the current hour, the
/// highest sub-index wins. Needs PM2.5 or PM10 and three pollutants in all,
/// as CPCB does.
fn naqi(v: &Value) -> Option<u16> {
    let h = v.get("hourly")?;
    let times: Vec<&str> = h
        .get("time")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let now = v
        .get("current")
        .and_then(|c| c.get("time"))
        .and_then(Value::as_str)?;
    // ISO times compare as strings: the last hour not after now.
    let end = times.iter().rposition(|t| *t <= now)?;
    let mut subs = Vec::new();
    let mut has_pm = false;
    for (key, hours, factor, tops) in NAQI_POLLUTANTS {
        let Some(series) = h.get(key).and_then(Value::as_array) else {
            continue;
        };
        let start = (end + 1).saturating_sub(hours);
        let vals: Vec<f64> = series
            .get(start..=end)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        // At least 3/4 of the window must be there to count.
        if vals.len() * 4 < hours * 3 {
            continue;
        }
        let avg = vals.iter().sum::<f64>() / vals.len() as f64 * factor;
        subs.push(naqi_sub(avg, &tops));
        has_pm |= key.starts_with("pm");
    }
    if !has_pm || subs.len() < 3 {
        return None;
    }
    let max = subs.into_iter().fold(0.0, f64::max);
    Some(max.round().clamp(0.0, 500.0) as u16)
}

fn apply_air(w: &mut Weather, v: &Value, scale: AqiScale) {
    let cur = v.get("current");
    let f = |k: &str| cur.and_then(|c| c.get(k)).and_then(Value::as_f64);
    w.aqi = match scale {
        AqiScale::Us => f("us_aqi").map(|a| a.clamp(0.0, 999.0).round() as u16),
        AqiScale::Eu => f("european_aqi").map(|a| a.clamp(0.0, 999.0).round() as u16),
        AqiScale::In => naqi(v),
    };
    w.aqi_scale = match scale {
        AqiScale::Us => "US",
        AqiScale::Eu => "EU",
        AqiScale::In => "IN",
    }
    .into();
    w.pm25 = f("pm2_5").map(|x| x as f32);
    w.pm10 = f("pm10").map(|x| x as f32);
}

fn parse_forecast(v: &Value, place: &Place, units: Units) -> Result<Weather, String> {
    let bad = || "Open-Meteo's answer had no current weather.".to_string();
    let cur = v.get("current").ok_or_else(bad)?;
    let f = |k: &str| cur.get(k).and_then(Value::as_f64);
    let day = |k: &str| {
        v.get("daily")
            .and_then(|d| d.get(k))
            .and_then(|a| a.get(0))
            .and_then(Value::as_f64)
    };
    let temp = f("temperature_2m").ok_or_else(bad)?;
    Ok(Weather {
        city: place.name.clone(),
        temp: temp as f32,
        tmax: day("temperature_2m_max").unwrap_or(temp) as f32,
        tmin: day("temperature_2m_min").unwrap_or(temp) as f32,
        code: f("weather_code").unwrap_or(0.0).clamp(0.0, 255.0) as u8,
        is_day: f("is_day").unwrap_or(1.0) >= 0.5,
        humidity: f("relative_humidity_2m").unwrap_or(0.0).clamp(0.0, 100.0) as u8,
        wind: f("wind_speed_10m").unwrap_or(0.0) as f32,
        unit: if units == Units::Imperial { "F" } else { "C" }.into(),
        wind_unit: if units == Units::Imperial {
            "mph"
        } else {
            "km/h"
        }
        .into(),
        aqi: None,
        aqi_scale: String::new(),
        pm25: None,
        pm10: None,
    })
}

/// Keep `live`'s weather fresh: on start, when the place or units change,
/// every 15 minutes, and 2 minutes after a failure.
pub fn start(live: Arc<Live>) {
    let spawned = std::thread::Builder::new()
        .name("deskbunny-weather".into())
        .spawn(move || loop {
            let cfg = live.config().weather;
            let now = live.weather_now();
            let changed = now.for_cfg.as_ref() != Some(&cfg);
            let wait = if now.error.is_some() { RETRY } else { REFRESH };
            let due = now.fetched.is_none_or(|t| t.elapsed() >= wait);
            if cfg.place.is_none() {
                if now.weather.is_some() || now.error.is_some() {
                    live.set_weather(WeatherNow::default());
                }
            } else if changed || due {
                let next = match fetch(&cfg) {
                    Ok(w) => WeatherNow {
                        weather: Some(w),
                        error: None,
                        age_secs: None,
                        fetched: Some(Instant::now()),
                        for_cfg: Some(cfg),
                    },
                    Err(e) => {
                        log::warn!("weather: {e}");
                        WeatherNow {
                            // Keep showing the last reading for the same place.
                            weather: if changed { None } else { now.weather },
                            error: Some(e),
                            age_secs: None,
                            fetched: Some(Instant::now()),
                            for_cfg: Some(cfg),
                        }
                    }
                };
                live.set_weather(next);
            }
            std::thread::sleep(Duration::from_secs(3));
        });
    if let Err(e) = spawned {
        log::warn!("weather: thread failed to start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_open_meteo() {
        let places = parse_places(&json!({"results":[
            {"name":"Pune","latitude":18.52,"longitude":73.86,"country":"India","admin1":"Maharashtra"},
            {"name":"Nowhere"}
        ]}));
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].detail, "Maharashtra, India");
        let w = parse_forecast(
            &json!({
                "current": {"temperature_2m": 27.4, "relative_humidity_2m": 41, "weather_code": 3,
                            "wind_speed_10m": 9.1, "is_day": 1},
                "daily": {"temperature_2m_max": [31.2], "temperature_2m_min": [18.0]}
            }),
            &places[0],
            Units::Metric,
        )
        .unwrap();
        assert_eq!(
            (w.code, w.humidity, w.unit.as_str(), w.city.as_str()),
            (3, 41, "C", "Pune")
        );
        assert!((w.tmax - 31.2).abs() < 0.01 && w.is_day);
        assert!(parse_forecast(&json!({}), &places[0], Units::Metric).is_err());
        let mut w = w;
        let air =
            json!({"current": {"us_aqi": 99, "european_aqi": 79, "pm2_5": 25.1, "pm10": 32.7}});
        apply_air(&mut w, &air, AqiScale::Us);
        assert_eq!((w.aqi, w.aqi_scale.as_str()), (Some(99), "US"));
        apply_air(&mut w, &air, AqiScale::Eu);
        assert_eq!((w.aqi, w.aqi_scale.as_str()), (Some(79), "EU"));
        assert!((w.pm25.unwrap() - 25.1).abs() < 0.01);
        apply_air(&mut w, &json!({}), AqiScale::Us);
        assert_eq!(w.aqi, None);
    }

    #[test]
    fn indian_aqi_follows_cpcb() {
        let tops = &NAQI_POLLUTANTS[1].3; // PM2.5
        assert_eq!(naqi_sub(0.0, tops), 0.0);
        assert_eq!(naqi_sub(30.0, tops), 50.0);
        assert_eq!(naqi_sub(60.0, tops), 100.0);
        assert!((naqi_sub(75.0, tops) - 150.5).abs() < 0.01); // halfway through 61-90 -> 101-200
        assert_eq!(naqi_sub(999.0, tops), 500.0);
        // 24 h of PM2.5 at 75, PM10 at 80, NO2 at 20; CO/O3 low: PM2.5 rules.
        let times: Vec<String> = (0..48)
            .map(|i| format!("2026-11-{:02}T{:02}:00", 9 + i / 24, i % 24))
            .collect();
        let flat = |x: f64| vec![x; 48];
        let v = json!({
            "current": {"time": "2026-11-10T12:30"},
            "hourly": {
                "time": times,
                "pm2_5": flat(75.0), "pm10": flat(80.0), "nitrogen_dioxide": flat(20.0),
                "sulphur_dioxide": flat(5.0), "carbon_monoxide": flat(400.0), "ozone": flat(30.0)
            }
        });
        assert_eq!(naqi(&v), Some(151));
        let mut w = parse_forecast(
            &json!({"current": {"temperature_2m": 20.0}}),
            &Place {
                name: "Delhi".into(),
                detail: String::new(),
                lat: 28.6,
                lon: 77.2,
            },
            Units::Metric,
        )
        .unwrap();
        apply_air(&mut w, &v, AqiScale::In);
        assert_eq!((w.aqi, w.aqi_scale.as_str()), (Some(151), "IN"));
        // Without particulate data there is no Indian AQI.
        let no_pm = json!({"current": {"time": "2026-11-10T12:30"}, "hourly": {"time": ["2026-11-10T12:00"]}});
        assert_eq!(naqi(&no_pm), None);
        assert!(parse_places(&json!({})).is_empty());
    }
}
