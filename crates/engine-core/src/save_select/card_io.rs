//! Save-block checksum, the memory-card status poll and I/O machine, and the card snapshots a host feeds the screen.
//! Split out of `save_select.rs`.

use super::*;

// ---------------------------------------------------------------------
// Save-block checksum
// ---------------------------------------------------------------------

/// A save block is exactly one memory-card block ([`CARD_BLOCK_BYTES`] =
/// `0x2000` bytes), read as `0x800` little-endian u32 words.
pub const SAVE_BLOCK_WORDS: usize = legaia_save::SC_BLOCK_WORDS;

/// Word index of the stored checksum inside a save block: the last word,
/// at byte offset `0x1FFC`. The checksum covers only the words *before*
/// it (`0..SAVE_BLOCK_CHECKSUM_WORD`).
pub const SAVE_BLOCK_CHECKSUM_WORD: usize = legaia_save::SC_BLOCK_CHECKSUM_WORD;

/// Additive checksum over a save block, matching retail's `FUN_801E38D8`.
///
/// The kernel itself lives in `legaia-save`, beside the SC-block bytes it
/// has to stay consistent with - a save the engine writes and a save the
/// `save-tool` / card-rack path writes must agree on this word, and one
/// copy is how that stays true. This is the word-slice face of it for
/// engine callers that already hold a block as `u32`s.
///
/// REF: FUN_801E38D8 (the `PORT:` is on `legaia_save::card::sc_block_checksum`,
/// the byte form - retail's argument is a byte pointer, and the byte form is
/// the one a real card path runs)
pub fn save_block_checksum(block: &[u32]) -> u32 {
    legaia_save::sc_block_checksum_words(block)
}

/// Whether a save block's stored checksum word matches a fresh
/// [`save_block_checksum`], the retail load-validity test.
///
/// Retail loads the stored word at byte `0x1FFC` and branches on
/// `stored == computed` (`FUN_801DD35C` state 5 at `0x801df888`:
/// `lw v1,0x1ffc(s1); beq v1,v0`); a match advances to sub-state `0x16`
/// and the block is copied into live RAM, a mismatch routes to `0x13`,
/// the "Damaged data." arm. A block too short to hold the checksum word
/// can never be valid.
///
/// REF: FUN_801DD35C (state 5 stored-vs-computed compare)
pub fn save_block_checksum_valid(block: &[u32]) -> bool {
    block.len() > SAVE_BLOCK_CHECKSUM_WORD
        && save_block_checksum(block) == block[SAVE_BLOCK_CHECKSUM_WORD]
}

// ---------------------------------------------------------------------
// Memory-card status poll
// ---------------------------------------------------------------------

/// Number of kernel event handles retail's card poll tests each frame.
pub const CARD_STATUS_EVENTS: usize = 4;

/// Frames the card poll waits before it forces [`CardStatus::Aborted`].
///
/// `FUN_801E3900` compares the counter against `0x78` **before**
/// incrementing it, so the first frame that trips the timeout is the one
/// entered with the counter already at 120 - i.e. the 121st poll.
pub const CARD_STATUS_TIMEOUT_FRAMES: u16 = 0x78;

/// Outcome of one frame of retail's memory-card status poll.
///
/// The discriminants are the integers `FUN_801E3900` returns, and the
/// names are what its caller `FUN_801E3294` does with each: `1` proceeds,
/// `2` tears the read down with result `-3`, `3` prints "NOT CARD" and
/// fails with result `-1`, `4` completes the read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardStatus {
    /// `0` - no event has fired yet; keep waiting.
    #[default]
    Pending,
    /// `1` - the card responded and the read may proceed.
    Ready,
    /// `2` - the read was torn down, or the poll timed out.
    Aborted,
    /// `3` - no card in the slot.
    NoCard,
    /// `4` - the read completed.
    Complete,
}

