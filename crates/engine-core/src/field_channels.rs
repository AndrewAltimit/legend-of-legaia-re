//! Per-actor field-VM channels (spawned placement contexts).
//!
//! Retail spawns every MAN partition-1 placement record as its own script
//! context at scene entry (`FUN_8003A1E4`, called per record by the scene
//! setup `FUN_8003AEB0`): the record base becomes the context's bytecode
//! buffer (`actor[+0x90]`), the first opcode offset its entry PC
//! (`actor[+0x9E]`), and the context's script id (`actor[+0x50]`) is
//! `partition-0 count + partition-1 record index` - the id space
//! cross-context (`0x80`-bit) ops resolve through `FUN_8003C83C` (a walk of
//! the actor list matching `ctx[+0x50] == target`).
//!
//! [`FieldChannel`] is the engine's mirror of one such spawned context. The
//! World spawns the full set when a cutscene timeline installs
//! ([`crate::world::World::install_cutscene_timeline_record`]) and steps each
//! channel run-until-yield per frame - the mechanism behind the opening
//! prologue's vignettes: the `opdeene` timeline pokes actor channels
//! (`0x05..0x0F`) with cross-context flag writes, and each poked channel's own
//! placement script responds by playing an animation (op `0x4B`) / moving
//! (op `0x23`).
//!
//! ## Clean-room boundary
//!
//! No Sony bytes live here: channel bytecode is sliced from the user's disc
//! MAN at runtime; this module holds only the per-context cursor and the
//! spawn rule.
//!
//! ## Scripted initial facing (op 0x43 sub-7) is NOT surfaced - and why
//!
//! A channel can set an actor's facing without moving via field-VM op 0x43
//! sub-7 (the VM writes `ctx.face_rotation = face_id`, mirror of `actor+0x6D`).
//! The engine does not convert that to a renderer heading
//! ([`crate::world::World::field_npc_headings`], the 12-bit `render_26`
//! convention), so a never-walked-but-turned NPC stays at its default facing.
//!
//! There is **no static `face_id -> heading` table to pin**. Op 0x43 sub-7
//! does not select a heading from ROM: it *writes* a per-face rotation-config
//! struct (four `u16` + one `u32`, 12-byte stride) into the RAM scratch array
//! at `0x8007BE60 + face_id*12` directly from the op's own 17-byte operand
//! stream, then registers a ramp of `actor+0x7A` (`0 -> 0x1000`, the lerp
//! fraction) over the op's `target` frames via `FUN_8003C5F0`. The facing is
//! *applied* by the per-actor transform builder `FUN_8001B47C` (arm at
//! `0x8001B484`): it fetches the struct via `actor+0x6D`, copies its four
//! `u16` fields into the render packet at `+0x40..+0x46`, and calls the GTE
//! rotation-matrix builder `FUN_80029888` with `a3 = (struct[+8] << 16) |
//! actor[+0x7A]` - a full 3-axis rotation matrix interpolated by the ramp
//! fraction, not a scalar 12-bit heading.
//!
//! Verdict: **blocked, structural** (not capture-blocked). Surfacing scripted
//! initial facings requires porting the matrix-rotation actor-transform path
//! (`FUN_8001B47C` -> `FUN_80029888`, both render-side), which cannot be
//! reduced to the engine's `render_26` scalar heading; a face_id->heading LUT
//! does not exist. Provenance: `overlay_0897_801de840.txt` (op 0x43 sub-7,
//! `iVar24 + -0x7ff841a0` = struct base `0x8007BE60`), `8001b47c.txt`,
//! `80029888.txt`, `8003c5f0.txt`.

use legaia_asset::man_section::ManFile;
use legaia_engine_vm::field::FieldCtx;

/// One spawned per-actor script context (retail `FUN_8003A1E4` output).
#[derive(Debug, Clone)]
pub struct FieldChannel {
    /// Partition-1 record index (`1..N1`; record 0 is the scene controller).
    /// Also the key [`crate::world::World::field_npc_positions`] and the
    /// windowed host's NPC clip players track.
    pub placement_index: usize,
    /// The context. `script_id` carries the retail id
    /// (`partition-0 count + placement_index`); `world_x`/`world_z` seed from
    /// the placement spawn tile.
    pub ctx: FieldCtx,
    /// Byte offset of the record base in the MAN buffer - the context's
    /// bytecode buffer base (retail `actor[+0x90]`); relative jumps wrap
    /// against it.
    pub record_offset: usize,
    /// Current PC, relative to [`Self::record_offset`].
    pub pc: usize,
    /// Set when the channel ran off its bytecode or hit an op the port cannot
    /// advance past - it stops stepping but stays resolvable as a
    /// cross-context target.
    pub done: bool,
    /// `true` for a `.MAP` **object-bind** context (retail `FUN_8003A55C`):
    /// a gate-0 kind-1 trigger binds a MAN record to a placed object and
    /// writes the trigger's **flat** record index into the actor's script id
    /// (`actor[+0x50] = trigger[2]`), so partition-0 records ARE resolvable
    /// cross-context targets - the `town01` Mei walk-on beat pokes the
    /// Vahn's-house door object as channel `0x01` this way. Object channels
    /// are poke targets only: the engine does not step them autonomously
    /// (their interaction bodies are driven by the touch/interact dispatch),
    /// and their state never writes through to the placement-keyed NPC
    /// surfaces ([`crate::world::World::field_npc_positions`]).
    pub object_bind: bool,
}

