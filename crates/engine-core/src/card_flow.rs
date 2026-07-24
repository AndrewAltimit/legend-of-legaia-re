//! The memory-card **write / format** flow that sits above the card I/O
//! machine: the three routines the per-frame ticker `FUN_801E1114` drives
//! over the state word `DAT_801F329C`.
//!
//! `save_select`'s [`CardIoMachine`](crate::save_select::CardIoMachine) is
//! the layer below - it polls kernel events and publishes a status code.
//! This module is what consumes that code:
//!
//! | Retail | Role | Port |
//! |---|---|---|
//! | `FUN_801E13B8` | write / format state machine over `DAT_801F329C` | [`CardWriteMachine`] |
//! | `FUN_801E16E0` | fold one poll result into the card-health counters | [`CardHealth::fold`] |
//! | `FUN_801E1934` | compose the `0x2000`-byte save block from live RAM | [`SaveBlockSummary`] |
//!
//! ## What the ticker keeps
//!
//! `FUN_801E1114` polls `FUN_801E3294` while `DAT_801F329C < 3` and stores
//! the code in two places (`0x801E1150..0x801E1164`): `DAT_801F3804` takes
//! it **unconditionally** (the store is in the branch-delay slot, so a `0`
//! "nothing happened" lands there too) and `DAT_801F3800` only when it is
//! non-zero. So `0x801F3804` is *this frame's* poll and `0x801F3800` is the
//! last meaningful one - [`CardWriteMachine::tick`] reads the former,
//! [`CardHealth::fold`] the latter.
//!
//! ## Naming the two counters
//!
//! `FUN_801E16E0` runs two saturating counters whose names come out of the
//! bytes rather than out of a guess:
//!
//! * `0x801F0218` is the **no-card fault counter**. Its increment arm ends
//!   in `printf("not card %d", n)` - and that argument is only visible in
//!   the disassembly: `a1` still holds the freshly incremented counter at
//!   the `jal`, and the decompiled C drops it (the dropped-register-argument
//!   artifact). `FUN_801E373C`, the card-subsystem init, seeds it to `1`,
//!   and `FUN_801E3294` clears it beside its sibling "not card count"
//!   `0x801F0214` the moment the card answers.
//! * `0x801F01BC` is the **card-changed debounce**. Only the `-2` result
//!   feeds it, and its effect (invalidating the cached directory) waits for
//!   two consecutive `-2`s - a single glitchy frame does not drop the card.
//!
//! Both saturate at `0x400`.
//!
//! PORT: FUN_801e13b8 - card write / format state machine
//! PORT: FUN_801e16e0 - poll-result fold + card-health counters
//! PORT: FUN_801e1934 - save-block composer
//! REF: FUN_801e1114 - the per-frame ticker that drives all three
//! REF: FUN_801e373c - card-subsystem init (seeds the fault counter to 1)
//!
//! Source: `ghidra/scripts/funcs/overlay_menu_801e13b8.txt`,
//! `overlay_menu_801e16e0.txt`, `overlay_menu_801e1934.txt`,
//! `overlay_menu_801e1114.txt`, `overlay_menu_801e373c.txt`.
//!
//! # NOT WIRED
//!
//! Same missing prerequisite as the rest of the card stack: the engine
//! saves to LGSF files (`legaia_save::SaveFile`), never to a `0x2000`-byte
//! card block, and no host owns a [`CardIoMachine`] to produce the poll
//! results these fold. `SaveSelectSession` runs its own `NowChecking` beat
//! straight off `card_status_poll`, so there is no frame on which to tick
//! [`CardWriteMachine`] and nothing that would consume a
//! [`SaveBlockSummary`].

/// Result code one card poll publishes, as `FUN_801E16E0` dispatches it.
///
/// The discriminants are the integers themselves: both jump tables in
/// `FUN_801E16E0` index `result + 3` over a six-entry table, so the domain
/// is exactly `-3..=2` and anything outside it falls through untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardPollResult {
    /// `-3` - the operation aborted or timed out. Resets the change
    /// debounce and invalidates the cached directory.
    Aborted,
    /// `-2` - the card looks different from the one last seen. Needs two
    /// consecutive frames before the directory is invalidated.
    Changed,
    /// `-1` - no card in the slot.
    NoCard,
    /// `0` - nothing happened this frame.
    #[default]
    Idle,
    /// `1` - the card is present and readable.
    Ready,
    /// `2` - readable, and the card has never been written by this game
    /// (retail raises a separate flag, [`CardHealth::unformatted`]).
    ReadyFresh,
}

