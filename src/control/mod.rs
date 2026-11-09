// share the same control protocol between the client and daemon
mod error;
mod protocol;
mod sleep;
mod socket;

pub use error::ControlError;
pub use sleep::{SleepCycleId, SleepEvent, notify_sleep};
pub use socket::{pause, query_status, query_status_json, reload, resume};

pub(crate) use protocol::{
    ControlEvent, ControlRequest, DbusInhibitorStatus, StatusFormat, StatusSnapshot,
    SystemdInhibitStatus,
};
pub(crate) use socket::ControlMonitor;
