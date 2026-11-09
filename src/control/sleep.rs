use std::time::Duration;

use super::{ControlError, socket};

pub(crate) const SLEEP_WAIT_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEvent {
    Before,
    After,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepCycleId(String);

impl TryFrom<&str> for SleepCycleId {
    type Error = ControlError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 40
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ControlError::InvalidSleepCycle);
        }
        Ok(Self(value.into()))
    }
}

impl SleepCycleId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl SleepEvent {
    pub(crate) fn command(self, cycle: &SleepCycleId) -> String {
        let name = match self {
            Self::Before => "before-sleep",
            Self::After => "after-resume",
            Self::Cancel => "cancel-sleep",
        };
        format!("{name} {}\n", cycle.as_str())
    }
}

pub fn notify_sleep(event: SleepEvent, cycle: &SleepCycleId) -> Result<String, ControlError> {
    if !cfg!(target_os = "freebsd") {
        return Err(ControlError::SleepEventsUnsupported);
    }
    let timeout = if event == SleepEvent::Before {
        SLEEP_WAIT_TIMEOUT + Duration::from_secs(1)
    } else {
        Duration::from_secs(2)
    };
    let response = socket::send_sleep_event(event, cycle, timeout)?;
    let expected = if event == SleepEvent::Before {
        "sweetidle: sleep ready\n"
    } else {
        "sweetidle: awake\n"
    };
    if response != expected {
        return Err(ControlError::InvalidResponse);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