/// Poll the four card kernel events for one frame and advance `counter`.
///
/// Retail calls `TestEvent` on each of the four handles in turn and
/// overwrites its running status whenever a handle reports `1`, so the
/// **last** handle to fire wins - the priority is fixed by call order,
/// not by comparison. Only handle 0 can leave the status at
/// [`CardStatus::Pending`]; the other three unconditionally overwrite.
///
/// `counter` is retail's `DAT_801EF17C`. It is incremented on every call
/// regardless of which branch the timeout test takes (the store sits in
/// the `bne`'s delay slot), and once it enters a call at
/// [`CARD_STATUS_TIMEOUT_FRAMES`] or above the returned status is forced
/// to [`CardStatus::Aborted`] whatever the events said.
///
/// PORT: FUN_801E3900
pub fn card_status_poll(events: [bool; CARD_STATUS_EVENTS], counter: &mut u16) -> CardStatus {
    // `xori v0,v0,1; sltiu s0,v0,1` - status starts at 1 iff handle 0
    // reported 1, else 0.
    let mut status = if events[0] {
        CardStatus::Ready
    } else {
        CardStatus::Pending
    };
    if events[1] {
        status = CardStatus::Aborted;
    }
    if events[2] {
        status = CardStatus::NoCard;
    }
    if events[3] {
        status = CardStatus::Complete;
    }

    let entered_at = *counter;
    *counter = counter.saturating_add(1);
    if entered_at >= CARD_STATUS_TIMEOUT_FRAMES {
        status = CardStatus::Aborted;
    }
    status
}

/// Drain the four card kernel events with their results discarded.
///
/// Retail's `TestEvent` consumes a pending event as it tests it, so the
/// drain is exactly a clear of all four flags - which is why the earlier
/// judgment that "a Rust caller that passes the event states in has
/// nothing left to drain" was almost right: the caller *does* still have
/// the flags themselves to reset between operations, and this is the
/// primitive the second-op step (`FUN_801E3294` state 2) runs before
/// arming the next BIOS call.
///
/// Driven from the session's `NowChecking` beat straight after the
/// per-frame [`card_status_poll`], which is where retail's `TestEvent`
/// consumption happens, and from a host handling
/// [`CardIoEffect::SecondOp`].
///
/// PORT: FUN_801E39A8 (four `TestEvent` calls on the `0x8007B9F0..FC`
/// handles, results ignored; see
/// `ghidra/scripts/funcs/overlay_menu_801e39a8.txt`)
pub fn card_events_drain(events: &mut [bool; CARD_STATUS_EVENTS]) {
    *events = [false; CARD_STATUS_EVENTS];
}

/// Number of retries the card I/O machine spends on a failing phase
/// before it commits an error result (`DAT_801E4FC4 == 5` tests at
/// `0x801e339c` / `0x801e34d4` / `0x801e3590` / `0x801e35c0`).
pub const CARD_IO_RETRIES: u8 = 5;

/// Side effect one [`CardIoMachine::tick`] asks the host to perform -
/// the BIOS thunk calls the retail machine makes at each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardIoEffect {
    /// State 0: arm the first BIOS card op (`FUN_8006EE14(chan)`).
    StartOp,
    /// State 2: drain the events ([`card_events_drain`]) and arm the
    /// second BIOS op (`FUN_801E39A8` + `FUN_8006EE24(chan)`).
    SecondOp,
    /// A `Complete` event fired during the first-op wait: run the
    /// finalize pair (`FUN_801E3A98` + `FUN_8006EE34(chan)` +
    /// `FUN_801E3A00`).
    Finalize,
    /// The finalize path with a non-`-1` hardware result also clears the
    /// live pad words (`_DAT_8007B850` / `_DAT_8007B874` at
    /// `0x801e34a8..0x801e34b4`) - a real input swallow, not bookkeeping.
    PadClear,
}

