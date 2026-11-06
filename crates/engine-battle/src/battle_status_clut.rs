//! Battle **status CLUT recolour** - the fourth pass of the per-frame actor
//! maintenance sweep `FUN_8004CE2C`.
//!
//! PORT: FUN_8004CE2C (the Stone and Rot arms of the status CLUT pass)
//! REF: FUN_800583C8 - `LoadImage`; the 240x1 rect the staged row uploads through.
//! REF: FUN_80053B9C - the battle loader's CLUT upload that seeds row `481 + slot`.
//!
//! Straight off the disassembly (`ghidra/scripts/funcs/8004ce2c.txt`,
//! `0x8004D6A8..0x8004D9B8`), the pass walks the actor pointer table
//! `DAT_801C9370` and, per actor:
//!
//! - `lhu v0,0x16e(s0) ; andi v0,v0,0x4` - the Stone bit of the status
//!   halfword. Zero skips the arm entirely.
//! - `lbu v0,0x220(s0)` - the **latch**. Zero skips: the recolour is armed
//!   once per affliction by the applier, not re-run every frame.
//! - `sb zero,0x220(s0)` - the latch is cleared by the pass that fires.
//! - the source is the battle context's own per-actor palette copy at
//!   `ctx[+0x894 + slot*0x1E0]` (`lhu v1,0x894(v0)`, and the loop tail
//!   `addiu s6,s6,0x1e0`) - 240 `u16` entries per slot. `0xE34 - 0x894 =
//!   0x5A0 = 3 * 0x1E0`, so the window holds exactly **three** slots: the
//!   recolour is party-only, and the staging buffer sits immediately after it.
//! - each entry is desaturated (the [`bgr555_to_grey`] kernel) into the shared
//!   staging buffer `ctx[+0xE34]`, `a1` counting to `0xF0` = 240.
//! - the upload packet is `{x: 0, y: s3 + 0x1E1, w: 0xF0, h: 1}`
//!   (`0x8004D77C..0x8004D790`) handed to `FUN_800583C8` - i.e. a 240x1
//!   `LoadImage` onto VRAM CLUT row **`481 + slot`**, the same row the battle
//!   loader wrote the assembled party palette to.
//!
//! It is a *palette* recolour, not a framebuffer read and not a per-frame
//! damage flash.
//!
//! # Why the pristine copy can be snapshotted from VRAM
//!
//! Retail's source is `ctx[+0x894]`, a copy the engine does not have. This
//! port snapshots VRAM row `481 + slot` instead, the first time a slot
//! recolours. That is exact rather than approximate: the write is the same
//! 240-entry window the read covers, so the two hold the same colours - and
//! the one place they could differ is bit 15 (the loader's `FUN_80053B9C`
//! STP-sets every non-zero colour on the way to VRAM), which
//! [`bgr555_to_grey`] masks off (`andi 0x1f` / `0x3e0` / `0x7c00`). The grey
//! is therefore identical whichever form the source is in.
//!
//! Keeping the copy is what makes the recolour idempotent, exactly as in
//! retail: the pass writes only the staging buffer, never `ctx[+0x894]`, so a
//! second fire re-greys the *original* colours instead of compounding.
//!
//! # The Rot arm
//!
//! The sibling arm on status bits `0x08`/`0x10`/`0x20` (Rot, one bit per
//! limb, `0x8004D7A8..0x8004D98C`) runs after the Stone arm, once per limb
//! `n = 0..3`:
//!
//! - `lhu v1,0x16e(s0) ; sllv v0,0x8,s2` - limb `n`'s bit; clear skips.
//! - `lbu v0,0x221(v1)` with `v1 = actor + n` - the limb's own latch at
//!   `actor[+0x221 + n]`, cleared by the pass that fires (`sb zero`).
//! - the window is the byte pair `[lo, hi)` at `DAT_80078630 + (id - 1) * 6 +
//!   n * 2` ([`RotClutWindows`]), `id` the seat's 1-based character id
//!   `DAT_8007BD10[slot]`.
//! - each entry `lo..hi` of the same pristine copy takes the blue ramp
//!   [`bgr555_to_rot_blue`] into the staging buffer, and a `(hi - lo) x 1`
//!   rect at `(lo, 481 + slot)` uploads just that span.
//!
//! So a rotted limb's palette span turns blue and the rest of the row keeps
//! whatever it last held, which is how a stoned-then-rotted seat ends up grey
//! with one blue limb. The windows split the party's 240-entry rows at the
//! 16-entry CLUT boundaries of the limb textures.

