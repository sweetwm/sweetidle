use super::*;

fn start(before: &str, wait: bool) -> Result<ReloadHarness, Box<dyn Error>> {
    ReloadHarness::start_with_general(
        "idle",
        None,
        2,
        "exec \"$0\" \"$@\"",
        &format!(
            "before_sleep_command = {before:?}\nafter_resume_command = 'exit 0'\nwait_before_sleep = {wait}"
        ),
    )
}

fn notify(harness: &ReloadHarness, event: &str, cycle: &str) -> Result<Child, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_sweetidle"))
        .args([event, cycle])
        .env("XDG_RUNTIME_DIR", harness.temp_dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?)
}

fn preparing(harness: &mut ReloadHarness) -> Result<bool, Box<dyn Error>> {
    let status: serde_json::Value = serde_json::from_str(&harness.status_json()?)?;
    status["preparing_for_sleep"]
        .as_bool()
        .ok_or_else(|| "missing sleep status".into())
}

fn wait_preparing(harness: &mut ReloadHarness, expected: bool) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + WAIT_LIMIT;
    loop {
        if preparing(harness)? == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("sleep state did not settle".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "runs the real daemon against a private Wayland compositor"]
fn readiness_resume_and_new_cycles_keep_control_responsive() -> Result<(), Box<dyn Error>> {
    let mut harness = start("sleep 0.5", true)?;
    harness.wait_for_markers(1)?;
    let mut before = notify(&harness, "before-sleep", "first")?;
    wait_preparing(&mut harness, true)?;
    assert!(before.try_wait()?.is_none());
    assert!(before.wait_with_output()?.status.success());
    assert!(
        notify(&harness, "before-sleep", "first")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(
        notify(&harness, "after-resume", "first")?
            .wait_with_output()?
            .status
            .success()
    );
    wait_preparing(&mut harness, false)?;
    assert!(
        notify(&harness, "after-resume", "first")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(
        !notify(&harness, "before-sleep", "first")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(
        notify(&harness, "before-sleep", "second")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(
        !notify(&harness, "cancel-sleep", "first")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(preparing(&mut harness)?);
    assert!(
        notify(&harness, "cancel-sleep", "second")?
            .wait_with_output()?
            .status
            .success()
    );
    wait_preparing(&mut harness, false)?;
    harness.stop()
}

#[test]
#[ignore = "runs the real daemon against a private Wayland compositor"]
fn failed_and_timed_out_hooks_cancel_and_allow_recovery() -> Result<(), Box<dyn Error>> {
    for command in [
        "exit 7",
        "sleep 4.5; touch \"$XDG_RUNTIME_DIR/sleep-complete\"",
    ] {
        let mut harness = start(command, true)?;
        harness.wait_for_markers(1)?;
        let result = notify(&harness, "before-sleep", "failure")?.wait_with_output()?;
        assert!(!result.status.success());
        wait_preparing(&mut harness, false)?;
        if command.starts_with("sleep") {
            let marker = harness.temp_dir.path().join("sleep-complete");
            wait_until(&mut harness.daemon, &harness.log_path, || marker.exists())?;
            harness.wait_for_log_count("command finished", 3)?;
        }
        let source = fs::read_to_string(&harness.config_path)?;
        fs::write(
            &harness.config_path,
            source.replace(&format!("{command:?}"), "'exit 0'"),
        )?;
        harness.signal_reload()?;
        harness.wait_for_log("configuration reloaded")?;
        assert!(
            notify(&harness, "before-sleep", "recovered")?
                .wait_with_output()?
                .status
                .success()
        );
        assert!(
            notify(&harness, "after-resume", "recovered")?
                .wait_with_output()?
                .status
                .success()
        );
        harness.stop()?;
    }
    Ok(())
}

#[test]
#[ignore = "runs the real daemon against a private Wayland compositor"]
fn fire_and_forget_acknowledges_spawn_and_removing_bridge_requires_restart()
-> Result<(), Box<dyn Error>> {
    let mut harness = start("sleep 2; touch \"$XDG_RUNTIME_DIR/sleep-complete\"", false)?;
    harness.wait_for_markers(1)?;
    assert!(
        notify(&harness, "before-sleep", "quick")?
            .wait_with_output()?
            .status
            .success()
    );
    assert!(!harness.temp_dir.path().join("sleep-complete").exists());
    harness.write_config("updated")?;
    harness.signal_reload()?;
    harness.wait_for_log("changing FreeBSD sleep integration")?;
    assert!(
        notify(&harness, "cancel-sleep", "quick")?
            .wait_with_output()?
            .status
            .success()
    );
    let marker = harness.temp_dir.path().join("sleep-complete");
    wait_until(&mut harness.daemon, &harness.log_path, || marker.exists())?;
    harness.wait_for_log_count("command finished", 3)?;
    harness.stop()
}
