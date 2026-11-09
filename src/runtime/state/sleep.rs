use crate::{
    command::{CommandContext, CommandId},
    control::{ControlRequest, SleepCycleId, SleepEvent},
    runtime::RuntimeEvent,
    session::DelayInhibitor,
};

use super::RuntimeState;

#[derive(Default)]
pub(super) struct SleepCycle {
    pub(super) events: Vec<(SleepEvent, SleepCycleId, Option<ControlRequest>)>,
    requests: Vec<ControlRequest>,
    completed: Option<bool>,
    command: Option<CommandId>,
    cycle: Option<SleepCycleId>,
    active: bool,
    wait: bool,
}

impl SleepCycle {
    pub(super) fn reject(&mut self, reason: &str) {
        for request in self.requests.drain(..) {
            request.respond(format!("error: {reason}\n"));
        }
    }

    fn resolve(&mut self, success: bool) {
        self.completed = Some(success);
        for request in self.requests.drain(..) {
            reply(request, success);
        }
    }
}

fn reply(request: ControlRequest, success: bool) {
    request.respond(if success {
        "sweetidle: sleep ready\n".into()
    } else {
        "error: before-sleep command failed\n".into()
    });
}

impl RuntimeState {
    pub(super) fn prepare_sleep(&mut self, inhibitor: Option<DelayInhibitor>) {
        if self.preparing_for_sleep {
            return;
        }
        self.preparing_for_sleep = true;
        self.runtime_events.push(RuntimeEvent::PreparingForSleep);
        self.queue_session_command(
            self.session_commands.before_sleep.clone(),
            CommandContext::BeforeSleep,
            inhibitor,
        );
    }

    pub(super) fn resume_sleep(&mut self) {
        self.preparing_for_sleep = false;
        // this cycle is over even if its hook is still running or queued
        self.commands.release_sleep_inhibitors();
        for intent in &mut self.session_command_intents {
            intent.inhibitor = None;
        }
        self.set_idle_hint(false);
        self.runtime_events.push(RuntimeEvent::ResumedFromSleep);
        self.queue_session_command(
            self.session_commands.after_resume.clone(),
            CommandContext::AfterResume,
            None,
        );
    }

    pub(super) fn route_native_sleep_events(&mut self) {
        for (event, cycle, request) in std::mem::take(&mut self.native_sleep.events) {
            match event {
                SleepEvent::Before => {
                    if self.native_sleep.cycle.as_ref() != Some(&cycle) {
                        self.native_sleep.reject("sleep preparation was superseded");
                        if self.preparing_for_sleep {
                            self.session_command_intents
                                .retain(|intent| intent.context != CommandContext::BeforeSleep);
                            self.resume_sleep();
                        }
                        self.native_sleep.cycle = Some(cycle);
                        self.native_sleep.command = None;
                        // latch the wait policy so reload cannot release an in-flight preparation
                        self.native_sleep.active = true;
                        self.native_sleep.wait = self.session_commands.wait_before_sleep;
                        self.native_sleep.completed =
                            self.session_commands.before_sleep.is_none().then_some(true);
                        self.prepare_sleep(None);
                    }
                    if let Some(request) = request {
                        if !self.native_sleep.active {
                            request.respond("error: sleep cycle has already ended\n".into());
                            continue;
                        }
                        if let Some(success) = self.native_sleep.completed {
                            reply(request, success);
                        } else {
                            self.native_sleep.requests.push(request);
                        }
                    }
                }
                SleepEvent::After | SleepEvent::Cancel => {
                    if self.native_sleep.cycle.as_ref() != Some(&cycle) {
                        if let Some(request) = request {
                            request.respond("error: stale sleep cycle\n".into());
                        }
                        continue;
                    }
                    self.native_sleep
                        .reject("sleep preparation ended before the command completed");
                    if self.preparing_for_sleep {
                        // cancelled preparations must not launch a hook that was only queued
                        self.session_command_intents
                            .retain(|intent| intent.context != CommandContext::BeforeSleep);
                        self.resume_sleep();
                    }
                    self.native_sleep.completed = None;
                    self.native_sleep.active = false;
                    if let Some(request) = request {
                        request.respond("sweetidle: awake\n".into());
                    }
                }
            }
        }
    }

    pub(super) fn observe_native_sleep_commands(&mut self, first: usize) {
        if !self.native_sleep.active
            || !self.preparing_for_sleep
            || self.native_sleep.completed.is_some()
        {
            return;
        }
        for event in &self.runtime_events[first..] {
            let outcome = match event {
                RuntimeEvent::CommandStarted(start)
                    if start.context == CommandContext::BeforeSleep =>
                {
                    self.native_sleep.command = Some(start.command_id);
                    (!self.native_sleep.wait).then_some(true)
                }
                RuntimeEvent::CommandSkipped(skipped)
                    if skipped.context == CommandContext::BeforeSleep =>
                {
                    // an older running hook cannot establish readiness for a new attempt
                    Some(false)
                }
                RuntimeEvent::CommandFinished {
                    command,
                    completion,
                } if self.native_sleep.command == Some(command.command_id) => {
                    Some(completion.status.success())
                }
                RuntimeEvent::CommandFailed { command, .. }
                    if command.context == CommandContext::BeforeSleep =>
                {
                    Some(false)
                }
                _ => None,
            };
            if let Some(success) = outcome {
                self.native_sleep.resolve(success);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests;
