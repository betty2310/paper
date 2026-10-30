//! Timestamps are stored as UTC RFC 3339 text with millisecond precision, so
//! they sort correctly as strings and read well in a database browser.

use std::time::SystemTime;

use jiff::Timestamp;

const FORMAT: &str = "%Y-%m-%dT%H:%M:%S.%3fZ";

/// The current time as stored in the database.
pub fn now() -> String {
    format(Timestamp::now())
}

/// `now` minus `days`, as stored in the database.
pub fn days_ago(days: i64) -> String {
    let seconds = Timestamp::now().as_second() - days * 24 * 60 * 60;
    format(Timestamp::from_second(seconds).expect("within the representable range"))
}

/// Parses a stored timestamp. Anything unreadable becomes the Unix epoch, so a
/// corrupt row still lists instead of failing the whole query.
pub fn parse(text: &str) -> SystemTime {
    text.parse::<Timestamp>()
        .map(SystemTime::from)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn format(timestamp: Timestamp) -> String {
    timestamp.strftime(FORMAT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_form_round_trips_and_sorts_as_text() {
        let text = now();
        assert_eq!(text.len(), "2026-10-30T12:34:56.789Z".len(), "{text}");
        assert!(text.ends_with('Z'));
        let parsed = parse(&text);
        let back = format(Timestamp::try_from(parsed).unwrap());
        assert_eq!(back, text);
        assert!(days_ago(30) < text);
        assert_eq!(parse("garbage"), SystemTime::UNIX_EPOCH);
    }
}
