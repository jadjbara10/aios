//! crates/netstack/src/dns.rs
//! DNS — query construction, A-record response parsing, and a tiny cache.
//!
//! The resolver speaks one-shot UDP/53 to a configured upstream server. It
//! builds a minimal DNS message (header + single question), hands the wire
//! bytes to the caller for TX, and parses A-record answers back into the
//! cache. No DNSSEC, no recursion-control beyond the RD bit.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(missing_docs)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use crate::Ipv4Addr;

/// Cache entry: hostname, resolved IPv4, TTL in seconds (decremented by poll).
type CacheEntry = (String, Ipv4Addr, u32);

/// A minimal DNS resolver with an in-memory A-record cache.
pub struct DnsResolver {
    cache: Vec<CacheEntry>,
    next_id: u16,
}

impl DnsResolver {
    /// Construct an empty resolver.
    pub fn new() -> Self {
        Self { cache: Vec::new(), next_id: 0x1234 }
    }

    /// Resolve a hostname. Returns a cached entry if present and live,
    /// otherwise None and the caller must [DnsResolver::build_query],
    /// TX it over UDP/53, and feed the response back through
    /// [DnsResolver::ingest].
    pub fn resolve(&mut self, name: &str) -> Option<Ipv4Addr> {
        for (host, ip, ttl) in self.cache.iter_mut() {
            if host.as_str() == name {
                if *ttl == 0 {
                    return None;
                }
                *ttl = ttl.saturating_sub(1);
                return Some(*ip);
            }
        }
        None
    }

    /// Build a DNS query packet for name (A record, class IN, RD=1).
    /// Returns the on-wire bytes; the caller owns UDP framing + TX.
    pub fn build_query(&mut self, name: &str) -> Vec<u8> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let mut pkt = Vec::new();
        // Header: id, flags=0x0100 (RD=1), qd=1, an/ns/ar=0.
        pkt.extend_from_slice(&id.to_be_bytes());
        pkt.extend_from_slice(&0x0100u16.to_be_bytes());
        pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
        pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
        pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
        pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
        // Question: <length-prefixed labels> 0, type A (1), class IN (1).
        for label in name.split('.') {
            pkt.push(label.len() as u8);
            pkt.extend_from_slice(label.as_bytes());
        }
        pkt.push(0); // root terminator
        pkt.extend_from_slice(&1u16.to_be_bytes()); // QTYPE = A
        pkt.extend_from_slice(&1u16.to_be_bytes()); // QCLASS = IN
        pkt
    }

    /// Parse an A-record response and seed the cache for name. Returns
    /// the first A-record found, if any. Malformed responses are dropped.
    pub fn ingest(&mut self, name: &str, resp: &[u8]) -> Option<Ipv4Addr> {
        if resp.len() < 12 {
            return None;
        }
        let qd = u16::from_be_bytes([resp[4], resp[5]]) as usize;
        let an = u16::from_be_bytes([resp[6], resp[7]]) as usize;
        let mut off = 12usize;
        // Skip the question section.
        for _ in 0..qd {
            off = skip_name(resp, off)?;
            off = off.checked_add(4)?; // QTYPE + QCLASS
        }
        // Walk the answer RRs; pick the first A-record.
        for _ in 0..an {
            off = skip_name(resp, off)?;
            if off.checked_add(10)? > resp.len() {
                return None;
            }
            let rtype = u16::from_be_bytes([resp[off], resp[off + 1]]);
            let rdlen = u16::from_be_bytes([resp[off + 8], resp[off + 9]]) as usize;
            off = off.checked_add(10)?;
            if rtype == 1 && rdlen == 4 && off.checked_add(4)? <= resp.len() {
                let ip = Ipv4Addr::new([resp[off], resp[off + 1], resp[off + 2], resp[off + 3]]);
                self.cache.push((String::from(name), ip, 300));
                return Some(ip);
            }
            off = off.checked_add(rdlen)?;
        }
        None
    }
}

impl Default for DnsResolver {
    fn default() -> Self { Self::new() }
}

/// Skip a DNS name (handling compression pointers) starting at off.
fn skip_name(buf: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= buf.len() {
            return None;
        }
        let len = buf[off];
        if len == 0 {
            return off.checked_add(1);
        }
        if len & 0xC0 == 0xC0 {
            return off.checked_add(2);
        }
        off = off.checked_add(1 + len as usize)?;
    }
}
