//! Festivals and public holidays, loaded from an embedded dataset.
//!
//! Most Nepali festivals are tithi-based (lunar), so they cannot be derived from
//! `BS_REFERENCE` the way month lengths can. Four entry kinds cover the ground:
//!
//! * `fixed`    - same BS month/day every year (Nepali New Year, Constitution Day)
//! * `ad_fixed` - same Gregorian month/day every year (Labour Day, Christmas)
//! * `computed` - a tithi rule in `data/rules.json`, resolved to a date by
//!   [`crate::panchanga`]. This covers the festivals that matter most — the
//!   Dashain and Tihar sequences, Teej, Shivaratri, Holi — for every year in
//!   range, with nothing to hand-enter and nothing to go stale.
//! * `lunar`    - a hand-verified date pinned to one BS year, which *overrides*
//!   the computed rule of the same name. Nepal's holiday gazette occasionally
//!   moves an observance for reasons no ephemeris can predict; this is where
//!   that goes, without giving up computation for the other ninety years.
//!
//! Both datasets are embedded with `include_str!`, so there is no runtime file
//! dependency and no network access. They are parsed once, lazily.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::BsDate;
use crate::panchanga::{self, Kala};

const FESTIVAL_DATA: &str = include_str!("data/festivals.json");
const RULE_DATA: &str = include_str!("data/rules.json");

/// A single festival or public holiday occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Festival {
    /// Name in English / romanized Nepali.
    pub en: String,
    /// Name in Devanagari.
    pub np: String,
    /// Whether this is a gazetted public holiday, as opposed to an observance.
    #[serde(default)]
    pub holiday: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct FixedEntry {
    month: u8,
    day: u8,
    #[serde(flatten)]
    festival: Festival,
}

#[derive(Debug, Clone, Deserialize)]
struct AdFixedEntry {
    ad_month: u32,
    ad_day: u32,
    #[serde(flatten)]
    festival: Festival,
}

#[derive(Debug, Clone, Deserialize)]
struct LunarEntry {
    year: u16,
    month: u8,
    day: u8,
    #[serde(flatten)]
    festival: Festival,
}

#[derive(Debug, Clone, Deserialize)]
struct FestivalData {
    /// Inclusive `[first, last]` BS years for which lunar entries were entered.
    /// Callers surface this so a user paging past it knows the grid is thinner
    /// there rather than assuming the year genuinely has no festivals.
    #[serde(default)]
    lunar_years_covered: Vec<u16>,
    #[serde(default)]
    fixed: Vec<FixedEntry>,
    #[serde(default)]
    ad_fixed: Vec<AdFixedEntry>,
    #[serde(default)]
    lunar: Vec<LunarEntry>,
}

// ---- computed (tithi) festivals ---------------------------------------------

