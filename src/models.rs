use chrono::DateTime;
use chrono_tz::Tz;

#[derive(Debug)]
pub struct LogEvent {
    pub label: String,
    pub content: String,
    pub timestamp: DateTime<Tz>,
}
