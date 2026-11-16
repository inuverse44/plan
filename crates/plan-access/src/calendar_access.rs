//! CalendarAccess: business days, days off per person and release freezes. Japanese public holidays are computed from the rules of the law (equinox formula valid for 1980–2099)
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use plan_contracts::{BusinessCalendar, Plan};
use std::collections::{BTreeMap, HashSet};

pub trait CalendarAccess {
    fn business_calendar(&self, plan: &Plan) -> BusinessCalendar;
}

pub struct JpRuleCalendar;

fn nth_monday(y: i32, m: u32, n: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let off = (7 + Weekday::Mon.num_days_from_monday() as i64 - first.weekday().num_days_from_monday() as i64) % 7;
    first + Duration::days(off + 7 * (n as i64 - 1))
}

/// Japanese public holidays of a year (including substitute holidays and citizens' holidays)
pub fn jp_holidays(y: i32) -> HashSet<NaiveDate> {
    let d = |m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
    let k = (y - 1980) as f64;
    let leap = ((y - 1980) / 4) as f64;
    let vernal = (20.8431 + 0.242194 * k - leap).floor() as u32;
    let autumnal = (23.2488 + 0.242194 * k - leap).floor() as u32;
    let mut h: HashSet<NaiveDate> = [
        d(1, 1), nth_monday(y, 1, 2), d(2, 11), d(2, 23), d(3, vernal), d(4, 29), d(5, 3), d(5, 4), d(5, 5),
        nth_monday(y, 7, 3), d(8, 11), nth_monday(y, 9, 3), d(9, autumnal), nth_monday(y, 10, 2), d(11, 3), d(11, 23),
    ].into_iter().collect();
    // Citizens' holiday: a weekday between two holidays
    let mut sandwiched = vec![];
    for x in h.iter() {
        let mid = *x + Duration::days(1);
        if h.contains(&(mid + Duration::days(1))) && !h.contains(&mid) && mid.weekday() != Weekday::Sun { sandwiched.push(mid); }
    }
    h.extend(sandwiched);
    // Substitute holiday: the first non-holiday after a holiday on a Sunday
    let mut subs = vec![];
    for x in h.iter().filter(|x| x.weekday() == Weekday::Sun) {
        let mut s = *x + Duration::days(1);
        while h.contains(&s) { s += Duration::days(1); }
        subs.push(s);
    }
    h.extend(subs);
    h
}

impl CalendarAccess for JpRuleCalendar {
    fn business_calendar(&self, plan: &Plan) -> BusinessCalendar {
        let c = &plan.calendar;
        let y0 = plan.start.year();
        let mut off: HashSet<NaiveDate> = c.off.iter().copied().collect();
        for y in y0..y0 + 3 {
            if c.year_end { for i in 0..6 { off.insert(NaiveDate::from_ymd_opt(y, 12, 29).unwrap() + Duration::days(i)); } }
            if c.holidays_jp { off.extend(jp_holidays(y)); }
        }
        let mut days = Vec::new();
        let mut weekday_off = Vec::new();
        let mut d = plan.start;
        while days.len() < 500 {
            let wk = d.weekday().num_days_from_monday() < 5;
            if wk && !off.contains(&d) { days.push(d) } else if wk { weekday_off.push(d) }
            d += Duration::days(1);
        }
        let frozen = days.iter().map(|x| c.freeze.iter().any(|(a, b)| a <= x && x <= b)).collect();
        let leave: BTreeMap<String, Vec<usize>> = plan.people.iter()
            .map(|(p, v)| (p.clone(), v.leave.iter().filter_map(|l| days.iter().position(|x| x == l)).collect())).collect();
        BusinessCalendar { days, start_slot: if plan.start_pm { 1 } else { 0 }, frozen, leave, off: weekday_off }
    }
}
