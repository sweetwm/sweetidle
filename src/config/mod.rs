// expose validated config types without exposing the parsing documents
mod loader;
mod model;
mod parser;
mod platform;

pub use loader::ConfigLoadError;
pub use model::{Config, GeneralConfig, ListenerConfig};
pub use parser::ConfigError;
pub(crate) use platform::LOGIND_SUPPORTED;
