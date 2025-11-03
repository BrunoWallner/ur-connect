use std::fmt;

/// Represents a single timetable entry downloaded from the campus portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimetableEntry {
    pub date: String,
    pub time: String,
    pub title: String,
    pub location: String,
    pub recurrence: Option<Recurrence>,
}

impl TimetableEntry {
    pub fn new(
        date: String,
        time: String,
        title: String,
        location: String,
        recurrence: Option<Recurrence>,
    ) -> Self {
        Self {
            date,
            time,
            title,
            location,
            recurrence,
        }
    }
}

impl fmt::Display for TimetableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if !self.date.is_empty() {
            parts.push(self.date.as_str());
        }
        if !self.time.is_empty() {
            parts.push(self.time.as_str());
        }
        if !self.title.is_empty() {
            parts.push(self.title.as_str());
        }
        let mut line = parts.join(" ");
        if !self.location.is_empty() {
            if line.is_empty() {
                line = self.location.clone();
            } else {
                line = format!("{} @ {}", line, self.location);
            }
        }

        if let Some(rule) = &self.recurrence {
            if line.is_empty() {
                write!(f, "{}", rule)
            } else {
                write!(f, "{} • {}", line, rule)
            }
        } else {
            write!(f, "{}", line)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Custom(String),
}

impl Recurrence {
    pub fn from_freq(freq: &str) -> Option<Self> {
        match freq.to_ascii_uppercase().as_str() {
            "DAILY" => Some(Self::Daily),
            "WEEKLY" => Some(Self::Weekly),
            "MONTHLY" => Some(Self::Monthly),
            "YEARLY" => Some(Self::Yearly),
            other if !other.is_empty() => Some(Self::Custom(other.to_string())),
            _ => None,
        }
    }
}

impl fmt::Display for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recurrence::Daily => write!(f, "Daily"),
            Recurrence::Weekly => write!(f, "Weekly"),
            Recurrence::Monthly => write!(f, "Monthly"),
            Recurrence::Yearly => write!(f, "Yearly"),
            Recurrence::Custom(value) => write!(f, "{}", value),
        }
    }
}
