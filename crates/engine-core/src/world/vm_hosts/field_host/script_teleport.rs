//! The op-`0x4C 0xC3` script-table teleport: re-seat a context at its own MAN
//! placement record and re-run its spawn section.

use super::*;

/// Apply the op-`0x4C 0xC3` script-table teleport onto `ctx`: re-seat the
/// executing context at its **own** MAN record's placement header.
///
/// Retail resolves the record via `FUN_8003C8F0(ctx+0x50, 0)`. That helper's
/// `param_2` is a *partition base* selector, and the call passes `0`, so the
/// base is zero and `param_1` (the context's `+0x50` script id) indexes the
/// **flattened** `[P0..P1..P2]` record table directly. Since the placement
/// spawner writes `+0x50 = N0 + placement_index`, the record it lands on is
/// the context's own partition-1 placement record - NOT partition-0 record
/// `script_id`. [`legaia_engine_field::man_field_scripts::flat_record_span`] models that
/// index space.
///
/// The header read is **not** partition-aware. `FUN_8003D0BC` skips the
/// `[u8 N][N*2]` name / locals field and the arm reads the four bytes after
/// it as `[model, anim, bx, bz]`, then rebases the script offset `+0x9E` to
/// the byte after those four (`0x801E2700..0x801E27A4`). That is the
/// partition-1 placement header. A partition-0 **object** record carries one
/// attribute byte there, not four (`FUN_8003A55C` seats `+0x9E` at
/// `1 + N*2 + 1`), so the same read on an object context takes `anim` from the
/// record's first opcode and `bx` / `bz` from the two bytes after it, and
/// leaves the script offset three bytes into the script. `nilboa` `P2[11]`
/// does exactly that to its six ravine objects (`CC 05 C3`, `CC 00 C3` ..
/// `CC 04 C3`, each record opening `25 74 56 05`): all six land on tile
/// `(0x74, 0x56)`, off the walkable map, which is how the ice gate `P0[4]`
/// stops blocking the way west the moment the beat sets `0x456`.
///
/// Returns the rebased script offset (`1 + N*2 + 4`, relative to the record)
/// on success.
///
/// It then writes (`overlay_0897_801de840.txt` case 3 of the nibble-C
/// dispatcher):
///
/// - `+0x14`/`+0x18` (`world_x`/`world_z`) = tile centre
///   `(b & 0x7F) * 0x80 + 0x40`, plus another `+0x40` when bit 7 is set;
/// - `+0x5C` ([`FieldCtx::move_id`]) = the header's anim byte;
/// - `+0x6A` = 8, `+0x72` = `0x1000`, `+0x62` = `0x15`;
/// - `+0x54` (wait accumulator), `+0x8E`, `+0x8B` = 0;
/// - flags word `&= 0x9EBFFAFE` (clears the halt bit `0x400` among others);
/// - `+0x8C`/`+0x8D` = tile column/row recomputed from the new world
///   position (signed `(w - 0x40) >> 7`).
///
/// - `+0x78` (the tint blend) = 0.
///
/// The `0x25` spawn-section re-run that follows is
/// [`FieldHostImpl::rerun_spawn_section`]. Not modelled (no [`FieldCtx`]
/// counterpart): the `+0x70`/`+0x9C`/`+0x2A` zeroes and the linked `+0x44`
/// struct's `+0x9A = 0xFFFF` write.
///
/// Returns `None` (ctx untouched) when the record cannot be resolved.
// REF: FUN_8003D0BC, FUN_8003A55C
pub(in crate::world) fn apply_script_table_teleport(
    man_file: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    ctx: &mut FieldCtx,
) -> Option<usize> {
    let (script_start, _pc0, _body_len) = legaia_engine_field::man_field_scripts::flat_record_span(
        man_file,
        man,
        ctx.script_id as usize,
    )?;
    // `FUN_8003D0BC`: past the `[u8 N][N*2]` field, whatever the partition.
    let n = usize::from(*man.get(script_start)?);
    let hdr_at = script_start + 1 + n * 2;
    let hdr = man.get(hdr_at..hdr_at + 4)?;
    let (anim, bx, bz) = (hdr[1], hdr[2], hdr[3]);
    let center = |b: u8| -> u16 {
        let base = u16::from(b & 0x7F) * 0x80 + 0x40;
        if b & 0x80 != 0 { base + 0x40 } else { base }
    };
    ctx.world_x = center(bx);
    ctx.world_z = center(bz);
    ctx.move_id = u16::from(anim);
    ctx.field_6a = 8;
    ctx.field_72 = 0x1000;
    ctx.wait_accum = 0;
    ctx.field_8e = 0;
    ctx.field_78 = 0;
    ctx.local_flags = 0x15;
    ctx.flags &= 0x9EBF_FAFE;
    // Tile column/row from the fresh world position - the retail signed
    // `>> 7` keeps a negative-rounding fixup that is unreachable here (the
    // tile-centre values are always positive); mirrored for fidelity.
    let grid = |w: u16| -> u8 {
        let w = i32::from(w as i16);
        let v = if w - 0x40 < 0 { w + 0x3F } else { w - 0x40 };
        (v >> 7) as u8
    };
    ctx.npc_x = grid(ctx.world_x);
    // `+0x8D` (named for its op-0x23 facing role) carries the tile ROW here.
    ctx.npc_facing = grid(ctx.world_z);
    ctx.field_8b = 0;
    Some(1 + n * 2 + 4)
}