use legaia_engine_battle_vm::bgr555_grey::{bgr555_to_grey, bgr555_to_rot_blue};

/// First VRAM CLUT row of the battle party palettes - retail's
/// `addiu v0,s3,0x1e1` (`0x8004D77C`), the same `481 + ordinal` the
/// registration-time `relocate_tsb_cba` pass targets.
pub const PARTY_CLUT_ROW_BASE: u16 = 481;

/// Entries in one party CLUT row: retail's `sltiu v0,a1,0xf0` loop bound and
/// the `0x1E0`-byte per-slot stride of `ctx[+0x894]`.
pub const PARTY_CLUT_ENTRIES: usize = 240;

/// Party slots the recolour covers. `ctx[+0xE34] - ctx[+0x894] = 3 * 0x1E0`,
/// so the retail palette window is exactly three actors wide.
pub const PARTY_CLUT_SLOTS: usize = 3;

/// VA of the per-character Rot CLUT window table in `SCUS_942.54`
/// (`addiu s7,v0,-0x79d0` at `0x8004D698`).
pub const ROT_CLUT_WINDOW_VA: u32 = 0x8007_8630;
/// Bytes per character row: three `[lo, hi)` byte pairs, one per limb bit.
pub const ROT_CLUT_ROW_LEN: usize = 6;
/// Rows the port reads: roster ids `1..=4`, the same span as the draw's
/// limb table (`legaia_engine_battle_vm::battle_actor_draw::RotLimbTable`).
pub const ROT_CLUT_ROWS: usize = 4;
/// Status bits (`+0x16E`) of the three Rot limbs, in window-pair order
/// (`li v0,0x8 ; sllv v0,v0,s2` at `0x8004D7AC`).
pub const ROT_CLUT_LIMB_BITS: [u16; 3] = [0x0008, 0x0010, 0x0020];

/// The per-character palette windows the Rot arm tints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RotClutWindows {
    /// `rows[id - 1]` for roster id `id`.
    pub rows: [[u8; ROT_CLUT_ROW_LEN]; ROT_CLUT_ROWS],
}

impl RotClutWindows {
    /// Decode the table from a `SCUS_942.54` image. `None` when the image is
    /// not a PSX-EXE or is too short to hold the rows.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let off = legaia_game_tables::new_game::scus_file_offset(scus, ROT_CLUT_WINDOW_VA)?;
        let bytes = scus.get(off..off + ROT_CLUT_ROW_LEN * ROT_CLUT_ROWS)?;
        let mut rows = [[0u8; ROT_CLUT_ROW_LEN]; ROT_CLUT_ROWS];
        for (row, chunk) in rows.iter_mut().zip(bytes.as_chunks::<ROT_CLUT_ROW_LEN>().0) {
            *row = *chunk;
        }
        Some(Self { rows })
    }

    /// The row for a 1-based roster id, `None` outside `1..=4` (retail
    /// indexes `id - 1` unchecked).
    pub fn row(&self, roster_id: u8) -> Option<[u8; ROT_CLUT_ROW_LEN]> {
        self.rows
            .get(usize::from(roster_id).checked_sub(1)?)
            .copied()
    }
}