impl FieldChannel {
    /// `true` while the channel still steps (not `done`).
    pub fn is_live(&self) -> bool {
        !self.done
    }
}

/// Spawn a [`FieldChannel`] per partition-1 placement record, mirroring the
/// retail per-record spawn loop (`FUN_8003AEB0` calling `FUN_8003A1E4` for
/// records `1..N1`).
///
/// Script id = `partition-0 count + record index` - pinned from
/// `FUN_8003A1E4`'s `ctx[+0x50] = param_1 + param_2` write, where the caller
/// passes the partition-0 count (MAN header `+0x11`) as the base. The context
/// spawns at the placement's tile-centre world position with the placement's
/// anim id in `move_id`'s sibling slot (`+0x5C` in retail; the engine's NPC
/// clip players key off the placement record instead).
// PORT: FUN_8003A1E4
// REF: FUN_8003AEB0 (the per-record spawn loop + script-id base)
pub fn spawn_channels(man_file: &ManFile, man: &[u8]) -> Vec<FieldChannel> {
    let p0_count = man_file.header.partition_counts[0].max(0) as usize;
    man_file
        .actor_placements(man)
        .into_iter()
        .map(|p| {
            let ctx = FieldCtx {
                script_id: (p0_count + p.index) as u16,
                world_x: p.world_x as u16,
                world_z: p.world_z as u16,
                // Placement anim id lands in `+0x5C` in retail
                // (`FUN_8003A1E4`); mirror it so scripts that read/replace
                // the clip see the seeded value.
                move_id: u16::from(p.anim_id),
                // Retail inits the `+0x94` payload slot to `-1`
                // (`FUN_8003A1E4`); the halt-acquire predicate reads it as
                // "acquireable" (non-zero).
                saved_pc: 0xFFFF_FFFF,
                ..FieldCtx::default()
            };
            FieldChannel {
                placement_index: p.index,
                ctx,
                record_offset: p.record_offset,
                pc: p.script_pc0,
                done: false,
                object_bind: false,
            }
        })
        .collect()
}

/// Spawn a [`FieldChannel`] per `.MAP` **object bind**, mirroring the retail
/// scene-init object walk (`FUN_8003A55C`): each bound object's actor gets
/// the gate-0 trigger's **flat** MAN record index as its script id
/// (`actor[+0x50] = trigger[2]`, the `sh t3,0x50(s0)` at `0x8003a8c4`), the
/// record base as its bytecode buffer (`actor[+0x90]`) and the record's
/// first-opcode offset as its entry PC (`actor[+0x9E]`). This is the id
/// space that makes partition-0 records resolvable cross-context targets
/// through the `FUN_8003C83C` actor-list walk.
///
/// `binds` carries `(flat_record_index, contact_centre)` pairs (see
/// [`crate::man_field_scripts::object_script_binds`]). Binds whose flat
/// record cannot be spanned are skipped. Spawn these AFTER the placement
/// channels: a flat index `>= N0` aliases a placement channel's script id,
/// and [`resolve_target`]'s first-match walk must keep the placement.
// REF: FUN_8003A55C (object-bind context: +0x50 script id, +0x90/+0x9E script)
// REF: FUN_8003C83C
pub fn spawn_object_channels(
    man_file: &ManFile,
    man: &[u8],
    binds: &[(usize, (i16, i16))],
) -> Vec<FieldChannel> {
    let mut out = Vec::new();
    for &(flat, (cx, cz)) in binds {
        if flat > u16::MAX as usize {
            continue;
        }
        let Some((record_offset, pc0, _len)) =
            crate::man_field_scripts::flat_record_span(man_file, man, flat)
        else {
            continue;
        };
        if out
            .iter()
            .any(|c: &FieldChannel| c.ctx.script_id == flat as u16)
        {
            continue;
        }
        let ctx = FieldCtx {
            script_id: flat as u16,
            world_x: cx as u16,
            world_z: cz as u16,
            // Retail inits the `+0x94` payload slot to `-1` (`FUN_8003A55C`).
            saved_pc: 0xFFFF_FFFF,
            ..FieldCtx::default()
        };
        out.push(FieldChannel {
            placement_index: flat,
            ctx,
            record_offset,
            pc: pc0,
            done: false,
            object_bind: true,
        });
    }
    out
}

/// Resolve a cross-context target id to a channel index
/// (`ctx[+0x50] == target` - the `FUN_8003C83C` actor-list walk). Returns
/// `None` for the special channels (`0xF8` player anchor, `0xFB` system) and
/// unmatched ids.
// REF: FUN_8003C83C
pub fn resolve_target(channels: &[FieldChannel], target: u8) -> Option<usize> {
    if target == 0xF8 || target == 0xFB {
        return None;
    }
    channels
        .iter()
        .position(|c| c.ctx.script_id == u16::from(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_target_matches_script_id_and_skips_specials() {
        let mk = |id: u16| FieldChannel {
            placement_index: id as usize,
            ctx: FieldCtx {
                script_id: id,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        };
        let channels = vec![mk(4), mk(5), mk(6)];
        assert_eq!(resolve_target(&channels, 5), Some(1));
        assert_eq!(resolve_target(&channels, 7), None);
        assert_eq!(resolve_target(&channels, 0xF8), None);
        assert_eq!(resolve_target(&channels, 0xFB), None);
    }
}
