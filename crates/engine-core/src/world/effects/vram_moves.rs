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
    /// the `FieldScreenHost::vram_move_image` host hook.
    pub fn from_words(words: [i16; 6]) -> Self {
        Self {
            src: (words[0], words[1]),
            size: (words[2], words[3]),
            dst: (words[4], words[5]),
        }
    }
}

impl World {
    /// Queue a field-VM `4C 60` VRAM `MoveImage` (the `vram_move_image`
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
    /// REF: FUN_80058490 (the sub-0x60 consumer: `jal` at 0x801E1B84 in the
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
        let recoloured = self.apply_battle_cell_fx(vram);
        // Capture alignment (empty outside a retail-compare child): a
        // retail state's live scroller rects, written after the engine's own
        // rotations as the field VRAM pass writes them.
        let seeded = self.apply_vram_rect_seed(vram);
        moved || loaded || scrolled || recoloured || seeded
    }

    /// The mode-3 recolours the battle parts queued this frame
    /// ([`crate::world::BattleState::cell_fx`]), in order: each rewrites its
    /// rect from the texels captured when the part first surfaced, through
    /// the `FUN_80019D50` HSV kernel, and lands the result where the retail
    /// `LoadImage` does. A part's first armed tick drops its rect's capture
    /// ([`Self::queue_battle_part_scrolls`]), so each op `0x2C` captures
    /// the rect afresh, as retail's `StoreImage` does.
    ///
    /// PORT: FUN_80019D50 (on the battle effect lists; the texel kernel is
    /// `legaia_engine_field::clut_cell_fx::apply_hsv_cell`)
    fn apply_battle_cell_fx(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        use super::super::ambient::{read_rect, write_rect};
        let mut wrote = false;
        for f in std::mem::take(&mut self.battle.cell_fx) {
            let (x, y, w, h) = f.rect;
            if w == 0 || h == 0 || w > 256 || h > 64 {
                continue;
            }
            let src = self
                .battle
                .cell_captures
                .entry(f.rect)
                .or_insert_with(|| read_rect(vram, x, y, w, h));
            let out = legaia_engine_field::clut_cell_fx::apply_hsv_cell(src, &f);
            if read_rect(vram, x, y, w, h) != out {
                write_rect(vram, x, y, w, &out);
                wrote = true;
            }
        }
        wrote
    }

    /// The mode-4 render tail of the part tick for every live part of an
    /// effect scene ticked in battle: `FUN_80021DF4` runs the cyclic VRAM
    /// scroller after the VM call on any part whose `+0x5A` is `4`
    /// (`0x80022CB8`), whatever list the part sits on, so a battle effect's
    /// scroller animates its rect exactly as a field ambient one does
    /// ([`legaia_engine_field::vram_scroll`]). The fired rotations queue on
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
            // The mode-3 arm: a `0x4000` part's HSV adds integrate every
            // tick, and from its second armed frame on it recolours the rect
            // its op `0x2C` captured (PROT 0929's arm-14 part `0x801F95A8`
            // hue-cycles the scanline backdrop's CLUT row 488).
            if part.state.move_submode == legaia_engine_effects::summon::RenderMode::Particle.raw()
            {
                let delta = self.move_vm.ramp_ratio.max(1);
                if part.state.field_9c == 1 {
                    let d = &part.state.keyframe_desc;
                    self.battle.cell_captures.remove(&(d[0], d[1], d[2], d[3]));
                }
                if let Some(fx) =
                    legaia_engine_field::clut_cell_fx::mode3_integrate(&mut part.state, delta)
                {
                    self.battle.cell_fx.push(fx);
                }
                continue;
            }
            if part.state.move_submode != legaia_engine_field::vram_scroll::RENDER_MODE_SCROLL {
                continue;
            }
            if let Some(fx) =
                legaia_engine_field::vram_scroll::mode4_integrate(&mut part.state, step)
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
        self.battle.cell_fx.clear();
        self.battle.vram_loads = Default::default();
    }
}

/// One `4C D4` / `4C D5` run: `StoreImage` the 16x1 rect at `(x, y)`, set the
/// mask bit on every non-zero word (`set`) or clear it on every word but the
/// mask-only `0x8000`, and `LoadImage` it back. The bit is what keeps a CLUT
/// entry an HSV cycler later darkens to black opaque: a `0x0000` entry is
/// transparent, `0x8000` is black.
///
/// PORT: FUN_801DE840 (`4C D4` / `4C D5` arm; `StoreImage` / `LoadImage`)
/// REF: FUN_8005842C (`StoreImage`), FUN_800583C8 (`LoadImage`) - libgpu
/// wrappers this replays against the port's VRAM model
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
