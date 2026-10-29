//! Pack dialogue on the faithful dialogue path.
//!
//! A pack line compiles
//! ([`legaia_expansion_format::dialog::compile_line`]) to the inline
//! interaction-script bytes the field VM's dialogue runner reads for an NPC
//! talk (`World::start_inline_dialogue`); this module tracks the run.
//!
//! The bytecode carries no field-VM flag op: a choice's pack-flag effects are
//! applied by the `World` after the talk ends, from the choice the runner
//! recorded ([`DialogRuntime`]), so a pack line can never write a retail
//! story flag.

use std::sync::Arc;

/// The pack line on screen, if any.
#[derive(Debug, Clone, Default)]
pub struct DialogRuntime {
    pub pending: Option<PendingLine>,
    /// Lines waiting for the dialogue channel to free up, oldest first (a
    /// quest's start line raised by the talk that started it).
    pub queued: Vec<String>,
}

/// A pack line the `World` started.
#[derive(Debug, Clone)]
pub struct PendingLine {
    /// The line id.
    pub line: String,
    /// The compiled bytes the runner was started on; identifies the run.
    pub bytes: Arc<Vec<u8>>,
    /// The option the player committed, once they have.
    pub choice: Option<usize>,
}

impl DialogRuntime {
    /// Record that `line`'s compiled `bytes` are running.
    pub fn begin(&mut self, line: &str, bytes: Arc<Vec<u8>>) {
        self.pending = Some(PendingLine {
            line: line.to_string(),
            bytes,
            choice: None,
        });
    }

    /// The runner on `bytes` committed `choice`. Ignored unless `bytes` is
    /// the pending line's run.
    pub fn observe_choice(&mut self, bytes: &Arc<Vec<u8>>, choice: usize) {
        if let Some(p) = self.pending.as_mut()
            && Arc::ptr_eq(&p.bytes, bytes)
        {
            p.choice = Some(choice);
        }
    }

    /// Whether the run on `bytes` is the pending line's.
    pub fn is_pending_run(&self, bytes: &Arc<Vec<u8>>) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(&p.bytes, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_is_recorded_only_for_its_own_run() {
        let mut rt = DialogRuntime::default();
        let mine = Arc::new(vec![0x21]);
        let other = Arc::new(vec![0x21]);
        rt.begin("l", mine.clone());
        rt.observe_choice(&other, 1);
        assert_eq!(rt.pending.as_ref().unwrap().choice, None);
        rt.observe_choice(&mine, 1);
        assert_eq!(rt.pending.as_ref().unwrap().choice, Some(1));
        assert!(rt.is_pending_run(&mine) && !rt.is_pending_run(&other));
    }
}
