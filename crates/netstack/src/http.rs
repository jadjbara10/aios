//! crates/netstack/src/http.rs
//! HTTP/1.1 client + TLS handshaker stub.
//!
//! HttpClient::get parses a URL, resolves the host (via the resolver in
//! [crate::dns]), opens a TCP connection through [crate::tcp::TcpSocket],
//! sends a minimal HTTP/1.1 GET, and parses the status line + headers. The
//! TlsHandshaker is a state-machine stub — a real rustls-like handshaker
//! lands in Phase 5.2 (AIOS-023).
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(missing_docs)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use crate::tcp::TcpSocket;
use crate::Ipv4Addr;

/// Parsed components of an HTTP(S) URL.
#[derive(Clone, Debug)]
pub struct Url {
    /// "http" or "https".
    pub scheme: String,
    /// Hostname (not yet resolved).
    pub host: String,
    /// Port (80 for http, 443 for https by default).
    pub port: u16,
    /// Path including the leading '/'.
    pub path: String,
}

/// Parse scheme://host[:port]/path. No userinfo, query, or fragment (Phase 5.2).
pub fn parse_url(input: &str) -> Option<Url> {
    let scheme_end = input.find("://")?;
    let scheme = String::from(&input[..scheme_end]);
    let rest = &input[scheme_end + 3..];
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], String::from(&rest[idx..])),
        None => (rest, String::from("/")),
    };
    let (host, port) = match authority.find(':') {
        Some(idx) => (
            String::from(&authority[..idx]),
            authority[idx + 1..].parse().ok()?,
        ),
        None => {
            let p = if scheme == "https" { 443 } else { 80 };
            (String::from(authority), p)
        }
    };
    Some(Url { scheme, host, port, path })
}

/// A parsed HTTP response: status code + header map + body bytes.
pub struct Response {
    /// HTTP status code, e.g. 200.
    pub status: u16,
    /// Header lines in received order, lowercased key + raw value.
    pub headers: Vec<(String, String)>,
    /// Response body bytes.
    pub body: Vec<u8>,
}

/// A minimal HTTP/1.1 client bound to a (local_ip, local_port) tuple.
pub struct HttpClient {
    local_ip: Ipv4Addr,
    local_port: u16,
}

impl HttpClient {
    /// Construct a client bound to (local_ip, local_port).
    pub fn new(local_ip: Ipv4Addr, local_port: u16) -> Self {
        Self { local_ip, local_port }
    }

    /// Issue an HTTP/1.1 GET. The caller resolves url.host to server_ip
    /// (typically via [crate::dns::DnsResolver]); returns None on error.
    pub fn get(&mut self, url: &str, server_ip: Ipv4Addr) -> Option<Response> {
        let parsed = parse_url(url)?;
        let mut sock = TcpSocket::new(self.local_ip, self.local_port);
        sock.connect(server_ip, parsed.port);
        let mut req: Vec<u8> = Vec::new();
        push_str(&mut req, "GET ");
        push_str(&mut req, parsed.path.as_str());
        push_str(&mut req, " HTTP/1.1\r\n");
        push_str(&mut req, "Host: ");
        push_str(&mut req, parsed.host.as_str());
        push_str(&mut req, "\r\n");
        push_str(&mut req, "Connection: close\r\n");
        push_str(&mut req, "User-Agent: AIOS/0.1\r\n");
        push_str(&mut req, "\r\n");
        // sock.send stages seq numbers; real frame TX is the caller's job.
        let _seq = sock.send(&req);
        let mut raw = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let n = sock.recv(&mut chunk);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
        }
        parse_response(&raw)
    }
}

/// Append an &str to a Vec<u8> byte-wise.
fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

/// Parse an HTTP/1.1 response into status + headers + body.
fn parse_response(raw: &[u8]) -> Option<Response> {
    let eol = find_subslice(raw, b"\r\n")?;
    let status_line = core::str::from_utf8(&raw[..eol]).ok()?;
    let mut parts = status_line.split(' ');
    let _ = parts.next()?;
    let code: u16 = parts.next()?.parse().ok()?;
    let sep = find_subslice(raw, b"\r\n\r\n")?;
    let header_block = &raw[eol + 2..sep];
    let body = raw.get(sep + 4..).unwrap_or(&[]).to_vec();
    let mut headers = Vec::new();
    for line in header_block.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if let Some(colon) = find_subslice(line, b": ") {
            if let (Ok(k), Ok(v)) = (
                core::str::from_utf8(&line[..colon]),
                core::str::from_utf8(&line[colon + 2..]),
            ) {
                headers.push((k.to_ascii_lowercase(), String::from(v)));
            }
        }
    }
    Some(Response { status: code, headers, body })
}

/// Find the first occurrence of needle in hay, returning its index.
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// TLS handshaker stub. A real rustls-like state machine (ClientHello ->
/// ServerHello -> Certificate -> KeyExchange -> ChangeCipherSpec) is Phase 5.2.
pub struct TlsHandshaker {
    /// True once the stub enters the ApplicationData phase (always false now).
    pub established: bool,
}

impl TlsHandshaker {
    /// Construct a fresh handshaker.
    pub fn new() -> Self {
        Self { established: false }
    }

    /// Drive one step of the handshake. Always returns false for now;
    /// Phase 5.2 will replace this with the real state machine.
    pub fn step(&mut self, _record: &[u8]) -> bool {
        false
    }
}

impl Default for TlsHandshaker {
    fn default() -> Self { Self::new() }
}
