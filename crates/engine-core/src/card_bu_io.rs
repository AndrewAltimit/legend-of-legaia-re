//! Retail's memory-card **file I/O layer** - the `bu` device wrappers the
//! save screen calls beneath [`save_select`](crate::save_select).
//!
//! `save_select` already models the *screen*: the slot rack, the
//! "Now checking" beat and its per-frame event poll
//! ([`card_status_poll`](crate::save_select::card_status_poll)). This module
//! is the layer under it - the thin routines in the menu overlay that turn
//! "port 1, block 0, this filename" into a PSX BIOS `bu` device path and run
//! one `open`/`read`/`write`/`erase`/`format` against it, plus the two
//! kernel-event arrays those routines wait on.
//!
//! ## The two event arrays
//!
//! Retail keeps **two** four-handle `TestEvent` arrays and they are not
//! interchangeable:
//!
//! * Array **A** (`0x8007B9F0..0x8007B9FC`) is the *asynchronous* set. It is
//!   drained before each `read`/`write` is issued and polled once per frame
//!   afterwards - by `FUN_801E3900`
//!   ([`card_status_poll`](crate::save_select::card_status_poll), with a
//!   120-frame backstop) and by [`poll_events_a`], which is the same probe
//!   with no backstop and first-handle-wins ordering.
//! * Array **B** (`0x8007BA04..0x8007BA10`) is the *synchronous* set, used
//!   only by [`format_card`]: drain ([`drain_events_b`]), issue, then spin in
//!   [`wait_events_b`] until a handle fires.
//!
//! ## Host model
//!
//! Nothing here performs I/O. Each routine returns a [`CardOp`] describing
//! the BIOS call retail would issue, so a host backed by a card image (or by
//! the engine's disk saves) can service it and feed the completion back
//! through [`CardIoState`]. That keeps the port free of a PSX kernel while
//! preserving the retail ordering - which flag is cleared before the call,
//! which one the completion sets, and which state the step machine leaves
//! behind.
//!
//! Evidence: `ghidra/scripts/funcs/overlay_menu_801e37cc.txt`,
//! `overlay_menu_801e3a00.txt`, `overlay_menu_801e3a98.txt`,
//! `overlay_menu_801e3bec.txt`, `overlay_menu_801e3c90.txt`,
//! `overlay_menu_801e3d68.txt`, `overlay_menu_801e3e7c.txt`,
//! `overlay_menu_801e435c.txt`, `overlay_menu_801e0598.txt`,
//! `overlay_menu_801e380c.txt` (all in PROT entry 0899, the menu overlay).

/// Number of kernel-event handles in each of retail's two arrays.
pub const CARD_EVENTS: usize = 4;

/// Directory-name cache slots retail clears on a cold card session.
///
/// `FUN_801E0598` walks 32 records of `0x28` bytes down from
/// `0x801F32A8 + 0x4D8`, zeroing each record's first byte - i.e. emptying a
/// 32-entry name table, one entry per card block across both ports plus
/// slack. [`find_directory_name`] is the lookup over that same table.
pub const CARD_DIR_SLOTS: usize = 32;

/// Stride of one directory-name record (`addiu s1,s1,0x28`).
pub const CARD_DIR_STRIDE: usize = 0x28;

/// Bytes retail seeks past before a block read, when the read is flagged as
/// starting at the second frame (`lseek(fd, 0x200, 0)`).
pub const CARD_FRAME_BYTES: u32 = 0x200;

/// `open` flag bits retail passes to the BIOS `bu` driver.
///
/// The low bits are the access mode, `0x8000` is the driver's
/// non-blocking bit, `0x200` is create, and bits 16.. carry the block count
/// a create should allocate. Read uses `0x8001`, write `0x8002`, and the
/// create pass in [`write_file`] uses `0x1_0200` - one block, create.
pub mod open_flags {
    /// Non-blocking read (`0x8001`).
    pub const READ: u32 = 0x8001;
    /// Non-blocking write (`0x8002`).
    pub const WRITE: u32 = 0x8002;
    /// Create one block (`(1 << 16) | 0x200`).
    pub const CREATE_ONE_BLOCK: u32 = 0x1_0200;
}

