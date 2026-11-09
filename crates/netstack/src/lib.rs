//! crates/netstack/src/lib.rs
//! AIOS native TCP/IP stack — no third-party networking.
//!
//! ETH/IP/ARP frames are framed, dispatched, and reassembled here over a HAL
//! network device. The stack is poll-driven: the kernel calls
//! [NetStack::poll] from its timer + IRQ bottom-half and the stack drains
//! the NIC RX ring and fires any pending timers. All packet buffers are owned
//! by the stack; there is no async runtime.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(missing_docs)]

extern crate alloc;

use alloc::boxed::Box;

pub mod tcp;
pub mod dns;
pub mod http;

/// Ethernet II frame type field for IPv4.
pub const ETH_TYPE_IPV4: u16 = 0x0800;
/// Ethernet II frame type field for ARP.
pub const ETH_TYPE_ARP: u16 = 0x0806;

/// A 6-byte EUI-48 MAC address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    /// Broadcast MAC (FF:FF:FF:FF:FF:FF).
    pub const BROADCAST: MacAddr = MacAddr::new([0xFF; 6]);
    /// Zero MAC (00:00:00:00:00:00).
    pub const ZERO: MacAddr = MacAddr::new([0x00; 6]);
    /// Construct from raw big-endian bytes.
    pub const fn new(bytes: [u8; 6]) -> Self { Self(bytes) }
    /// Read the raw bytes.
    pub const fn bytes(&self) -> [u8; 6] { self.0 }
}

/// A 4-octet IPv4 address in network byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Addr([u8; 4]);

impl Ipv4Addr {
    /// 127.0.0.1 loopback.
    pub const LOOPBACK: Ipv4Addr = Ipv4Addr::new([127, 0, 0, 1]);
    /// 0.0.0.0 unspecified.
    pub const UNSPECIFIED: Ipv4Addr = Ipv4Addr::new([0, 0, 0, 0]);
    /// Construct from raw big-endian bytes.
    pub const fn new(bytes: [u8; 4]) -> Self { Self(bytes) }
    /// Read the raw bytes.
    pub const fn bytes(&self) -> [u8; 4] { self.0 }
}

/// HAL-facing network device trait. Implemented by the NIC driver (e.g. an
/// Intel 82574L e1000 in crates/drivers/e1000).
pub trait HalDevice: Send {
    /// Read up to buf.len() bytes of one received frame; returns bytes read.
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize>;
    /// Transmit a fully-formed frame; returns bytes written.
    fn send(&mut self, buf: &[u8]) -> Option<usize>;
    /// The MAC programmed into the NIC.
    fn mac(&self) -> MacAddr;
    /// Poll the link state — true if carrier is up.
    fn link_up(&self) -> bool;
}

/// The top-level network interface. Owns the bound HAL device and dispatches
/// inbound frames to the ARP cache, IP demuxer, and the TCP socket table.
pub struct NetStack {
    iface: Box<dyn HalDevice>,
    mac: MacAddr,
    ip: Ipv4Addr,
}

impl NetStack {
    /// Construct the stack over a boxed HAL device. Defaults to an
    /// unspecified IPv4 address; call [NetStack::set_ipv4] after DHCP.
    pub fn init(nic: Box<dyn HalDevice>) -> Self {
        let mac = nic.mac();
        Self { iface: nic, mac, ip: Ipv4Addr::UNSPECIFIED }
    }

    /// Assign an IPv4 address (post-DHCP).
    pub fn set_ipv4(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }

    /// Drain the NIC RX ring and feed each frame into the ETH/IP/ARP
    /// dispatch path. Called by the kernel's timer tick + IRQ bottom-half.
    pub fn poll(&mut self) {
        let mut frame = [0u8; 1536];
        while let Some(n) = self.iface.recv(&mut frame) {
            // ETH/IP/ARP frame dispatch happens inline so the optimizer can
            // elide the slice copy on the happy path.
            self.dispatch_frame(&frame[..n]);
        }
    }

    /// Demultiplex a raw Ethernet II frame into ARP or IP handling.
    fn dispatch_frame(&mut self, frame: &[u8]) {
        if frame.len() < 14 {
            return;
        }
        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        match ethertype {
            ETH_TYPE_ARP => self.handle_arp(&frame[14..]),
            ETH_TYPE_IPV4 => self.handle_ipv4(&frame[14..]),
            _ => {}
        }
    }

    /// ARP responder: only answers ARP requests for our IPv4. The neighbour
    /// cache itself is Phase 5.1 (see AIOS-022).
    fn handle_arp(&mut self, _payload: &[u8]) {
        // TODO Phase 5.1: parse ARP request, build reply, update cache.
    }

    /// IPv4 demuxer: hands TCP segments to the socket table, drops the rest.
    fn handle_ipv4(&mut self, _payload: &[u8]) {
        // TODO Phase 5.1: validate header, dispatch proto 6 -> tcp::on_segment.
    }

    /// Borrow the underlying HAL device (used by TCP sockets for TX).
    pub fn device(&mut self) -> &mut dyn HalDevice {
        self.iface.as_mut()
    }
}
