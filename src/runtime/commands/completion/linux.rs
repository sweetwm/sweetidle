use std::{collections::HashSet, io};

use calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic};
use rustix::process::{Pid, PidfdFlags, pidfd_open};

use crate::{
    command::{CommandId, ProcessAdapter, ProcessCompletion, ProcessStart},
    runtime::{RuntimeError, state::RuntimeState},
};

#[derive(Default)]
pub(in crate::runtime) struct CompletionMonitor {
    ready: Vec<CommandId>,
    // another pidfd may become ready after its child was already reaped
    completed_before_monitor: HashSet<CommandId>,
}

impl CompletionMonitor {
    pub(in crate::runtime) fn record(&mut self, command_id: CommandId) {
        self.ready.push(command_id);
    }

    pub(in crate::runtime) fn collect(
        &mut self,
        processes: &mut ProcessAdapter,
    ) -> Result<Vec<ProcessCompletion>, RuntimeError> {
        let ready = std::mem::take(&mut self.ready);
        if ready.is_empty() {
            return Ok(Vec::new());
        }

        let completions = processes
            .collect_finished()
            .map_err(RuntimeError::Process)?;
        let completed_ids = completions
            .iter()
            .map(|completion| completion.command_id)
            .collect::<HashSet<_>>();

        for completion in &completions {
            if !ready.contains(&completion.command_id) {
                self.completed_before_monitor.insert(completion.command_id);
            }
        }
        for command_id in ready {
            if completed_ids.contains(&command_id)
                || self.completed_before_monitor.remove(&command_id)
            {
                continue;
            }
            return Err(RuntimeError::ProcessNotReady(command_id));
        }
        Ok(completions)
    }

    pub(in crate::runtime) fn watch(
        &mut self,
        handle: &LoopHandle<'static, RuntimeState>,
        start: ProcessStart,
    ) -> Result<(), RuntimeError> {
        let raw_pid = i32::try_from(start.process_id)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or(RuntimeError::InvalidProcessId(start.process_id))?;
        let pidfd = pidfd_open(raw_pid, PidfdFlags::NONBLOCK).map_err(|source| {
            RuntimeError::MonitorProcess {
                command_id: start.command_id,
                process_id: start.process_id,
                source,
            }
        })?;
        let command_id = start.command_id;

        // level readiness catches prior exits; removing the source prevents repeated wakeups
        handle
            .insert_source(
                Generic::new(pidfd, Interest::READ, Mode::Level),
                move |_, _, state: &mut RuntimeState| {
                    state.commands.completions.record(command_id);
                    Ok::<PostAction, io::Error>(PostAction::Remove)
                },
            )
            .map_err(|error| RuntimeError::RegisterProcessMonitor {
                command_id,
                source: error.error,
            })?;

        Ok(())
    }
}
