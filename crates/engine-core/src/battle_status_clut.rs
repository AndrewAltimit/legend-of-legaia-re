//! Battle **status CLUT recolour** - the fourth pass of the per-frame actor
//! maintenance sweep `FUN_8004CE2C`.
//!
//! PORT: FUN_8004CE2C (the Stone arm; the Rot arm is disclosed below)
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
//! # What is not ported
//!
//! The sibling arm on status bits `0x08`/`0x10`/`0x20` (Rot, latched via
//! `actor[+0x221..=+0x223]`, `0x8004D7A8..0x8004D98C`) builds the same
//! luminance plus `b = (l * 3) >> 1` and sets the STP bit, but applies it only
//! over a **per-character index window** read from the 3-pair byte table at
//! `DAT_80078630` (stride 6, indexed by the 1-based character id
//! `DAT_8007BD10[slot]`, pair `s2` selecting the limb). No crate in this
//! workspace parses that table - `DAT_80078630` appears in
//! `docs/subsystems/battle.md` and nowhere in `crates/` - so the arm has a
//! ramp but no window to apply it to. It stays out until the table has a
//! reader.

use legaia_engine_vm::scus_battle_helpers::bgr555_to_grey;

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

/// The engine's stand-in for the three things retail's pass reads that the
/// clean-room battle context has never carried: the per-actor palette copy
/// (`ctx[+0x894 + slot*0x1E0]`), the per-affliction latch (`actor[+0x220]`),
/// and the staged row the upload comes from (`ctx[+0xE34]`).
///
/// Lives on [`crate::battle_hud::BattleHud`] because that is the one
/// per-frame battle struct both hosts own; [`Self::arm`] is driven from
/// [`crate::battle_hud::BattleHud::sync_status`], which every host and the
/// `battle_session` driver already call once per slot per frame.
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
}

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

    /// Whether any slot's latch is waiting for a [`Self::step`].
    pub fn armed(&self) -> bool {
        self.latch.iter().any(|&l| l)
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
            if !self.latch[slot] {
                continue;
            }
            let row = PARTY_CLUT_ROW_BASE + slot as u16;
            if self.pristine[slot].is_none() {
                let words: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
                    .map(|x| vram.pixel(x, row as usize))
                    .collect();
                if words.iter().all(|&w| w == 0) {
                    // Palette not resident yet - hold the latch.
                    continue;
                }
                self.pristine[slot] = Some(words);
            }
            let Some(src) = self.pristine[slot].as_ref() else {
                continue;
            };
            let staged: Vec<u8> = src
                .iter()
                .flat_map(|&c| bgr555_to_grey(c).to_le_bytes())
                .collect();
            vram.write_clut_row(0, row, &staged);
            // retail `sb zero,0x220(s0)` - the latch is spent.
            self.latch[slot] = false;
            wrote = true;
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

    #[test]
    fn monster_slots_are_outside_the_three_slot_window() {
        let mut st = StatusClutState::default();
        st.arm(3, true);
        st.arm(7, true);
        assert!(!st.armed(), "ctx[+0x894] holds exactly 3 * 0x1E0 bytes");
    }
}