impl CardPollResult {
    /// Decode the raw integer a poll returns. Values outside `-3..=2` land
    /// on [`CardPollResult::Idle`], matching the jump table's bounds check
    /// (`sltiu (result + 3), 6`) falling through to the default arm.
    pub fn from_code(code: i32) -> Self {
        match code {
            -3 => CardPollResult::Aborted,
            -2 => CardPollResult::Changed,
            -1 => CardPollResult::NoCard,
            1 => CardPollResult::Ready,
            2 => CardPollResult::ReadyFresh,
            _ => CardPollResult::Idle,
        }
    }

    /// The raw integer.
    pub fn code(self) -> i32 {
        match self {
            CardPollResult::Aborted => -3,
            CardPollResult::Changed => -2,
            CardPollResult::NoCard => -1,
            CardPollResult::Idle => 0,
            CardPollResult::Ready => 1,
            CardPollResult::ReadyFresh => 2,
        }
    }
}

/// Saturation ceiling both `FUN_801E16E0` counters clamp to.
pub const CARD_COUNTER_CEILING: i32 = 0x400;

/// Consecutive [`CardPollResult::Changed`] frames the debounce needs
/// before it invalidates the cached directory.
pub const CARD_CHANGE_DEBOUNCE: i32 = 2;

/// Phase value `FUN_801E16E0` publishes on a good card.
pub const CARD_PHASE_READY: u32 = 3;

/// The card-health block `FUN_801E16E0` maintains.
///
/// Field names carry their retail global so a capture can be diffed
/// against them directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardHealth {
    /// `_DAT_801F0218` - the no-card fault counter (`"not card %d"`).
    /// [`CardHealth::at_init`] seeds it to `1` the way `FUN_801E373C` does.
    pub fault_count: i32,
    /// `_DAT_801F01BC` - the card-changed debounce counter.
    pub change_count: i32,
    /// `_DAT_801F021C` - the card phase word. Cleared at the top of every
    /// fold and raised to [`CARD_PHASE_READY`] only by a good result.
    pub phase: u32,
    /// `_DAT_801F0224` - "rescan the directory" request, raised when a
    /// no-card run ends.
    pub rescan_requested: bool,
    /// `DAT_801EF100` - set once the card answers [`CardPollResult::ReadyFresh`].
    pub unformatted: bool,
    /// `DAT_801EF168 == -1` - the cached card contents are stale.
    pub directory_stale: bool,
    /// `_DAT_801F01EC` / `_DAT_801F01F0` / `_DAT_801F01E4` - the cached
    /// directory scan the fault tail clears.
    pub directory_cache_valid: bool,
}

impl Default for CardHealth {
    fn default() -> Self {
        Self {
            fault_count: 0,
            change_count: 0,
            phase: 0,
            rescan_requested: false,
            unformatted: false,
            directory_stale: false,
            directory_cache_valid: true,
        }
    }
}

impl CardHealth {
    /// The post-`FUN_801E373C` state: the card subsystem starts *faulted*
    /// (`_DAT_801F0218 = 1`) with a rescan pending, so the first good poll
    /// is what clears it rather than what confirms it.
    ///
    /// REF: FUN_801e373c
    pub fn at_init() -> Self {
        Self {
            fault_count: 1,
            rescan_requested: true,
            ..Self::default()
        }
    }

