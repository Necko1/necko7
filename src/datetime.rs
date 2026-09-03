use chrono::{DateTime, Utc};

pub trait DateTimeExt {
    fn remaining_pretty(&self) -> String;
}

impl DateTimeExt for DateTime<Utc> {
    fn remaining_pretty(&self) -> String {
        let diff = *self - Utc::now();
        let total_secs = diff.num_seconds();

        if total_secs <= 0 {
            return "0 sec".to_string();
        }

        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        let mut parts = Vec::new();
        if days > 0 { parts.push(format!("{} d", days)); }
        if hours > 0 { parts.push(format!("{} h", hours)); }
        if mins > 0 { parts.push(format!("{} min", mins)); }
        if secs > 0 || parts.is_empty() { parts.push(format!("{} sec", secs)); }

        parts.join(" ")
    }
}