/// The engine's stand-in for the three things retail's pass reads that the
/// port's battle context has never carried: the per-actor palette copy
/// (`ctx[+0x894 + slot*0x1E0]`), the per-affliction latch (`actor[+0x220]`),
/// and the staged row the upload comes from (`ctx[+0xE34]`).
///
/// Lives on `crate::battle_hud_state::BattleHud` because that is the one
/// per-frame battle struct both hosts own; [`Self::arm`] is driven from
/// `crate::battle_hud_state::BattleHud::sync_status`, which every host already
/// calls once per slot per frame.
#[derive(Debug, Clone, Default)]
pub struct StatusClutState {
    /// The pristine palette per party slot - retail `ctx[+0x894 + slot*0x1E0]`.
    /// Snapshotted lazily off VRAM row `481 + slot` (see the module note).
    pristine: [Option<Vec<u16>>; PARTY_CLUT_SLOTS],
    /// `actor[+0x220]`: set on the affliction edge, cleared by the pass that
    /// fires it.
    latch: [bool; PARTY_CLUT_SLOTS],
    /// Last-seen Stone bit, the edge detector standing in for retail's
    /// applier-side latch write. A cure followed by a re-petrify re-arms,
    /// which is what the retail applier does too.
    seen: [bool; PARTY_CLUT_SLOTS],
    /// `actor[+0x221..=+0x223]`: one latch per Rot limb.
    rot_latch: [[bool; 3]; PARTY_CLUT_SLOTS],
    /// Last-seen Rot limb bits (`+0x16E & 0x38`), the edge detector.
    rot_seen: [u16; PARTY_CLUT_SLOTS],
    /// Each slot's [`RotClutWindows`] row, set by the host from the seat's
    /// character id ([`Self::set_rot_windows`]).
    windows: [Option<[u8; ROT_CLUT_ROW_LEN]>; PARTY_CLUT_SLOTS],
    /// Last-seen `+0x16E & 0x3C` (Stone and the Rot limbs), the cure detector
    /// of [`Self::sync_word`].
    marked: [u16; PARTY_CLUT_SLOTS],
    /// A cure took a mark off: the next [`Self::step`] puts the pristine row
    /// back, as `FUN_8005133C` does.
    restore: [bool; PARTY_CLUT_SLOTS],
}

/// The status bits whose recolour a cure undoes: Stone (`0x04`) and the three
/// Rot limbs (`0x38`) - the `andi v0,v0,0x3c` gate ahead of each
/// `jal 0x8005133c` in the applier (`0x80040E9C`, `0x80040FC0`).
pub const CLUT_MARK_BITS: u16 = 0x003C;

impl StatusClutState {
    /// Fold this frame's Stone bit for `slot` (actor `+0x16E & 0x0004`) into
    /// the latch. The rising edge arms it; a steady-state affliction does
    /// not re-arm, which is what keeps the recolour "once per affliction"
    /// rather than per frame.
    pub fn arm(&mut self, slot: u8, stoned: bool) {
        let i = slot as usize;
        if i >= PARTY_CLUT_SLOTS {
            return;
        }
        if stoned && !self.seen[i] {
            self.latch[i] = true;
        }
        self.seen[i] = stoned;
    }

    /// Fold this frame's status word for `slot` into the three Rot limb
    /// latches. Each limb bit's rising edge arms its own latch, as the
    /// applier's per-limb latch write does; a limb already rotted does not
    /// re-arm.
    pub fn arm_rot(&mut self, slot: u8, status: u16) {
        let i = slot as usize;
        if i >= PARTY_CLUT_SLOTS {
            return;
        }
        for (n, &bit) in ROT_CLUT_LIMB_BITS.iter().enumerate() {
            if status & bit != 0 && self.rot_seen[i] & bit == 0 {
                self.rot_latch[i][n] = true;
            }
        }
        self.rot_seen[i] = status & ROT_CLUT_LIMB_BITS.iter().fold(0, |m, b| m | b);
    }

    /// Fold this frame's whole `+0x16E` word for `slot`: the Stone and Rot
    /// latches ([`Self::arm`], [`Self::arm_rot`]) and the cure that undoes
    /// them. A cure in retail (the applier's classes, Vera, Orb, Spoon, Horn,
    /// the fatal-decision module) runs `FUN_8005133C` when the word carried
    /// any of [`CLUT_MARK_BITS`]: it uploads the pristine palette back over
    /// row `481 + slot` and sets all four latches `+0x220..+0x223` to `1`, so
    /// any mark the cure left re-fires on the next pass. Here a mark leaving
    /// the word is that cure.
    ///
    /// PORT: FUN_8005133C (the palette restore and the latch re-arm)
    pub fn sync_word(&mut self, slot: u8, word: u16) {
        let i = slot as usize;
        if i >= PARTY_CLUT_SLOTS {
            return;
        }
        let marked = word & CLUT_MARK_BITS;
        if self.marked[i] & !marked != 0 {
            self.restore[i] = true;
            self.seen[i] = false;
            self.rot_seen[i] = 0;
        }
        self.marked[i] = marked;
        self.arm(slot, word & 0x0004 != 0);
        self.arm_rot(slot, word);
    }