    /// Fold one poll result in.
    ///
    /// `sm_busy` is `_DAT_801F329C > 2`: while the write/format machine is
    /// past its polling states retail clears the phase word and returns
    /// without touching a counter.
    ///
    /// PORT: FUN_801e16e0
    pub fn fold(&mut self, result: CardPollResult, sm_busy: bool) {
        // `_DAT_801F021C = 0` runs before the busy test and again inside it.
        self.phase = 0;
        if sm_busy {
            return;
        }
        match result {
            CardPollResult::Ready | CardPollResult::ReadyFresh => {
                if result == CardPollResult::ReadyFresh {
                    self.unformatted = true;
                }
                self.phase = CARD_PHASE_READY;
                self.fault_count = 0;
                self.change_count = 0;
            }
            CardPollResult::NoCard => {
                self.fault_count = self.fault_count.wrapping_add(1);
                // Retail's `blez` arm - only reachable from a negative
                // counter - is the debug print; the ordinary path raises
                // the rescan request and invalidates the cache.
                if self.fault_count > 0 {
                    self.rescan_requested = true;
                    self.change_count = 0;
                    self.directory_stale = true;
                }
            }
            CardPollResult::Changed => {
                self.fault_count = 0;
                self.change_count = self.change_count.wrapping_add(1);
                if self.change_count >= CARD_CHANGE_DEBOUNCE {
                    self.directory_stale = true;
                }
            }
            CardPollResult::Aborted => {
                self.change_count = 0;
                self.directory_stale = true;
            }
            CardPollResult::Idle => {}
        }

        // Tail, run on every non-busy fold regardless of which arm ran.
        if self.fault_count > 0 {
            self.directory_cache_valid = false;
            self.directory_stale = true;
        }
        self.fault_count = self.fault_count.min(CARD_COUNTER_CEILING);
        self.change_count = self.change_count.min(CARD_COUNTER_CEILING);
    }
}

// ---------------------------------------------------------------------
// FUN_801E13B8 - write / format state machine
// ---------------------------------------------------------------------

/// Delay `FUN_801E13B8` arms before it opens the card for a write, in the
/// same units as the per-frame delta `DAT_1F800393`.
pub const CARD_WRITE_DELAY: i32 = 0x18;

/// Full block size handed to the write call.
pub const CARD_BLOCK_BYTES: usize = 0x2000;

/// Retries the format state spends on a busy card before giving up.
pub const CARD_FORMAT_RETRIES: u32 = 5;

/// Menu mode `FUN_801E13B8` requests when a **save** is interrupted by a
/// card change (`_DAT_801F0204 = 0x17`).
pub const CARD_MODE_SAVE_INTERRUPTED: u32 = 0x17;
/// Menu mode requested when a **load** is interrupted the same way.
pub const CARD_MODE_LOAD_INTERRUPTED: u32 = 0x13;

/// States of `DAT_801F329C`. Retail leaves `0`, `4` and `6` as terminal
/// parking states the machine itself never advances out of - the screen
/// SM does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardWriteState {
    /// `0` - idle / finished (success or failure both land here).
    #[default]
    Idle,
    /// `1` - a save was requested; waiting for the card to answer.
    SaveArmed,
    /// `2` - a load was requested; waiting for the card to answer.
    LoadArmed,
    /// `3` - counting [`CARD_WRITE_DELAY`] down, then writing.
    WriteDelay,
    /// `4` - the write completed.
    WriteDone,
    /// `5` - reading the block back.
    Reading,
    /// `6` - the read completed.
    ReadDone,
    /// `7` - formatting the card.
    Formatting,
    /// Any other value: the machine ignores it.
    Other(u8),
}

impl CardWriteState {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => CardWriteState::Idle,
            1 => CardWriteState::SaveArmed,
            2 => CardWriteState::LoadArmed,
            3 => CardWriteState::WriteDelay,
            4 => CardWriteState::WriteDone,
            5 => CardWriteState::Reading,
            6 => CardWriteState::ReadDone,
            7 => CardWriteState::Formatting,
            other => CardWriteState::Other(other),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            CardWriteState::Idle => 0,
            CardWriteState::SaveArmed => 1,
            CardWriteState::LoadArmed => 2,
            CardWriteState::WriteDelay => 3,
            CardWriteState::WriteDone => 4,
            CardWriteState::Reading => 5,
            CardWriteState::ReadDone => 6,
            CardWriteState::Formatting => 7,
            CardWriteState::Other(v) => v,
        }
    }
}

