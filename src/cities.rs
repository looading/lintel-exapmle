use chrono_tz::Tz;

pub const CITIES: &[(&str, &str)] = &[
    ("上海", "Asia/Shanghai"),
    ("香港", "Asia/Hong_Kong"),
    ("台北", "Asia/Taipei"),
    ("东京", "Asia/Tokyo"),
    ("首尔", "Asia/Seoul"),
    ("新加坡", "Asia/Singapore"),
    ("曼谷", "Asia/Bangkok"),
    ("雅加达", "Asia/Jakarta"),
    ("加尔各答", "Asia/Kolkata"),
    ("加德满都", "Asia/Kathmandu"),
    ("迪拜", "Asia/Dubai"),
    ("伊斯坦布尔", "Europe/Istanbul"),
    ("伦敦", "Europe/London"),
    ("巴黎", "Europe/Paris"),
    ("柏林", "Europe/Berlin"),
    ("阿姆斯特丹", "Europe/Amsterdam"),
    ("马德里", "Europe/Madrid"),
    ("开罗", "Africa/Cairo"),
    ("约翰内斯堡", "Africa/Johannesburg"),
    ("纽约", "America/New_York"),
    ("芝加哥", "America/Chicago"),
    ("洛杉矶", "America/Los_Angeles"),
    ("温哥华", "America/Vancouver"),
    ("墨西哥城", "America/Mexico_City"),
    ("圣保罗", "America/Sao_Paulo"),
    ("悉尼", "Australia/Sydney"),
    ("珀斯", "Australia/Perth"),
    ("奥克兰", "Pacific/Auckland"),
    ("檀香山", "Pacific/Honolulu"),
    ("圣约翰斯", "America/St_Johns"),
];

pub fn name(zone: Tz) -> String {
    let id = zone.name();
    CITIES.iter().find(|(_, key)| *key == id).map_or_else(
        || id.rsplit('/').next().unwrap_or(id).replace('_', " "),
        |(label, _)| (*label).into(),
    )
}

pub fn search(query: &str, selected: &[Tz]) -> Vec<Tz> {
    let query = query.trim().to_lowercase().replace('_', " ");
    let featured = CITIES.iter().filter_map(|(_, id)| id.parse::<Tz>().ok());
    let remaining = chrono_tz::TZ_VARIANTS
        .iter()
        .copied()
        .filter(|zone| !CITIES.iter().any(|(_, id)| *id == zone.name()));
    featured
        .chain(remaining)
        .filter(|zone| {
            !selected.contains(zone)
                && (zone
                    .name()
                    .to_lowercase()
                    .replace('_', " ")
                    .contains(&query)
                    || name(*zone).to_lowercase().contains(&query))
        })
        .collect()
}

pub fn coordinates(zone: Tz) -> Option<sunrise::Coordinates> {
    use std::{collections::HashMap, sync::OnceLock};
    use sunrise::Coordinates;

    static LOCATIONS: OnceLock<HashMap<&'static str, Coordinates>> = OnceLock::new();
    let locations = LOCATIONS.get_or_init(|| {
        // Public-domain IANA TZDB 2025b, matching chrono-tz. Bundle the
        // location table since Windows doesn't provide the Unix zone.tab file.
        let mut locations: HashMap<_, _> = include_str!("../assets/zone-2025b.tab")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.split('\t');
                fields.next()?;
                let pair = fields.next()?;
                let zone = fields.next()?;
                let split = pair.get(1..)?.find(['+', '-'])? + 1;
                let latitude = coordinate(&pair[..split], 2)?;
                let longitude = coordinate(&pair[split..], 3)?;
                Some((zone, Coordinates::new(latitude, longitude)?))
            })
            .collect();

        for line in include_str!("../assets/zone-backward-2025b").lines() {
            let mut parts = line.split_ascii_whitespace();
            if parts.next() != Some("Link") {
                continue;
            }
            if let (Some(target), Some(alias)) = (parts.next(), parts.next())
                && let Some(&location) = locations.get(target)
            {
                locations.entry(alias).or_insert(location);
            }
        }
        locations
    });
    locations.get(zone.name()).copied()
}

pub fn solar_events(
    zone: Tz,
    date: chrono::NaiveDate,
) -> Option<(
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
)> {
    use sunrise::{SolarDay, SolarEvent};

    let day = SolarDay::new(coordinates(zone)?, date);
    Some((
        day.event_time(SolarEvent::Sunrise),
        day.event_time(SolarEvent::Sunset),
    ))
}

fn coordinate(value: &str, degree_digits: usize) -> Option<f64> {
    let (sign, value) = value.split_at_checked(1)?;
    let (degrees, rest) = value.split_at_checked(degree_digits)?;
    let (minutes, seconds) = rest.split_at_checked(2)?;
    let seconds = if seconds.is_empty() {
        0.0
    } else {
        seconds.parse::<f64>().ok()?
    };
    let value =
        degrees.parse::<f64>().ok()? + minutes.parse::<f64>().ok()? / 60.0 + seconds / 3600.0;
    Some(if sign == "-" { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_coordinates_cover_watch_cities_and_saved_aliases() {
        let shanghai = coordinates(chrono_tz::Asia::Shanghai).unwrap();
        assert!((30.0..32.0).contains(&shanghai.lat()));
        assert!((120.0..123.0).contains(&shanghai.lon()));

        let new_york = coordinates(chrono_tz::America::New_York).unwrap();
        let alias = coordinates("US/Eastern".parse().unwrap()).unwrap();
        assert_eq!((new_york.lat(), new_york.lon()), (alias.lat(), alias.lon()));
        assert!(coordinates(chrono_tz::UTC).is_none());
    }
}
