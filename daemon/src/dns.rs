//! DNS sinkhole: a tiny resolver on `127.0.0.1:53` that intercepts blocked names
//! and **relays everything else** to an upstream resolver.
//!
//! A blocked name is answered one of two ways:
//! - if the loopback **block page** server is up, `A`/`AAAA` queries resolve to
//!   loopback (`127.0.0.1` / `::1`) so the browser lands on the block page;
//! - otherwise (or for other record types) it gets **NXDOMAIN**.
//!
//! The DNS message handling is hand-rolled rather than pulling a full DNS crate:
//! we only need the question name + type to synthesize a small answer, and
//! non-blocked queries are forwarded as opaque bytes, so every record type and
//! flag passes through untouched. The blocklist is read live from
//! [`StateManager`], so GUI edits take effect on the next query.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use crate::state::StateManager;

/// Upstream query timeout. Kept short so a dead resolver fails fast.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(3);

/// DNS record types we synthesize answers for.
const QTYPE_A: u16 = 1;
const QTYPE_AAAA: u16 = 28;

/// TTL for synthesized block answers. Short so un-blocking takes effect quickly.
const BLOCK_TTL_SECS: u32 = 30;

/// Bound UDP + TCP listeners on `:53`, ready to serve. Splitting bind from serve
/// lets the caller confirm `:53` is ours *before* repointing system DNS at it.
pub struct Listeners {
    udp: Arc<UdpSocket>,
    tcp: TcpListener,
}

/// Bind UDP + TCP on `addr` (needs root for `:53`).
pub async fn bind(addr: SocketAddr) -> std::io::Result<Listeners> {
    let udp = Arc::new(UdpSocket::bind(addr).await?);
    let tcp = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "DNS sinkhole bound");
    Ok(Listeners { udp, tcp })
}

impl Listeners {
    /// Serve forever. `upstream` is the list of real resolvers (captured before we
    /// hijacked system DNS) to forward non-blocked queries to. When `block_page`
    /// is set, blocked `A`/`AAAA` queries resolve to loopback instead of NXDOMAIN.
    pub async fn serve(self, state: Arc<StateManager>, upstream: Vec<SocketAddr>, block_page: bool) {
        tracing::info!(upstream = ?upstream, block_page, "DNS sinkhole serving");
        tokio::join!(
            serve_udp(self.udp, Arc::clone(&state), upstream.clone(), block_page),
            serve_tcp(self.tcp, state, upstream, block_page),
        );
    }
}

async fn serve_udp(
    sock: Arc<UdpSocket>,
    state: Arc<StateManager>,
    upstream: Vec<SocketAddr>,
    block_page: bool,
) {
    let mut buf = vec![0u8; 4096];
    loop {
        let (len, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "udp recv failed");
                continue;
            }
        };
        let req = buf[..len].to_vec();
        let sock = Arc::clone(&sock);
        let state = Arc::clone(&state);
        let upstream = upstream.clone();
        tokio::spawn(async move {
            let resp = match decide(&req, &state, block_page) {
                Some(nx) => Some(nx),
                None => forward_udp(&req, &upstream).await,
            };
            if let Some(resp) = resp {
                let _ = sock.send_to(&resp, peer).await;
            }
        });
    }
}

async fn serve_tcp(
    listener: TcpListener,
    state: Arc<StateManager>,
    upstream: Vec<SocketAddr>,
    block_page: bool,
) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "tcp accept failed");
                continue;
            }
        };
        let state = Arc::clone(&state);
        let upstream = upstream.clone();
        tokio::spawn(handle_tcp(stream, state, upstream, block_page));
    }
}

async fn handle_tcp(
    mut stream: TcpStream,
    state: Arc<StateManager>,
    upstream: Vec<SocketAddr>,
    block_page: bool,
) {
    loop {
        // DNS over TCP frames each message with a 2-byte big-endian length.
        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            return; // clean EOF or error: done with this connection
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut req = vec![0u8; len];
        if stream.read_exact(&mut req).await.is_err() {
            return;
        }

        let resp = match decide(&req, &state, block_page) {
            Some(nx) => Some(nx),
            None => forward_tcp(&req, &upstream).await,
        };
        let Some(resp) = resp else { continue };

        let out_len = (resp.len() as u16).to_be_bytes();
        if stream.write_all(&out_len).await.is_err()
            || stream.write_all(&resp).await.is_err()
            || stream.flush().await.is_err()
        {
            return;
        }
    }
}