/// One tithi rule from `data/rules.json`.
#[derive(Debug, Clone, Deserialize)]
struct ComputedRule {
    /// 1-12, the amanta lunar month: the lunation named for the BS solar month
    /// its opening new moon falls in.
    lunar_month: u8,
    paksha: Paksha,
    /// 1-15 within the paksha.
    tithi: u8,
    kala: KalaSpec,
    #[serde(flatten)]
    festival: Festival,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Paksha {
    Shukla,
    Krishna,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum KalaSpec {
    Udaya,
    Madhyanna,
    Aparahna,
    Pradosh,
    Nishita,
    Begins,
}

impl From<KalaSpec> for Kala {
    fn from(spec: KalaSpec) -> Self {
        match spec {
            KalaSpec::Udaya => Kala::Udaya,
            KalaSpec::Madhyanna => Kala::Madhyanna,
            KalaSpec::Aparahna => Kala::Aparahna,
            KalaSpec::Pradosh => Kala::Pradosh,
            KalaSpec::Nishita => Kala::Nishita,
            KalaSpec::Begins => Kala::Begins,
        }
    }
}

impl ComputedRule {
    /// The tithi as a 1-30 index from Shukla Pratipada, which is how
    /// [`panchanga::tithi_at`] numbers them.
    fn tithi_index(&self) -> u8 {
        match self.paksha {
            Paksha::Shukla => self.tithi,
            Paksha::Krishna => 15 + self.tithi,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RuleData {
    #[serde(default)]
    computed: Vec<ComputedRule>,
    #[serde(default)]
    spans: Vec<SpanRule>,
}

/// A gazetted holiday block between two computed festivals, such as Dashain's
/// Fulpati-to-Dwadashi. Days inside it with no holiday of their own get its name.
#[derive(Debug, Clone, Deserialize)]
struct SpanRule {
    from: String,
    to: String,
    #[serde(default)]
    extra_days: i64,
    #[serde(flatten)]
    festival: Festival,
}

fn rules() -> &'static RuleData {
    static RULES: OnceLock<RuleData> = OnceLock::new();
    RULES
        .get_or_init(|| serde_json::from_str(RULE_DATA).expect("embedded rules.json must be valid"))
}

/// The new moon opening each amanta lunar month of BS year `bs_year`, keyed by
/// month number.
///
/// A lunar month is named for the BS solar month containing its opening new moon.
/// Two new moons can land in one solar month — an adhika (leap) month — in which
/// case the second is the nija month that carries the festivals, so the last
/// match wins.
///
/// All twelve are found in one sweep. Scanning the lunar series is the expensive
/// part of resolving a year, and doing it once per month instead of once per year
/// was the difference between a month of JSON taking 160ms and taking 15ms.
fn lunar_month_starts(bs_year: u16) -> HashMap<u8, f64> {
    let mut starts = HashMap::new();

    let Ok(year_start) = crate::bs_to_ad(BsDate {
        year: bs_year,
        month: 1,
        day: 1,
    }) else {
        return starts;
    };
    let Ok(last_month_len) = crate::bs_month_len(bs_year, 12) else {
        return starts;
    };
    let Ok(year_end) = crate::bs_to_ad(BsDate {
        year: bs_year,
        month: 12,
        day: last_month_len,
    }) else {
        return starts;
    };

    // A lunar month opens up to ~30 days before the solar month it is named for
    // ends, so widen the window rather than clipping it to the BS year.
    let from = panchanga::julian_day(year_start, 0.0) - 35.0;
    let to = panchanga::julian_day(year_end, 0.0) + 5.0;

    for new_moon in panchanga::tithi_starts_between(1, from, to) {
        let Some(date) = panchanga::npt_date(new_moon) else {
            continue;
        };
        let Ok(bs) = crate::ad_to_bs(date) else {
            continue;
        };
        if bs.year == bs_year {
            starts.insert(bs.month, new_moon);
        }
    }

    starts
}

/// The Gregorian date a tithi beginning at `tithi_start` is celebrated on, given
/// the festival's kala rule.
///
/// The tithi owns whichever day has its kala instant inside the tithi's span. A
/// kshaya tithi — one short enough to slip entirely between two consecutive kala
/// instants — owns no such day, and falls back to the day it began on.
fn festival_date(tithi_start: f64, tithi_index: u8, kala: Kala) -> Option<NaiveDate> {
    let start_date = panchanga::npt_date(tithi_start)?;
    if kala == Kala::Begins {
        return Some(start_date);
    }

    let next = if tithi_index == 30 {
        1
    } else {
        tithi_index + 1
    };
    // A tithi runs 19-26 hours, so its end is comfortably inside two days.
    let end =
        *panchanga::tithi_starts_between(next, tithi_start + 0.1, tithi_start + 2.5).first()?;

    for offset in -1..=2 {
        let day = start_date.checked_add_signed(Duration::days(offset))?;
        let claims = if kala == Kala::Nishita {
            // Nishita is a muhurta, not a moment: the tithi has to cover all of
            // it. Ashtami that begins a few minutes into the night has not made
            // that night its own.
            panchanga::nishita_window(day)
                .map(|(from, to)| tithi_start <= from && to <= end)
                .unwrap_or(false)
        } else {
            panchanga::kala_instant(day, kala)
                .map(|instant| instant >= tithi_start && instant < end)
                .unwrap_or(false)
        };
        if claims {
            return Some(day);
        }
    }

    // A kshaya tithi can slip between two consecutive instants of its own kala and
    // claim no day by that rule. It still has a sunrise it is running at, and
    // udaya is the universal fallback — not the day it happened to begin on,
    // which for a tithi starting after noon is a day too early.
    if kala != Kala::Udaya {
        return festival_date(tithi_start, tithi_index, Kala::Udaya);
    }

    Some(start_date)
}

/// One BS year's resolved festivals, shared out of the memo by handle.
type YearFestivals = Arc<Vec<(BsDate, Festival)>>;

/// Every computed festival in one BS year, as `(bs_date, festival)`.
///
/// Resolving one rule costs several bisections against the lunar series, so a
/// whole year is resolved at once and memoised: paging months in a GUI must not
/// re-solve the ephemeris on every redraw.
fn computed_for_year(bs_year: u16) -> YearFestivals {
    static CACHE: OnceLock<Mutex<HashMap<u16, YearFestivals>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(&bs_year)
    {
        return Arc::clone(hit);
    }

    let mut found: Vec<(BsDate, Festival)> = Vec::new();
    let month_starts = lunar_month_starts(bs_year);

    for rule in &rules().computed {
        let Some(&new_moon) = month_starts.get(&rule.lunar_month) else {
            continue;
        };

        let index = rule.tithi_index();
        // Tithi 1 begins at the new moon itself; the rest follow within a lunation.
        let tithi_start = if index == 1 {
            new_moon
        } else {
            let Some(&s) =
                panchanga::tithi_starts_between(index, new_moon + 0.5, new_moon + 30.0).first()
            else {
                continue;
            };
            s
        };

        let Some(ad) = festival_date(tithi_start, index, rule.kala.into()) else {
            continue;
        };
        let Ok(bs) = crate::ad_to_bs(ad) else {
            continue;
        };
        found.push((bs, rule.festival.clone()));
    }

    fill_spans(&mut found);
    found.sort_by_key(|(bs, _)| (bs.year, bs.month, bs.day));

    let entry = Arc::new(found);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(bs_year, Arc::clone(&entry));
    }
    entry
}

/// Mark every day of each gazetted span as a holiday. A span whose endpoints did
/// not resolve this year is skipped rather than guessed at.
fn fill_spans(found: &mut Vec<(BsDate, Festival)>) {
    let ad_of = |found: &[(BsDate, Festival)], name: &str| {
        found
            .iter()
            .find(|(_, f)| f.en == name)
            .and_then(|(bs, _)| crate::bs_to_ad(*bs).ok())
    };
    for span in &rules().spans {
        let (Some(from), Some(to)) = (ad_of(found, &span.from), ad_of(found, &span.to)) else {
            continue;
        };
        let mut day = from;
        while day <= to + Duration::days(span.extra_days) {
            if let Ok(bs) = crate::ad_to_bs(day)
                && !found.iter().any(|(when, f)| *when == bs && f.holiday)
            {
                found.push((bs, span.festival.clone()));
            }
            day += Duration::days(1);
        }
    }
}

fn data() -> &'static FestivalData {
    static DATA: OnceLock<FestivalData> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(FESTIVAL_DATA).expect("embedded festivals.json must be valid")
    })
}

