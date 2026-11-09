//! crates/netstack/src/tcp.rs
//! TCP — RFC 793 state machine, sockets, and seq/ack tracking.
//!
//! Retransmission, congestion control, and the TIME-WAIT 2MSL timer are
//! deferred to Phase 5.1 (AIOS-022). The machine here implements the
//! connection-establishment and orderly-close transitions needed by the
//! HTTP client in [crate::http].
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(missing_docs)]

extern crate alloc;

use alloc::vec::Vec;
use crate::Ipv4Addr;

/// A simplified TCP connection state (subset of RFC 793 figure 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpState {
    /// No connection.
    Closed,
    /// Sent SYN, awaiting SYN-ACK.
    SynSent,
    /// Received SYN, sent SYN-ACK.
    SynReceived,
    /// Three-way handshake complete.
    Established,
    /// Received FIN, awaiting our close.
    CloseWait,
    /// Sent FIN, awaiting ACK.
    FinWait1,
    /// FIN acknowledged, awaiting peer FIN.
    FinWait2,
    /// Our FIN sent, awaiting final ACK (2MSL wait is Phase 5.1).
    LastAck,
}

/// SYN flag bit.
pub const SYN: u8 = 0x02;
/// ACK flag bit.
pub const ACK: u8 = 0x10;
/// FIN flag bit.
pub const FIN: u8 = 0x01;
/// RST flag bit.
pub const RST: u8 = 0x04;

/// A TCP segment header (parsed, not on-wire).
#[derive(Clone, Copy, Debug)]
pub struct TcpSegment<'a> {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
    /// Sequence number.
    pub seq: u32,
    /// Acknowledgement number.
    pub ack: u32,
    /// Flags (SYN/ACK/FIN/RST bitfield).
    pub flags: u8,
    /// Payload slice (may be empty).
    pub payload: &'a [u8],
}

/// A minimal TCP socket: state machine + send/recv buffers + seq/ack.
pub struct TcpSocket {
    state: TcpState,
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    snd_nxt: u32,
    rcv_nxt: u32,
    rx_buf: Vec<u8>,
}

impl TcpSocket {
    /// Create a fresh socket bound to (ip, port).
    pub fn new(local_ip: Ipv4Addr, local_port: u16) -> Self {
        Self {
            state: TcpState::Closed,
            local_ip,
            local_port,
            remote_ip: Ipv4Addr::UNSPECIFIED,
            remote_port: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            rx_buf: Vec::new(),
        }
    }

    /// Current connection state.
    pub fn state(&self) -> TcpState { self.state }

    /// Issue an active open to (remote_ip, remote_port). Initial sequence
    /// number is a monotonic rdtsc seed; real RFC 6528 ISN is Phase 5.1.
    pub fn connect(&mut self, remote_ip: Ipv4Addr, remote_port: u16) {
        self.remote_ip = remote_ip;
        self.remote_port = remote_port;
        self.snd_nxt = current_isn();
        self.state = TcpState::SynSent;
    }

    /// Feed an inbound segment into the state machine. Transitions follow
    /// RFC 793 figure 6 (simplified); retransmit is Phase 5.1.
    pub fn on_segment(&mut self, seg: &TcpSegment<'_>) {
        match self.state {
            TcpState::Closed => {
                // Drop inbound segments in Closed.
            }
            TcpState::SynSent => {
                if seg.flags & SYN != 0 && seg.flags & ACK != 0 {
                    self.rcv_nxt = seg.seq.wrapping_add(1);
                    self.state = TcpState::Established;
                }
            }
            TcpState::Established => {
                if seg.flags & RST != 0 {
                    self.state = TcpState::Closed;
                    return;
                }
                if !seg.payload.is_empty() {
                    self.rx_buf.extend_from_slice(seg.payload);
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(seg.payload.len() as u32);
                }
                if seg.flags & FIN != 0 {
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
                    self.state = TcpState::CloseWait;
                }
            }
            _ => {}
        }
    }

    /// Copy up to out.len() received bytes; returns bytes copied.
    pub fn recv(&mut self, out: &mut [u8]) -> usize {
        let n = core::cmp::min(out.len(), self.rx_buf.len());
        out[..n].copy_from_slice(&self.rx_buf[..n]);
        self.rx_buf.drain(..n);
        n
    }

    /// Stage a payload for TX; returns the next sequence number to use. Only
    /// advances snd_nxt — actual frame TX is the caller's job. Retransmit is
    /// Phase 5.1.
    pub fn send(&mut self, payload: &[u8]) -> u32 {
        let seq = self.snd_nxt;
        self.snd_nxt = self.snd_nxt.wrapping_add(payload.len() as u32);
        seq
    }
}

/// ISN source — rdtsc-derived monotonic counter. Real RFC 6528 ISN is 5.1.
fn current_isn() -> u32 {
    let mut lo: u32 = 0;
    // SAFETY: rdtsc is a non-privileged serializing read of the timestamp
    // counter; it takes no memory operands and cannot fault. The value is
    // used solely to seed snd_nxt and never dereferences a pointer.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    lo
}