/// Build the BIOS `bu` device path retail's `sprintf` produces.
///
/// The format has two single-digit fields - the controller port and the
/// card unit - then `:` then the filename. Callers that only address the
/// device (the [`format_card`] path) pass `None` for the name.
///
/// REF: FUN_801E37CC (the `sprintf` half; the erase half is [`erase_file`])
pub fn bu_path(port: u8, unit: u8, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("bu{}{}:{}", port % 10, unit % 10, n),
        None => format!("bu{}{}:", port % 10, unit % 10),
    }
}

/// One BIOS call the retail routine would issue, handed to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardOp {
    /// `open(path, flags)` then `read(fd, buf, len)`; `seek_second_frame`
    /// mirrors retail's conditional `lseek(fd, 0x200, 0)`.
    Read {
        path: String,
        flags: u32,
        len: u32,
        seek_second_frame: bool,
    },
    /// `open(path, flags)` then `write(fd, buf, len)`. When `create` is set
    /// retail first opens with [`open_flags::CREATE_ONE_BLOCK`] and closes
    /// that handle before reopening for write.
    Write {
        path: String,
        flags: u32,
        len: u32,
        create: bool,
    },
    /// `erase(path)` - BIOS `B(0x45)`, no completion event is waited on.
    Erase { path: String },
    /// `format(path)` - BIOS `B(0x41)`, followed by a blocking wait on
    /// event array B.
    Format { path: String },
}

/// Result of one card routine: the op to issue, or an immediate failure.
///
/// Retail returns `0` from [`read_file`] / [`write_file`] once the call is
/// issued and `-1` when the `open` failed outright; the failure arm also
/// latches the matching sticky flag in [`CardIoState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardIssue {
    /// The BIOS call to run. Retail's return value is `0`.
    Issued(CardOp),
    /// `open` returned `-1`. Retail's return value is `-1`.
    OpenFailed,
}

/// Outcome retail's blocking card wait resolves to.
///
/// [`wait_events_b`] returns the 1-based index of the handle that fired;
/// [`format_card_result`] is the mapping `FUN_801E3E7C` applies to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatResult {
    /// Handle 1 fired - success. Retail returns `1`.
    Ok,
    /// Handle 3 fired - no card. Retail returns `-1`.
    NoCard,
    /// Handle 2 or 4 fired - error or timeout. Retail returns `-3`.
    Error,
}

impl FormatResult {
    /// The integer retail actually returns.
    pub fn code(self) -> i32 {
        match self {
            FormatResult::Ok => 1,
            FormatResult::NoCard => -1,
            FormatResult::Error => -3,
        }
    }
}

/// Which transfer the step machine believes is in flight.
///
/// The discriminants are the values retail stores in `DAT_801F329C`, which
/// `FUN_801E380C` switches on. Only `4` and `6` are handled there; every
/// other value falls straight through, so the step is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardPhase {
    /// Nothing in flight (`0`, and every value the step ignores).
    #[default]
    Idle,
    /// A write is in flight (`4`).
    Writing,
    /// A read is in flight (`6`).
    Reading,
}

impl CardPhase {
    /// Lift retail's raw `DAT_801F329C` word.
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            4 => CardPhase::Writing,
            6 => CardPhase::Reading,
            _ => CardPhase::Idle,
        }
    }

    /// The raw word retail would hold for this phase.
    pub fn raw(self) -> u32 {
        match self {
            CardPhase::Idle => 0,
            CardPhase::Writing => 4,
            CardPhase::Reading => 6,
        }
    }
}

/// The mutable card-session state the overlay keeps in scattered globals.
///
/// Field-by-field provenance:
///
/// * `phase` - `DAT_801F329C`, the transfer-in-flight selector.
/// * `write_failed` - `DAT_801EF13C`, cleared when a write is issued
///   (`FUN_801E3D68`) and latched by a non-success write completion.
/// * `read_failed` - `DAT_801EF140`, cleared when a read is issued
///   (`FUN_801E3C90`) and latched by a non-success read completion.
/// * `dir_names` - the 32-slot `0x28`-stride name cache at `0x801F32A8`
///   that `FUN_801E0598` empties and `FUN_801E3BEC` searches.
/// * `fd_open` - retail keeps the raw descriptor in `DAT_801EF138`; the
///   port only needs to know whether the step machine still owes a `close`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardIoState {
    pub phase: CardPhase,
    pub write_failed: bool,
    pub read_failed: bool,
    pub dir_names: Vec<String>,
    pub fd_open: bool,
}

