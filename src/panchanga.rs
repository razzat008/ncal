//! Panchanga arithmetic: tithi, sunrise, and the kāla instants Nepali festival
//! rules are stated against.
//!
//! Most Nepali festivals are not on a fixed BS date. They are on a *tithi* — one
//! thirtieth of a lunation, defined as the moon's elongation from the sun in
//! steps of 12°. Which calendar day carries the festival then depends on the
//! festival's own rule: Vijaya Dashami wants dashami during the afternoon,
//! Shivaratri wants chaturdashi at midnight, Laxmi Puja wants amavasya at dusk.
//!
//! Two facts make this computable rather than a lookup table:
//!
//! * A tithi is a *difference* of two longitudes, so the ayanamsa — the tropical
//!   to sidereal offset that sidereal astronomy argues about — cancels exactly.
//!   There is nothing to calibrate and no convention to pick.
//! * Moderate-precision series for the sun (Meeus ch. 25) and moon (Meeus ch. 47)
//!   are good to well under an arcminute here, which puts a tithi boundary within
//!   a couple of minutes. Festival rules compare a boundary against sunrise or
//!   dusk, so minutes are the resolution that matters.
//!
//! Everything is plain arithmetic: no ephemeris file, no network, no dependency
//! beyond `chrono` for the calendar date itself.

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

/// Kathmandu, where Nepal's calendar is determined.
pub const KATHMANDU_LAT: f64 = 27.7172;
pub const KATHMANDU_LON: f64 = 85.3240;

/// Nepal Time, UTC+05:45, as a fraction of a day.
pub const NPT_OFFSET: f64 = 5.75 / 24.0;

const DEG: f64 = std::f64::consts::PI / 180.0;

/// The instant a festival rule is evaluated at.
///
/// Daylight is divided into five equal parts; three of these name one of them.
/// `Nishita` is the odd one out and is the middle-of-night muhurta in the night
/// that *follows* the named civil day (computed from that day's sunset to the
/// next sunrise), and for festival rules it is treated as a span, not an instant.
/// See [`nishita_window`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kala {
    /// Sunrise. The default: "the tithi running at daybreak owns the day."
    Udaya,
    /// Midday, the third fifth of daylight. Bhai Tika and Ram Navami.
    Madhyanna,
    /// Start of the fourth fifth of daylight. Vijaya Dashami is aparāhna-vyāpini.
    Aparahna,
    /// Dusk, shortly after sunset. Laxmi Puja's amavasya and Chhath's arghya.
    Pradosh,
    /// The nishita muhurta — the middle of the night that *follows* this day.
    /// Unlike the others this is a span, not an instant, and the tithi has to
    /// cover the whole of it. See [`nishita_window`]. Shivaratri, Janmashtami.
    Nishita,
    /// Not an instant: the day on which the tithi *begins*.
    Begins,
}

/// Julian Day for a Gregorian date at a UT day fraction.
pub fn julian_day(date: NaiveDate, day_fraction: f64) -> f64 {
    let (mut y, mut m) = (date.year(), date.month() as i32);
    let d = date.day() as f64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = y.div_euclid(100);
    let b = 2 - a + a.div_euclid(4);
    (365.25 * (y + 4716) as f64).floor() + (30.6001 * (m + 1) as f64).floor() + d + b as f64
        - 1524.5
        + day_fraction
}

/// Difference between Terrestrial Time and Universal Time, in seconds.
///
/// Espenak & Meeus' piecewise polynomials. Over the BS 2000–2090 window
/// (AD 1943–2033) ΔT runs from roughly 27s to 75s — about a quarter of a degree
/// of lunar motion, which is a fifth of a tithi step. It is not optional.
pub fn delta_t(year: f64) -> f64 {
    if year < 1920.0 {
        let t = year - 1900.0;
        -2.79 + 1.494119 * t - 0.0598939 * t * t + 0.0061966 * t.powi(3) - 0.000197 * t.powi(4)
    } else if year < 1941.0 {
        let t = year - 1920.0;
        21.20 + 0.84493 * t - 0.076100 * t * t + 0.0020936 * t.powi(3)
    } else if year < 1961.0 {
        let t = year - 1950.0;
        29.07 + 0.407 * t - t * t / 233.0 + t.powi(3) / 2547.0
    } else if year < 1986.0 {
        let t = year - 1975.0;
        45.45 + 1.067 * t - t * t / 260.0 - t.powi(3) / 718.0
    } else if year < 2005.0 {
        let t = year - 2000.0;
        63.86 + 0.3345 * t - 0.060374 * t * t + 0.0017275 * t.powi(3) + 0.000651814 * t.powi(4)
            + 0.00002373599 * t.powi(5)
    } else if year < 2050.0 {
        let t = year - 2000.0;
        62.92 + 0.32217 * t + 0.005589 * t * t
    } else {
        let t = year - 1820.0;
        -20.0 + 32.0 * (t / 100.0).powi(2) - 0.5628 * (2150.0 - year)
    }
}

