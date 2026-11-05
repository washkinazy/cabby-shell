//! Transport-independent, active-only notification policy.
//!
//! A single event-loop owner serializes mutations. Callers inject monotonic time,
//! acknowledge actual presentation, and schedule `next_deadline` rather than poll.
//! Tokens belong to one store lifetime; discard all callbacks when replacing it.
use std::fmt;
use std::time::{Duration, Instant};

pub const MAX_ENTRIES: usize = 64;
pub const MAX_VISIBLE: usize = 3;
pub const MAX_TEXT_BYTES: usize = 256;
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_ACTIONS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

#[derive(Clone)]
pub struct Request {
    pub replaces_id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// Alternating wire-format keys and labels. Validation checks pair boundaries.
    pub actions: Vec<String>,
    pub urgency: Urgency,
    pub resident: bool,
    /// Milliseconds; -1 selects the default, 0 never expires.
    pub expire_timeout: i32,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            replaces_id: 0,
            app_name: String::new(),
            summary: String::new(),
            body: String::new(),
            actions: Vec::new(),
            urgency: Urgency::Normal,
            resident: false,
            expire_timeout: -1,
        }
    }
}

// No sensitive payload is included in diagnostics, including action identifiers.
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Request { content: [redacted] }")
    }
}

#[derive(Clone)]
pub struct Action {
    pub key: String,
    pub label: String,
}
impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Action { content: [redacted] }")
    }
}

/// Revision-qualified handle for UI events and timer callbacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    id: u32,
    revision: u64,
}
impl Token {
    pub fn id(self) -> u32 {
        self.id
    }
}

#[derive(Clone)]
pub struct Entry {
    token: Token,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<Action>,
    pub urgency: Urgency,
    pub resident: bool,
    lifetime: Option<Duration>,
    presented: bool,
    deadline: Option<Instant>,
}
impl Entry {
    pub fn token(&self) -> Token {
        self.token
    }
    pub fn presented(&self) -> bool {
        self.presented
    }
}
impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Client = 3,
}

#[derive(PartialEq, Eq)]
pub enum Event {
    Closed { id: u32, reason: CloseReason },
    ActionInvoked { id: u32, key: String },
}
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed { id, reason } => f
                .debug_struct("Closed")
                .field("id", id)
                .field("reason", reason)
                .finish(),
            Self::ActionInvoked { id, .. } => f
                .debug_struct("ActionInvoked")
                .field("id", id)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidText,
    InvalidActions,
    InvalidTimeout,
    Capacity,
    UnknownId,
    StaleToken,
    UnknownAction,
    NotVisible,
    ClockOverflow,
    RevisionExhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "notification policy: {self:?}")
    }
}
impl std::error::Error for Error {}

fn text(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

impl Request {
    fn validate(&self) -> Result<(), Error> {
        if self.app_name.len() > MAX_TEXT_BYTES
            || self.summary.len() > MAX_TEXT_BYTES
            || self.body.len() > MAX_BODY_BYTES
        {
            return Err(Error::InvalidText);
        }
        if self.expire_timeout < -1 {
            return Err(Error::InvalidTimeout);
        }
        if !self.actions.len().is_multiple_of(2) || self.actions.len() > 2 * MAX_ACTIONS {
            return Err(Error::InvalidActions);
        }
        let mut keys = Vec::new();
        for pair in self.actions.as_chunks::<2>().0 {
            let key = &pair[0];
            if key.is_empty()
                || key.contains('\0')
                || key.len() > MAX_TEXT_BYTES
                || pair[1].len() > MAX_TEXT_BYTES
                || keys.contains(&key)
            {
                return Err(Error::InvalidActions);
            }
            keys.push(key);
        }
        Ok(())
    }

    fn into_entry(self, token: Token) -> Entry {
        let lifetime = if self.urgency == Urgency::Critical || self.expire_timeout == 0 {
            None
        } else if self.expire_timeout == -1 {
            Some(Duration::from_secs(5))
        } else {
            Some(Duration::from_millis(self.expire_timeout as u64))
        };
        Entry {
            token,
            app_name: text(&self.app_name),
            summary: text(&self.summary),
            body: text(&self.body),
            actions: self
                .actions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| Action {
                    key: pair[0].clone(),
                    label: text(&pair[1]),
                })
                .collect(),
            urgency: self.urgency,
            resident: self.resident,
            lifetime,
            presented: false,
            deadline: None,
        }
    }
}

