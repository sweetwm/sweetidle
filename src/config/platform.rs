use super::{Config, ConfigError};

pub(crate) const LOGIND_SUPPORTED: bool = cfg!(target_os = "linux");

impl Config {
    // parsing stays portable; loading and runtime setup also check platform capabilities
    pub fn validate_platform(&self) -> Result<(), ConfigError> {
        validate_features(self, LOGIND_SUPPORTED, cfg!(target_os = "freebsd"))
    }
}

fn validate_features(
    config: &Config,
    supported: bool,
    native_sleep: bool,
) -> Result<(), ConfigError> {
    if supported {
        return Ok(());
    }
    for (field, configured) in [
        (
            "general.lock_command",
            config.general.lock_command.is_some(),
        ),
        (
            "general.unlock_command",
            config.general.unlock_command.is_some(),
        ),
        (
            "general.before_sleep_command",
            config.general.before_sleep_command.is_some() && !native_sleep,
        ),
        (
            "general.after_resume_command",
            config.general.after_resume_command.is_some() && !native_sleep,
        ),
        ("idle_hint", config.idle_hint_timeout_seconds.is_some()),
    ] {
        if configured {
            return Err(ConfigError::Unsupported {
                field,
                platform: std::env::consts::OS,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, LOGIND_SUPPORTED, validate_features};

    #[test]
    fn rejects_each_system_session_feature_without_logind_support() {
        for field in [
            "lock_command",
            "unlock_command",
            "before_sleep_command",
            "after_resume_command",
        ] {
            let config = Config::parse(&format!("[general]\n{field} = 'exit 0'\n"))
                .unwrap_or_else(|error| panic!("{error}"));
            let error = validate_features(&config, false, false)
                .expect_err("unsupported session hook was accepted");
            assert!(matches!(error, ConfigError::Unsupported { .. }));
            assert!(error.to_string().contains(field));
            assert!(validate_features(&config, true, false).is_ok());
        }
        let config =
            Config::parse("[idle_hint]\ntimeout = 30\n").unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            validate_features(&config, false, false),
            Err(ConfigError::Unsupported {
                field: "idle_hint",
                ..
            })
        ));
        assert!(validate_features(&config, true, false).is_ok());
    }

    #[test]
    fn accepts_basic_idle_config_without_ignoring_supported_inhibitors() {
        let config = Config::parse("[[listener]]\ntimeout = 30\non_timeout = 'exit 0'\n")
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(validate_features(&config, false, false).is_ok());
        assert!(!config.general.ignore_dbus_inhibit);
        assert!(!config.general.ignore_systemd_inhibit);
    }

    #[test]
    fn platform_validation_matches_available_backend() {
        let config = Config::parse("[general]\nbefore_sleep_command = 'exit 0'\n")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            config.validate_platform().is_ok(),
            LOGIND_SUPPORTED || cfg!(target_os = "freebsd")
        );
    }

    #[test]
    fn native_sleep_support_does_not_enable_logind_session_features() {
        for field in ["before_sleep_command", "after_resume_command"] {
            let config = Config::parse(&format!("[general]\n{field} = 'exit 0'\n"))
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(validate_features(&config, false, true).is_ok());
        }
        for source in [
            "[general]\nlock_command = 'exit 0'\n",
            "[general]\nunlock_command = 'exit 0'\n",
            "[idle_hint]\ntimeout = 30\n",
        ] {
            let config = Config::parse(source).unwrap_or_else(|error| panic!("{error}"));
            assert!(validate_features(&config, false, true).is_err());
        }
    }
}
