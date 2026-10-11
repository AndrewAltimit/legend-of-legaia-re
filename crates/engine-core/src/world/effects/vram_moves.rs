//! Scripted VRAM MoveImage stamps: the field-VM queue, the battle-side stamps
//! and part scrolls, and the VRAM appliers.

use super::*;
use legaia_engine_mode::mode::SceneMode;

// --- scripted VRAM MoveImage stamps (field-VM 0x4C n6 sub-0x60) ------------

/// One queued literal-operand VRAM rect copy - the field-VM op `4C 60`.
///
/// The 14-byte instruction `[4C, 60, src_x, src_y, w, h, dst_x, dst_y]`
/// carries six little-endian u16s (read via the misaligned-u16 helper
/// `FUN_8003CE9C`); the handler arm at `0x801E1B28..0x801E1B90` inside the
/// field-VM dispatcher `FUN_801DE840` hands them straight to the libgpu
/// `MoveImage` wrapper (`jal FUN_80058490` at `0x801E1B84`) - a `w x h`
/// halfword VRAM-to-VRAM rect copy. Retail scripts use it for the one-shot
/// face-frame stamps onto the player texture atlas (blink / mouth variants;
/// see `docs/formats/character-mesh.md` "Runtime scroll-cell residue").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptVramMove {
    /// Source rect origin `(x, y)` in VRAM halfword coordinates.
    pub src: (i16, i16),
    /// Rect size `(w, h)` - halfwords x rows.
    pub size: (i16, i16),
    /// Destination rect origin `(x, y)`.
    pub dst: (i16, i16),
}

impl ScriptVramMove {
    /// Build from the six decoded operand words in instruction order
    /// `[src_x, src_y, w, h, dst_x, dst_y]` - the payload the field VM hands
    /// the `FieldScreenHost::op4c_n6_sub0_emitter6` host hook.
    pub fn from_words(words: [i16; 6]) -> Self {
        Self {
            src: (words[0], words[1]),
            size: (words[2], words[3]),
            dst: (words[4], words[5]),
        }
    }
}

impl World {
    /// Queue a field-VM `4C 60` VRAM `MoveImage` (the `op4c_n6_sub0_emitter6`
    /// host hook). Applied against the host's software VRAM by
    /// [`Self::apply_script_vram_moves`] - the queue keeps `World`
    /// renderer-free, mirroring the [`Self::spawn_clut_cell_fx`] /
    /// [`Self::step_clut_fx`] split of the sibling sub-`0x61` family.
    pub fn queue_script_vram_move(&mut self, words: [i16; 6]) {
        self.ambient
            .script_vram_moves
            .push(ScriptVramMove::from_words(words));
    }

    /// Drain the queued `4C 60` stamps into `vram` (the host's software
    /// VRAM: play-window's `cpu_vram_base`, a test's scratch
    /// [`legaia_tim::Vram`]). Returns `true` when anything was copied (the
    /// host re-uploads its GPU copy).
    ///
    /// Retail hands the literal operands to libgpu unchecked, but no retail
    /// script emits a negative or empty rect - one here is a decode fault,
    /// dropped rather than alias-wrapped.
    ///
    /// PORT: FUN_80058490 (the sub-0x60 consumer: `jal` at 0x801E1B84 in the
    /// handler arm 0x801E1B28..0x801E1B90 of FUN_801DE840)
    pub fn apply_script_vram_moves(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let moves = std::mem::take(&mut self.ambient.script_vram_moves);
        let mut wrote = apply_vram_moves(moves, vram);
        for (x, y, set) in std::mem::take(&mut self.ambient.script_vram_stp) {
            wrote |= apply_vram_stp(vram, x, y, set);
        }
        wrote
    }

