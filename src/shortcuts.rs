use chrono::NaiveTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shortcut {
    Timer { seconds: u64, label: String },
    Alarm { time: NaiveTime, label: String },
}

impl Shortcut {
    pub fn from_args(kind: &str, value: &str, label: &str) -> Option<Self> {
        if label.chars().count() > 80 || label.chars().any(char::is_control) {
            return None;
        }
        match kind {
            "timer" => {
                let seconds = value.parse().ok()?;
                (1..360_000).contains(&seconds).then(|| Self::Timer {
                    seconds,
                    label: label.to_owned(),
                })
            }
            "alarm" => Some(Self::Alarm {
                time: NaiveTime::parse_from_str(value, "%H:%M").ok()?,
                label: label.to_owned(),
            }),
            _ => None,
        }
    }

    pub fn from_url(value: &str) -> Option<Self> {
        if value.len() > 1_500 {
            return None;
        }
        let url = url::Url::parse(value).ok()?;
        if url.scheme() != "lintel-world-clock"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || !matches!(url.path(), "" | "/")
            || url.fragment().is_some()
        {
            return None;
        }
        let kind = url.host_str()?;
        let value_name = match kind {
            "timer" => "seconds",
            "alarm" => "time",
            _ => return None,
        };
        let mut value = None;
        let mut label = None;
        for (key, data) in url.query_pairs() {
            match key.as_ref() {
                key if key == value_name && value.is_none() => value = Some(data),
                "label" if label.is_none() => label = Some(data),
                _ => return None,
            }
        }
        Self::from_args(kind, &value?, label.as_deref().unwrap_or_default())
    }

    pub fn description(&self) -> String {
        match self {
            Self::Timer { seconds, label } => format!("开始 {seconds} 秒计时器  {label}"),
            Self::Alarm { time, label } => format!("添加 {} 的闹钟  {label}", time.format("%H:%M")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_external_requests_before_showing_confirmation() {
        assert_eq!(
            Shortcut::from_url("lintel-world-clock://timer?label=%E7%85%AE%E8%8C%B6&seconds=60"),
            Some(Shortcut::Timer {
                seconds: 60,
                label: "煮茶".into()
            }),
        );
        assert!(Shortcut::from_url("lintel-world-clock://timer?seconds=10&seconds=20").is_none());
        assert!(Shortcut::from_url("lintel-world-clock://alarm?time=25:00").is_none());
        assert!(Shortcut::from_url("https://timer?seconds=3").is_none());
    }

    #[test]
    fn validate_shortcut_arguments() {
        assert_eq!(
            Shortcut::from_args("timer", "60", "煮茶"),
            Some(Shortcut::Timer {
                seconds: 60,
                label: "煮茶".into()
            }),
        );
        assert!(Shortcut::from_args("alarm", "25:00", "").is_none());
        assert!(Shortcut::from_args("timer", "360000", "").is_none());
        assert!(Shortcut::from_args("timer", "10", "title\n").is_none());
    }
}
