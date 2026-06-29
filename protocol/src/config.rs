//! App-owned configuration model: the website library and scheduled sessions.
//!
//! These types are **not** part of the daemon wire protocol — the daemon never
//! sees them. They live here, in the shared crate, so the recurrence engine in
//! [`crate::schedule`] can be unit-tested centrally and so this model could move
//! into the daemon if scheduling ever becomes daemon-native. The GUI persists
//! them as JSON and mirrors them 1:1 in TypeScript.

use serde::{Deserialize, Serialize};

/// A reusable "site" in the library: a label plus the domains and CIDRs that
/// blocking it should cover (e.g. `Reddit` = `reddit.com` + `*.reddit.com`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsiteItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
}

/// A named group of library items with one or more schedule rules. The session
/// is "active" when it is enabled and any of its rules matches the current
/// local moment (see [`crate::schedule`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    /// References into the library by [`WebsiteItem::id`].
    #[serde(default)]
    pub item_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<ScheduleRule>,
}

fn default_true() -> bool {
    true
}

/// One recurrence plus the daily time windows it applies to. An empty `windows`
/// list means "all day" on the days the recurrence matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub recurrence: Recurrence,
    #[serde(default)]
    pub windows: Vec<TimeWindow>,
}

/// A within-day time window in minutes from local midnight, half-open
/// `[start_min, end_min)`. Same-day only (`start_min < end_min`); a window that
/// crosses midnight is expressed as two windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start_min: u16,
    pub end_min: u16,
}

/// A structured recurrence preset. Days are `0=Mon .. 6=Sun`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    /// Every calendar day.
    Everyday,
    /// Specific weekdays, e.g. Mon–Fri = `[0,1,2,3,4]`.
    Weekdays { days: Vec<u8> },
    /// An inclusive day-of-month range, e.g. the 1st–10th = `{start_day:1, end_day:10}`.
    MonthDays { start_day: u8, end_day: u8 },
    /// Every `n` days counting from `anchor` (inclusive of the anchor day).
    EveryNDays { n: u32, anchor: CivilDate },
    /// An inclusive fixed calendar range (a one-off span).
    DateRange { start: CivilDate, end: CivilDate },
}

/// A timezone-free calendar date (interpreted in the user's local time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}