/// The libcd I/O state machine of the save/load flow - the driver the
/// per-frame ticker ([`card_frame_tick`]) advances until it yields a
/// non-zero result.
///
/// Retail keeps five states in `DAT_801EF188`: `0` arm first op, `1`
/// wait, `2` arm second op, `3` wait, `4` publish the pending result and
/// reset. Both wait states consume the per-frame status poll
/// ([`card_status_poll`]) and share a retry budget (`DAT_801E4FC4`,
/// [`CARD_IO_RETRIES`]): a failing phase re-runs the whole two-op cycle
/// with result `0` until the budget is spent, then commits `-1` (no
/// card), `-2` (stray complete in phase two) or `-3` (abort/timeout).
/// The `both_acked` latch (`DAT_801EED20`) records that phase two
/// acknowledged, letting the *next* cycle short-circuit to success off
/// the first ack alone - success publishes result `1`.
///
/// PORT: FUN_801E3294 (see
/// `ghidra/scripts/funcs/overlay_menu_801e3294.txt`; the state table in
/// `docs/subsystems/save-screen.md` is the same machine)
///
/// WIRED: [`crate::save_screen::SaveScreenFlow`] keeps one for as long as a
/// card-rack screen is up and advances it every frame through
/// [`card_frame_tick`], so both hosts drive it - the flow is the one kernel
/// they share. The card is the host's block backend: blocks installed for the
/// port on screen poll `Ready`, a mount with nothing readable polls `NoCard`
/// and spends the retry budget, an unanswered read polls `Pending`. The
/// machine's published result is what gates a confirm and what the outer
/// dispatcher's card driver waits on.
#[derive(Debug, Clone, Default)]
pub struct CardIoMachine {
    /// `DAT_801EF188`.
    pub(super) state: u8,
    /// `DAT_801E4FC4` - shared retry budget.
    pub(super) retry: u8,
    /// `DAT_801EED20` - phase-two-acknowledged latch.
    pub(super) both_acked: bool,
    /// `DAT_801EF184` - result staged for the next state-4 publish.
    pub(super) pending: i32,
    /// `DAT_801EF180` - last published result (the return value).
    pub(super) result: i32,
    /// `_DAT_801F0214` - the "not card count" printed at retry
    /// exhaustion.
    pub(super) not_card_count: i32,
    /// `DAT_801EF0FC` - count of published `2` results.
    pub aborted_publishes: u32,
    /// `_DAT_801F3808` - last non-zero published result; the finalize
    /// branch keys on it. Retail's finalize helpers also write it, so a
    /// host mirroring real hardware may override via
    /// [`Self::set_hw_result`].
    pub(super) last_result: i32,
}

impl CardIoMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Last published result (`0` = busy / none yet).
    pub fn result(&self) -> i32 {
        self.result
    }

    /// Override the hardware-result word (`_DAT_801F3808`) the finalize
    /// branch inspects; retail's BIOS-side helpers write it out of band.
    pub fn set_hw_result(&mut self, v: i32) {
        self.last_result = v;
    }

    pub(super) fn publish(&mut self, pending: i32) -> i32 {
        // State 4 body, run inline where retail parks one frame: reset,
        // publish, count/latch the non-zero results.
        self.state = 0;
        self.result = pending;
        if pending == 2 {
            self.aborted_publishes += 1;
        }
        if pending != 0 {
            self.last_result = pending;
        }
        self.result
    }

    pub(super) fn fail_or_retry(&mut self, exhausted_result: i32, clear_latch_on_retry: bool) {
        if self.retry >= CARD_IO_RETRIES {
            self.retry = 0;
            self.both_acked = false;
            self.pending = exhausted_result;
        } else {
            self.retry += 1;
            if clear_latch_on_retry {
                self.both_acked = false;
            }
            self.pending = 0;
        }
        self.state = 4;
    }

    /// Advance one frame. `status` is this frame's poll outcome (only the
    /// two wait states consume it); `poll_counter` is the shared
    /// [`card_status_poll`] backstop counter, which the arm states reset
    /// (`DAT_801EF17C = 0` at `0x801e32f8` / state 2). Returns the
    /// published result (`0` while busy) plus any host effect.
    pub fn tick(
        &mut self,
        status: CardStatus,
        poll_counter: &mut u16,
    ) -> (i32, Option<CardIoEffect>) {
        match self.state {
            0 => {
                self.state = 1;
                *poll_counter = 0;
                self.result = 0;
                (self.result, Some(CardIoEffect::StartOp))
            }
            1 => match status {
                CardStatus::Pending => (self.result, None),
                CardStatus::Ready => {
                    self.not_card_count = 0;
                    self.pending = 1;
                    if self.both_acked {
                        // Prior cycle acked phase two - one ack completes.
                        self.state = 4;
                    } else {
                        self.state = 2;
                    }
                    (self.result, None)
                }
                CardStatus::NoCard => {
                    if self.retry >= CARD_IO_RETRIES {
                        // Retail prints "not card count:%d" here; the -1
                        // result only fires while the count is >= 0, and
                        // the count advances either way.
                        if self.not_card_count >= 0 {
                            self.not_card_count = 0;
                            self.pending = -1;
                            self.state = 4;
                            self.retry = 0;
                            self.both_acked = false;
                        }
                        self.not_card_count += 1;
                        (self.result, None)
                    } else {
                        self.fail_or_retry(-1, false);
                        (self.result, None)
                    }
                }
                CardStatus::Complete => {
                    // A completion event during the first wait: finalize.
                    if self.last_result == -1 {
                        self.both_acked = false;
                        self.pending = 2;
                        self.state = 2;
                        (self.result, Some(CardIoEffect::Finalize))
                    } else {
                        if self.last_result < 0 {
                            // Retail raises DAT_801EF10C here (a UI error
                            // flag) - surfaced through the negative
                            // last_result itself.
                        }
                        self.pending = 0;
                        self.state = 4;
                        (self.result, Some(CardIoEffect::PadClear))
                    }
                }
                CardStatus::Aborted => {
                    self.fail_or_retry(-3, false);
                    (self.result, None)
                }
            },
            2 => {
                self.state = 3;
                *poll_counter = 0;
                (self.result, Some(CardIoEffect::SecondOp))
            }
            3 => match status {
                CardStatus::Pending => (self.result, None),
                CardStatus::Ready => {
                    self.both_acked = true;
                    self.state = 4;
                    (self.result, None)
                }
                CardStatus::NoCard => {
                    self.fail_or_retry(-1, false);
                    (self.result, None)
                }
                CardStatus::Complete => {
                    self.fail_or_retry(-2, true);
                    (self.result, None)
                }
                CardStatus::Aborted => {
                    self.fail_or_retry(-3, false);
                    (self.result, None)
                }
            },
            _ => {
                let pending = self.pending;
                (self.publish(pending), None)
            }
        }
    }
}

