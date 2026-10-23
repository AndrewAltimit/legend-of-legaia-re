//! Memory-card service for the save screen: answer the pending card read,
//! perform the pending save, and apply a finished session's outcome.

use super::*;

#[wasm_bindgen::prelude::wasm_bindgen]
impl LegaiaRuntime {
    /// The page could not store the card `card_take_written` handed it
    /// (browser storage full or blocked): raise the save screen's
    /// `CardWriteFailed` notice, the one the native window raises when its
    /// `card.persist()` fails inside the same commit. Without it the screen
    /// said the save worked and a reload lost it. `false` when no pause menu
    /// is up to show it.
    pub fn card_store_failed(&mut self) -> bool {
        let Some(menu) = self.play_menu.as_mut() else {
            return false;
        };
        menu.save_flow.refuse(SaveRefusal::CardWriteFailed);
        true
    }

    /// Opt in to reporting card Saves only once the page has stored them.
    ///
    /// With it on, a Save's "Saving to MEMORY CARD" beat waits after the
    /// in-memory write for [`Self::card_store_result`], so the result line
    /// is the store's real outcome - "Unable to save." when the browser
    /// refused the card. Off, the beat reports the write at once and a
    /// refused store could only raise a notice after "Save successful." had
    /// already been shown. A page that opts in and never answers gets
    /// "Unable to save." after [`CARD_STORE_WAIT_FRAMES`]: nothing stored the
    /// card.
    pub fn card_store_handshake(&mut self, on: bool) {
        self.card_store_handshake = on;
    }

    /// The page's answer to a written card: `true` when it stored the card
    /// image (browser storage accepted it). Finishes a Save beat waiting on
    /// the store; `false` when none was waiting.
    pub fn card_store_result(&mut self, stored: bool) -> bool {
        let Some(m) = self.play_menu.as_mut() else {
            return false;
        };
        if m.store_wait.take().is_none() {
            return false;
        }
        if let Some(PlaySub::Session(session)) = m.sub.as_mut()
            && let FieldMenuSubsession::Save(s) = session.as_mut()
        {
            m.save_flow.finish_save_request(s, stored);
            return true;
        }
        false
    }
}

/// Frames a Save beat waits for [`LegaiaRuntime::card_store_result`] before
/// it reports the Save failed. The page answers within its 100 ms store poll.
pub const CARD_STORE_WAIT_FRAMES: u16 = 120;

impl LegaiaRuntime {
    /// Answer the card read [`SaveScreenFlow`] is waiting on, if any.
    ///
    /// The kernel decides *when* a port must be read (once per port, never
    /// per frame); this host is what turns a port number into fifteen block
    /// snapshots, because only it holds the card image.
    pub(super) fn service_card_read(&mut self) {
        let Some(m) = self.play_menu.as_ref() else {
            return;
        };
        let Some(PlaySub::Session(session)) = m.sub.as_ref() else {
            return;
        };
        let FieldMenuSubsession::Save(s) = session.as_ref() else {
            return;
        };
        let Some(port) = m.save_flow.pending_read(s) else {
            return;
        };
        let blocks = self.card_block_snapshots(port as usize);
        if let Some(m) = self.play_menu.as_mut() {
            m.save_flow.install_blocks(port, blocks);
        }
    }

    /// Answer a card Save's commit beat: write the live session into the
    /// block it asked for while "Saving to MEMORY CARD" is up, and report the
    /// result so the beat's result line is the write's real outcome.
    pub(super) fn service_card_save(&mut self) {
        let request = self.play_menu.as_ref().and_then(|m| match m.sub.as_ref() {
            Some(PlaySub::Session(session)) => match session.as_ref() {
                FieldMenuSubsession::Save(s) => m.save_flow.save_request(s),
                _ => None,
            },
            _ => None,
        });
        let Some(request) = request else {
            return;
        };
        // A write already handed to the page: wait for its store, and give
        // up as failed if the page never answers.
        if let Some(m) = self.play_menu.as_mut()
            && let Some(waited) = m.store_wait.as_mut()
        {
            *waited += 1;
            if *waited < CARD_STORE_WAIT_FRAMES {
                return;
            }
            m.store_wait = None;
            crate::console_log("play menu: the page never stored the written card");
            if let Some(PlaySub::Session(session)) = m.sub.as_mut()
                && let FieldMenuSubsession::Save(s) = session.as_mut()
            {
                m.save_flow.finish_save_request(s, false);
            }
            return;
        }
        let ok = match self.write_session_into_card(request.port as usize, request.cell + 1) {
            Ok(()) => true,
            Err(e) => {
                crate::console_log(&format!("play menu: card save failed: {e}"));
                false
            }
        };
        if ok && self.card_store_handshake {
            if let Some(m) = self.play_menu.as_mut() {
                m.store_wait = Some(0);
            }
            return;
        }
        if let Some(m) = self.play_menu.as_mut()
            && let Some(PlaySub::Session(session)) = m.sub.as_mut()
            && let FieldMenuSubsession::Save(s) = session.as_mut()
        {
            m.save_flow.finish_save_request(s, ok);
        }
    }

    /// Commit a finished Load / Save session against the memory-card rack.
    ///
    /// [`SaveScreenFlow::commit`] resolves the port off the session's outcome
    /// and the cell off the grid; this host maps cell `i` to card block
    /// `i + 1` (block 0 is the card directory) and moves the bytes. A failure
    /// (card ejected mid-flow, unreadable block) is logged and drops the
    /// player back to the menu rather than throwing - the world is left
    /// untouched.
    pub(super) fn apply_card_outcome(
        &mut self,
        flow: &SaveScreenFlow,
        session: &SaveSelectSession,
    ) {
        let Some(commit) = flow.commit(session) else {
            return;
        };
        let block = commit.cell + 1;
        match commit.kind {
            SaveCommitKind::Load => {
                match self.load_session_from_card(commit.port as usize, block) {
                    Ok(scene) => {
                        // Retail resumes a save in the scene it was written in.
                        // The page owns scene entry, so park the label for it.
                        if let Some(m) = self.play_menu.as_mut() {
                            m.pending_load_scene = Some(scene);
                        }
                    }
                    Err(e) => {
                        crate::console_log(&format!("play menu: card load failed: {e}"));
                        if let Some(m) = self.play_menu.as_mut() {
                            m.save_flow.refuse(SaveRefusal::CardReadFailed);
                        }
                    }
                }
            }
            SaveCommitKind::Save => {
                if let Err(e) = self.write_session_into_card(commit.port as usize, block) {
                    crate::console_log(&format!("play menu: card save failed: {e}"));
                    // A console line is not an answer to the player: the
                    // screen closed and nothing had happened. Raise the
                    // shared refusal notice the native window raises.
                    if let Some(m) = self.play_menu.as_mut() {
                        m.save_flow.refuse(SaveRefusal::CardWriteFailed);
                    }
                }
            }
        }
    }
}