/// Convert a UT Julian Day to the TT the ephemeris series expect.
fn tt_from_ut(jd_ut: f64) -> f64 {
    // The year only feeds ΔT, which changes on a decade scale; a day's slop here
    // is immaterial, so the cheap linear estimate is fine.
    let year = 2000.0 + (jd_ut - 2451545.0) / 365.25;
    jd_ut + delta_t(year) / 86400.0
}

fn julian_centuries(jd_tt: f64) -> f64 {
    (jd_tt - 2451545.0) / 36525.0
}

fn norm360(x: f64) -> f64 {
    x.rem_euclid(360.0)
}

/// Apparent geometric longitude of the sun, in degrees. Meeus ch. 25.
pub fn sun_longitude(jd_tt: f64) -> f64 {
    let t = julian_centuries(jd_tt);
    let l0 = 280.46646 + 36000.76983 * t + 0.0003032 * t * t;
    let m = 357.52911 + 35999.05029 * t - 0.0001537 * t * t;
    let c = (1.914602 - 0.004817 * t - 0.000014 * t * t) * (m * DEG).sin()
        + (0.019993 - 0.000101 * t) * (2.0 * m * DEG).sin()
        + 0.000289 * (3.0 * m * DEG).sin();
    norm360(l0 + c)
}

/// Periodic terms for the moon's longitude: `(D, M, M', F, coefficient)` with the
/// coefficient in units of 1e-6 degrees. Meeus table 47.A, the ΣL column.
#[rustfmt::skip]
const MOON_TERMS: [(i8, i8, i8, i8, i32); 60] = [
    (0, 0, 1, 0, 6288774), (2, 0, -1, 0, 1274027), (2, 0, 0, 0, 658314),
    (0, 0, 2, 0, 213618), (0, 1, 0, 0, -185116), (0, 0, 0, 2, -114332),
    (2, 0, -2, 0, 58793), (2, -1, -1, 0, 57066), (2, 0, 1, 0, 53322),
    (2, -1, 0, 0, 45758), (0, 1, -1, 0, -40923), (1, 0, 0, 0, -34720),
    (0, 1, 1, 0, -30383), (2, 0, 0, -2, 15327), (0, 0, 1, 2, -12528),
    (0, 0, 1, -2, 10980), (4, 0, -1, 0, 10675), (0, 0, 3, 0, 10034),
    (4, 0, -2, 0, 8548), (2, 1, -1, 0, -7888), (2, 1, 0, 0, -6766),
    (1, 0, -1, 0, -5163), (1, 1, 0, 0, 4987), (2, -1, 1, 0, 4036),
    (2, 0, 2, 0, 3994), (4, 0, 0, 0, 3861), (2, 0, -3, 0, 3665),
    (0, 1, -2, 0, -2689), (2, 0, -1, 2, -2602), (2, -1, -2, 0, 2390),
    (1, 0, 1, 0, -2348), (2, -2, 0, 0, 2236), (0, 1, 2, 0, -2120),
    (0, 2, 0, 0, -2069), (2, -2, -1, 0, 2048), (2, 0, 1, -2, -1773),
    (2, 0, 0, 2, -1595), (4, -1, -1, 0, 1215), (0, 0, 2, 2, -1110),
    (3, 0, -1, 0, -892), (2, 1, 1, 0, -810), (4, -1, -2, 0, 759),
    (0, 2, -1, 0, -713), (2, 2, -1, 0, -700), (2, 1, -2, 0, 691),
    (2, -1, 0, -2, 596), (4, 0, 1, 0, 549), (0, 0, 4, 0, 537),
    (4, -1, 0, 0, 520), (1, 0, -2, 0, -487), (2, 1, 0, -2, -399),
    (0, 0, 2, -2, -381), (1, 1, 1, 0, 351), (3, 0, -2, 0, -340),
    (4, 0, -3, 0, 330), (2, -1, 2, 0, 327), (0, 2, 1, 0, -323),
    (1, 1, -1, 0, 299), (2, 0, 3, 0, 294), (2, 0, -1, -2, 0),
];