/// What one [`CardIoState::step`] resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStep {
    /// No transfer in flight, or no handle has fired yet.
    Waiting,
    /// The in-flight transfer finished; `ok` is retail's "handle 1 fired"
    /// arm. A `false` here is what latches the sticky failure flag.
    Finished { ok: bool },
}

impl CardIoState {
    /// Retail's cold card-session reset.
    ///
    /// `FUN_801E0598` zeroes the whole scattered global set, then - only
    /// when its argument is zero - empties the 32-slot directory-name
    /// cache. A non-zero argument is the "warm" path: the flags reset but
    /// the cached names survive, which is what lets the save screen
    /// re-enter without re-walking the card.
    ///
    /// The same routine re-points `DAT_801F32A0` at `0x80084140`, the
    /// save-block existence array the save screen scans; the port keeps
    /// that array in [`save_select`](crate::save_select) rather than
    /// behind a pointer, so there is nothing to re-point here.
    ///
    /// PORT: FUN_801E0598
    /// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
    pub fn reset(&mut self, keep_directory: bool) {
        self.phase = CardPhase::Idle;
        self.write_failed = false;
        self.read_failed = false;
        self.fd_open = false;
        if !keep_directory {
            self.dir_names.clear();
        }
    }

    /// Issue a card read.
    ///
    /// `FUN_801E3C90` clears the read-failure flag, formats the path,
    /// opens non-blocking for read and - if the caller's frame selector
    /// says so - seeks one 512-byte frame in before the `read`. The
    /// completion arrives later through [`Self::step`], not here.
    ///
    /// PORT: FUN_801E3C90
    /// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
    pub fn read_file(
        &mut self,
        port: u8,
        unit: u8,
        name: &str,
        len: u32,
        seek_second_frame: bool,
    ) -> CardIssue {
        self.read_failed = false;
        let path = bu_path(port, unit, Some(name));
        self.fd_open = true;
        self.phase = CardPhase::Reading;
        CardIssue::Issued(CardOp::Read {
            path,
            flags: open_flags::READ,
            len,
            seek_second_frame,
        })
    }

    /// Issue a card write.
    ///
    /// `FUN_801E3D68` clears the write-failure flag, and when the caller
    /// asks for a new file it first opens with the create flag (one block)
    /// and closes that handle before reopening for write. A failed create
    /// latches the failure flag and returns without issuing the write.
    ///
    /// PORT: FUN_801E3D68
    /// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
    pub fn write_file(
        &mut self,
        port: u8,
        unit: u8,
        name: &str,
        len: u32,
        create: bool,
    ) -> CardIssue {
        self.write_failed = false;
        let path = bu_path(port, unit, Some(name));
        self.fd_open = true;
        self.phase = CardPhase::Writing;
        CardIssue::Issued(CardOp::Write {
            path,
            flags: open_flags::WRITE,
            len,
            create,
        })
    }

    /// The `open` arm retail takes when the BIOS returns `-1`.
    ///
    /// Both `FUN_801E3C90` and `FUN_801E3D68` latch here, but only the
    /// write path sets a flag: the read path just returns `-1`.
    pub fn open_failed(&mut self, writing: bool) -> CardIssue {
        self.fd_open = false;
        self.phase = CardPhase::Idle;
        if writing {
            self.write_failed = true;
        }
        CardIssue::OpenFailed
    }

