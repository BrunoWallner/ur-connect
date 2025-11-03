use std::io::Cursor;

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use ical::{parser::ical::IcalParser, property::Property};

use crate::model::{Recurrence, TimetableEntry};

pub fn parse_ics(content: &str) -> Vec<TimetableEntry> {
    if content.trim().is_empty() {
        return Vec::new();
    }

    let cursor = Cursor::new(content.as_bytes());
    let mut parser = IcalParser::new(cursor);
    let mut entries = Vec::new();

    while let Some(result) = parser.next() {
        let calendar = match result {
            Ok(calendar) => calendar,
            Err(_) => continue,
        };

        for event in calendar.events {
            let summary = property_value(&event.properties, "SUMMARY");
            let description = property_value(&event.properties, "DESCRIPTION");
            let location = property_value(&event.properties, "LOCATION");
            let dt_start_raw = property_value(&event.properties, "DTSTART");
            let dt_end_raw = property_value(&event.properties, "DTEND");
            let rrule_raw = property_value(&event.properties, "RRULE");

            let dt_start = dt_start_raw
                .as_deref()
                .and_then(|value| parse_ics_date(value));
            let dt_end = dt_end_raw
                .as_deref()
                .and_then(|value| parse_ics_date(value));

            let date_text = dt_start
                .as_ref()
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let time_text = match (dt_start.as_ref(), dt_end.as_ref()) {
                (Some(start), Some(end)) => {
                    format!("{} - {}", start.format("%H:%M"), end.format("%H:%M"))
                }
                (Some(start), None) => start.format("%H:%M").to_string(),
                _ => String::new(),
            };

            let title = summary
                .or(description)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let loc = location.map(|s| s.trim().to_string()).unwrap_or_default();
            let recurrence = rrule_raw
                .as_deref()
                .and_then(|rule| recurrence_from_rule(rule));

            if date_text.is_empty() && title.is_empty() {
                continue;
            }

            entries.push(TimetableEntry::new(
                date_text, time_text, title, loc, recurrence,
            ));
        }
    }

    entries
}

fn property_value(properties: &[Property], name: &str) -> Option<String> {
    let target = name.to_ascii_uppercase();
    for property in properties {
        if property.name.eq_ignore_ascii_case(&target) {
            return property.value.clone();
        }
    }
    None
}

fn recurrence_from_rule(rule: &str) -> Option<Recurrence> {
    for part in rule.split(';') {
        let mut iter = part.splitn(2, '=');
        let key = iter.next()?.trim().to_ascii_uppercase();
        let value = iter.next().unwrap_or("").trim();
        if key == "FREQ" {
            return Recurrence::from_freq(value);
        }
    }
    None
}

fn parse_ics_date(raw: &str) -> Option<DateTime<Local>> {
    let trimmed = raw.trim();
    let mut value = trimmed;
    if let Some(idx) = trimmed.find(':') {
        value = trimmed[idx + 1..].trim();
    }
    if value.is_empty() {
        return None;
    }

    if value.ends_with('Z') {
        let value_no_z = &value[..value.len() - 1];
        for fmt in ["%Y%m%dT%H%M%S", "%Y%m%dT%H%M"] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(value_no_z, fmt) {
                let utc = Utc.from_utc_datetime(&naive);
                return Some(utc.with_timezone(&Local));
            }
        }
        if let Ok(date) = NaiveDate::parse_from_str(value_no_z, "%Y%m%d") {
            let naive = date.and_hms_opt(0, 0, 0)?;
            let utc = Utc.from_utc_datetime(&naive);
            return Some(utc.with_timezone(&Local));
        }
    } else {
        for fmt in ["%Y%m%dT%H%M%S", "%Y%m%dT%H%M"] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(value, fmt) {
                return Some(to_local_datetime(naive));
            }
        }
        if let Ok(date) = NaiveDate::parse_from_str(value, "%Y%m%d") {
            if let Some(naive) = date.and_hms_opt(0, 0, 0) {
                return Some(to_local_datetime(naive));
            }
        }
        if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
            return Some(dt.with_timezone(&Local));
        }
    }

    None
}

fn to_local_datetime(naive: NaiveDateTime) -> DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(first, second) => {
            if first.timestamp() <= second.timestamp() {
                first
            } else {
                second
            }
        }
        LocalResult::None => Utc.from_utc_datetime(&naive).with_timezone(&Local),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_and_parses_basic_event() {
        let input = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Test Event\nLOCATION:Room 101\nDTSTART;TZID=Europe/Berlin:20241001T080000\nDTEND;TZID=Europe/Berlin:20241001T093000\nEND:VEVENT\nEND:VCALENDAR";
        let entries = parse_ics(input);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.title, "Test Event");
        assert_eq!(entry.location, "Room 101");
        assert_eq!(entry.time.len(), 13);
        assert!(entry.recurrence.is_none());
    }

    #[test]
    fn captures_recurrence_frequency() {
        let input = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Weekly Seminar\nDTSTART:20241001T080000Z\nDTEND:20241001T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=TU\nEND:VEVENT\nEND:VCALENDAR";
        let entries = parse_ics(input);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(matches!(entry.recurrence, Some(Recurrence::Weekly)));
    }
}