/// Apparent geometric longitude of the moon, in degrees. Meeus ch. 47.
pub fn moon_longitude(jd_tt: f64) -> f64 {
    let t = julian_centuries(jd_tt);

    let lp = 218.3164477 + 481267.88123421 * t - 0.0015786 * t * t + t.powi(3) / 538841.0
        - t.powi(4) / 65194000.0;
    let d = 297.8501921 + 445267.1114034 * t - 0.0018819 * t * t + t.powi(3) / 545868.0
        - t.powi(4) / 113065000.0;
    let m = 357.5291092 + 35999.0502909 * t - 0.0001536 * t * t + t.powi(3) / 24490000.0;
    let mp = 134.9633964 + 477198.8675055 * t + 0.0087414 * t * t + t.powi(3) / 69699.0
        - t.powi(4) / 14712000.0;
    let f = 93.2720950 + 483202.0175233 * t - 0.0036539 * t * t - t.powi(3) / 3526000.0
        + t.powi(4) / 863310000.0;

    // The sun's orbital eccentricity decreases slowly; terms in M are scaled by it.
    let e = 1.0 - 0.002516 * t - 0.0000074 * t * t;

    let mut sum = 0.0;
    for (cd, cm, cmp, cf, coeff) in MOON_TERMS {
        if coeff == 0 {
            continue;
        }
        let arg = (cd as f64 * d + cm as f64 * m + cmp as f64 * mp + cf as f64 * f) * DEG;
        sum += coeff as f64 * e.powi(cm.unsigned_abs() as i32) * arg.sin();
    }

    // Additive terms: Venus (A1), Jupiter (A2), and the flattening of the Earth.
    let a1 = 119.75 + 131.849 * t;
    let a2 = 53.09 + 479264.290 * t;
    sum += 3958.0 * (a1 * DEG).sin() + 1962.0 * ((lp - f) * DEG).sin() + 318.0 * (a2 * DEG).sin();

    norm360(lp + sum / 1_000_000.0)
}

/// Annual aberration of the sun, in degrees (Meeus 25.10: 20.4898″ at 1 AU).
///
/// Nutation shifts both longitudes alike and cancels in the elongation;
/// aberration displaces only the sun, so it does not. Leaving it out made every
/// tithi boundary about 40 seconds late against JPL DE440 — enough to move a
/// festival whose tithi begins within a minute of its kala instant.
const SUN_ABERRATION: f64 = 20.4898 / 3600.0;

/// The moon's apparent elongation from the sun, 0–360°, at a UT instant. One
/// tithi is 12°.
pub fn elongation(jd_ut: f64) -> f64 {
    let jd_tt = tt_from_ut(jd_ut);
    norm360(moon_longitude(jd_tt) - (sun_longitude(jd_tt) - SUN_ABERRATION))
}

/// The tithi running at a UT instant, numbered 1–30 from Shukla Pratipada.
///
/// 1–15 are the waxing (shukla) half, 15 being Purnima; 16–30 are the waning
/// (krishna) half, 30 being Amavasya.
pub fn tithi_at(jd_ut: f64) -> u8 {
    (elongation(jd_ut) / 12.0) as u8 + 1
}

/// Signed distance from a UT instant's elongation to `target`, in (-180, 180].
///
/// Negative before the target, positive after — so a crossing is a sign change
/// from non-positive to positive, and the 180° wrap goes the other way and is
/// never mistaken for one.
fn elongation_offset(jd: f64, target: f64) -> f64 {
    let mut x = elongation(jd) - target;
    while x <= -180.0 {
        x += 360.0;
    }
    while x > 180.0 {
        x -= 360.0;
    }
    x
}

/// Bisect a bracketed crossing of `target` to sub-second precision.
fn bisect_crossing(target: f64, mut a: f64, mut b: f64) -> f64 {
    // Thirty halvings take a `SCAN_STEP` bracket below a microsecond, which is
    // far finer than the ephemeris itself is accurate. More is wasted work, and
    // this loop is the hot path when a whole year of festivals is resolved.
    for _ in 0..30 {
        let mid = (a + b) / 2.0;
        if elongation_offset(mid, target) <= 0.0 {
            a = mid;
        } else {
            b = mid;
        }
    }
    (a + b) / 2.0
}

/// Elongation advances at 10.9-14.8°/day and never runs backwards, so in three
/// quarters of a day it gains at most 11.1° — always less than the 12° a tithi
/// spans, so no crossing can hide inside one step.
const SCAN_STEP: f64 = 0.75;

