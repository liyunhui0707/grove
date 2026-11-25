//! Offline GeoNames place centres. See THIRD-PARTY-NOTICES.txt.
use std::sync::LazyLock;

struct City {
    lat: f64,
    lon: f64,
    name: &'static str,
    chinese: Option<&'static str>,
}

static CITIES: LazyLock<Vec<City>> = LazyLock::new(|| {
    include_str!("cities.tsv")
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            City {
                lat: fields.next().unwrap().parse().unwrap(),
                lon: fields.next().unwrap().parse().unwrap(),
                name: fields.next().unwrap(),
                chinese: fields.next(),
            }
        })
        .collect()
});

pub(super) fn nearby(lat: f64, lon: f64) -> Option<&'static str> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let angle = 25.0_f64 / 6371.0; // maximum great-circle distance, 25 km
    let band = angle.to_degrees();
    let start = CITIES.partition_point(|c| c.lat < lat - band);
    let mut best = (angle / 2.0).sin().powi(2);
    let mut name = None;
    for city in CITIES[start..].iter().take_while(|c| c.lat <= lat + band) {
        let dlat = (city.lat - lat).to_radians();
        let dlon = (city.lon - lon).to_radians();
        let distance = (dlat / 2.0).sin().powi(2)
            + lat.to_radians().cos() * city.lat.to_radians().cos() * (dlon / 2.0).sin().powi(2);
        if distance < best {
            best = distance;
            name = city.chinese;
        }
    }
    name
}

/// Translate embedded city metadata only when all places of that name agree.
pub(super) fn embedded_label(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    if raw.is_empty() || !super::valid_city(raw) {
        return None;
    }
    let mut matches = CITIES
        .iter()
        .filter(|city| city.name.eq_ignore_ascii_case(raw));
    if let Some(label) = matches.next().and_then(|city| city.chinese)
        && matches.all(|city| city.chinese == Some(label))
    {
        return Some(label);
    }
    let han = |c: char| matches!(c, '㐀'..='鿿' | '𠀀'..='𮯯');
    (raw.chars().any(han) && raw.chars().all(|c| han(c) || matches!(c, '·' | '・'))).then_some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_nearest_places_chinese_name_or_omits_the_city() {
        assert_eq!(nearby(39.90750, 116.39723), Some("北京"));
        assert_eq!(nearby(27.99942, 120.66682), Some("温州市"));
        let untranslated = CITIES.iter().find(|city| city.chinese.is_none()).unwrap();
        assert_eq!(nearby(untranslated.lat, untranslated.lon), None);
    }

    #[test]
    fn embedded_city_requires_chinese_text_or_an_unambiguous_translation() {
        assert_eq!(embedded_label(" Wenzhou "), Some("温州市"));
        assert_eq!(embedded_label("温州"), Some("温州"));
        assert_eq!(embedded_label("Unknownville"), None);
        assert_eq!(embedded_label("温州 Wenzhou"), None);
        assert_eq!(embedded_label("··"), None);
    }
}