pub struct Store {
    entries: Vec<Entry>,
    next_id: u32,
    revision: u64,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            revision: 0,
        }
    }
}
impl Store {
    /// Arrival order, including visible and pending entries. No closed history.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Candidates for the three visible cards. The UI acknowledges presentation
    /// only when an output is available; queued cards have no expiry deadline.
    pub fn visible(&self) -> &[Entry] {
        &self.entries[..self.entries.len().min(MAX_VISIBLE)]
    }

    /// A nonzero replacement ID is returned unchanged, including an absent ID.
    /// Replacing an existing entry preserves its queue position.
    pub fn notify(&mut self, request: Request) -> Result<Token, Error> {
        request.validate()?;
        let position = self
            .entries
            .iter()
            .position(|e| e.token.id == request.replaces_id);
        if position.is_none() && self.entries.len() == MAX_ENTRIES {
            return Err(Error::Capacity);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let id = if request.replaces_id != 0 {
            request.replaces_id
        } else {
            // At most 64 active IDs: a free nonzero ID is always available.
            let mut candidate = self.next_id;
            while self.entries.iter().any(|e| e.token.id == candidate) {
                candidate = candidate.wrapping_add(1).max(1);
            }
            candidate
        };
        let token = Token { id, revision };
        let generated = request.replaces_id == 0;
        let entry = request.into_entry(token);
        if let Some(position) = position {
            self.entries[position] = entry;
        } else {
            self.entries.push(entry);
        }
        self.revision = revision;
        if generated {
            self.next_id = id.wrapping_add(1).max(1);
        }
        Ok(token)
    }

    pub fn presented(&mut self, token: Token, now: Instant) -> Result<(), Error> {
        let position = self.position(token)?;
        if position >= MAX_VISIBLE {
            return Err(Error::NotVisible);
        }
        let entry = &mut self.entries[position];
        if !entry.presented {
            entry.deadline = entry
                .lifetime
                .map(|duration| now.checked_add(duration).ok_or(Error::ClockOverflow))
                .transpose()?;
            entry.presented = true;
        }
        Ok(())
    }

    /// The event loop can replace its one-shot timer whenever the model changes.
    pub fn next_deadline(&self) -> Option<(Token, Instant)> {
        self.entries
            .iter()
            .filter_map(|e| e.deadline.map(|time| (e.token, time)))
            .min_by_key(|(_, time)| *time)
    }

    /// Old timer callbacks and already-closed IDs are harmless no-ops.
    pub fn expire(&mut self, token: Token, now: Instant) -> Option<Event> {
        let position = self.position(token).ok()?;
        if self.entries[position]
            .deadline
            .is_some_and(|deadline| now >= deadline)
        {
            Some(self.remove(position, CloseReason::Expired))
        } else {
            None
        }
    }

    /// Protocol clients address the current ID, unlike revision-qualified UI events.
    pub fn close(&mut self, id: u32) -> Result<Event, Error> {
        let position = self
            .entries
            .iter()
            .position(|e| e.token.id == id)
            .ok_or(Error::UnknownId)?;
        Ok(self.remove(position, CloseReason::Client))
    }

    pub fn dismiss(&mut self, token: Token) -> Result<Event, Error> {
        let position = self.position(token)?;
        Ok(self.remove(position, CloseReason::Dismissed))
    }

    pub fn invoke(&mut self, token: Token, key: &str) -> Result<Vec<Event>, Error> {
        let position = self.position(token)?;
        let entry = &self.entries[position];
        if position >= MAX_VISIBLE || !entry.presented {
            return Err(Error::NotVisible);
        }
        if !entry.actions.iter().any(|a| a.key == key) {
            return Err(Error::UnknownAction);
        }
        let mut events = vec![Event::ActionInvoked {
            id: token.id,
            key: key.into(),
        }];
        if !entry.resident {
            events.push(self.remove(position, CloseReason::Dismissed));
        } else {
            self.entries[position].deadline = None;
        }
        Ok(events)
    }

    fn position(&self, token: Token) -> Result<usize, Error> {
        self.entries
            .iter()
            .position(|e| e.token == token)
            .ok_or(Error::StaleToken)
    }
    fn remove(&mut self, position: usize, reason: CloseReason) -> Event {
        Event::Closed {
            id: self.entries.remove(position).token.id,
            reason,
        }
    }
}

#[cfg(test)]
mod tests;