/// Every UT instant in `[from, to]` at which `tithi` begins.
///
/// A tithi recurs each lunation, so a window wider than ~29.5 days can hold two.
pub fn tithi_starts_between(tithi: u8, from: f64, to: f64) -> Vec<f64> {
    let target = (tithi as f64 - 1.0) * 12.0;
    let mut found = Vec::new();

    let mut lo = from;
    let mut f_lo = elongation_offset(lo, target);
    while lo < to {
        let hi = (lo + SCAN_STEP).min(to);
        let f_hi = elongation_offset(hi, target);
        if f_lo <= 0.0 && f_hi > 0.0 {
            found.push(bisect_crossing(target, lo, hi));
        }
        if hi >= to {
            break;
        }
        lo = hi;
        f_lo = f_hi;
    }

    found
}

/// Sunrise and sunset for Kathmandu, as UT Julian Days. NOAA's solar calculator.
///
/// Returns `None` only for a latitude where the sun does not rise or set, which
/// Kathmandu at 27.7°N never is — the branch exists so a bad input cannot panic.
pub fn sun_events(date: NaiveDate) -> Option<(f64, f64)> {
    let jd_noon = julian_day(date, 0.5) - KATHMANDU_LON / 360.0;
    let t = julian_centuries(tt_from_ut(jd_noon));

    let l0 = 280.46646 + 36000.76983 * t + 0.0003032 * t * t;
    let m = 357.52911 + 35999.05029 * t - 0.0001537 * t * t;
    let e = 0.016708634 - 0.000042037 * t - 0.0000001267 * t * t;
    let c = (1.914602 - 0.004817 * t - 0.000014 * t * t) * (m * DEG).sin()
        + (0.019993 - 0.000101 * t) * (2.0 * m * DEG).sin()
        + 0.000289 * (3.0 * m * DEG).sin();
    let lambda = norm360(l0 + c);
    let eps = 23.439291 - 0.0130042 * t;

    let dec = ((eps * DEG).sin() * (lambda * DEG).sin()).asin();

    // Equation of time, in minutes.
    let y = (eps * DEG / 2.0).tan().powi(2);
    let l0r = l0 * DEG;
    let mr = m * DEG;
    let eot = 4.0 / DEG
        * (y * (2.0 * l0r).sin() - 2.0 * e * mr.sin() + 4.0 * e * y * mr.sin() * (2.0 * l0r).cos()
            - 0.5 * y * y * (4.0 * l0r).sin()
            - 1.25 * e * e * (2.0 * mr).sin());

    // 90.833° accounts for refraction plus the sun's semidiameter.
    let cos_ha = (90.833 * DEG).cos() / ((KATHMANDU_LAT * DEG).cos() * dec.cos())
        - (KATHMANDU_LAT * DEG).tan() * dec.tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = cos_ha.acos() / DEG;

    let solar_noon = (720.0 - 4.0 * KATHMANDU_LON - eot) / 1440.0;
    let midnight = julian_day(date, 0.0);
    Some((
        midnight + solar_noon - ha * 4.0 / 1440.0,
        midnight + solar_noon + ha * 4.0 / 1440.0,
    ))
}

/// Sunrise over Kathmandu, as a UT Julian Day.
pub fn sunrise(date: NaiveDate) -> Option<f64> {
    sun_events(date).map(|(rise, _)| rise)
}

/// Sunset over Kathmandu, as a UT Julian Day.
pub fn sunset(date: NaiveDate) -> Option<f64> {
    sun_events(date).map(|(_, set)| set)
}

/// The UT instant a festival rule is evaluated at on a given day.
///
/// `Kala::Begins` has no instant — it asks which day a tithi starts on, not what
/// is running at a moment — so it falls back to sunrise and callers that support
/// it must branch before getting here.
pub fn kala_instant(date: NaiveDate, kala: Kala) -> Option<f64> {
    let (rise, set) = sun_events(date)?;
    Some(match kala {
        Kala::Udaya | Kala::Begins => rise,
        // The classical day is five equal parts; madhyahna is the third and
        // aparahna the fourth.
        Kala::Madhyanna => rise + (set - rise) * 0.4,
        Kala::Aparahna => rise + (set - rise) * 0.6,
        Kala::Pradosh => set + 45.0 / 1440.0,
        // The midpoint of the nishita muhurta, for callers that only want an
        // instant. Day-ownership decisions must use `nishita_window` instead.
        Kala::Nishita => {
            let (from, to) = nishita_window(date)?;
            (from + to) / 2.0
        }
    })
}