    /// Seat the Rot windows the next [`Self::step`] tints over, one row per
    /// party slot (`None` for an empty seat or a host without the table).
    pub fn set_rot_windows(&mut self, windows: [Option<[u8; ROT_CLUT_ROW_LEN]>; PARTY_CLUT_SLOTS]) {
        self.windows = windows;
    }

    /// Whether any slot's latch is waiting for a [`Self::step`].
    pub fn armed(&self) -> bool {
        self.latch.iter().any(|&l| l)
            || self.rot_latch.iter().flatten().any(|&l| l)
            || self.restore.iter().any(|&r| r)
    }

    /// The pristine palette for `slot`, snapshotted off VRAM on first use.
    /// `None` while the row is still blank (not resident yet).
    fn pristine_row(&mut self, vram: &legaia_tim::Vram, slot: usize) -> Option<&Vec<u16>> {
        if self.pristine[slot].is_none() {
            let row = PARTY_CLUT_ROW_BASE + slot as u16;
            let words: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
                .map(|x| vram.pixel(x, row as usize))
                .collect();
            if words.iter().all(|&w| w == 0) {
                return None;
            }
            self.pristine[slot] = Some(words);
        }
        self.pristine[slot].as_ref()
    }

    /// Drop the latches, the edge state and the palette copies - the battle
    /// boundary, where retail's context (and its `+0x220` bytes) are rebuilt.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Run the pass against the host's battle VRAM: for every latched slot,
    /// stage the grey copy of its pristine palette and `LoadImage` it back
    /// over CLUT row `481 + slot`. Returns `true` when VRAM changed, so the
    /// caller knows to re-upload.
    ///
    /// One deliberate divergence: retail clears `+0x220` unconditionally,
    /// because its palette copy is always populated by the time the pass
    /// runs. Here the copy is taken from VRAM, so a latch that fires before
    /// the loader has written the row would snapshot blank. An all-zero row
    /// therefore leaves the latch set and retries next frame instead of
    /// caching a blank pristine.
    pub fn step(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let mut wrote = false;
        for slot in 0..PARTY_CLUT_SLOTS {
            let row = PARTY_CLUT_ROW_BASE + slot as u16;
            // A cure's `FUN_8005133C`: the pristine row back. With no copy
            // taken, nothing was recoloured and there is nothing to undo.
            if self.restore[slot] {
                self.restore[slot] = false;
                if let Some(src) = self.pristine[slot].as_ref() {
                    let staged: Vec<u8> = src.iter().flat_map(|&c| c.to_le_bytes()).collect();
                    vram.write_clut_row(0, row, &staged);
                    wrote = true;
                }
            }
            // The Stone arm (`0x8004D6C0..0x8004D79C`): the whole row grey.
            if self.latch[slot]
                && let Some(src) = self.pristine_row(vram, slot)
            {
                let staged: Vec<u8> = src
                    .iter()
                    .flat_map(|&c| bgr555_to_grey(c).to_le_bytes())
                    .collect();
                vram.write_clut_row(0, row, &staged);
                // retail `sb zero,0x220(s0)` - the latch is spent.
                self.latch[slot] = false;
                wrote = true;
            }
            // (A latched slot whose palette is not resident yet holds its
            // latch for the next frame.)

            // The Rot arm (`0x8004D7A8..0x8004D98C`), limb by limb after
            // the Stone arm: the limb's window of the pristine copy, blue.
            for limb in 0..3 {
                if !self.rot_latch[slot][limb] {
                    continue;
                }
                let Some(w) = self.windows[slot] else {
                    // No character row for the seat: the window is unknown,
                    // so the latch waits rather than tinting a guessed span.
                    continue;
                };
                let lo = usize::from(w[limb * 2]).min(PARTY_CLUT_ENTRIES);
                let hi = usize::from(w[limb * 2 + 1]).min(PARTY_CLUT_ENTRIES);
                let Some(src) = self.pristine_row(vram, slot) else {
                    continue;
                };
                // `sb zero,0x221(v1)` - spent whether or not the span is
                // empty; an empty `[lo, hi)` uploads a zero-width rect.
                if lo < hi {
                    let staged: Vec<u8> = src[lo..hi]
                        .iter()
                        .flat_map(|&c| bgr555_to_rot_blue(c).to_le_bytes())
                        .collect();
                    vram.write_clut_row(lo as u16, row, &staged);
                    wrote = true;
                }
                self.rot_latch[slot][limb] = false;
            }
        }
        wrote
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A palette with a distinct, non-grey colour per entry.
    fn seed_row(vram: &mut legaia_tim::Vram, row: u16) -> Vec<u16> {
        let words: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
            .map(|i| {
                let r = (i % 31 + 1) as u16;
                let g = ((i / 2) % 31 + 1) as u16;
                let b = ((i / 3) % 31 + 1) as u16;
                0x8000 | r | (g << 5) | (b << 10)
            })
            .collect();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        vram.write_clut_row(0, row, &bytes);
        words
    }

    fn read_row(vram: &legaia_tim::Vram, row: u16) -> Vec<u16> {
        (0..PARTY_CLUT_ENTRIES)
            .map(|x| vram.pixel(x, row as usize))
            .collect()
    }

    #[test]
    fn stone_edge_greys_the_slot_row_once() {
        let mut vram = legaia_tim::Vram::new();
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE + 1);
        let mut st = StatusClutState::default();

        // Not afflicted: nothing latches, nothing writes.
        st.arm(1, false);
        assert!(!st.armed());
        assert!(!st.step(&mut vram), "no affliction -> no VRAM write");
        assert_eq!(read_row(&vram, PARTY_CLUT_ROW_BASE + 1), base);

        // Rising edge arms; the pass greys the whole 240-entry row.
        st.arm(1, true);
        assert!(st.armed());
        assert!(st.step(&mut vram));
        let greyed = read_row(&vram, PARTY_CLUT_ROW_BASE + 1);
        let expect: Vec<u16> = base.iter().map(|&c| bgr555_to_grey(c)).collect();
        assert_eq!(greyed, expect);
        for w in &greyed {
            let (r, g, b) = (w & 0x1F, (w >> 5) & 0x1F, (w >> 10) & 0x1F);
            assert_eq!((r, g), (r, b), "grey writes one luminance to all three");
        }

        // Latch spent: a steady-state affliction does not re-run the pass.
        st.arm(1, true);
        assert!(!st.armed());
        assert!(!st.step(&mut vram), "the +0x220 latch fires once");
    }

