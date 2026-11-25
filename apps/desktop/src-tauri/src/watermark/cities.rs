//! Offline GeoNames place centres. See THIRD-PARTY-NOTICES.txt.
use std::sync::LazyLock;

struct City {
    lat: f64,
    lon: f64,
    name: &'static str,
}

static CITIES: LazyLock<Vec<City>> = LazyLock::new(|| {
    include_str!("cities.tsv")
        .lines()
        .map(|line| {
            let mut fields = line.splitn(3, '\t');
            City {
                lat: fields.next().unwrap().parse().unwrap(),
                lon: fields.next().unwrap().parse().unwrap(),
                name: fields.next().unwrap(),
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
            name = Some(city.name);
        }
    }
    name
}