/// A mounted card's directory entries, as the [`CardDirEntry`] list the
/// retail scan / budget pair consumes.
///
/// The BIOS `firstfile` walk retail's table fill rides builds each `DIRENTRY`
/// from the raw 128-byte frame: the 20-byte filename at `+0x0A`, the byte
/// size at `+0x04`.
pub fn card_dir_entries(card: &legaia_save::emu::MountedCard) -> Vec<CardDirEntry> {
    (1..=crate::save_screen::SLOT_GRID_CELLS)
        .filter(|&b| card.block_is_save_start(b))
        .filter_map(|b| card.dir_frame(b))
        .filter_map(|f| {
            let mut name = [0u8; CARD_DIRENTRY_NAME_LEN];
            name.copy_from_slice(f.get(0x0A..0x0A + CARD_DIRENTRY_NAME_LEN)?);
            let s = f.get(4..8)?;
            Some(CardDirEntry {
                name,
                size: u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            })
        })
        .collect()
}

/// One save block, rendered as the grid cell that prints it.
///
/// The lead record's name / level / HP / MP through one derivation, so a card
/// save and a slot file print the same rows. The location row is retail's own
/// field (the scene banner name); a save written before the resume trailer
/// existed falls back to the scene label, then to nothing - never to an
/// invented kingdom.
pub fn snapshot_for_save(
    slot: u8,
    sf: &legaia_save::SaveFile,
    resume: &legaia_save::SaveResume,
) -> SlotSnapshot {
    let Some(leader) = sf.leader_summary() else {
        return SlotSnapshot::foreign(slot);
    };
    let location = if resume.location.is_empty() {
        resume.scene.clone()
    } else {
        resume.location.clone()
    };
    SlotSnapshot {
        slot,
        present: true,
        damaged: false,
        content: SlotContent::LegaiaSave,
        label: if leader.name.is_empty() {
            format!("Slot {}", slot + 1)
        } else {
            leader.name.clone()
        },
        play_time_seconds: sf.ext_v2.play_time_seconds,
        party_lv: leader.level,
        location,
        money: sf.ext.money.max(0) as u32,
        leader_char_id: leader.char_id,
        leader_name: leader.name,
        leader_hp: leader.hp,
        leader_mp: leader.mp,
    }
}

