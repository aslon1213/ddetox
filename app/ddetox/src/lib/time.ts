// Small time helpers for rendering session countdowns and unlock times.

/** Seconds since the Unix epoch, right now. */
export function nowUnix(): number {
  return Math.floor(Date.now() / 1000);
}

/**
 * Format a remaining duration (in seconds) as a compact countdown, e.g.
 * "1h 04m 09s" or "12m 30s". Clamps negatives to "0s".
 */
export function formatCountdown(secondsLeft: number): string {
  let s = Math.max(0, Math.floor(secondsLeft));
  const h = Math.floor(s / 3600);
  s -= h * 3600;
  const m = Math.floor(s / 60);
  s -= m * 60;

  const pad = (n: number) => String(n).padStart(2, "0");
  if (h > 0) return `${h}h ${pad(m)}m ${pad(s)}s`;
  if (m > 0) return `${m}m ${pad(s)}s`;
  return `${s}s`;
}

/** Format a Unix timestamp as a local wall-clock time, e.g. "14:05". */
export function formatClock(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}