impl FieldHostImpl<'_> {
    /// The second half of op `4C C3`: re-run the re-seated context's spawn
    /// section.
    ///
    /// After the teleport retail rebases the context's script offset `+0x9E`
    /// to the byte after the four header bytes (`0x801E2798..0x801E27A4`;
    /// `pc0` here, a partition-1 record's first opcode), and when
    /// that byte is `0x25` it raises the scene word `*(_DAT_801C6EA4) + 8`,
    /// runs the context through `FUN_8003CF7C` from there and drops the word
    /// again (`0x801E2800..0x801E282C`). `FUN_8003CF7C` executes ops until it
    /// has run a `0x21`, the PC stops moving, or the next byte is below the
    /// opcode band - the same slice the scene-entry install gives a
    /// placement (`FUN_8003A1E4`), so the section's story-flag dispatch picks
    /// the actor's seat again from the live flags. `nilboa` `P2[27]` sends
    /// the Fire Ravine boulders home this way and their sections, finding
    /// `0x457` set, put them back where they were pushed.
    ///
    /// The slice runs with the scene-entry pre-run's semantics
    /// ([`crate::world::FieldVmState::entry_prerun`]): a seat op seats this
    /// context and never the player. An op the section aims at another
    /// context is stepped over by its width; the caller writes the final
    /// position through to the actor.
    ///
    /// REF: FUN_8003CF7C, FUN_801DE840 (`0x801E2798..0x801E282C`)
    fn rerun_spawn_section(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
        ctx: &mut FieldCtx,
        pc0: usize,
    ) {
        if self.world.field_vm.respawn_rerun {
            return;
        }
        let Some((start, _pc0, _len)) = legaia_engine_field::man_field_scripts::flat_record_span(
            man_file,
            man,
            ctx.script_id as usize,
        ) else {
            return;
        };
        let Some(bc) = man.get(start..) else {
            return;
        };
        if bc.get(pc0) != Some(&0x25) {
            return;
        }
        let prev_prerun = self.world.field_vm.entry_prerun;
        self.world.field_vm.entry_prerun = true;
        self.world.field_vm.respawn_rerun = true;
        let own = ctx.script_id;
        let mut pc = pc0;
        for _ in 0..256 {
            let Some(&op) = bc.get(pc) else {
                break;
            };
            if op & 0x7F < 0x20 {
                break;
            }
            let foreign = vm::field::peek_extended(bc, pc).is_some_and(|t| u16::from(t) != own);
            let next = if foreign {
                match legaia_scene_formats::field_disasm::decode(bc, pc) {
                    Ok(insn) => pc + insn.size,
                    Err(_) => break,
                }
            } else {
                match field_step_routed(self, ctx, bc, pc) {
                    FieldStepResult::Advance { next_pc } => next_pc,
                    FieldStepResult::Yield { .. }
                    | FieldStepResult::Halt { .. }
                    | FieldStepResult::Pending { .. }
                    | FieldStepResult::Unknown { .. } => break,
                }
            };
            if op == 0x21 || next == pc {
                break;
            }
            pc = next;
        }
        self.world.field_vm.respawn_rerun = false;
        self.world.field_vm.entry_prerun = prev_prerun;
    }
}

impl FieldHostImpl<'_> {
    // PORT: FUN_8003C8F0 - the `_DAT_8007B898` partition-table record
    // resolver (prefix-summed s16 counts at `+0x22` + flattened u24
    // record-offset walk) feeding the op-`0x4C 0xC3` script-table teleport;
    // the record walk is shared with
    // [`legaia_engine_field::man_field_scripts::partition_record_offset`], the descriptor
    // decode + ctx write-set live in [`apply_script_table_teleport`].
    pub(super) fn script_table_teleport(&mut self, ctx: &mut FieldCtx) {
        // Resolve against the scene's resident MAN (kept on the world while
        // per-actor channels run). Without one the trait's default no-op
        // semantics stand - there is no partition table to resolve against.
        let Some(man) = self.world.field_vm.channels_man.clone() else {
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            return;
        };
        let Some(pc0) = apply_script_table_teleport(&man_file, &man, ctx) else {
            return;
        };
        self.rerun_spawn_section(&man_file, &man, ctx, pc0);
    }
}