/// The BIOS `bu` operations `FUN_801E13B8` calls out to. The retail
/// implementations are the `bu` file-I/O layer (`FUN_801E3AF0` /
/// `FUN_801E3BA0` / `FUN_801E3BEC` / `FUN_801E3C90` / `FUN_801E3D68` /
/// `FUN_801E3E7C`); the machine is expressed against this trait so it can
/// be driven from a test double or from a disk backend.
pub trait CardWriteIo {
    /// `FUN_801E3AF0(handle, 0)` - open the card directory, returning the
    /// handle the block-count and find calls take.
    fn open_directory(&mut self) -> i32;
    /// `FUN_801E3BA0(handle, dir)` - free blocks on the card.
    fn free_blocks(&mut self, dir: i32) -> i32;
    /// `FUN_801E3BEC(dir, name)` - non-zero when the save file already
    /// exists (an overwrite rather than a create).
    fn find_file(&mut self, dir: i32) -> i32;
    /// `FUN_801E3D68(handle, 0, name, buf, 0x2000, create)` - write the
    /// block. Returns `0` on success.
    fn write_block(&mut self, create: bool) -> i32;
    /// `FUN_801E3C90(handle, 0, name, buf, slot)` - read the block back.
    /// Returns `0` on success.
    fn read_block(&mut self) -> i32;
    /// `FUN_801E3E7C(handle, 0)` - one format attempt. `-3` means busy and
    /// is retried up to [`CARD_FORMAT_RETRIES`] times.
    fn format(&mut self) -> i32;
    /// `FUN_801E1934()` - compose the block into the staging buffer. Run
    /// once, immediately before the write.
    fn compose_block(&mut self);
}

/// What one [`CardWriteMachine::tick`] asks the surrounding screen to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardWriteEvent {
    /// A card change interrupted the operation: raise the fault latch,
    /// switch to the given menu mode, and flag the save/load as failed.
    Interrupted { mode: u32, saving: bool },
    /// The write finished successfully (retail parks at state `4`).
    WriteOk,
    /// The write failed (retail prints `"write error"`).
    WriteFailed,
    /// The card has no free block and the file does not exist yet, so
    /// nothing was attempted.
    OutOfSpace,
    /// The read-back finished successfully (state `6`).
    ReadOk,
    /// The read-back failed.
    ReadFailed,
    /// Format succeeded.
    FormatOk,
    /// Format found no card.
    FormatNoCard,
    /// Format failed for any other reason.
    FormatFailed,
}

/// The write / format machine over `DAT_801F329C`.
///
/// PORT: FUN_801e13b8
#[derive(Debug, Clone, Default)]
pub struct CardWriteMachine {
    state: CardWriteState,
    /// `DAT_801EF128` - the pre-write delay countdown.
    delay: i32,
}

impl CardWriteMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current state.
    pub fn state(&self) -> CardWriteState {
        self.state
    }

    /// Arm a state directly (the screen SM's job in retail).
    pub fn set_state(&mut self, state: CardWriteState) {
        self.state = state;
    }

    /// Whether [`CardHealth::fold`] should skip this frame
    /// (`_DAT_801F329C > 2`).
    pub fn health_gated(&self) -> bool {
        self.state.code() > 2
    }

    /// Advance one frame. `poll` is `DAT_801F3804` (this frame's raw poll)
    /// and `frame_delta` is `DAT_1F800393`.
    ///
    /// PORT: FUN_801e13b8
    pub fn tick(
        &mut self,
        poll: CardPollResult,
        frame_delta: i32,
        io: &mut dyn CardWriteIo,
    ) -> Option<CardWriteEvent> {
        match self.state {
            CardWriteState::SaveArmed => self.armed(poll, CardWriteState::WriteDelay, true),
            CardWriteState::LoadArmed => self.armed(poll, CardWriteState::Reading, false),
            CardWriteState::WriteDelay => self.write_delay(frame_delta, io),
            CardWriteState::Reading => {
                if io.read_block() == 0 {
                    self.state = CardWriteState::ReadDone;
                    Some(CardWriteEvent::ReadOk)
                } else {
                    self.state = CardWriteState::Idle;
                    Some(CardWriteEvent::ReadFailed)
                }
            }
            CardWriteState::Formatting => self.formatting(io),
            _ => None,
        }
    }

    /// States 1 and 2: both watch the poll and differ only in the mode
    /// they request on an interrupt and the state they advance to.
    ///
    /// The guard chain is a hair-trigger worth spelling out: a `-2` is an
    /// interrupt, anything below `-2` or above `2` is ignored, `0` and
    /// `-1` are ignored, and only a strictly positive result advances.
    fn armed(
        &mut self,
        poll: CardPollResult,
        next: CardWriteState,
        saving: bool,
    ) -> Option<CardWriteEvent> {
        let code = poll.code();
        if code == -2 {
            return Some(CardWriteEvent::Interrupted {
                mode: if saving {
                    CARD_MODE_SAVE_INTERRUPTED
                } else {
                    CARD_MODE_LOAD_INTERRUPTED
                },
                saving,
            });
        }
        // `slt (-2)` / `slti 3` / `blez` in that order: out of `-2..=2`, or
        // not strictly positive, and the state is left alone.
        if !(-2..=2).contains(&code) || code <= 0 {
            return None;
        }
        if saving {
            self.delay = CARD_WRITE_DELAY;
        }
        self.state = next;
        None
    }

    /// State 3: burn the delay, then open, size, find, compose and write.
    fn write_delay(
        &mut self,
        frame_delta: i32,
        io: &mut dyn CardWriteIo,
    ) -> Option<CardWriteEvent> {
        let remaining = self.delay - frame_delta;
        if remaining > 0 {
            self.delay = remaining;
            return None;
        }
        self.delay = 0;

        let dir = io.open_directory();
        let free = io.free_blocks(dir);
        let found = io.find_file(dir);
        // `found != 0` -> overwrite; otherwise create, and a create needs a
        // free block. The create flag is set in the guard's delay slot, so
        // it is `1` on both sides of the branch - only the state differs.
        let create = found == 0;
        if create && free == 0 {
            self.state = CardWriteState::Idle;
            return Some(CardWriteEvent::OutOfSpace);
        }

        io.compose_block();
        if io.write_block(create) == 0 {
            self.state = CardWriteState::WriteDone;
            Some(CardWriteEvent::WriteOk)
        } else {
            self.state = CardWriteState::Idle;
            Some(CardWriteEvent::WriteFailed)
        }
    }

    /// State 7: retry a busy format up to [`CARD_FORMAT_RETRIES`] times,
    /// then classify. Only `1` is success.
    fn formatting(&mut self, io: &mut dyn CardWriteIo) -> Option<CardWriteEvent> {
        let mut result = io.format();
        let mut attempts = 0;
        while result == -3 && attempts + 1 < CARD_FORMAT_RETRIES {
            attempts += 1;
            result = io.format();
        }
        self.state = CardWriteState::Idle;
        Some(match result {
            -1 => CardWriteEvent::FormatNoCard,
            1 => CardWriteEvent::FormatOk,
            _ => CardWriteEvent::FormatFailed,
        })
    }
}

