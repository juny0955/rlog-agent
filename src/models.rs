use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct LogEvent {
    pub label: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}