    #[test]
    fn a_cure_puts_the_pristine_row_back_and_re_arms() {
        let mut vram = legaia_tim::Vram::new();
        let row = PARTY_CLUT_ROW_BASE;
        let original = seed_row(&mut vram, row);
        let mut st = StatusClutState::default();
        // Stoned with a rotted limb: grey, then the limb's window blue.
        st.set_rot_windows([Some([0, 16, 16, 32, 32, 48]), None, None]);
        st.sync_word(0, 0x0014);
        assert!(st.step(&mut vram));
        assert_ne!(read_row(&vram, row), original);
        // A cure that leaves the Rot: the row comes back, then the limb
        // re-tints off the pristine copy, as the re-armed latch does.
        st.sync_word(0, 0x0010);
        assert!(st.armed());
        assert!(st.step(&mut vram));
        let after = read_row(&vram, row);
        assert_eq!(after[0..16], original[0..16]);
        assert_eq!(after[32..], original[32..]);
        assert_ne!(after[16..32], original[16..32]);
        // The whole cure: everything back.
        st.sync_word(0, 0);
        assert!(st.step(&mut vram));
        assert_eq!(read_row(&vram, row), original);
        assert!(!st.armed());
    }

    #[test]
    fn a_second_fire_re_greys_the_original_not_the_grey() {
        let mut vram = legaia_tim::Vram::new();
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE);
        let mut st = StatusClutState::default();
        st.arm(0, true);
        assert!(st.step(&mut vram));
        let first = read_row(&vram, PARTY_CLUT_ROW_BASE);
        // Cure, re-petrify: retail's applier re-arms +0x220.
        st.arm(0, false);
        st.arm(0, true);
        assert!(st.step(&mut vram));
        assert_eq!(
            read_row(&vram, PARTY_CLUT_ROW_BASE),
            first,
            "the pristine copy is the source, so grey does not compound"
        );
        assert_ne!(first, base);
    }

    #[test]
    fn a_blank_row_holds_the_latch_instead_of_caching_blank() {
        let mut vram = legaia_tim::Vram::new();
        let mut st = StatusClutState::default();
        st.arm(2, true);
        assert!(!st.step(&mut vram), "nothing resident -> nothing written");
        assert!(st.armed(), "latch survives so the next frame retries");
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE + 2);
        assert!(st.step(&mut vram));
        let expect: Vec<u16> = base.iter().map(|&c| bgr555_to_grey(c)).collect();
        assert_eq!(read_row(&vram, PARTY_CLUT_ROW_BASE + 2), expect);
    }

    const WINDOWS: [u8; ROT_CLUT_ROW_LEN] = [0xD0, 0xF0, 0xB0, 0xD0, 0x70, 0x90];

    #[test]
    fn a_rot_limb_tints_only_its_window_blue_once() {
        let mut vram = legaia_tim::Vram::new();
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE);
        let mut st = StatusClutState::default();
        st.set_rot_windows([Some(WINDOWS), None, None]);

        // Limb 1 (`0x10`) rots: entries 0xB0..0xD0 turn blue, the rest stay.
        st.arm_rot(0, 0x0010);
        assert!(st.armed());
        assert!(st.step(&mut vram));
        let row = read_row(&vram, PARTY_CLUT_ROW_BASE);
        for (i, (&got, &was)) in row.iter().zip(&base).enumerate() {
            if (0xB0..0xD0).contains(&i) {
                assert_eq!(got, bgr555_to_rot_blue(was), "entry {i:#x}");
            } else {
                assert_eq!(got, was, "entry {i:#x} outside the window");
            }
        }
        // The latch is spent; a steady Rot does not re-run.
        st.arm_rot(0, 0x0010);
        assert!(!st.armed());

        // A second limb arms on its own edge and leaves the first in place.
        st.arm_rot(0, 0x0030);
        assert!(st.step(&mut vram));
        let row = read_row(&vram, PARTY_CLUT_ROW_BASE);
        assert_eq!(row[0x70], bgr555_to_rot_blue(base[0x70]));
        assert_eq!(row[0xB0], bgr555_to_rot_blue(base[0xB0]));
        assert_eq!(row[0x60], base[0x60]);
    }

    #[test]
    fn stone_then_rot_leaves_a_grey_row_with_a_blue_limb() {
        let mut vram = legaia_tim::Vram::new();
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE + 2);
        let mut st = StatusClutState::default();
        st.set_rot_windows([None, None, Some(WINDOWS)]);
        st.arm(2, true);
        st.arm_rot(2, 0x0008);
        assert!(st.step(&mut vram));
        let row = read_row(&vram, PARTY_CLUT_ROW_BASE + 2);
        // The Rot arm runs after the Stone arm off the pristine copy.
        assert_eq!(row[0xD0], bgr555_to_rot_blue(base[0xD0]));
        assert_eq!(row[0x10], bgr555_to_grey(base[0x10]));
    }

    #[test]
    fn a_rot_latch_waits_for_its_window() {
        let mut vram = legaia_tim::Vram::new();
        let base = seed_row(&mut vram, PARTY_CLUT_ROW_BASE + 1);
        let mut st = StatusClutState::default();
        st.arm_rot(1, 0x0020);
        assert!(!st.step(&mut vram), "no window, no write");
        assert!(st.armed());
        st.set_rot_windows([None, Some(WINDOWS), None]);
        assert!(st.step(&mut vram));
        assert_eq!(
            read_row(&vram, PARTY_CLUT_ROW_BASE + 1)[0x80],
            bgr555_to_rot_blue(base[0x80])
        );
    }

    #[test]
    fn rot_windows_decode_from_a_scus_shaped_image() {
        assert!(RotClutWindows::from_scus(&[0u8; 16]).is_none());
        let t = RotClutWindows {
            rows: [WINDOWS; ROT_CLUT_ROWS],
        };
        assert_eq!(t.row(1), Some(WINDOWS));
        assert_eq!(t.row(0), None);
        assert_eq!(t.row(5), None);
    }

    #[test]
    fn monster_slots_are_outside_the_three_slot_window() {
        let mut st = StatusClutState::default();
        st.arm(3, true);
        st.arm(7, true);
        assert!(!st.armed(), "ctx[+0x894] holds exactly 3 * 0x1E0 bytes");
    }
}
