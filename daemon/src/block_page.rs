//! Loopback HTTP server that serves the "site blocked" page.
//!
//! The DNS sinkhole points blocked `A`/`AAAA` lookups at loopback (see
//! [`crate::dns`]); this server answers the HTTP request the browser then makes,
//! so the user sees a friendly explanation instead of a connection error.
//!
//! It is deliberately tiny and dependency-free: read the request, pull the
//! `Host` header for display, and write one self-contained HTML response. Like
//! the DNS sinkhole it binds a privileged port (`:80`), so it only runs as root.
//!
//! Note: this can only present a page for plain **HTTP**. HTTPS sites terminate
//! TLS before sending `Host`, and we hold no certificate for them, so those
//! lookups simply fail to connect — the standard limitation of DNS-based blocking.

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Bind the block-page listener on `addr` (needs root for `:80`).
pub async fn bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "block page server bound");
    Ok(listener)
}

/// Accept and serve the block page forever.
pub async fn serve(listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(handle(stream));
            }
            Err(e) => tracing::warn!(error = %e, "block page accept failed"),
        }
    }
}

async fn handle(mut stream: TcpStream) {
    // Read just the request head; we only need the Host line, and the body (if
    // any) is irrelevant. A single read is plenty for a GET's headers.
    let mut buf = [0u8; 2048];
    let n = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(_) => return,
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    let host = parse_host(&head).unwrap_or("this site");

    let body = render_page(host);
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Extract the `Host` header value from a raw request head, if present.
fn parse_host(head: &str) -> Option<&str> {
    head.lines()
        .find_map(|line| line.strip_prefix("Host:").or_else(|| line.strip_prefix("host:")))
        .map(str::trim)
        .filter(|h| !h.is_empty())
}

/// Minimal HTML-escaping for the one piece of attacker-influenced text (the Host).
fn escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            other => vec![other],
        })
        .collect()
}

/// The block page HTML. Self-contained (inline CSS + SVG) so it renders with no
/// further requests — which would themselves be blocked.
fn render_page(host: &str) -> String {
    let host = escape(host);
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Blocked &middot; {host}</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; min-height: 100vh; display: grid; place-items: center;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif;
    color: #e6e6e8;
    background: radial-gradient(1200px 600px at 50% -10%, #1b2440 0%, #0e0e10 60%);
  }}
  .card {{
    width: min(92vw, 460px); text-align: center; padding: 2.4rem 2rem;
    background: #161823; border: 1px solid #262a3a; border-radius: 18px;
    box-shadow: 0 24px 60px rgba(0,0,0,.45);
  }}
  .logo {{ width: 76px; height: 76px; margin: 0 auto 1.2rem; display: block; }}
  h1 {{ font-size: 1.5rem; margin: 0 0 .5rem; }}
  p {{ color: #aab0c0; line-height: 1.5; margin: .4rem 0; }}
  .host {{
    display: inline-block; margin-top: 1rem; padding: .5rem .9rem;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .95rem;
    color: #cfe0ff; background: #1b2440; border: 1px solid #2c3a64; border-radius: 10px;
  }}
  .foot {{ margin-top: 1.6rem; font-size: .8rem; color: #6f7488; }}
</style>
</head>
<body>
  <main class="card">
    <svg class="logo" viewBox="0 0 512 512" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <defs>
        <linearGradient id="g" x1="0" y1="0" x2="512" y2="512" gradientUnits="userSpaceOnUse">
          <stop stop-color="#3b82f6"/><stop offset="1" stop-color="#6366f1"/>
        </linearGradient>
      </defs>
      <rect width="512" height="512" rx="114" fill="url(#g)"/>
      <path d="M256 92 L398 142 V268 C398 350 332 404 256 432 C180 404 114 350 114 268 V142 Z" fill="#eef3ff"/>
      <g stroke="#3b5bdb" stroke-width="26" stroke-linecap="round" fill="none">
        <circle cx="256" cy="262" r="74"/><line x1="206" y1="212" x2="306" y2="312"/>
      </g>
    </svg>
    <h1>Site blocked</h1>
    <p>Access to this site is blocked by <strong>Blocker</strong> while a focus session is active.</p>
    <span class="host">{host}</span>
    <p class="foot">Manage your blocklist and sessions in the Blocker app.</p>
  </main>
</body>
</html>
"##,
        host = host
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_header_case_insensitively() {
        let req = "GET / HTTP/1.1\r\nHost: ads.example.com\r\nAccept: */*\r\n\r\n";
        assert_eq!(parse_host(req), Some("ads.example.com"));
        let lower = "get / http/1.1\r\nhost: lower.example.com\r\n\r\n";
        assert_eq!(parse_host(lower), Some("lower.example.com"));
        assert_eq!(parse_host("GET / HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn escapes_host_in_page() {
        let page = render_page("<script>x</script>");
        assert!(!page.contains("<script>x"));
        assert!(page.contains("&lt;script&gt;"));
    }
}