// ---------------------------------------------------------------------
// FUN_801E1934 - save-block composer
// ---------------------------------------------------------------------

/// PSX save-block header magic `FUN_801E1934` stamps at block `+0`:
/// `"SC"`, the icon-frame descriptor `0x11`, and the block count `1`.
pub const SAVE_HEADER_MAGIC: [u8; 4] = [b'S', b'C', 0x11, 0x01];

/// Low byte of the Shift-JIS full-width digit zero. The block title's two
/// slot digits are written as `SAVE_TITLE_DIGIT_BASE + digit`, which is
/// what makes them render as full-width numerals in the BIOS card browser.
pub const SAVE_TITLE_DIGIT_BASE: u8 = 0x4F;

/// Bytes of live game state copied into the block (`0x80084140 +
/// 0x1A18`). The trailing word at [`SAVE_BLOCK_CHECKSUM_OFFSET`] is
/// written separately.
pub const SAVE_PAYLOAD_BYTES: usize = 0x1A18;

/// Byte offset of the checksum word inside the block.
pub const SAVE_BLOCK_CHECKSUM_OFFSET: usize = 0x1FFC;

/// Per-member summary the save block carries so the slot info panel can be
/// drawn without loading the whole save.
///
/// Every field is copied straight off the character record; the offsets in
/// the comments are relative to the record base `0x80084708 + n*0x414`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SaveBlockMember {
    /// Party-roster entry (`0x80084598 + i`) - the record index.
    pub char_index: u8,
    /// Display name, record `+0x2A7`, 12 bytes verbatim.
    pub name: [u8; 12],
    /// Displayed level, record `+0x130`.
    pub level: u8,
    /// HP maximum, record `+0x104`.
    pub hp_max: u16,
    /// HP current, record `+0x106`.
    pub hp_cur: u16,
    /// MP maximum, record `+0x108`.
    pub mp_max: u16,
    /// MP current, record `+0x10A`.
    pub mp_cur: u16,
}