/// A mounted card's fifteen blocks as the 5x3 preview grid reads them - one
/// derivation, both hosts.
///
/// The free-cell captions are priced by the retail free-block budget
/// (`FUN_801E3AF0` -> `FUN_801E3BA0`): enumerate the card's files off its live
/// directory frames, fill the fixed table, and ask the card how many blocks it
/// itself says are free. Retail only captions a cell "free" while that budget
/// pays for it - absence of a claim is not evidence a block is free, so an
/// unclaimed cell past the budget captions as foreign rather than inviting an
/// overwrite.
///
/// A block the directory *claims* but which will not lift is someone else's
/// save, not a free block: retail captions the two differently, and folding
/// them invites a Save to overwrite what it never read.
///
/// The native window used to skip the budget entirely and caption every
/// unclaimed cell free, so the same card image printed differently on the two
/// hosts past the budget.
pub fn card_block_snapshots(card: &legaia_save::emu::MountedCard) -> Vec<SlotSnapshot> {
    let entries = card_dir_entries(card);
    let (dir_table, dir_count) = card_directory_scan(&entries);
    let mut free_budget = card_free_blocks(&dir_table, dir_count).max(0);
    (0..crate::save_screen::SLOT_GRID_CELLS)
        .map(|cell| {
            let block = cell + 1;
            if !card.block_is_save_start(block) {
                if free_budget > 0 {
                    free_budget -= 1;
                    return SlotSnapshot::empty(cell);
                }
                return SlotSnapshot::foreign(cell);
            }
            // Past here the block IS claimed, so every way of failing to read
            // it is someone else's save rather than a free block.
            //
            // Retail's directory walk (`FUN_801E1208`) classifies a block a
            // Legaia save by its **filename** alone - one of the two regional
            // prefixes - so a file another game wrote is foreign however its
            // bytes happen to parse.
            let ours = card
                .dir_frame(block)
                .and_then(|f| f.get(0x0A..))
                .and_then(super::card_dir_slot_of)
                .is_some();
            if !ours {
                return SlotSnapshot::foreign(cell);
            }
            match card.save_at(cell) {
                Some((sf, resume)) => SlotSnapshot {
                    // The read's verify, ahead of time: retail sums the
                    // block it read and refuses it on a mismatch.
                    damaged: !card
                        .sc_block(block)
                        .is_some_and(legaia_save::card::sc_block_checksum_valid),
                    ..snapshot_for_save(cell, &sf, &resume)
                },
                None => SlotSnapshot::foreign(cell),
            }
        })
        .collect()
}

/// One frame of the save-commit ticker: advance the card I/O machine and,
/// on the save-commit beat, run the directory rebuild chain.
///
/// Retail's ticker advances `FUN_801E3294(chan, 0)` every frame while its
/// gate word allows (`_DAT_801F329C < 3` - pass `sm_gate`), latching any
/// non-zero result, and - when the commit phase word `_DAT_801F021C`
/// reads `3` and the rebuild request `_DAT_801F0224` is raised -
/// sequences the ported directory trio: fill the table
/// ([`card_directory_scan`]), cost it ([`card_free_blocks`]), classify it
/// (the [`classify_card_directory`] walk via [`card_directory_slots`]),
/// then clears the request.
///
/// PORT: FUN_801E1114 (see
/// `ghidra/scripts/funcs/overlay_menu_801e1114.txt`)
/// REF: FUN_801E13B8 / FUN_801E16E0 (the ticker's sibling per-frame calls,
/// ported in `crate::card_flow` - neither is a display-list emitter:
/// `FUN_801E13B8` is the write/format state machine and `FUN_801E16E0`
/// folds the poll result into the card-health counters)
/// REF: FUN_801E380C (the remaining sibling call, unported)
///
/// WIRED: [`crate::save_screen::SaveScreenFlow::before_tick`] calls this once
/// per frame for as long as a card-rack save screen is up, with the poll
/// status derived from the host's own block backend. The rebuild arm is the
/// half that stays unexercised: the port commits a save in one call, so no
/// commit phase is ever raised for the directory rebuild to key on, and the
/// flow passes `0` there and asserts nothing comes back.
#[allow(clippy::too_many_arguments)]
pub fn card_frame_tick(
    io: &mut CardIoMachine,
    status: CardStatus,
    poll_counter: &mut u16,
    sm_gate: bool,
    commit_phase: u32,
    rebuild_requested: &mut bool,
    entries: &[CardDirEntry],
) -> (i32, Option<CardIoEffect>, Option<Vec<SlotSnapshot>>) {
    let (result, effect) = if sm_gate {
        io.tick(status, poll_counter)
    } else {
        (io.result(), None)
    };

    let rebuilt = if commit_phase == 3 && *rebuild_requested {
        let (table, count) = card_directory_scan(entries);
        let free = card_free_blocks(&table, count);
        let frames: Vec<Vec<u8>> = table
            .iter()
            .take(count)
            .map(|e| {
                let mut frame = vec![0u8; CARD_DIRENTRY_STRIDE];
                frame[..CARD_DIRENTRY_NAME_LEN].copy_from_slice(&e.name);
                frame[CARD_DIRENTRY_SIZE_OFFSET..CARD_DIRENTRY_SIZE_OFFSET + 4]
                    .copy_from_slice(&e.size.to_le_bytes());
                frame
            })
            .collect();
        let frame_refs: Vec<&[u8]> = frames.iter().map(|f| f.as_slice()).collect();
        *rebuild_requested = false;
        Some(card_directory_slots(&frame_refs, free.max(0) as u32))
    } else {
        None
    };

    (result, effect, rebuilt)
}