    /// One frame of the in-flight-transfer step machine.
    ///
    /// `FUN_801E380C` runs only for the two live phases. It polls event
    /// array A ([`poll_events_a`]); a zero means nothing fired and the
    /// phase stays. Otherwise it closes the descriptor, and unless handle
    /// 1 fired it latches the phase's sticky failure flag - `DAT_801EF13C`
    /// for a write, `DAT_801EF140` for a read (the read arm also prints).
    /// Either way the phase drops back to idle.
    ///
    /// PORT: FUN_801E380C
    /// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
    pub fn step(&mut self, events: [bool; CARD_EVENTS]) -> CardStep {
        let phase = self.phase;
        if phase == CardPhase::Idle {
            return CardStep::Waiting;
        }
        let fired = poll_events_a(events);
        if fired == 0 {
            return CardStep::Waiting;
        }
        self.fd_open = false;
        let ok = fired == 1;
        if !ok {
            match phase {
                CardPhase::Writing => self.write_failed = true,
                CardPhase::Reading => self.read_failed = true,
                CardPhase::Idle => {}
            }
        }
        self.phase = CardPhase::Idle;
        CardStep::Finished { ok }
    }

    /// Search the cached directory-name table for an exact name.
    ///
    /// `FUN_801E3BEC` walks `count` records of `0x28` bytes from
    /// `0x801F32A8`, `strcmp`ing each against the caller's name, and stops
    /// at the first equal one. `count` is the caller's, not the table's,
    /// so a short walk is normal.
    ///
    /// Note on the disassembly: the leading `printf` is reached with only
    /// `a0` loaded - `a1` still holds the incoming name, so the format's
    /// `%s` consumes it. The decompiled C drops that register argument,
    /// which is the artifact the repo's Ghidra notes warn about.
    ///
    /// PORT: FUN_801E3BEC
    /// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
    pub fn find_directory_name(&self, count: usize, name: &str) -> bool {
        self.dir_names.iter().take(count).any(|n| n == name)
    }
}

/// Poll event array A, first handle wins, no backstop.
///
/// `FUN_801E435C` `TestEvent`s the four `0x8007B9F0..FC` handles in order
/// and returns the 1-based index of the first that reports ready, or `0`.
/// It is the sibling of `FUN_801E3900`
/// ([`card_status_poll`](crate::save_select::card_status_poll)), which
/// probes the same four handles but lets a **later** handle overwrite an
/// earlier one and folds in the 120-frame timeout. The two disagree
/// whenever more than one handle fires in the same frame.
///
/// PORT: FUN_801E435C
/// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
pub fn poll_events_a(events: [bool; CARD_EVENTS]) -> u32 {
    for (i, fired) in events.iter().enumerate() {
        if *fired {
            return i as u32 + 1;
        }
    }
    0
}

/// Consume every pending event in array B.
///
/// `FUN_801E3A98` `TestEvent`s all four `0x8007BA04..0x8007BA10` handles
/// and discards the results - `TestEvent` clears the flag it reports, so
/// this is a drain, not a query. It is the array-B twin of `FUN_801E39A8`
/// ([`card_events_drain`](crate::save_select::card_events_drain)).
///
/// PORT: FUN_801E3A98
/// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
pub fn drain_events_b(events: &mut [bool; CARD_EVENTS]) {
    *events = [false; CARD_EVENTS];
}

/// Block until one of the array-B handles fires; return its 1-based index.
///
/// `FUN_801E3A00` spins the four handles in order and restarts the sweep
/// from handle 1 whenever handle 4 fails to report - it has no timeout and
/// no yield, so on hardware it is a busy-wait the card interrupt breaks.
/// The port takes the already-latched flags and returns `None` when the
/// host has nothing to report, leaving the caller to decide whether to
/// spin, so a host-side deadlock is a choice rather than a translation.
///
/// PORT: FUN_801E3A00
/// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
pub fn wait_events_b(events: [bool; CARD_EVENTS]) -> Option<u32> {
    let fired = poll_events_a(events);
    (fired != 0).then_some(fired)
}

/// Map a resolved array-B handle onto retail's `format` return value.
///
/// `FUN_801E3E7C` treats handle 1 as success, handle 3 as "no card" and
/// **both** handle 2 and handle 4 as the generic error - note that handle
/// 4 is the *completion* handle in the async poll's vocabulary, so the two
/// families do not share a result mapping.
pub fn format_card_result(handle: u32) -> FormatResult {
    match handle {
        1 => FormatResult::Ok,
        3 => FormatResult::NoCard,
        _ => FormatResult::Error,
    }
}