    /// Apply the battle `MoveImage`s a stage module queued
    /// ([`crate::world::BattleState::vram_moves`]) to the host's battle VRAM,
    /// in order. Both hosts call it once per battle frame; returns `true`
    /// when any texel moved (the host re-uploads).
    pub fn apply_battle_vram_moves(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let loads = std::mem::take(&mut self.battle.vram_loads);
        let loaded = !loads.is_empty();
        loads.replay(vram);
        let moves = std::mem::take(&mut self.battle.vram_moves);
        let moved = apply_vram_moves(moves, vram);
        let mut scrolled = false;
        for fx in std::mem::take(&mut self.battle.vram_scrolls) {
            scrolled |= super::super::ambient::apply_scroll_fx(vram, &fx);
        }
        moved || loaded || scrolled
    }

    /// The mode-4 render tail of the part tick for every live part of an
    /// effect scene ticked in battle: `FUN_80021DF4` runs the cyclic VRAM
    /// scroller after the VM call on any part whose `+0x5A` is `4`
    /// (`0x80022CB8`), whatever list the part sits on, so a battle effect's
    /// scroller animates its rect exactly as a field ambient one does
    /// ([`super::super::ambient::vram_scroll`]). The fired rotations queue on
    /// [`crate::world::BattleState::vram_scrolls`] for the hosts' battle VRAM.
    ///
    /// PORT: FUN_80021DF4 (the mode-4 arm, `0x80022CB8..0x80022EE0`, on the
    /// battle effect lists)
    pub(crate) fn queue_battle_part_scrolls(
        &mut self,
        parts: &mut [legaia_engine_effects::summon::SummonPartRuntime],
    ) {
        if self.mode != SceneMode::Battle {
            return;
        }
        let step = self.clock.frame_step.max(1);
        for part in parts.iter_mut().filter(|p| !p.finished) {
            if part.state.move_submode != super::super::ambient::vram_scroll::RENDER_MODE_SCROLL {
                continue;
            }
            if let Some(fx) =
                super::super::ambient::vram_scroll::mode4_integrate(&mut part.state, step)
            {
                self.battle.vram_scrolls.push(fx);
            }
        }
    }

    /// Drop this frame's queued battle VRAM edits unapplied - the hosts'
    /// path when no battle VRAM is resident to apply them to.
    pub fn discard_battle_vram_edits(&mut self) {
        self.battle.vram_moves.clear();
        self.battle.vram_scrolls.clear();
        self.battle.vram_loads = Default::default();
    }
}

/// One `4C D4` / `4C D5` run: `StoreImage` the 16x1 rect at `(x, y)`, set the
/// mask bit on every non-zero word (`set`) or clear it on every word but the
/// mask-only `0x8000`, and `LoadImage` it back. The bit is what keeps a CLUT
/// entry an HSV cycler later darkens to black opaque: a `0x0000` entry is
/// transparent, `0x8000` is black.
///
/// PORT: FUN_801DE840 (`4C D4` / `4C D5` arm; `FUN_8005842C` / `FUN_800583C8`)
fn apply_vram_stp(vram: &mut legaia_tim::Vram, x: u16, y: u16, set: bool) -> bool {
    if usize::from(x) + 16 > 1024 || usize::from(y) >= 512 {
        return false;
    }
    let mut changed = false;
    let mut bytes = Vec::with_capacity(32);
    for i in 0..16usize {
        let w = vram.pixel(usize::from(x) + i, usize::from(y));
        let n = if set {
            if w != 0 { w | 0x8000 } else { w }
        } else if w != 0x8000 {
            w & 0x7FFF
        } else {
            w
        };
        changed |= n != w;
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    if changed {
        vram.write_block(x, y, 16, 1, &bytes);
    }
    changed
}

/// The shared `MoveImage` kernel behind the field and battle queues.
fn apply_vram_moves(moves: Vec<ScriptVramMove>, vram: &mut legaia_tim::Vram) -> bool {
    let mut wrote = false;
    for mv in moves {
        let (sx, sy) = mv.src;
        let (w, h) = mv.size;
        let (dx, dy) = mv.dst;
        if w <= 0 || h <= 0 || sx < 0 || sy < 0 || dx < 0 || dy < 0 {
            continue;
        }
        vram.move_image(
            sx as u16, sy as u16, w as u16, h as u16, dx as u16, dy as u16,
        );
        wrote = true;
    }
    wrote
}
