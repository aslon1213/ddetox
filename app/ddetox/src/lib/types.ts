// TS mirror of `protocol::config` (app-owned configuration). Field names match
// the Rust serde representation (snake_case; Recurrence is tagged by `kind`).

export interface WebsiteItem {
  id: string;
  label: string;
  domains: string[];
  cidrs: string[];
}

/** Minutes from local midnight, half-open [start_min, end_min). */
export interface TimeWindow {
  start_min: number;
  end_min: number;
}

export interface CivilDate {
  year: number;
  month: number; // 1-12
  day: number; // 1-31
}

/** Structured recurrence presets. Weekdays are 0=Mon .. 6=Sun. */
export type Recurrence =
  | { kind: "everyday" }
  | { kind: "weekdays"; days: number[] }
  | { kind: "month_days"; start_day: number; end_day: number }
  | { kind: "every_n_days"; n: number; anchor: CivilDate }
  | { kind: "date_range"; start: CivilDate; end: CivilDate };

export type RecurrenceKind = Recurrence["kind"];

export interface ScheduleRule {
  recurrence: Recurrence;
  /** Empty = all day on matching days. */
  windows: TimeWindow[];
}

export interface Session {
  id: string;
  name: string;
  item_ids: string[];
  enabled: boolean;
  rules: ScheduleRule[];
}

/** Returned by `get_schedule_state` / `reconcile_now`. */
export interface ScheduleState {
  active_session_ids: string[];
  managed_domains: string[];
  managed_cidrs: string[];
}

export const WEEKDAY_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
