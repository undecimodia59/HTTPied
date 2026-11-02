//! Bounded application diagnostics, independent of request history.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
pub const LIMIT: usize = 500;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    Info,
    Warning,
    Error,
}
impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: i64,
    pub when_ms: u64,
    pub level: Level,
    pub message: String,
    pub repeats: u64,
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn bounded(message: &str) -> String {
    let mut end = message.len().min(4096);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}
#[derive(Default)]
pub struct Book {
    pub records: VecDeque<Record>,
    next_id: i64,
    pub unread: usize,
}
impl Book {
    pub fn push(&mut self, level: Level, message: &str) -> Record {
        let message = bounded(message);
        let when_ms = now_ms();
        self.unread = self.unread.saturating_add(1);
        if let Some(last) = self.records.back_mut()
            && last.level == level
            && last.message == message
            && when_ms.saturating_sub(last.when_ms) <= 5000
        {
            last.repeats = last.repeats.saturating_add(1);
            last.when_ms = when_ms;
            return last.clone();
        }
        self.next_id = self.next_id.saturating_add(1).max(1);
        let record = Record {
            id: self.next_id,
            when_ms,
            level,
            message,
            repeats: 1,
        };
        self.records.push_back(record.clone());
        while self.records.len() > LIMIT {
            self.records.pop_front();
        }
        record
    }
    pub fn restore(&mut self, records: Vec<Record>) -> Vec<Record> {
        let pending = std::mem::take(&mut self.records);
        self.next_id = records
            .iter()
            .map(|record| record.id)
            .max()
            .unwrap_or(0)
            .max(0);
        self.records = records.into_iter().collect();
        let mut to_store = Vec::new();
        for record in pending {
            let mut restored = self.push(record.level, &record.message);
            restored.when_ms = record.when_ms;
            restored.repeats = record.repeats;
            if let Some(last) = self.records.back_mut() {
                *last = restored.clone();
            }
            to_store.push(restored);
        }
        while self.records.len() > LIMIT {
            self.records.pop_front();
        }
        self.unread = 0;
        to_store
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_bound_memory_coalesce_and_keep_severity() {
        let mut book = Book::default();
        book.push(Level::Error, "connection failed");
        book.push(Level::Error, "connection failed");
        assert_eq!(book.records.len(), 1);
        assert_eq!(book.records[0].repeats, 2);
        book.push(Level::Warning, "connection failed");
        assert_eq!(book.records.len(), 2);
        for i in 0..600 {
            book.push(Level::Info, &format!("{i}: {}", "界".repeat(2000)));
        }
        assert_eq!(book.records.len(), LIMIT);
        assert!(
            book.records
                .iter()
                .all(|record| record.message.len() <= 4096)
        );
    }
}
