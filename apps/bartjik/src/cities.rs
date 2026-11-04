//! Offline approximate city labels. Device coordinates never leave this process.
use std::sync::LazyLock;

struct City {
    name: &'static str,
    country: &'static str,
    latitude: f64,
    longitude: f64,
    population: u64,
}
static CITIES: LazyLock<Vec<City>> = LazyLock::new(|| {
    include_str!("../data/cities.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 5 {
                return None;
            }
            Some(City {
                name: fields[0],
                country: fields[1],
                latitude: fields[2].parse::<f64>().ok()?.to_radians(),
                longitude: fields[3].parse::<f64>().ok()?.to_radians(),
                population: fields[4].parse().ok()?,
            })
        })
        .collect()
});

pub fn near(latitude: f64, longitude: f64) -> Option<String> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90. ..=90.).contains(&latitude)
        || !(-180. ..=180.).contains(&longitude)
    {
        return None;
    }
    let (lat, lon) = (latitude.to_radians(), longitude.to_radians());
    let distance = |city: &City| {
        let a = ((lat - city.latitude) / 2.).sin().powi(2)
            + lat.cos() * city.latitude.cos() * ((lon - city.longitude) / 2.).sin().powi(2);
        12742. * a.clamp(0., 1.).sqrt().asin()
    };
    // Prefer a nearby metropolitan centre over a ward or housing district.
    // The "Near" qualifier deliberately avoids asserting municipal boundaries.
    if let Some((city, _)) = CITIES
        .iter()
        .filter(|city| city.population >= 1_000_000)
        .map(|city| (city, distance(city)))
        .filter(|(_, km)| *km <= 25.)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
    {
        return Some(format!("Near {}, {}", city.name, city.country));
    }
    let (city, km) =
        CITIES
            .iter()
            .map(|city| (city, distance(city)))
            .min_by(|(a, da), (b, db)| {
                da.total_cmp(db)
                    .then_with(|| b.population.cmp(&a.population))
            })?;
    // A nearby settlement is useful context, not a claim about city boundaries.
    (km <= 50.).then(|| format!("Near {}, {}", city.name, city.country))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cities_offline_and_handles_remote_or_invalid_fixes() {
        assert!(CITIES.len() > 20000);
        assert_eq!(near(51.50721, -0.12758).as_deref(), Some("Near London, GB"));
        assert_eq!(
            near(-33.86785, 151.20732).as_deref(),
            Some("Near Sydney, AU")
        );
        assert_eq!(
            near(22.2841, 114.21631).as_deref(),
            Some("Near Hong Kong, HK")
        );
        assert_eq!(near(35.70206, 139.63745).as_deref(), Some("Near Tokyo, JP"));
        assert!(near(0., 0.).is_none());
        assert!(near(f64::NAN, 0.).is_none());
        assert!(near(91., 0.).is_none());
    }
}