/// Inclusive BS year range for which tithi-based festivals are available.
///
/// Since they are computed rather than tabulated, this is simply the whole range
/// the BS conversion table covers. A consumer can stop warning users that Dashain
/// might be missing.
pub fn festival_coverage() -> (u16, u16) {
    (crate::BS_START_YEAR, crate::BS_END_YEAR)
}

/// Inclusive BS year range with hand-verified overrides, as `(first, last)`, or
/// `None` when there are none. These correct computed dates, never replace them.
pub fn override_years() -> Option<(u16, u16)> {
    let years = &data().lunar_years_covered;
    match years.len() {
        0 => None,
        1 => Some((years[0], years[0])),
        _ => Some((years[0], years[1])),
    }
}

/// Every festival falling on one day. `ad` is the Gregorian date the BS date maps
/// to, needed for the AD-fixed holidays.
pub fn festivals_on(bs: BsDate, ad: NaiveDate) -> Vec<Festival> {
    let data = data();
    let mut found = Vec::new();

    for entry in &data.fixed {
        if entry.month == bs.month && entry.day == bs.day {
            found.push(entry.festival.clone());
        }
    }

    for entry in &data.ad_fixed {
        if entry.ad_month == ad.month() && entry.ad_day == ad.day() {
            found.push(entry.festival.clone());
        }
    }

    // A hand-verified entry for this BS year replaces the computed rule of the
    // same name, so a gazetted change wins without silencing the rest of the year.
    let overridden: Vec<&str> = data
        .lunar
        .iter()
        .filter(|entry| entry.year == bs.year)
        .map(|entry| entry.festival.en.as_str())
        .collect();

    // A festival in the last lunar month of a BS year can land in the next one,
    // so the previous year's resolution is consulted too — but only near the
    // year boundary. Resolving a year costs a sweep of the lunar series, and
    // doing it twice for a month in midsummer buys nothing.
    let mut years = vec![bs.year];
    if bs.month <= 2 {
        years.insert(0, bs.year.saturating_sub(1));
    }
    for year in years {
        for (when, festival) in computed_for_year(year).iter() {
            if *when == bs && !overridden.contains(&festival.en.as_str()) {
                found.push(festival.clone());
            }
        }
    }

    for entry in &data.lunar {
        if entry.year == bs.year && entry.month == bs.month && entry.day == bs.day {
            found.push(entry.festival.clone());
        }
    }

    found.dedup_by(|a, b| a.en == b.en);
    found
}

