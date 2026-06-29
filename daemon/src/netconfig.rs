//! System DNS management for the sinkhole (macOS, via `networksetup`).
//!
//! Captures each network service's current DNS, points it at `127.0.0.1`, and —
//! through [`DnsGuard`]'s `Drop` — restores the originals on **every** exit path
//! (clean, signal, or panic). Also captures the real upstream resolvers *before*
//! the switch so the sinkhole has somewhere to forward non-blocked queries.
//!
//! All of this needs root; the caller only invokes it when privileged.

use std::net::SocketAddr;

/// Capture the upstream resolvers to forward to, read from `/etc/resolv.conf`
/// before we change anything. Loopback entries are skipped (we must not forward
/// to ourselves); falls back to public resolvers if none are found.
pub fn capture_upstream() -> Vec<SocketAddr> {
    let mut servers = std::fs::read_to_string("/etc/resolv.conf")
        .map(|t| parse_resolv_conf(&t))
        .unwrap_or_default();
    if servers.is_empty() {
        tracing::warn!("no usable upstream in /etc/resolv.conf; falling back to 1.1.1.1 / 8.8.8.8");
        servers = vec![
            "1.1.1.1:53".parse().unwrap(),
            "8.8.8.8:53".parse().unwrap(),
        ];
    }
    servers
}

fn parse_resolv_conf(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("nameserver")?;
            let ip: std::net::IpAddr = rest.trim().parse().ok()?;
            (!ip.is_loopback()).then_some(SocketAddr::new(ip, 53))
        })
        .collect()
}

/// Restores each captured service's DNS when dropped.
pub struct DnsGuard {
    /// (service name, original DNS servers — empty means "was DHCP/unset").
    saved: Vec<(String, Vec<String>)>,
}

/// Point every active network service's DNS at `127.0.0.1`, remembering the
/// previous values so [`DnsGuard`] can restore them. Requires root.
pub fn capture_and_redirect() -> DnsGuard {
    let mut saved = Vec::new();
    for service in network_services() {
        let current = get_dns(&service);
        set_dns(&service, &["127.0.0.1".to_string()]);
        tracing::info!(service = %service, previous = ?current, "redirected DNS to 127.0.0.1");
        saved.push((service, current));
    }
    flush_dns_cache();
    DnsGuard { saved }
}

impl Drop for DnsGuard {
    fn drop(&mut self) {
        for (service, servers) in &self.saved {
            set_dns(service, servers);
            tracing::info!(service = %service, restored = ?servers, "restored DNS");
        }
        flush_dns_cache();
    }
}

fn networksetup(args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new("networksetup").args(args).output()
}

/// Active (non-disabled) network services. The first output line is a header;
/// disabled services are prefixed with `*`.
fn network_services() -> Vec<String> {
    let Ok(out) = networksetup(&["-listallnetworkservices"]) else {
        tracing::warn!("could not list network services");
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('*'))
        .map(str::to_string)
        .collect()
}

/// Current DNS servers for a service. Empty means none are set (DHCP-provided).
fn get_dns(service: &str) -> Vec<String> {
    let Ok(out) = networksetup(&["-getdnsservers", service]) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("aren't any DNS Servers") {
        return Vec::new();
    }
    text.lines()
        .map(str::trim)
        .filter(|l| l.parse::<std::net::IpAddr>().is_ok())
        .map(str::to_string)
        .collect()
}

/// Set a service's DNS. Empty restores DHCP behavior (`networksetup ... Empty`).
fn set_dns(service: &str, servers: &[String]) {
    let mut args: Vec<&str> = vec!["-setdnsservers", service];
    if servers.is_empty() {
        args.push("Empty");
    } else {
        args.extend(servers.iter().map(String::as_str));
    }
    if let Err(e) = networksetup(&args) {
        tracing::warn!(error = %e, service, "failed to set DNS servers");
    }
}

fn flush_dns_cache() {
    let _ = std::process::Command::new("dscacheutil").arg("-flushcache").status();
    let _ = std::process::Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resolv_conf_skipping_loopback() {
        let text = "\
# comment
nameserver 192.168.1.1
nameserver 8.8.8.8
nameserver 127.0.0.1
search lan
";
        let got = parse_resolv_conf(text);
        assert_eq!(
            got,
            vec![
                "192.168.1.1:53".parse().unwrap(),
                "8.8.8.8:53".parse().unwrap(),
            ]
        );
    }

    #[test]
    fn capture_upstream_always_has_a_fallback() {
        // Whatever the host's resolv.conf says, we never hand back an empty list.
        assert!(!capture_upstream().is_empty());
    }
}
