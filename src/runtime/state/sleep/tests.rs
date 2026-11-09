use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::{
    command::CommandContext,
    config::Config,
    control::{ControlEvent, ControlRequest, SleepEvent},
    listener::ListenerSet,
    runtime::{
        RuntimeEvent,
        state::RuntimeState,
        test_support::{new_event_loop, wait_for_process_events},
    },
    wayland::IdleWaylandState,
};

fn state(command: Option<&str>, wait: bool) -> RuntimeState {
    let mut config = Config::default();
    config.general.before_sleep_command = command.map(str::to_owned);
    config.general.wait_before_sleep = wait;
    RuntimeState::new(
        IdleWaylandState::default(),
        ListenerSet::from_config(&config),
        &config,
        None,
    )
}

fn request(state: &mut RuntimeState, event: SleepEvent) -> Receiver<String> {
    request_cycle(state, event, "test-cycle")
}

fn request_cycle(state: &mut RuntimeState, event: SleepEvent, cycle: &str) -> Receiver<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    state.record_control_event(ControlEvent::Sleep {
        event,
        cycle: cycle.try_into().unwrap_or_else(|error| panic!("{error}")),
        request: Some(ControlRequest::new(sender)),
    });
    receiver
}

#[test]
fn fresh_cycle_cannot_reuse_ready_state_when_resume_was_missed() {
    let mut state = state(None, true);
    let first = request_cycle(&mut state, SleepEvent::Before, "first");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(first.try_recv().is_ok());
    state.session_commands.before_sleep = Some("exit 0".into());
    let second = request_cycle(&mut state, SleepEvent::Before, "second");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(second.try_recv(), Err(TryRecvError::Empty)));
    let stale = request_cycle(&mut state, SleepEvent::Cancel, "first");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        stale
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .contains("stale")
    );
    assert!(state.preparing_for_sleep);
    assert_eq!(
        state
            .session_command_intents
            .iter()
            .filter(|intent| intent.context == CommandContext::BeforeSleep)
            .count(),
        1
    );
}

#[test]
fn completed_cycle_cannot_be_replayed_after_resume() {
    let mut state = state(None, true);
    let first = request(&mut state, SleepEvent::Before);
    request(&mut state, SleepEvent::After);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(first.try_recv().is_ok());
    let replay = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        replay
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .contains("already ended")
    );
    assert!(!state.preparing_for_sleep);
}

#[test]
fn waiting_preparations_share_one_hook_and_reply_only_after_completion() {
    let mut state = state(Some("sleep 0.05"), true);
    let mut event_loop = new_event_loop();
    let first = request(&mut state, SleepEvent::Before);
    let second = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(state.session_command_intents.len(), 1);
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(first.try_recv(), Err(TryRecvError::Empty)));
    assert!(matches!(second.try_recv(), Err(TryRecvError::Empty)));
    let _ = state.take_runtime_events();
    let events = wait_for_process_events(&mut event_loop, &mut state);
    assert!(matches!(
        events.as_slice(),
        [RuntimeEvent::CommandFinished { .. }]
    ));
    assert_eq!(
        first.try_recv().unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
    assert_eq!(
        second.try_recv().unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
    let repeated = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(state.session_command_intents.is_empty());
    assert_eq!(
        repeated
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
}

#[test]
fn fire_and_forget_replies_after_spawn_while_hook_is_still_running() {
    let mut state = state(Some("sleep 0.05"), false);
    let mut event_loop = new_event_loop();
    let receiver = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        receiver
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
    assert_eq!(state.commands.running_count(), 1);
    let _ = state.take_runtime_events();
    wait_for_process_events(&mut event_loop, &mut state);
    assert!(state.commands.is_empty());
}

#[test]
fn failed_hook_rejects_preparation_instead_of_releasing_sleep() {
    let mut state = state(Some("exit 7"), true);
    let mut event_loop = new_event_loop();
    let receiver = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    let _ = state.take_runtime_events();
    wait_for_process_events(&mut event_loop, &mut state);
    assert_eq!(
        receiver
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}")),
        "error: before-sleep command failed\n"
    );
}

#[test]
fn cancellation_discards_queued_hook_and_runs_resume_cleanup_once() {
    let mut state = state(Some("exit 0"), true);
    state.session_commands.after_resume = Some("exit 0".into());
    let before = request(&mut state, SleepEvent::Before);
    request(&mut state, SleepEvent::Cancel);
    request(&mut state, SleepEvent::After);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!state.preparing_for_sleep);
    assert!(
        before
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .starts_with("error: ")
    );
    assert_eq!(state.session_command_intents.len(), 1);
    assert_eq!(
        state.session_command_intents[0].context,
        CommandContext::AfterResume
    );
    assert_eq!(
        state
            .take_runtime_events()
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ResumedFromSleep))
            .count(),
        1
    );
}

#[test]
fn preparation_without_a_before_hook_is_immediately_ready() {
    let mut state = state(None, true);
    let receiver = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        receiver
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
    assert!(state.preparing_for_sleep);
    assert!(state.session_command_intents.is_empty());
}

#[test]
fn reload_does_not_change_the_wait_policy_of_a_pending_preparation() {
    let mut state = state(Some("sleep 0.05"), true);
    let mut event_loop = new_event_loop();
    let receiver = request(&mut state, SleepEvent::Before);
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    state.session_commands.wait_before_sleep = false;
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    let _ = state.take_runtime_events();
    wait_for_process_events(&mut event_loop, &mut state);
    assert_eq!(
        receiver
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}")),
        "sweetidle: sleep ready\n"
    );
}

#[test]
fn a_hook_from_an_older_cycle_cannot_release_a_new_preparation() {
    let mut state = state(Some("sleep 0.05"), true);
    let mut event_loop = new_event_loop();
    let first = request_cycle(&mut state, SleepEvent::Before, "old");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    request_cycle(&mut state, SleepEvent::Cancel, "old");
    let second = request_cycle(&mut state, SleepEvent::Before, "new");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        first
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .starts_with("error:")
    );
    state
        .dispatch_command_intents(&event_loop.handle())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        second
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .starts_with("error:")
    );
    let _ = state.take_runtime_events();
    wait_for_process_events(&mut event_loop, &mut state);
    let repeated = request_cycle(&mut state, SleepEvent::Before, "new");
    state
        .route_pending()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repeated
            .try_recv()
            .unwrap_or_else(|error| panic!("{error}"))
            .starts_with("error:")
    );
}
