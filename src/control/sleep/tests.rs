use std::{
    error::Error,
    sync::mpsc::TryRecvError,
    thread,
    time::{Duration, Instant},
};

use super::{SleepCycleId, SleepEvent};
use crate::control::{
    ControlError, ControlEvent,
    socket::{ControlMonitor, SocketKind, send_command_with_timeout},
};

#[test]
fn sleep_and_regular_sockets_reject_each_others_commands() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    for (kind, command) in [
        (SocketKind::Sleep, b"status\n".as_slice()),
        (SocketKind::Control, b"before-sleep cycle\n".as_slice()),
    ] {
        let path = directory.path().join("socket");
        let (_monitor, events) = ControlMonitor::connect_for(path.clone(), kind)?;
        assert!(
            matches!(send_command_with_timeout(&path, command, Duration::from_secs(2)), Err(ControlError::ServerRejected(message)) if message == "unknown control command")
        );
        assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));
    }
    Ok(())
}

#[test]
fn cycle_ids_are_bounded_and_cannot_inject_protocol_fields() {
    for invalid in ["", "a b", "a\nb", "../cycle", &"x".repeat(41)] {
        assert!(SleepCycleId::try_from(invalid).is_err());
    }
    let cycle = SleepCycleId::try_from("00000000-0000-0000-0000-000000000001")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        SleepEvent::Before.command(&cycle),
        "before-sleep 00000000-0000-0000-0000-000000000001\n"
    );
}

#[test]
fn failed_preparation_replies_also_cancel_the_matching_cycle() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("sleep");
    let (_monitor, events) = ControlMonitor::connect_for(path.clone(), SocketKind::Sleep)?;
    let client = thread::spawn(move || {
        send_command_with_timeout(&path, b"before-sleep failure\n", Duration::from_secs(6))
    });
    let ControlEvent::Sleep {
        event: SleepEvent::Before,
        cycle,
        request: Some(request),
    } = receive(&events)?
    else {
        panic!("expected preparation");
    };
    request.respond("error: before-sleep command failed\n".into());
    assert!(matches!(
        client.join().unwrap_or_else(|_| panic!("client panicked")),
        Err(ControlError::ServerRejected(_))
    ));
    assert!(
        matches!(receive(&events)?, ControlEvent::Sleep { event: SleepEvent::Cancel, cycle: cancelled, request: None } if cancelled == cycle)
    );
    Ok(())
}

#[test]
fn unanswered_preparation_has_a_bounded_wait_and_cancels_its_cycle() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("sleep");
    let (_monitor, events) = ControlMonitor::connect_for(path.clone(), SocketKind::Sleep)?;
    let client = thread::spawn(move || {
        send_command_with_timeout(&path, b"before-sleep timeout\n", Duration::from_secs(6))
    });
    let ControlEvent::Sleep {
        cycle,
        request: Some(request),
        ..
    } = receive(&events)?
    else {
        panic!("expected preparation");
    };
    assert!(
        matches!(client.join().unwrap_or_else(|_| panic!("client panicked")), Err(ControlError::ServerRejected(message)) if message == "daemon did not answer in time")
    );
    assert!(
        matches!(receive(&events)?, ControlEvent::Sleep { event: SleepEvent::Cancel, cycle: cancelled, request: None } if cancelled == cycle)
    );
    drop(request);
    Ok(())
}

#[test]
fn normal_control_remains_responsive_while_preparation_waits() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let sleep_path = directory.path().join("sleep");
    let control_path = directory.path().join("control");
    let (_sleep, sleep_events) =
        ControlMonitor::connect_for(sleep_path.clone(), SocketKind::Sleep)?;
    let (_control, control_events) =
        ControlMonitor::connect_for(control_path.clone(), SocketKind::Control)?;
    let sleep_client = thread::spawn(move || {
        send_command_with_timeout(
            &sleep_path,
            b"before-sleep waiting\n",
            Duration::from_secs(6),
        )
    });
    let ControlEvent::Sleep {
        request: Some(preparation),
        ..
    } = receive(&sleep_events)?
    else {
        panic!("expected preparation");
    };
    let client = thread::spawn(move || {
        send_command_with_timeout(&control_path, b"status\n", Duration::from_secs(2))
    });
    let ControlEvent::Status { request, .. } = receive(&control_events)? else {
        panic!("expected status");
    };
    request.respond("still responsive\n".into());
    assert_eq!(
        client
            .join()
            .unwrap_or_else(|_| panic!("client panicked"))?,
        "still responsive\n"
    );
    preparation.respond("sweetidle: sleep ready\n".into());
    assert_eq!(
        sleep_client
            .join()
            .unwrap_or_else(|_| panic!("client panicked"))?,
        "sweetidle: sleep ready\n"
    );
    Ok(())
}

fn receive(
    events: &calloop::channel::Channel<ControlEvent>,
) -> Result<ControlEvent, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match events.try_recv() {
            Ok(event) => return Ok(event),
            Err(TryRecvError::Empty) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error.into()),
        }
    }
}
