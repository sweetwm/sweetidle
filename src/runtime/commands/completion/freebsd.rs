use calloop::LoopHandle;

use crate::{
    command::{ProcessAdapter, ProcessCompletion, ProcessStart},
    runtime::{RuntimeError, state::RuntimeState},
};

#[derive(Default)]
pub(in crate::runtime) struct CompletionMonitor {
    changed: bool,
}

impl CompletionMonitor {
    pub(in crate::runtime) fn record(&mut self) {
        self.changed = true;
    }

    pub(in crate::runtime) fn watch(
        &mut self,
        _handle: &LoopHandle<'static, RuntimeState>,
        _start: ProcessStart,
    ) -> Result<(), RuntimeError> {
        // a fast exit may already be waiting before the next signal callback
        self.changed = true;
        Ok(())
    }

    pub(in crate::runtime) fn collect(
        &mut self,
        processes: &mut ProcessAdapter,
    ) -> Result<Vec<ProcessCompletion>, RuntimeError> {
        if !std::mem::take(&mut self.changed) {
            return Ok(Vec::new());
        }
        // child signals can coalesce or describe a stopped child, so check every owned child
        processes.collect_finished().map_err(RuntimeError::Process)
    }
}
