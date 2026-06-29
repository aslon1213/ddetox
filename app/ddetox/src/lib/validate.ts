// Client-side input validation for blocklist entries.
//
// This is honest-UX validation only: it normalizes input and catches obvious
// mistakes before they hit the wire. The daemon validates authoritatively and
// is the real gate — never assume something is accepted just because it passed
// here.

// A DNS label: 1–63 chars, alphanumeric with internal hyphens.
const LABEL = "[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?";
// A fully-qualified host (no mode prefix), e.g. "reddit.com".
const HOST_RE = new RegExp(`^(?:${LABEL}\\.)+${LABEL}$`, "i");

/**
 * Normalize and validate a blocklist domain, preserving an optional matching-mode
 * prefix (mirrors the daemon's `normalize_domain`):
 *
 * - `=host`  — exact host only
 * - `*.host` — subdomains only
 * - `host`   — host + all subdomains (default)
 *
 * Returns the canonical lowercased entry, or `null` if invalid.
 */
export function validateDomain(input: string): string | null {
  let d = input.trim().toLowerCase().replace(/\.$/, "");
  if (!d) return null;

  let prefix = "";
  if (d.startsWith("=")) {
    prefix = "=";
    d = d.slice(1);
  } else if (d.startsWith("*.")) {
    prefix = "*.";
    d = d.slice(2);
  }

  if (!d || d.length > 253) return null;
  return HOST_RE.test(d) ? prefix + d : null;
}

/**
 * Normalize and validate a CIDR (or a bare IP, which is treated as a host route).
 * Returns the canonical `addr/prefix` string, or `null` if invalid.
 */
export function validateCidr(input: string): string | null {
  const c = input.trim().toLowerCase();
  if (!c) return null;

  // IPv4, with optional /prefix (a bare address becomes /32).
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:\/(\d{1,2}))?$/;
  const m4 = c.match(v4);
  if (m4) {
    const octets = m4.slice(1, 5).map(Number);
    if (octets.some((o) => o > 255)) return null;
    const prefix = m4[5] === undefined ? 32 : Number(m4[5]);
    if (prefix > 32) return null;
    return `${octets.join(".")}/${prefix}`;
  }

  // IPv6 (loose check — daemon does the strict parse), with optional /prefix.
  if (c.includes(":") && /^[0-9a-f:]+(?:\/\d{1,3})?$/.test(c)) {
    const slash = c.indexOf("/");
    if (slash === -1) return `${c}/128`;
    const prefix = Number(c.slice(slash + 1));
    return prefix <= 128 ? c : null;
  }

  return null;
}