/// Issue a card format.
///
/// `FUN_801E3E7C` builds the device-only path, drains event array B,
/// calls the BIOS formatter and blocks in [`wait_events_b`]. The caller
/// resolves the handle through [`format_card_result`].
///
/// PORT: FUN_801E3E7C
/// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
pub fn format_card(port: u8, unit: u8, events: &mut [bool; CARD_EVENTS]) -> CardOp {
    drain_events_b(events);
    CardOp::Format {
        path: bu_path(port, unit, None),
    }
}

/// Delete a file from the card.
///
/// `FUN_801E37CC` formats the path and calls the BIOS erase. It waits on
/// nothing and returns nothing - the only routine in this family with no
/// completion beat at all.
///
/// PORT: FUN_801E37CC
/// NOT WIRED: no card-image backend sits behind the save-slot session; the engine's saves are disk-backed LGSF
pub fn erase_file(port: u8, unit: u8, name: &str) -> CardOp {
    CardOp::Erase {
        path: bu_path(port, unit, Some(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_carries_two_single_digit_fields() {
        assert_eq!(bu_path(0, 0, Some("SAVE")), "bu00:SAVE");
        assert_eq!(bu_path(1, 0, Some("SAVE")), "bu10:SAVE");
        assert_eq!(bu_path(1, 0, None), "bu10:");
    }

    // -- FUN_801E435C --------------------------------------------------

    #[test]
    fn poll_a_is_first_wins_unlike_the_backstop_poll() {
        assert_eq!(poll_events_a([false; 4]), 0);
        assert_eq!(poll_events_a([true, false, false, false]), 1);
        assert_eq!(poll_events_a([false, false, true, false]), 3);
        // The distinguishing case: FUN_801E3900 would answer 4 here
        // because each later handle overwrites; FUN_801E435C returns on
        // the first.
        assert_eq!(poll_events_a([true, true, true, true]), 1);
    }

    // -- FUN_801E3A00 / FUN_801E3A98 -----------------------------------

    #[test]
    fn wait_b_reports_nothing_until_a_handle_is_latched() {
        assert_eq!(wait_events_b([false; 4]), None);
        assert_eq!(wait_events_b([false, true, false, false]), Some(2));
    }

    #[test]
    fn drain_b_clears_every_handle() {
        let mut e = [true, false, true, true];
        drain_events_b(&mut e);
        assert_eq!(e, [false; 4]);
    }

    // -- FUN_801E3E7C --------------------------------------------------

    #[test]
    fn format_maps_handle_four_to_error_not_completion() {
        assert_eq!(format_card_result(1).code(), 1);
        assert_eq!(format_card_result(3).code(), -1);
        assert_eq!(format_card_result(2).code(), -3);
        assert_eq!(format_card_result(4).code(), -3);
    }

    #[test]
    fn format_drains_array_b_before_issuing() {
        let mut e = [true; 4];
        let op = format_card(1, 0, &mut e);
        assert_eq!(e, [false; 4]);
        assert_eq!(
            op,
            CardOp::Format {
                path: "bu10:".to_string()
            }
        );
    }

    // -- FUN_801E37CC --------------------------------------------------

    #[test]
    fn erase_takes_the_named_path() {
        assert_eq!(
            erase_file(0, 0, "BASLUS"),
            CardOp::Erase {
                path: "bu00:BASLUS".to_string()
            }
        );
    }

    // -- FUN_801E3C90 / FUN_801E3D68 / FUN_801E380C --------------------

    #[test]
    fn read_clears_its_own_flag_and_leaves_the_write_flag_alone() {
        let mut st = CardIoState {
            read_failed: true,
            write_failed: true,
            ..Default::default()
        };
        let issue = st.read_file(0, 0, "SAVE", 0x2000, true);
        assert!(!st.read_failed);
        assert!(st.write_failed, "read must not touch the write flag");
        assert_eq!(st.phase, CardPhase::Reading);
        assert_eq!(
            issue,
            CardIssue::Issued(CardOp::Read {
                path: "bu00:SAVE".to_string(),
                flags: open_flags::READ,
                len: 0x2000,
                seek_second_frame: true,
            })
        );
    }

    #[test]
    fn write_create_carries_the_one_block_create_flag() {
        let mut st = CardIoState::default();
        let issue = st.write_file(1, 0, "SAVE", 0x2000, true);
        assert_eq!(
            issue,
            CardIssue::Issued(CardOp::Write {
                path: "bu10:SAVE".to_string(),
                flags: open_flags::WRITE,
                len: 0x2000,
                create: true,
            })
        );
        assert_eq!(open_flags::CREATE_ONE_BLOCK, (1 << 16) | 0x200);
    }

    #[test]
    fn failed_open_latches_only_on_the_write_path() {
        let mut st = CardIoState::default();
        assert_eq!(st.open_failed(false), CardIssue::OpenFailed);
        assert!(!st.write_failed);
        assert_eq!(st.open_failed(true), CardIssue::OpenFailed);
        assert!(st.write_failed);
    }

    #[test]
    fn step_is_inert_outside_the_two_live_phases() {
        let mut st = CardIoState::default();
        assert_eq!(st.step([true; 4]), CardStep::Waiting);
        assert_eq!(st.phase, CardPhase::Idle);
    }

    #[test]
    fn step_holds_the_phase_until_a_handle_fires() {
        let mut st = CardIoState::default();
        st.read_file(0, 0, "SAVE", 0x200, false);
        assert_eq!(st.step([false; 4]), CardStep::Waiting);
        assert_eq!(st.phase, CardPhase::Reading);
        assert!(st.fd_open);
    }

    #[test]
    fn non_handle_one_completion_latches_the_phase_flag() {
        let mut st = CardIoState::default();
        st.write_file(0, 0, "SAVE", 0x200, false);
        assert_eq!(
            st.step([false, true, false, false]),
            CardStep::Finished { ok: false }
        );
        assert!(st.write_failed);
        assert!(!st.read_failed, "write completion must not flag the read");
        assert_eq!(st.phase, CardPhase::Idle);
        assert!(!st.fd_open, "the step closes the descriptor");

        let mut st = CardIoState::default();
        st.read_file(0, 0, "SAVE", 0x200, false);
        assert_eq!(
            st.step([false, false, false, true]),
            CardStep::Finished { ok: false }
        );
        assert!(st.read_failed);
        assert!(!st.write_failed);
    }

    #[test]
    fn handle_one_completion_is_clean() {
        let mut st = CardIoState::default();
        st.write_file(0, 0, "SAVE", 0x200, false);
        assert_eq!(st.step([true; 4]), CardStep::Finished { ok: true });
        assert!(!st.write_failed);
    }

    // -- FUN_801E0598 / FUN_801E3BEC -----------------------------------

    #[test]
    fn cold_reset_empties_the_name_cache_but_a_warm_one_does_not() {
        let mut st = CardIoState {
            phase: CardPhase::Reading,
            write_failed: true,
            read_failed: true,
            fd_open: true,
            dir_names: vec!["A".into(), "B".into()],
        };
        st.reset(true);
        assert_eq!(st.phase, CardPhase::Idle);
        assert!(!st.write_failed && !st.read_failed && !st.fd_open);
        assert_eq!(st.dir_names.len(), 2);
        st.reset(false);
        assert!(st.dir_names.is_empty());
    }

    #[test]
    fn directory_lookup_honours_the_callers_count() {
        let st = CardIoState {
            dir_names: vec!["A".into(), "B".into(), "C".into()],
            ..Default::default()
        };
        assert!(st.find_directory_name(3, "C"));
        assert!(!st.find_directory_name(2, "C"), "count bounds the walk");
        assert!(!st.find_directory_name(3, "D"));
    }

    #[test]
    fn directory_table_shape_matches_the_cleared_region() {
        // FUN_801E0598 steps 0x28 down from +0x4D8, 32 times.
        assert_eq!((CARD_DIR_SLOTS - 1) * CARD_DIR_STRIDE, 0x4D8);
    }
}
