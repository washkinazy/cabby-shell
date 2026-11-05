//! Local/solar day-night schedule, adapted from Noctalia's MIT implementation.
//! See licenses/noctalia-MIT.txt and docs/night-light.md for provenance.
use cabby_config::Location;
#[derive(Clone, Copy)]
pub struct Calendar {
    pub minute: f64,
    pub day: u16,
    pub offset: f64,
}
impl Calendar {
    pub fn now() -> Option<Self> {
        unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return None;
            }
            Some(Self {
                minute: f64::from(tm.tm_hour * 60 + tm.tm_min) + f64::from(tm.tm_sec) / 60.,
                day: (tm.tm_yday + 1) as u16,
                offset: tm.tm_gmtoff as f64 / 60.,
            })
        }
    }
}
pub struct Evaluation {
    pub night: bool,
    pub since: f64,
    pub until: f64,
}
pub fn evaluate(location: &Location, clock: Calendar) -> Option<Evaluation> {
    let (sunset, sunrise) = if location.custom_schedule {
        (
            f64::from(cabby_config::clock_minutes(&location.sunset)?),
            f64::from(cabby_config::clock_minutes(&location.sunrise)?),
        )
    } else {
        let lat = location.latitude?.to_radians();
        let lon = location.longitude?;
        let y = 2. * std::f64::consts::PI / 365. * (f64::from(clock.day) - 1.);
        let eq = 229.18
            * (0.000075 + 0.001868 * y.cos()
                - 0.032077 * y.sin()
                - 0.014615 * (2. * y).cos()
                - 0.040849 * (2. * y).sin());
        let dec = 0.006918 - 0.399912 * y.cos() + 0.070257 * y.sin() - 0.006758 * (2. * y).cos()
            + 0.000907 * (2. * y).sin()
            - 0.002697 * (3. * y).cos()
            + 0.00148 * (3. * y).sin();
        let arg = 90.833_f64.to_radians().cos() / (lat.cos() * dec.cos()) - lat.tan() * dec.tan();
        if arg.abs() > 1. {
            return Some(Evaluation {
                night: arg > 1.,
                since: 60.,
                until: 60.,
            });
        }
        let angle = arg.clamp(-1., 1.).acos().to_degrees();
        let noon = 720. - 4. * lon - eq + clock.offset;
        (
            (noon + angle * 4.).round().rem_euclid(1440.),
            (noon - angle * 4.).round().rem_euclid(1440.),
        )
    };
    let night = if sunset < sunrise {
        clock.minute >= sunset && clock.minute < sunrise
    } else {
        clock.minute >= sunset || clock.minute < sunrise
    };
    Some(Evaluation {
        night,
        since: (clock.minute - if night { sunset } else { sunrise }).rem_euclid(1440.),
        until: ((if night { sunrise } else { sunset }) - clock.minute).rem_euclid(1440.),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn astronomical_seasons_poles_and_no_schedule() {
        let mut l = Location {
            latitude: Some(0.),
            longitude: Some(0.),
            ..Location::default()
        };
        let clock = Calendar {
            minute: 720.,
            day: 80,
            offset: 0.,
        };
        assert!(!evaluate(&l, clock).unwrap().night);
        assert!(
            evaluate(
                &l,
                Calendar {
                    minute: 0.,
                    ..clock
                }
            )
            .unwrap()
            .night
        );
        l.latitude = Some(89.);
        assert!(!evaluate(&l, Calendar { day: 172, ..clock }).unwrap().night);
        assert!(evaluate(&l, Calendar { day: 355, ..clock }).unwrap().night);
        assert!(evaluate(&Location::default(), clock).is_none());
    }
}
