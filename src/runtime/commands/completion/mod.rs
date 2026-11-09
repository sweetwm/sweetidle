#[cfg(target_os = "freebsd")]
mod freebsd;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "freebsd")]
pub(in crate::runtime) use freebsd::CompletionMonitor;
#[cfg(target_os = "linux")]
pub(in crate::runtime) use linux::CompletionMonitor;