/// Decide what to do with a request: `Some(bytes)` to answer it ourselves
/// (block page redirect or NXDOMAIN), or `None` to forward upstream. Unparseable
/// requests forward (fail open). Blocked queries are recorded for statistics.
fn decide(req: &[u8], state: &StateManager, block_page: bool) -> Option<Vec<u8>> {
    let (name, qend, qtype) = parse_question(req)?;
    state.on_query(&name)?; // None (not blocked) -> forward upstream
    if block_page && (qtype == QTYPE_A || qtype == QTYPE_AAAA) {
        tracing::info!(query = %name, "sinkholed → block page (loopback)");
        Some(build_address_answer(req, qend, qtype))
    } else {
        tracing::info!(query = %name, "sinkholed → NXDOMAIN");
        Some(build_nxdomain(req, qend))
    }
}

/// Parse the first question's name, the byte offset just past the question
/// section, and the QTYPE. The question name is never compressed, so this is a
/// straight label walk. Returns `None` on a malformed or question-less message.
fn parse_question(msg: &[u8]) -> Option<(String, usize, u16)> {
    if msg.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]);
    if qdcount < 1 {
        return None;
    }

    let mut pos = 12;
    let mut labels: Vec<String> = Vec::new();
    loop {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xC0 != 0 {
            return None; // compression pointer not expected in a question
        }
        pos += 1;
        let label = msg.get(pos..pos + len)?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        pos += len;
    }
    // QTYPE (2) + QCLASS (2)
    let qtype_bytes = msg.get(pos..pos + 2)?;
    let qtype = u16::from_be_bytes([qtype_bytes[0], qtype_bytes[1]]);
    pos += 4;
    if pos > msg.len() {
        return None;
    }
    Some((labels.join(".").to_ascii_lowercase(), pos, qtype))
}

/// Build an answer that points a blocked name at loopback so the browser reaches
/// the local block page: the request header + question, an `ANCOUNT` of 1, and a
/// single `A`/`AAAA` record (compressed name pointer to the question) with RDATA
/// `127.0.0.1` / `::1`.
fn build_address_answer(req: &[u8], qend: usize, qtype: u16) -> Vec<u8> {
    let mut resp = req[..qend].to_vec();
    // byte 2: set QR, keep Opcode + RD, clear AA/TC.
    let opcode = resp[2] & 0x78;
    let rd = resp[2] & 0x01;
    resp[2] = 0x80 | opcode | rd;
    // byte 3: RA=1, RCODE=0 (NoError).
    resp[3] = 0x80;
    // QDCOUNT (4..6) stays 1; ANCOUNT (6..8) = 1; NS/AR (8..12) = 0.
    resp[6] = 0x00;
    resp[7] = 0x01;
    for b in &mut resp[8..12] {
        *b = 0;
    }
    // Answer RR: NAME = pointer to the question at offset 12.
    resp.extend_from_slice(&[0xC0, 0x0C]);
    resp.extend_from_slice(&qtype.to_be_bytes()); // TYPE
    resp.extend_from_slice(&[0x00, 0x01]); // CLASS = IN
    resp.extend_from_slice(&BLOCK_TTL_SECS.to_be_bytes()); // TTL
    if qtype == QTYPE_AAAA {
        resp.extend_from_slice(&[0x00, 0x10]); // RDLENGTH = 16
        resp.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
    } else {
        resp.extend_from_slice(&[0x00, 0x04]); // RDLENGTH = 4
        resp.extend_from_slice(&[127, 0, 0, 1]);
    }
    resp
}

/// Build an NXDOMAIN response: the request's header + question, with QR=1, RA=1,
/// RCODE=NXDomain(3), and the answer/authority/additional counts zeroed.
fn build_nxdomain(req: &[u8], qend: usize) -> Vec<u8> {
    let mut resp = req[..qend].to_vec();
    // byte 2: QR | Opcode(4) | AA | TC | RD — set QR, keep Opcode + RD, clear AA/TC.
    let opcode = resp[2] & 0x78;
    let rd = resp[2] & 0x01;
    resp[2] = 0x80 | opcode | rd;
    // byte 3: RA | Z(3) | RCODE(4) — RA=1, RCODE=3.
    resp[3] = 0x80 | 0x03;
    // QDCOUNT (4..6) stays 1; zero AN/NS/AR (6..12).
    for b in &mut resp[6..12] {
        *b = 0;
    }
    resp
}

async fn forward_udp(req: &[u8], upstream: &[SocketAddr]) -> Option<Vec<u8>> {
    for up in upstream {
        let bind = if up.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let Ok(sock) = UdpSocket::bind(bind).await else { continue };
        if sock.send_to(req, up).await.is_err() {
            continue;
        }
        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(UPSTREAM_TIMEOUT, sock.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => return Some(buf[..len].to_vec()),
            _ => continue,
        }
    }
    tracing::warn!("no upstream answered (udp)");
    None
}

