//! Undo/redo, the operation log and idempotency tracking.
//!
//! Undo entries are full `Document` snapshots. Thanks to shared, copy-on-write tiles a
//! snapshot costs a few pointer copies per cel instead of a copy of every pixel.

use std::collections::{HashMap, VecDeque};

use crate::project::Document;

pub const UNDO_LIMIT: usize = 256;
pub const LOG_LIMIT: usize = 10_000;
pub const IDEMPOTENCY_LIMIT: usize = 2048;

/// One applied mutation, kept for auditing who changed what.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub revision: u64,
    pub actor: String,
    pub op: String,
    pub operation_id: Option<String>,
    pub changed_pixels: u64,
    pub timestamp_ms: u64,
}

struct UndoEntry {
    before: Document,
    after_revision: u64,
    label: String,
    actor: String,
}

#[derive(Default)]
pub struct History {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    log: VecDeque<LogEntry>,
    idempotency: HashMap<String, Vec<u8>>,
    idempotency_order: VecDeque<String>,
}

pub struct UndoInfo {
    pub label: String,
    pub actor: String,
}

impl History {
    pub fn record(&mut self, before: Document, after_revision: u64, label: &str, actor: &str) {
        self.undo.push(UndoEntry { before, after_revision, label: label.into(), actor: actor.into() });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Swaps `current` with the previous state. Returns what was undone.
    pub fn undo(&mut self, current: &mut Document, revision: u64) -> Option<UndoInfo> {
        let entry = self.undo.pop()?;
        let after = std::mem::replace(current, entry.before);
        let info = UndoInfo { label: entry.label.clone(), actor: entry.actor.clone() };
        self.redo.push(UndoEntry { before: after, after_revision: revision, label: entry.label, actor: entry.actor });
        let _ = entry.after_revision;
        Some(info)
    }

    pub fn redo(&mut self, current: &mut Document, revision: u64) -> Option<UndoInfo> {
        let entry = self.redo.pop()?;
        let before = std::mem::replace(current, entry.before);
        let info = UndoInfo { label: entry.label.clone(), actor: entry.actor.clone() };
        self.undo.push(UndoEntry { before, after_revision: revision, label: entry.label, actor: entry.actor });
        Some(info)
    }

    pub fn can_undo(&self) -> usize {
        self.undo.len()
    }

    pub fn can_redo(&self) -> usize {
        self.redo.len()
    }

    pub fn reset(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn log(&mut self, entry: LogEntry) {
        self.log.push_back(entry);
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    pub fn log_entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.log.iter()
    }

    pub fn replace_log(&mut self, entries: Vec<LogEntry>) {
        self.log = entries.into();
    }

    pub fn cached_response(&self, operation_id: &str) -> Option<&Vec<u8>> {
        self.idempotency.get(operation_id)
    }

    pub fn cache_response(&mut self, operation_id: &str, response: Vec<u8>) {
        if self.idempotency.insert(operation_id.to_string(), response).is_none() {
            self.idempotency_order.push_back(operation_id.to_string());
        }
        while self.idempotency_order.len() > IDEMPOTENCY_LIMIT {
            if let Some(old) = self.idempotency_order.pop_front() {
                self.idempotency.remove(&old);
            }
        }
    }

    pub fn undo_labels(&self) -> Vec<(String, String)> {
        self.undo.iter().rev().map(|e| (e.label.clone(), e.actor.clone())).collect()
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