/// The block summary `FUN_801E1934` composes before the memcpy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SaveBlockSummary {
    /// `_DAT_801F0210` - the destination slot index, `0`-based.
    pub slot: u32,
    /// The two title digits, already biased by [`SAVE_TITLE_DIGIT_BASE`].
    pub title_digits: [u8; 2],
    /// `DAT_80084594` - live party size.
    pub party_count: u8,
    /// One entry per live party member, roster order.
    pub members: Vec<SaveBlockMember>,
    /// World position words at block `+0x428` / `+0x42C`, read as `i16`s
    /// from the field-state struct `+0x14` / `+0x18` and widened.
    pub world_pos: (i32, i32),
}

/// The two slot digits the block title carries.
///
/// The slot is displayed **1-based**, so slot `0` writes `"01"`. The
/// retail code reaches the tens digit through the `0x66666667` reciprocal
/// multiply, which is an exact `/10` for the range in play.
///
/// PORT: FUN_801e1934 (`0x801E1974..0x801E19EC`)
pub fn save_title_digits(slot: u32) -> [u8; 2] {
    let n = slot.wrapping_add(1);
    let tens = (n / 10) as u8;
    let ones = (n % 10) as u8;
    [
        SAVE_TITLE_DIGIT_BASE.wrapping_add(tens),
        SAVE_TITLE_DIGIT_BASE.wrapping_add(ones),
    ]
}