async fn forward_tcp(req: &[u8], upstream: &[SocketAddr]) -> Option<Vec<u8>> {
    for up in upstream {
        let connect = tokio::time::timeout(UPSTREAM_TIMEOUT, TcpStream::connect(up)).await;
        let mut stream = match connect {
            Ok(Ok(s)) => s,
            _ => continue,
        };
        let len = (req.len() as u16).to_be_bytes();
        if stream.write_all(&len).await.is_err()
            || stream.write_all(req).await.is_err()
            || stream.flush().await.is_err()
        {
            continue;
        }
        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            continue;
        }
        let rlen = u16::from_be_bytes(len_buf) as usize;
        let mut resp = vec![0u8; rlen];
        if stream.read_exact(&mut resp).await.is_err() {
            continue;
        }
        return Some(resp);
    }
    tracing::warn!("no upstream answered (tcp)");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DNS query for `tauri.app` IN A, RD set.
    fn tauri_query() -> Vec<u8> {
        let mut m = vec![
            0x12, 0x34, // id
            0x01, 0x00, // flags: RD
            0x00, 0x01, // qdcount
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // an/ns/ar
        ];
        m.push(5);
        m.extend_from_slice(b"tauri");
        m.push(3);
        m.extend_from_slice(b"app");
        m.push(0);
        m.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // qtype A, qclass IN
        m
    }

    #[test]
    fn parses_question_name_offset_and_type() {
        let q = tauri_query();
        let (name, qend, qtype) = parse_question(&q).unwrap();
        assert_eq!(name, "tauri.app");
        assert_eq!(qend, q.len()); // question is the whole message here
        assert_eq!(qtype, QTYPE_A);
    }

    #[test]
    fn parse_rejects_truncated() {
        assert!(parse_question(&[0u8; 5]).is_none());
    }

    #[test]
    fn builds_nxdomain_flags_and_counts() {
        let q = tauri_query();
        let (_, qend, _) = parse_question(&q).unwrap();
        let r = build_nxdomain(&q, qend);
        assert_eq!(r[0..2], q[0..2]); // id preserved
        assert_ne!(r[2] & 0x80, 0); // QR set
        assert_eq!(r[2] & 0x01, 0x01); // RD preserved
        assert_eq!(r[3] & 0x0F, 0x03); // RCODE = NXDOMAIN
        assert_eq!(u16::from_be_bytes([r[4], r[5]]), 1); // qdcount stays 1
        assert_eq!(&r[6..12], &[0u8; 6]); // an/ns/ar zeroed
    }

    #[test]
    fn builds_a_answer_pointing_at_loopback() {
        let q = tauri_query();
        let (_, qend, qtype) = parse_question(&q).unwrap();
        let r = build_address_answer(&q, qend, qtype);
        assert_eq!(r[0..2], q[0..2]); // id preserved
        assert_ne!(r[2] & 0x80, 0); // QR set
        assert_eq!(r[3] & 0x0F, 0x00); // RCODE = NoError
        assert_eq!(u16::from_be_bytes([r[4], r[5]]), 1); // qdcount
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1); // ancount
        // The answer's final 4 RDATA bytes are 127.0.0.1.
        assert_eq!(&r[r.len() - 4..], &[127, 0, 0, 1]);
        // Name is the compression pointer to the question.
        assert_eq!(&r[qend..qend + 2], &[0xC0, 0x0C]);
    }

    #[tokio::test]
    async fn forward_udp_relays_via_upstream() {
        // Mock upstream that replies with a canned payload to whoever asks.
        let up = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let up_addr = up.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            if let Ok((_, peer)) = up.recv_from(&mut buf).await {
                let _ = up.send_to(b"UPSTREAM-REPLY", peer).await;
            }
        });

        let resp = forward_udp(b"QUERY", &[up_addr]).await;
        assert_eq!(resp.as_deref(), Some(&b"UPSTREAM-REPLY"[..]));
    }

    #[tokio::test]
    async fn forward_udp_times_out_on_dead_upstream() {
        // Reserve a local UDP port and immediately drop it; nothing will answer.
        let dead = {
            let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            s.local_addr().unwrap()
        };
        // Use a tiny timeout indirectly: just assert it returns None (None means
        // no answer). This relies on UPSTREAM_TIMEOUT; keep the test fast by not
        // waiting the full 3s in CI is acceptable since there is one upstream.
        let resp = tokio::time::timeout(Duration::from_secs(5), forward_udp(b"Q", &[dead])).await;
        assert_eq!(resp.unwrap(), None);
    }
}
