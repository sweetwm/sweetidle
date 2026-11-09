mod completion;

use std::collections::HashMap;

use calloop::LoopHandle;

use completion::CompletionMonitor;

use super::{RuntimeError, RuntimeEvent, state::RuntimeState};
use crate::{
    command::{
        CommandContext, CommandDispatcher, CommandId, DispatchDecision, ProcessAdapter, SkipReason,
    },
    listener::{CommandIntent, ListenerId},
    session::DelayInhibitor,
};

#[derive(Default)]
pub(super) struct CommandRuntime {
    dispatcher: CommandDispatcher,
    processes: ProcessAdapter,
    completions: CompletionMonitor,
    delay_inhibitors: HashMap<CommandId, Vec<DelayInhibitor>>,
}

impl CommandRuntime {
    pub(super) fn dispatch_listener(
        &mut self,
        handle: &LoopHandle<'static, RuntimeState>,
        intent: CommandIntent,
        events: &mut Vec<RuntimeEvent>,
    ) -> Result<(), RuntimeError> {
        let decision = self.dispatcher.dispatch(intent);
        self.dispatch_decision(handle, decision, None, events)
    }

    pub(super) fn dispatch_session(
        &mut self,
        handle: &LoopHandle<'static, RuntimeState>,
        context: CommandContext,
        command: String,
        inhibitor: Option<DelayInhibitor>,
        events: &mut Vec<RuntimeEvent>,
    ) -> Result<(), RuntimeError> {
        let decision = self.dispatcher.dispatch_command(context, command);
        self.dispatch_decision(handle, decision, inhibitor, events)
    }

    pub(super) fn remap_listener_slots(&mut self, mapping: &[Option<ListenerId>]) {
        self.dispatcher.remap_listener_slots(mapping);
    }

    pub(super) fn release_sleep_inhibitors(&mut self) {
        self.delay_inhibitors.clear();
    }

    #[cfg(test)]
    pub(super) fn reserve_listener(&mut self, intent: CommandIntent) -> DispatchDecision {
        // reload policy tests reserve slots without spawning configured helpers
        self.dispatcher.dispatch(intent)
    }

    #[cfg(test)]
    pub(super) fn running_count(&self) -> usize {
        self.processes.running_count()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.dispatcher.is_empty() && self.processes.is_empty() && self.delay_inhibitors.is_empty()
    }

    pub(super) fn collect_ready_processes(
        &mut self,
        events: &mut Vec<RuntimeEvent>,
    ) -> Result<(), RuntimeError> {
        for completion in self.completions.collect(&mut self.processes)? {
            self.delay_inhibitors.remove(&completion.command_id);
            let command = self
                .dispatcher
                .finish(completion.command_id)
                .map_err(RuntimeError::CommandDispatch)?;
            events.push(RuntimeEvent::CommandFinished {
                command,
                completion,
            });
        }

        Ok(())
    }

    #[cfg(target_os = "freebsd")]
    pub(super) fn mark_children_changed(&mut self) {
        self.completions.record();
    }

    fn dispatch_decision(
        &mut self,
        handle: &LoopHandle<'static, RuntimeState>,
        decision: DispatchDecision,
        inhibitor: Option<DelayInhibitor>,
        events: &mut Vec<RuntimeEvent>,
    ) -> Result<(), RuntimeError> {
        match decision {
            DispatchDecision::Skip(decision) => {
                if let Some(inhibitor) = inhibitor {
                    // a new sleep attempt must still wait for its already-running before-sleep hook
                    let SkipReason::AlreadyRunning(command_id) = decision.reason;
                    self.delay_inhibitors
                        .entry(command_id)
                        .or_default()
                        .push(inhibitor);
                }
                events.push(RuntimeEvent::CommandSkipped(decision));
            }
            DispatchDecision::Spawn(request) => {
                if let Some(inhibitor) = inhibitor {
                    self.delay_inhibitors
                        .entry(request.command_id)
                        .or_default()
                        .push(inhibitor);
                }

                match self.processes.spawn(&request) {
                    Ok(start) => {
                        self.completions.watch(handle, start)?;
                        events.push(RuntimeEvent::CommandStarted(start));
                    }
                    Err(error) => {
                        self.delay_inhibitors.remove(&request.command_id);
                        let command = self
                            .dispatcher
                            .finish(request.command_id)
                            .map_err(RuntimeError::CommandDispatch)?;
                        events.push(RuntimeEvent::CommandFailed { command, error });
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
