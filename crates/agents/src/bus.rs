// crates/agents/src/bus.rs
//! AgentMessageBus — pub/sub + direct mailboxes over kernel IPC channels.
//!
//! Every agent owns a bounded mailbox backed by a sched::ipc MPSC channel.
//! Two delivery modes: direct (msg.to routes to that agent's mailbox) and
//! pub/sub (agents subscribe to a [Topic]; broadcast fans out to all
//! subscribers). CapRequest payloads are dropped when the recipient lacks
//! the named capability — enforced in [AgentMessageBus::publish].
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::{AgentId, Capability, Message, Payload};

/// Maximum mailbox depth before backpressure kicks in. Mailboxes are
/// bounded; overflows cause publish to return false so the caller
/// can apply a drop-oldest or block policy.
pub const MAILBOX_DEPTH: usize = 32;

/// A 16-byte topic tag. Agents subscribe by tag; the bus fans out by tag.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Topic(pub [u8; 16]);

impl Topic {
    /// Construct a topic from a static ASCII label (truncated and
    /// zero-padded to 16 bytes).
    pub fn from_label(label: &str) -> Self {
        let mut buf = [0u8; 16];
        let n = label.len().min(16);
        buf[..n].copy_from_slice(&label.as_bytes()[..n]);
        Topic(buf)
    }
}

/// One slot in a bounded mailbox.
#[derive(Clone)]
pub struct MailboxEntry {
    /// The message itself.
    pub msg: Message,
}

/// Bounded MPSC mailbox backed by sched::ipc. The kernel owns the
/// actual channel buffer; this is the agent-side handle.
pub struct Mailbox {
    /// Inline ring buffer of MAILBOX_DEPTH entries.
    ring: Vec<Option<MailboxEntry>>,
    head: usize,
    tail: usize,
    len: usize,
}

impl Mailbox {
    /// Construct an empty mailbox pre-allocated to MAILBOX_DEPTH slots.
    pub fn new() -> Self {
        let mut ring: Vec<Option<MailboxEntry>> = Vec::with_capacity(MAILBOX_DEPTH);
        let mut i = 0;
        while i < MAILBOX_DEPTH {
            ring.push(None);
            i += 1;
        }
        Self { ring, head: 0, tail: 0, len: 0 }
    }

    /// Push a message into the mailbox. Returns false if the mailbox is
    /// full and the message could not be enqueued (backpressure signal).
    pub fn push(&mut self, entry: MailboxEntry) -> bool {
        if self.len == MAILBOX_DEPTH {
            return false;
        }
        self.ring[self.tail] = Some(entry);
        self.tail = (self.tail + 1) % MAILBOX_DEPTH;
        self.len += 1;
        true
    }

    /// Pop the oldest message from the mailbox.
    pub fn pop(&mut self) -> Option<MailboxEntry> {
        if self.len == 0 {
            return None;
        }
        let entry = self.ring[self.head].take();
        self.head = (self.head + 1) % MAILBOX_DEPTH;
        self.len -= 1;
        entry
    }

    /// Current depth.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if the mailbox is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// The AgentMessageBus itself. Owns one [Mailbox] per agent and a
/// Topic -> Vec<AgentId> subscription table.
pub struct AgentMessageBus {
    mailboxes: BTreeMap<AgentId, Mailbox>,
    subs: BTreeMap<Topic, Vec<AgentId>>,
    /// Capability table: agent -> caps it currently holds. Populated at
    /// register time; the bus consults it before delivering CapRequest.
    caps: BTreeMap<AgentId, &'static [Capability]>,
}

impl AgentMessageBus {
    /// Construct an empty bus.
    pub fn new() -> Self {
        Self {
            mailboxes: BTreeMap::new(),
            subs: BTreeMap::new(),
            caps: BTreeMap::new(),
        }
    }

    /// Register a new agent's mailbox along with its capability set.
    pub fn register(&mut self, id: AgentId, caps: &'static [Capability]) {
        self.mailboxes.entry(id).or_insert_with(Mailbox::new);
        self.caps.insert(id, caps);
    }

    /// Subscribe an agent to a topic.
    pub fn subscribe(&mut self, topic: Topic, agent: AgentId) {
        self.subs.entry(topic).or_default().push(agent);
    }

    /// Publish a message to its direct recipient. Returns true on
    /// successful enqueue, false on backpressure or unknown recipient.
    /// CapRequest payloads are silently dropped when the recipient does
    /// not hold the named capability.
    pub fn publish(&mut self, msg: Message) -> bool {
        if let Payload::CapRequest(cap) = &msg.payload {
            let holds = self.caps.get(&msg.to).copied().unwrap_or(&[]);
            if !holds.iter().any(|c| c == cap) {
                return false; // capability mismatch — drop silently
            }
        }
        if let Some(mb) = self.mailboxes.get_mut(&msg.to) {
            return mb.push(MailboxEntry { msg });
        }
        false
    }

    /// Broadcast a message to all subscribers of a topic. Returns the
    /// number of mailboxes the message was delivered to.
    pub fn broadcast(&mut self, topic: Topic, mut msg: Message) -> usize {
        let mut delivered = 0;
        if let Some(subs) = self.subs.get(&topic).cloned() {
            for agent in subs {
                msg.to = agent;
                if let Some(mb) = self.mailboxes.get_mut(&agent) {
                    if mb.push(MailboxEntry { msg: msg.clone() }) {
                        delivered += 1;
                    }
                }
            }
        }
        delivered
    }

    /// Drain the next message from an agent's mailbox.
    pub fn recv(&mut self, id: AgentId) -> Option<Message> {
        self.mailboxes.get_mut(&id).and_then(|mb| mb.pop()).map(|e| e.msg)
    }

    /// Number of registered agents.
    pub fn agents(&self) -> usize {
        self.mailboxes.len()
    }
}
