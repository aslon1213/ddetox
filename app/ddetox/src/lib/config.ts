// Typed bridge to the app-config commands, plus a TS port of the recurrence
// matcher so the calendar can evaluate concrete days without a round-trip per cell.

import { invoke } from "@tauri-apps/api/core";
import {
  WEEKDAY_LABELS,
  type CivilDate,
  type Recurrence,
  type ScheduleRule,
  type ScheduleState,
  type Session,
  type TimeWindow,
  type WebsiteItem,
} from "./types";

export async function getLibrary(): Promise<WebsiteItem[]> {
  return invoke<WebsiteItem[]>("get_library");
}

export async function saveLibrary(items: WebsiteItem[]): Promise<void> {
  await invoke("save_library", { items });
}

export async function getSessions(): Promise<Session[]> {
  return invoke<Session[]>("get_sessions");
}

export async function saveSessions(sessions: Session[]): Promise<void> {
  await invoke("save_sessions", { sessions });
}

export async function getScheduleState(): Promise<ScheduleState> {
  return invoke<ScheduleState>("get_schedule_state");
}

/**
 * Force an immediate reconcile against the daemon. Throws if the daemon is
 * unreachable — callers should treat that softly (the background loop retries).
 */
export async function reconcileNow(): Promise<ScheduleState> {
  return invoke<ScheduleState>("reconcile_now");
}

// ---- recurrence matcher (mirror of protocol::schedule) ----

/** Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm). */
export function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const mp = month > 2 ? month - 3 : month + 9;
  const doy = Math.floor((153 * mp + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

/** Weekday as 0=Mon .. 6=Sun. */
export function weekdayMon0(year: number, month: number, day: number): number {
  return (((daysFromCivil(year, month, day) + 3) % 7) + 7) % 7;
}

export function recurrenceMatchesDay(
  r: Recurrence,
  year: number,
  month: number,
  day: number,
): boolean {
  switch (r.kind) {
    case "everyday":
      return true;
    case "weekdays":
      return r.days.includes(weekdayMon0(year, month, day));
    case "month_days": {
      const lo = Math.min(r.start_day, r.end_day);
      const hi = Math.max(r.start_day, r.end_day);
      return day >= lo && day <= hi;
    }
    case "every_n_days": {
      if (r.n <= 0) return false;
      const delta =
        daysFromCivil(year, month, day) -
        daysFromCivil(r.anchor.year, r.anchor.month, r.anchor.day);
      return delta >= 0 && delta % r.n === 0;
    }
    case "date_range": {
      const d = daysFromCivil(year, month, day);
      const a = daysFromCivil(r.start.year, r.start.month, r.start.day);
      const b = daysFromCivil(r.end.year, r.end.month, r.end.day);
      return d >= Math.min(a, b) && d <= Math.max(a, b);
    }
  }
}

/** The time windows a session is active for on a given local date (for the calendar). */
export function sessionWindowsOnDay(
  session: Session,
  year: number,
  month: number,
  day: number,
): TimeWindow[] {
  if (!session.enabled) return [];
  const out: TimeWindow[] = [];
  for (const rule of session.rules) {
    if (recurrenceMatchesDay(rule.recurrence, year, month, day)) {
      if (rule.windows.length === 0) out.push({ start_min: 0, end_min: 1440 });
      else out.push(...rule.windows);
    }
  }
  return out;
}

// ---- small formatting helpers shared by the schedule UI ----

/** "09:30" from minutes-of-day. */
export function minToHHMM(min: number): string {
  const h = Math.floor(min / 60);
  const m = min % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** Minutes-of-day from "HH:MM" (returns null if malformed). */
export function hhmmToMin(value: string): number | null {
  const m = value.match(/^(\d{1,2}):(\d{2})$/);
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 24 || min > 59 || (h === 24 && min !== 0)) return null;
  return h * 60 + min;
}

function fmtCivil(d: CivilDate): string {
  return `${d.year}-${String(d.month).padStart(2, "0")}-${String(d.day).padStart(2, "0")}`;
}

/** Human summary of a recurrence, e.g. "Mon, Tue, Wed, Thu, Fri". */
export function describeRecurrence(r: Recurrence): string {
  switch (r.kind) {
    case "everyday":
      return "Every day";
    case "weekdays":
      return r.days.length ? r.days.map((d) => WEEKDAY_LABELS[d]).join(", ") : "No days";
    case "month_days":
      return `Days ${r.start_day}–${r.end_day} of month`;
    case "every_n_days":
      return `Every ${r.n} day(s) from ${fmtCivil(r.anchor)}`;
    case "date_range":
      return `${fmtCivil(r.start)} → ${fmtCivil(r.end)}`;
  }
}

/** Human summary of one rule, e.g. "Mon–Fri, 09:00–17:00". */
export function describeRule(rule: ScheduleRule): string {
  const days = describeRecurrence(rule.recurrence);
  const times =
    rule.windows.length === 0
      ? "all day"
      : rule.windows.map((w) => `${minToHHMM(w.start_min)}–${minToHHMM(w.end_min)}`).join(", ");
  return `${days}, ${times}`;
}

/** Human summary of a whole schedule, e.g. "Mon–Fri, 09:00–17:00 · Every day, all day". */
export function describeSchedule(rules: ScheduleRule[]): string {
  return rules.length ? rules.map(describeRule).join(" · ") : "never";
}

/** crypto.randomUUID with a fallback for older webviews. */
export function newId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