/// The Gregorian date in Kathmandu that a UT instant falls on.
pub fn npt_date(jd_ut: f64) -> Option<NaiveDate> {
    // JD 0 begins at noon, so shift by half a day before taking the integer part.
    let local = jd_ut + NPT_OFFSET + 0.5;
    let z = local.floor() as i64;
    let alpha = ((z as f64 - 1867216.25) / 36524.25).floor() as i64;
    let a = z + 1 + alpha - alpha.div_euclid(4);
    let b = a + 1524;
    let c = ((b as f64 - 122.1) / 365.25).floor() as i64;
    let d = (365.25 * c as f64).floor() as i64;
    let e = ((b - d) as f64 / 30.6001).floor() as i64;
    let day = b - d - (30.6001 * e as f64).floor() as i64;
    let month = if e < 14 { e - 1 } else { e - 13 };
    let year = if month > 2 { c - 4716 } else { c - 4715 };
    NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
}

/// The UT instant the tithi running at `jd_ut` gives way to the next.
///
/// A tithi runs 19-26 hours, so the successor's start is always inside a
/// two-and-a-half-day window ahead.
pub fn tithi_end(jd_ut: f64) -> Option<f64> {
    let current = tithi_at(jd_ut);
    let next = if current == 30 { 1 } else { current + 1 };
    tithi_starts_between(next, jd_ut, jd_ut + 2.5).first().copied()
}

/// The nishita muhurta of `date`: the span the classical texts mean by "the dead
/// of night", as `(from, to)` in UT Julian Days.
///
/// The night from sunset to the next sunrise is divided into fifteen muhurtas and
/// nishita is the eighth, so it straddles the middle of the night and runs about
/// forty minutes. Treating it as a span rather than an instant is what makes
/// Krishna Janmashtami and Maha Shivaratri come out right from one rule: a tithi
/// is nishita-vyapini only if it covers the *whole* muhurta, so ashtami beginning
/// a few minutes past midnight does not claim the night it barely entered.
pub fn nishita_window(date: NaiveDate) -> Option<(f64, f64)> {
    let (_, set) = sun_events(date)?;
    let next = date.checked_add_signed(chrono::Duration::days(1))?;
    let (next_rise, _) = sun_events(next)?;
    let night = next_rise - set;
    Some((set + night * 7.0 / 15.0, set + night * 8.0 / 15.0))
}

/// The lunar-day reading of a date: what a patro prints under the number.
///
/// Everything here is computed from the ephemeris rather than looked up, so it
/// is available for every date in range. Tithi is a *difference* between the
/// moon's and the sun's longitudes, which is why no ayanamsa appears anywhere:
/// whatever constant you would add to both cancels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reading {
    /// 1-30, counting from Shukla Pratipada.
    pub tithi: u8,
    pub tithi_name: &'static str,
    pub tithi_name_np: &'static str,
    /// "Shukla" or "Krishna".
    pub paksha: &'static str,
    pub paksha_np: &'static str,
    /// Nepal-time "HH:MM" the tithi running at sunrise gives way to the next, or
    /// `null` when it outlasts the day.
    pub tithi_ends: Option<String>,
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
}