/// Every festival in one BS month, as `(bs_day, festival)` sorted by day.
///
/// AD-fixed holidays are resolved by walking the month, so a Gregorian date that
/// lands mid-month is picked up correctly.
pub fn festivals_in_month(year: u16, month: u8) -> Vec<(u8, Festival)> {
    let Ok(month_len) = crate::bs_month_len(year, month) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for day in 1..=month_len {
        let bs = BsDate { year, month, day };
        let Ok(ad) = crate::bs_to_ad(bs) else {
            continue;
        };
        for festival in festivals_on(bs, ad) {
            found.push((day, festival));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bs_to_ad;

    #[test]
    fn embedded_dataset_parses() {
        // `data()` panics on malformed JSON; touching it is the test.
        assert!(!data().fixed.is_empty());
    }

    #[test]
    fn new_year_is_found_in_every_year() {
        for year in [2080u16, 2083, 2090] {
            let bs = BsDate {
                year,
                month: 1,
                day: 1,
            };
            let ad = bs_to_ad(bs).unwrap();
            let names: Vec<_> = festivals_on(bs, ad).into_iter().map(|f| f.en).collect();
            assert!(
                names.iter().any(|n| n.contains("New Year")),
                "BS {year}-01-01 should carry Nepali New Year, got {names:?}"
            );
        }
    }

    #[test]
    fn a_year_without_lunar_data_still_returns_fixed_festivals() {
        // BS 2005 predates any hand-entered lunar data; it must not error out.
        let bs = BsDate {
            year: 2005,
            month: 1,
            day: 1,
        };
        let ad = bs_to_ad(bs).unwrap();
        assert!(!festivals_on(bs, ad).is_empty());
    }

    #[test]
    fn ordinary_days_have_no_festivals() {
        // Baisakh 2083 carries New Year, Mata Tirtha Aunsi, Akshaya Tritiya and
        // Buddha Jayanti; the 11th is between all of them.
        let bs = BsDate {
            year: 2083,
            month: 1,
            day: 11,
        };
        let ad = bs_to_ad(bs).unwrap();
        assert!(festivals_on(bs, ad).is_empty());
    }

    #[test]
    fn month_lookup_is_sorted_and_bounded() {
        let found = festivals_in_month(2083, 1);
        assert!(found.iter().any(|(day, _)| *day == 1));
        assert!(found.windows(2).all(|w| w[0].0 <= w[1].0));
        let month_len = crate::bs_month_len(2083, 1).unwrap();
        assert!(found.iter().all(|(day, _)| *day >= 1 && *day <= month_len));
    }

    #[test]
    fn out_of_range_month_is_empty_not_a_panic() {
        assert!(festivals_in_month(2083, 13).is_empty());
        assert!(festivals_in_month(9999, 1).is_empty());
    }

    /// Resolve a festival by name anywhere in a BS year.
    fn find(bs_year: u16, name: &str) -> Option<NaiveDate> {
        (1..=12).find_map(|month| {
            festivals_in_month(bs_year, month)
                .into_iter()
                .find(|(_, f)| f.en == name)
                .and_then(|(day, _)| {
                    crate::bs_to_ad(BsDate {
                        year: bs_year,
                        month,
                        day,
                    })
                    .ok()
                })
        })
    }

    /// The gazetted Dashain and Tihar blocks are holidays end to end, including
    /// days like Ekadashi that carry no festival of their own.
    #[test]
    fn dashain_and_tihar_blocks_are_holidays_throughout() {
        // (BS year, first (m, d), last (m, d)) as published by the Ministry of Home Affairs.
        let blocks = [
            (2081, (6, 24), (6, 28)),
            (2081, (7, 15), (7, 19)),
            (2082, (6, 13), (6, 18)),
            (2082, (7, 3), (7, 7)),
            (2083, (6, 31), (7, 6)),
            (2083, (7, 22), (7, 26)),
        ];
        for (year, first, last) in blocks {
            for month in first.0..=last.0 {
                let from = if month == first.0 { first.1 } else { 1 };
                let to = if month == last.0 {
                    last.1
                } else {
                    crate::bs_month_len(year, month).unwrap()
                };
                for day in from..=to {
                    let bs = BsDate { year, month, day };
                    let fs = festivals_on(bs, bs_to_ad(bs).unwrap());
                    assert!(
                        fs.iter().any(|f| f.holiday),
                        "BS {year}-{month}-{day} should be a holiday, got {fs:?}"
                    );
                }
            }
        }
    }

    /// The test that decides whether this ships.
    ///
    /// Every date here is one published for Nepal, not one this code produced.
    /// They deliberately include the awkward years: BS 2081 lost a tithi, so Maha
    /// Ashtami and Maha Navami share 11 October 2024, and Kukur Tihar shares 31
    /// October with Laxmi Puja. A rule that cannot be made to match a real date is
    /// removed and the date pinned in `festivals.json`'s `lunar` array instead —
    /// a festival computing the wrong day is worse than one that is missing.
    #[test]
    fn known_festival_dates_match() {
        let cases: &[(u16, &str, (i32, u32, u32))] = &[
            // BS 2081 — Dashain, the kshaya-tithi year
            (2081, "Ghatasthapana", (2024, 10, 3)),
            (2081, "Fulpati", (2024, 10, 10)),
            (2081, "Maha Ashtami", (2024, 10, 11)),
            (2081, "Maha Navami", (2024, 10, 11)),
            (2081, "Vijaya Dashami", (2024, 10, 12)),
            (2081, "Kojagrat Purnima", (2024, 10, 17)),
            // BS 2081 — Tihar, where Kukur Tihar and Laxmi Puja coincide
            (2081, "Kaag Tihar", (2024, 10, 30)),
            (2081, "Kukur Tihar", (2024, 10, 31)),
            (2081, "Laxmi Puja", (2024, 10, 31)),
            (2081, "Govardhan and Mha Puja", (2024, 11, 2)),
            (2081, "Bhai Tika", (2024, 11, 3)),
            (2081, "Chhath", (2024, 11, 7)),
            // BS 2081 — spring
            (2081, "Maha Shivaratri", (2025, 2, 26)),
            // Gazette: Falgun 29 for the hills, Chaitra 1 for the Terai.
            (2081, "Fagu Purnima (hills)", (2025, 3, 13)),
            (2081, "Fagu Purnima (Terai)", (2025, 3, 14)),
            (2081, "Jitiya", (2024, 9, 25)),
            (2081, "Guru Nanak Jayanti", (2024, 11, 15)),
            // BS 2082 — Dashain and Tihar in an ordinary year
            (2082, "Ghatasthapana", (2025, 9, 22)),
            (2082, "Fulpati", (2025, 9, 29)),
            (2082, "Maha Ashtami", (2025, 9, 30)),
            (2082, "Maha Navami", (2025, 10, 1)),
            (2082, "Vijaya Dashami", (2025, 10, 2)),
            (2082, "Kojagrat Purnima", (2025, 10, 7)),
            (2082, "Kaag Tihar", (2025, 10, 19)),
            (2082, "Kukur Tihar", (2025, 10, 20)),
            (2082, "Laxmi Puja", (2025, 10, 20)),
            (2082, "Govardhan and Mha Puja", (2025, 10, 22)),
            (2082, "Bhai Tika", (2025, 10, 23)),
            (2082, "Chhath", (2025, 10, 27)),
            // BS 2082 — the rest of the year
            (2082, "Buddha Jayanti", (2025, 5, 12)),
            (2082, "Janai Purnima", (2025, 8, 9)),
            (2082, "Gai Jatra", (2025, 8, 10)),
            (2082, "Krishna Janmashtami", (2025, 8, 16)),
            (2082, "Haritalika Teej", (2025, 8, 26)),
            (2082, "Rishi Panchami", (2025, 8, 28)),
            (2082, "Indra Jatra", (2025, 9, 6)),
            (2082, "Maha Shivaratri", (2026, 2, 15)),
            (2082, "Fagu Purnima (hills)", (2026, 3, 2)),
            (2082, "Fagu Purnima (Terai)", (2026, 3, 3)),
            // The gazette's own date, a day after udaya ashtami: a `lunar` override.
            (2082, "Jitiya", (2025, 9, 15)),
            (2082, "Ghode Jatra", (2026, 3, 18)),
            (2082, "Ram Navami", (2026, 3, 27)),
            // BS 2083 — the run every source agrees on. Vijaya Dashami 2083 is
            // deliberately absent: popular sources say 20 October 2026, while
            // the Panchang Nirnayak Samiti publishes a tika muhurta of 11:53 on
            // the 21st. Dashami does not begin until 13:06 on the 20th, so it is
            // running at neither midday nor aparahna that day and the samiti's
            // own time is only satisfied on the 21st, which is what the rule
            // here produces. Pinning either date would be asserting more than is
            // settled.
            (2083, "Ghatasthapana", (2026, 10, 11)),
            (2083, "Fulpati", (2026, 10, 17)),
            (2083, "Maha Ashtami", (2026, 10, 18)),
            (2083, "Maha Navami", (2026, 10, 19)),
            // BS 2083 — the rest of the gazetted list.
            (2083, "Buddha Jayanti", (2026, 5, 1)),
            (2083, "Janai Purnima", (2026, 8, 28)),
            (2083, "Gai Jatra", (2026, 8, 29)),
            (2083, "Krishna Janmashtami", (2026, 9, 4)),
            (2083, "Haritalika Teej", (2026, 9, 14)),
            (2083, "Indra Jatra", (2026, 9, 25)),
            (2083, "Jitiya", (2026, 10, 4)),
            (2083, "Laxmi Puja", (2026, 11, 8)),
            (2083, "Govardhan and Mha Puja", (2026, 11, 10)),
            (2083, "Bhai Tika", (2026, 11, 11)),
            (2083, "Chhath", (2026, 11, 15)),
            (2083, "Yomari Punhi", (2026, 12, 24)),
            (2083, "Sonam Losar", (2027, 2, 7)),
            (2083, "Basanta Panchami", (2027, 2, 11)),
            (2083, "Maha Shivaratri", (2027, 3, 6)),
            (2083, "Gyalpo Losar", (2027, 3, 9)),
            (2083, "Fagu Purnima (hills)", (2027, 3, 21)),
            (2083, "Fagu Purnima (Terai)", (2027, 3, 22)),
            (2083, "Ghode Jatra", (2027, 4, 6)),
            (2083, "Martyrs' Day", (2027, 1, 30)),
        ];

        let mut wrong = Vec::new();
        for (year, name, (y, m, d)) in cases {
            let expected = NaiveDate::from_ymd_opt(*y, *m, *d).unwrap();
            match find(*year, name) {
                Some(got) if got == expected => {}
                Some(got) => {
                    wrong.push(format!("BS {year} {name}: got {got}, expected {expected}"))
                }
                None => wrong.push(format!("BS {year} {name}: not found, expected {expected}")),
            }
        }
        assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
    }
}