/// Compose the block summary for `slot` from the live party.
///
/// `members` is the roster in `0x80084598` order; the caller has already
/// resolved each entry's character record. This is the *data* half of
/// `FUN_801E1934` - the VRAM icon grab it does next (three `StoreImage`
/// strips at `(0x3C0 + slot*4, 0xE0)` into block `+0x80` / `+0x100` /
/// `+0x180`, plus the 16-entry CLUT row into `+0x60`) is a host job, and
/// so is the final `memcpy` + [`checksum`](crate::save_select::save_block_checksum).
///
/// PORT: FUN_801e1934
pub fn save_block_summary(
    slot: u32,
    members: &[SaveBlockMember],
    world_pos: (i32, i32),
) -> SaveBlockSummary {
    SaveBlockSummary {
        slot,
        title_digits: save_title_digits(slot),
        party_count: members.len() as u8,
        members: members.to_vec(),
        world_pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeIo {
        free: i32,
        found: i32,
        write_err: i32,
        read_err: i32,
        format_results: Vec<i32>,
        format_calls: usize,
        composed: usize,
        create_flag: Option<bool>,
    }

    impl CardWriteIo for FakeIo {
        fn open_directory(&mut self) -> i32 {
            7
        }
        fn free_blocks(&mut self, _dir: i32) -> i32 {
            self.free
        }
        fn find_file(&mut self, _dir: i32) -> i32 {
            self.found
        }
        fn write_block(&mut self, create: bool) -> i32 {
            self.create_flag = Some(create);
            self.write_err
        }
        fn read_block(&mut self) -> i32 {
            self.read_err
        }
        fn format(&mut self) -> i32 {
            let r = self
                .format_results
                .get(self.format_calls)
                .copied()
                .unwrap_or(1);
            self.format_calls += 1;
            r
        }
        fn compose_block(&mut self) {
            self.composed += 1;
        }
    }

    #[test]
    fn poll_codes_round_trip_and_clamp_out_of_range_to_idle() {
        for c in -3..=2 {
            assert_eq!(CardPollResult::from_code(c).code(), c);
        }
        assert_eq!(CardPollResult::from_code(-4), CardPollResult::Idle);
        assert_eq!(CardPollResult::from_code(9), CardPollResult::Idle);
    }

    #[test]
    fn card_subsystem_starts_faulted() {
        let h = CardHealth::at_init();
        assert_eq!(h.fault_count, 1);
        assert!(h.rescan_requested);
    }

    #[test]
    fn a_good_poll_clears_both_counters_and_raises_the_phase() {
        let mut h = CardHealth::at_init();
        h.change_count = 5;
        h.fold(CardPollResult::Ready, false);
        assert_eq!(h.fault_count, 0);
        assert_eq!(h.change_count, 0);
        assert_eq!(h.phase, CARD_PHASE_READY);
        assert!(!h.unformatted);
    }

    #[test]
    fn a_fresh_card_raises_the_unformatted_flag_and_still_reports_ready() {
        let mut h = CardHealth::default();
        h.fold(CardPollResult::ReadyFresh, false);
        assert!(h.unformatted);
        assert_eq!(h.phase, CARD_PHASE_READY);
    }

    #[test]
    fn a_missing_card_faults_immediately_and_invalidates_the_cache() {
        let mut h = CardHealth::default();
        h.fold(CardPollResult::NoCard, false);
        assert_eq!(h.fault_count, 1);
        assert!(h.rescan_requested);
        assert!(!h.directory_cache_valid);
        assert!(h.directory_stale);
    }

    #[test]
    fn one_changed_frame_is_not_enough_to_drop_the_directory() {
        let mut h = CardHealth::default();
        h.fold(CardPollResult::Changed, false);
        assert_eq!(h.change_count, 1);
        assert!(!h.directory_stale);
        h.fold(CardPollResult::Changed, false);
        assert_eq!(h.change_count, 2);
        assert!(h.directory_stale);
    }

    #[test]
    fn a_changed_frame_clears_the_fault_counter() {
        let mut h = CardHealth::at_init();
        h.fold(CardPollResult::Changed, false);
        assert_eq!(h.fault_count, 0);
    }

    #[test]
    fn counters_saturate_rather_than_run_away() {
        let mut h = CardHealth {
            fault_count: CARD_COUNTER_CEILING,
            ..Default::default()
        };
        h.fold(CardPollResult::NoCard, false);
        assert_eq!(h.fault_count, CARD_COUNTER_CEILING);

        let mut h = CardHealth {
            change_count: CARD_COUNTER_CEILING,
            ..Default::default()
        };
        h.fold(CardPollResult::Changed, false);
        assert_eq!(h.change_count, CARD_COUNTER_CEILING);
    }

    #[test]
    fn a_busy_write_machine_gates_the_fold_but_still_clears_the_phase() {
        let mut h = CardHealth {
            phase: CARD_PHASE_READY,
            ..Default::default()
        };
        h.fold(CardPollResult::NoCard, true);
        assert_eq!(h.phase, 0);
        assert_eq!(h.fault_count, 0);
    }

    #[test]
    fn health_gate_matches_the_state_word_test() {
        let mut m = CardWriteMachine::new();
        for state in [
            CardWriteState::Idle,
            CardWriteState::SaveArmed,
            CardWriteState::LoadArmed,
        ] {
            m.set_state(state);
            assert!(!m.health_gated());
        }
        for state in [CardWriteState::WriteDelay, CardWriteState::Formatting] {
            m.set_state(state);
            assert!(m.health_gated());
        }
    }

    #[test]
    fn an_armed_save_only_advances_on_a_strictly_positive_poll() {
        let mut io = FakeIo::default();
        for poll in [
            CardPollResult::Idle,
            CardPollResult::NoCard,
            CardPollResult::Aborted,
        ] {
            let mut m = CardWriteMachine::new();
            m.set_state(CardWriteState::SaveArmed);
            assert_eq!(m.tick(poll, 1, &mut io), None);
            assert_eq!(m.state(), CardWriteState::SaveArmed);
        }
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::SaveArmed);
        assert_eq!(m.tick(CardPollResult::Ready, 1, &mut io), None);
        assert_eq!(m.state(), CardWriteState::WriteDelay);
    }

    #[test]
    fn a_card_change_interrupts_with_the_mode_that_matches_the_direction() {
        let mut io = FakeIo::default();
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::SaveArmed);
        assert_eq!(
            m.tick(CardPollResult::Changed, 1, &mut io),
            Some(CardWriteEvent::Interrupted {
                mode: CARD_MODE_SAVE_INTERRUPTED,
                saving: true
            })
        );
        // Retail leaves the state alone - the screen SM owns the exit.
        assert_eq!(m.state(), CardWriteState::SaveArmed);

        m.set_state(CardWriteState::LoadArmed);
        assert_eq!(
            m.tick(CardPollResult::Changed, 1, &mut io),
            Some(CardWriteEvent::Interrupted {
                mode: CARD_MODE_LOAD_INTERRUPTED,
                saving: false
            })
        );
    }

    #[test]
    fn the_write_waits_out_the_delay_before_touching_the_card() {
        let mut io = FakeIo {
            free: 1,
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::SaveArmed);
        m.tick(CardPollResult::Ready, 1, &mut io);
        // 0x18 units at 8 per frame = three frames of waiting.
        assert_eq!(m.tick(CardPollResult::Idle, 8, &mut io), None);
        assert_eq!(m.tick(CardPollResult::Idle, 8, &mut io), None);
        assert_eq!(io.composed, 0);
        assert_eq!(
            m.tick(CardPollResult::Idle, 8, &mut io),
            Some(CardWriteEvent::WriteOk)
        );
        assert_eq!(io.composed, 1);
        assert_eq!(m.state(), CardWriteState::WriteDone);
    }

    #[test]
    fn an_existing_file_is_overwritten_and_needs_no_free_block() {
        let mut io = FakeIo {
            free: 0,
            found: 1,
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::WriteDelay);
        assert_eq!(
            m.tick(CardPollResult::Idle, 0x100, &mut io),
            Some(CardWriteEvent::WriteOk)
        );
        assert_eq!(io.create_flag, Some(false));
    }

    #[test]
    fn a_new_file_on_a_full_card_never_reaches_the_write() {
        let mut io = FakeIo {
            free: 0,
            found: 0,
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::WriteDelay);
        assert_eq!(
            m.tick(CardPollResult::Idle, 0x100, &mut io),
            Some(CardWriteEvent::OutOfSpace)
        );
        assert_eq!(io.composed, 0);
        assert_eq!(io.create_flag, None);
        assert_eq!(m.state(), CardWriteState::Idle);
    }

    #[test]
    fn a_failing_write_lands_back_on_idle() {
        let mut io = FakeIo {
            free: 1,
            write_err: -1,
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::WriteDelay);
        assert_eq!(
            m.tick(CardPollResult::Idle, 0x100, &mut io),
            Some(CardWriteEvent::WriteFailed)
        );
        assert_eq!(m.state(), CardWriteState::Idle);
    }

    #[test]
    fn the_read_state_classifies_both_outcomes() {
        let mut io = FakeIo::default();
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::Reading);
        assert_eq!(
            m.tick(CardPollResult::Idle, 1, &mut io),
            Some(CardWriteEvent::ReadOk)
        );
        assert_eq!(m.state(), CardWriteState::ReadDone);

        io.read_err = -1;
        m.set_state(CardWriteState::Reading);
        assert_eq!(
            m.tick(CardPollResult::Idle, 1, &mut io),
            Some(CardWriteEvent::ReadFailed)
        );
    }

    #[test]
    fn format_retries_a_busy_card_five_times_then_reports_the_last_code() {
        let mut io = FakeIo {
            format_results: vec![-3; 8],
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::Formatting);
        assert_eq!(
            m.tick(CardPollResult::Idle, 1, &mut io),
            Some(CardWriteEvent::FormatFailed)
        );
        assert_eq!(io.format_calls, CARD_FORMAT_RETRIES as usize);
    }

    #[test]
    fn format_classifies_no_card_and_success() {
        let mut io = FakeIo {
            format_results: vec![-3, -1],
            ..Default::default()
        };
        let mut m = CardWriteMachine::new();
        m.set_state(CardWriteState::Formatting);
        assert_eq!(
            m.tick(CardPollResult::Idle, 1, &mut io),
            Some(CardWriteEvent::FormatNoCard)
        );

        let mut io = FakeIo {
            format_results: vec![1],
            ..Default::default()
        };
        m.set_state(CardWriteState::Formatting);
        assert_eq!(
            m.tick(CardPollResult::Idle, 1, &mut io),
            Some(CardWriteEvent::FormatOk)
        );
    }

    #[test]
    fn title_digits_are_one_based_and_biased_to_full_width() {
        assert_eq!(save_title_digits(0), [0x4F, 0x50]);
        assert_eq!(save_title_digits(8), [0x4F, 0x58]);
        assert_eq!(save_title_digits(9), [0x50, 0x4F]);
        assert_eq!(save_title_digits(14), [0x50, 0x54]);
    }

    #[test]
    fn block_summary_carries_the_roster_verbatim() {
        let m = SaveBlockMember {
            char_index: 2,
            name: *b"GALA\0\0\0\0\0\0\0\0",
            level: 12,
            hp_max: 210,
            hp_cur: 180,
            mp_max: 40,
            mp_cur: 33,
        };
        let s = save_block_summary(3, &[m], (0x100, -0x40));
        assert_eq!(s.slot, 3);
        assert_eq!(s.party_count, 1);
        assert_eq!(s.title_digits, [0x4F, 0x53]);
        assert_eq!(s.members[0], m);
        assert_eq!(s.world_pos, (0x100, -0x40));
    }
}