/// Format a UT Julian Day as Nepal-time "HH:MM".
fn npt_clock(jd: f64) -> String {
    let hours = (jd + NPT_OFFSET + 0.5).rem_euclid(1.0) * 24.0;
    // Round to the minute, and let 23:59:40 read as the next midnight rather
    // than as "24:00".
    let minutes = (hours * 60.0).round() as u32 % 1440;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

impl Reading {
    /// The reading of `date` as of its own sunrise, which is where the Hindu day
    /// begins and therefore which tithi names it.
    pub fn for_date(date: NaiveDate) -> Self {
        let reference = sunrise(date)
            .unwrap_or_else(|| julian_day(date, 0.25 - NPT_OFFSET));
        let tithi = tithi_at(reference);
        let (tithi_name, tithi_name_np) = crate::names::tithi_name(tithi);
        let (paksha, paksha_np) = crate::names::paksha_name(tithi);

        // Only report an end time that falls on this day; a tithi that runs past
        // midnight has no useful "until" to show against this date.
        let tithi_ends = tithi_end(reference)
            .filter(|jd| npt_date(*jd) == Some(date))
            .map(npt_clock);

        Self {
            tithi,
            tithi_name,
            tithi_name_np,
            paksha,
            paksha_np,
            tithi_ends,
            sunrise: sunrise(date).map(npt_clock),
            sunset: sunset(date).map(npt_clock),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn julian_day_matches_meeus_examples() {
        // Meeus example 7.a and 7.b.
        assert!((julian_day(date(1957, 10, 4), 0.81) - 2436116.31).abs() < 1e-6);
        assert!((julian_day(date(2000, 1, 1), 0.5) - 2451545.0).abs() < 1e-6);
    }

    /// New moons from JPL DE440 (via Skyfield), as UT Julian Days. The apparent
    /// elongation is zero there; without solar aberration it came out ~0.0057°
    /// short, which is every tithi boundary landing about 40 seconds late.
    #[test]
    fn elongation_is_zero_at_jpl_new_moons() {
        for jd in [2448183.150475, 2460970.017483, 2467171.585537] {
            let e = elongation(jd);
            let off = if e > 180.0 { e - 360.0 } else { e };
            assert!(off.abs() < 0.004, "elongation {off}° at JPL new moon {jd}");
        }
    }

    #[test]
    fn sun_longitude_matches_meeus_example_25b() {
        // 1992 October 13.0 TD: apparent longitude 199.90895°. The geometric
        // longitude this returns differs by nutation and aberration, ~0.01°.
        let lambda = sun_longitude(2448908.5);
        assert!(
            (lambda - 199.90988).abs() < 0.02,
            "sun longitude {lambda} should be near 199.909"
        );
    }

    #[test]
    fn moon_longitude_matches_meeus_example_47a() {
        // 1992 April 12.0 TD: λ = 133.162655°.
        let lambda = moon_longitude(2448724.5);
        assert!(
            (lambda - 133.162655).abs() < 0.001,
            "moon longitude {lambda} should be near 133.1627"
        );
    }

    #[test]
    fn tithi_numbering_spans_the_lunation() {
        // New moon on 2025-03-29 at 10:58 UT closes tithi 30 and opens tithi 1;
        // full moon on 2025-03-14 at 06:55 UT closes 15 and opens 16.
        assert_eq!(tithi_at(julian_day(date(2025, 3, 29), 0.40)), 30);
        assert_eq!(tithi_at(julian_day(date(2025, 3, 29), 0.50)), 1);
        assert_eq!(tithi_at(julian_day(date(2025, 3, 14), 0.25)), 15);
        assert_eq!(tithi_at(julian_day(date(2025, 3, 14), 0.35)), 16);
    }

    #[test]
    fn sunrise_over_kathmandu_is_plausible() {
        // Kathmandu sunrise runs about 05:10 in June and 06:45 in December NPT.
        for (d, lo, hi) in [
            (date(2025, 6, 21), 5.0, 5.5),
            (date(2025, 12, 21), 6.5, 7.0),
        ] {
            let rise = sunrise(d).unwrap();
            let npt_hour = ((rise + NPT_OFFSET - julian_day(d, 0.0)) * 24.0).rem_euclid(24.0);
            assert!(
                npt_hour > lo && npt_hour < hi,
                "sunrise on {d} was {npt_hour:.2}h NPT, expected {lo}–{hi}"
            );
        }
    }

    #[test]
    fn tithi_start_is_a_real_boundary() {
        // Shukla Dashami before Vijaya Dashami 2025 (2 October).
        let guess = julian_day(date(2025, 10, 1), 0.0);
        let start = *tithi_starts_between(10, guess - 3.0, guess + 3.0)
            .first()
            .unwrap();
        // Just after the boundary it is dashami; just before, navami.
        assert_eq!(tithi_at(start + 0.01), 10);
        assert_eq!(tithi_at(start - 0.01), 9);
    }

    #[test]
    fn npt_date_round_trips() {
        for d in [date(1943, 4, 14), date(2025, 10, 2), date(2033, 12, 31)] {
            // Midday NPT is unambiguously that date.
            let jd = julian_day(d, 0.5 - NPT_OFFSET);
            assert_eq!(npt_date(jd), Some(d));
        }
    }

    #[test]
    fn delta_t_is_continuous_across_its_pieces() {
        for boundary in [1920.0, 1941.0, 1961.0, 1986.0, 2005.0, 2050.0] {
            let before = delta_t(boundary - 0.001);
            let after = delta_t(boundary + 0.001);
            assert!(
                (before - after).abs() < 2.5,
                "ΔT jumps {:.2}s at {boundary}",
                (before - after).abs()
            );
        }
    }
}
