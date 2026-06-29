//! Dependency-free recurrence engine: does a [`Session`] block at a given local
//! moment? The caller supplies the current wall-clock as a [`LocalMoment`]
//! (the GUI builds it from `chrono::Local::now()`); all date math here is plain
//! integer arithmetic so the crate stays std-only and trivially testable.

use crate::config::{CivilDate, Recurrence, ScheduleRule, Session, TimeWindow};

/// A concrete local wall-clock instant to evaluate schedules against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalMoment {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl LocalMoment {
    /// Minutes elapsed since local midnight (0..=1439).
    pub fn minute_of_day(&self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }

    /// Days since 1970-01-01 (can be negative for earlier dates).
    pub fn epoch_day(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// Weekday as `0=Mon .. 6=Sun`. 1970-01-01 was a Thursday (=3).
    pub fn weekday_mon0(&self) -> u8 {
        (self.epoch_day() + 3).rem_euclid(7) as u8
    }
}

/// Days since 1970-01-01 for a civil (proleptic Gregorian) date.
/// Howard Hinnant's `days_from_civil`. Valid for any reasonable date.
pub fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = month as i64;
    let d = day as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // Mar=0 .. Feb=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

impl TimeWindow {
    /// Whether `minute_of_day` falls in this half-open window `[start, end)`.
    pub fn contains(&self, minute_of_day: u16) -> bool {
        minute_of_day >= self.start_min && minute_of_day < self.end_min
    }
}

impl Recurrence {
    /// Whether this recurrence selects the *day* of `m` (ignoring time of day).
    pub fn matches_day(&self, m: &LocalMoment) -> bool {
        match self {
            Recurrence::Everyday => true,
            Recurrence::Weekdays { days } => days.contains(&m.weekday_mon0()),
            Recurrence::MonthDays { start_day, end_day } => {
                let (lo, hi) = if start_day <= end_day {
                    (*start_day, *end_day)
                } else {
                    (*end_day, *start_day)
                };
                m.day >= lo && m.day <= hi
            }
            Recurrence::EveryNDays { n, anchor } => {
                if *n == 0 {
                    return false;
                }
                let delta = m.epoch_day() - anchor.epoch_day();
                delta >= 0 && delta % (*n as i64) == 0
            }
            Recurrence::DateRange { start, end } => {
                let day = m.epoch_day();
                let (a, b) = (start.epoch_day(), end.epoch_day());
                day >= a.min(b) && day <= b.max(a)
            }
        }
    }
}

impl CivilDate {
    pub fn epoch_day(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

impl ScheduleRule {
    /// Whether this rule is active at `m`: the recurrence selects the day, and
    /// (if any windows are defined) the time falls inside one. No windows = all day.
    pub fn matches(&self, m: &LocalMoment) -> bool {
        if !self.recurrence.matches_day(m) {
            return false;
        }
        if self.windows.is_empty() {
            return true;
        }
        let minute = m.minute_of_day();
        self.windows.iter().any(|w| w.contains(minute))
    }
}

impl Session {
    /// Whether this session should be blocking at `m`.
    pub fn is_active_at(&self, m: &LocalMoment) -> bool {
        self.enabled && self.rules.iter().any(|r| r.matches(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Recurrence::*;

    fn at(y: i32, mo: u8, d: u8, h: u8, mi: u8) -> LocalMoment {
        LocalMoment { year: y, month: mo, day: d, hour: h, minute: mi }
    }

    #[test]
    fn epoch_day_anchors() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 1, 1), 10957);
    }

    #[test]
    fn weekday_mapping() {
        // 2024-01-01 was a Monday.
        assert_eq!(at(2024, 1, 1, 0, 0).weekday_mon0(), 0);
        assert_eq!(at(2024, 1, 7, 0, 0).weekday_mon0(), 6); // Sunday
    }

    #[test]
    fn everyday_matches_any_day() {
        let r = Everyday;
        assert!(r.matches_day(&at(2024, 6, 15, 3, 0)));
    }

    #[test]
    fn weekdays_mon_to_fri() {
        let r = Weekdays { days: vec![0, 1, 2, 3, 4] };
        // 2024-06-14 is Friday (in), 2024-06-15 Saturday (out).
        assert!(r.matches_day(&at(2024, 6, 14, 9, 0)));
        assert!(!r.matches_day(&at(2024, 6, 15, 9, 0)));
    }

    #[test]
    fn month_days_first_to_tenth() {
        let r = MonthDays { start_day: 1, end_day: 10 };
        assert!(r.matches_day(&at(2024, 6, 1, 0, 0)));
        assert!(r.matches_day(&at(2024, 6, 10, 0, 0)));
        assert!(!r.matches_day(&at(2024, 6, 11, 0, 0)));
    }

    #[test]
    fn every_n_days_from_anchor() {
        let r = EveryNDays { n: 3, anchor: CivilDate { year: 2024, month: 6, day: 1 } };
        assert!(r.matches_day(&at(2024, 6, 1, 0, 0))); // delta 0
        assert!(!r.matches_day(&at(2024, 6, 2, 0, 0))); // delta 1
        assert!(r.matches_day(&at(2024, 6, 4, 0, 0))); // delta 3
        assert!(!r.matches_day(&at(2024, 5, 29, 0, 0))); // before anchor
    }

    #[test]
    fn date_range_inclusive() {
        let r = DateRange {
            start: CivilDate { year: 2024, month: 6, day: 10 },
            end: CivilDate { year: 2024, month: 6, day: 12 },
        };
        assert!(!r.matches_day(&at(2024, 6, 9, 23, 59)));
        assert!(r.matches_day(&at(2024, 6, 10, 0, 0)));
        assert!(r.matches_day(&at(2024, 6, 12, 23, 59)));
        assert!(!r.matches_day(&at(2024, 6, 13, 0, 0)));
    }

    #[test]
    fn windows_gate_time_of_day() {
        let rule = ScheduleRule {
            recurrence: Everyday,
            windows: vec![TimeWindow { start_min: 9 * 60, end_min: 17 * 60 }],
        };
        assert!(!rule.matches(&at(2024, 6, 15, 8, 59)));
        assert!(rule.matches(&at(2024, 6, 15, 9, 0)));
        assert!(rule.matches(&at(2024, 6, 15, 16, 59)));
        assert!(!rule.matches(&at(2024, 6, 15, 17, 0))); // half-open end
    }

    #[test]
    fn no_windows_means_all_day() {
        let rule = ScheduleRule { recurrence: Everyday, windows: vec![] };
        assert!(rule.matches(&at(2024, 6, 15, 0, 0)));
        assert!(rule.matches(&at(2024, 6, 15, 23, 59)));
    }

    #[test]
    fn session_requires_enabled_and_a_matching_rule() {
        let mut s = Session {
            id: "s1".into(),
            name: "Focus".into(),
            item_ids: vec!["i1".into()],
            enabled: true,
            rules: vec![ScheduleRule {
                recurrence: Weekdays { days: vec![0, 1, 2, 3, 4] },
                windows: vec![TimeWindow { start_min: 9 * 60, end_min: 12 * 60 }],
            }],
        };
        assert!(s.is_active_at(&at(2024, 6, 14, 10, 0))); // Fri 10:00
        assert!(!s.is_active_at(&at(2024, 6, 14, 13, 0))); // Fri 13:00, outside window
        assert!(!s.is_active_at(&at(2024, 6, 15, 10, 0))); // Sat
        s.enabled = false;
        assert!(!s.is_active_at(&at(2024, 6, 14, 10, 0)));
    }
}
