//! NPC motion-pause kick, the port of `FUN_8003C9AC` (SCUS_942.54).
//!
//! PORT: FUN_8003C9AC
//! REF: FUN_801D5B5C, FUN_80038158, FUN_8003BC08
//!
//! When a field interaction engages (the touch-event post `FUN_801D5B5C` in
//! the field overlay - it marks the player + touched actor flags, bumps the
//! touch counters, then tail-calls this kick), retail sweeps the scene actor
//! list at `*_DAT_8007C354` and, for every **moving-class** actor
//! (`flags & 0x20000` at `+0x10`) that has a motion-VM bytecode stream
//! installed (`+0x80 != 0`, the `FUN_8003A9D4` install slot), reloads the
//! actor's requested-move pair from the per-actor default table at
//! `0x801C6470`:
//!
//! ```text
//!   b = byte at 0x801C6470 + (u16 at actor+0x50) * 4   ; stride-4 records
//!   if b != 0x8C:                                      ; 0x8C = "unset"
//!       u16 at actor+0x88 = b
//!       u16 at actor+0x5C = b
//! ```
//!
//! `+0x5C` is the actor's requested move-table id (the field-VM op 0x22
//! target, [`crate::field::FieldCtx::move_id`]; the actor tick
//! `FUN_8003BC08` runs the move-table consumer while it is `> 0`), and
//! `+0x88` is the motion-VM mirror the `FUN_80038158` interpreter writes in
//! lock-step with every `+0x5C` store. The table record's first byte is the
//! actor's default move id, seeded/reset to the `0x8C` sentinel by the
//! motion-VM variant-swap preamble and rewritten by motion op `0x17`; the
//! kick therefore snaps every wandering NPC back onto its default motion
//! cycle while the dialog runs.
//!
//! Port boundary: `ghidra/scripts/funcs/8003c9ac.txt` is the spec; no
//! Sony bytes live here. Tests use synthetic actor lists + tables.
//!
//! ## NOT WIRED
//!
//! The kick has two retail callers, both verified as `jal 0x8003C9AC` in the
//! bytes: the touch-event post `FUN_801D5B5C` (`0x801D5BF0`, PROT 0897 at
//! base `0x801CE818`) and SCUS `FUN_8003BDE0` (`0x8003C0D4`). The engine's
//! analogue of the first is `World::trigger_field_interact`, which the walk
//! touch and the button interact both reach.
//!
//! The write target is no longer the gap - an earlier version of this note
//! said field NPCs carry no `+0x5C` / `+0x88`, and they do now: each NPC with
//! a bound tail-section-1 stream owns a `legaia_engine_vm::ambient_motion::AmbientMotion`
//! channel whose `requested_move` / `move_pair` are exactly that pair and
//! whose `default_move` is its `0x801C6470` record. Two things are still
//! missing, and both would make a wired kick a no-op that only looks live:
//!
//! - **The gate bit.** The kick only touches actors with `+0x10 & 0x20000`.
//!   The channel's `actor_flags` is that word, but nothing seeds it - it
//!   starts at `0` and only the motion VM's own bit ops write it - so the
//!   moving-class bit is clear on every channel and the sweep would kick
//!   nobody. Its retail setter is the spawner, which the port does not model
//!   per channel.
//! - **The consumer.** Retail plays `+0x5C` through the move-table consumer
//!   `FUN_800204F8`, which `FUN_8003BC08` runs whenever `+0x5C > 0`. The
//!   engine reads `requested_move` only on a frame the channel walked
//!   (`World::tick_field_npc_ambient` -> `carry_npc_run_anim`), and a
//!   standing NPC mid-dialogue does not walk, so the reloaded id would not
//!   change what draws.
//!
//! Wiring means seeding the moving-class bit where retail's spawner does and
//! giving a standing NPC's requested move a consumer - an animation-path
//! change measured against the field oracles, not a call insertion.

/// Moving-class actor bit in the `+0x10` flag word. Only actors with this
/// bit set are candidates for the pause kick.
pub const MOVING_CLASS: u32 = 0x20000;

/// "Unset" sentinel in the per-actor default-move table: a record whose
/// first byte is `0x8C` is skipped (the motion-VM preamble seeds fresh
/// records with this value).
pub const PAUSE_SENTINEL: u8 = 0x8C;

/// Stride of the per-actor table at retail `0x801C6470` - one 4-byte record
/// per actor id; the kick reads only the record's first byte.
pub const PAUSE_TABLE_STRIDE: usize = 4;

/// Minimal per-actor view of the fields `FUN_8003C9AC` touches. Engine hosts
/// project their scene-actor records into this and write the results back
/// (or hold these fields directly).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PauseKickActor {
    /// `+0x10` - actor flag word. Gate: `flags & MOVING_CLASS != 0`.
    pub flags: u32,
    /// `+0x80` non-zero - a motion-VM bytecode stream is installed
    /// (`FUN_8003A9D4` binding). Gate: must be `true`.
    pub has_motion_stream: bool,
    /// `+0x50` - actor id (the MAN partition-1 placement index); the index
    /// into the stride-4 default-move table.
    pub actor_id: u16,
    /// `+0x5C` - requested move-table id (field-VM `move_id`). Reloaded from
    /// the table byte when the gates pass.
    pub move_id: u16,
    /// `+0x88` - motion-VM lock-step mirror of [`move_id`](Self::move_id).
    /// Reloaded together with it.
    pub move_id_mirror: u16,
}

/// The kick: one pass over the scene actor list. For every actor passing the
/// moving-class + motion-stream gates, reload `move_id` / `move_id_mirror`
/// from `per_actor_table[actor_id * PAUSE_TABLE_STRIDE]` unless that byte is
/// [`PAUSE_SENTINEL`]. Returns how many actors were kicked.
///
/// An `actor_id` whose record lies beyond `per_actor_table` is skipped (the
/// retail table is a fixed RAM arena; a short engine-side table means "no
/// record", which behaves like the sentinel).
// PORT: FUN_8003C9AC
// REF: FUN_801D5B5C
pub fn motion_pause_kick(actors: &mut [PauseKickActor], per_actor_table: &[u8]) -> usize {
    let mut kicked = 0;
    for actor in actors.iter_mut() {
        if actor.flags & MOVING_CLASS == 0 || !actor.has_motion_stream {
            continue;
        }
        let idx = actor.actor_id as usize * PAUSE_TABLE_STRIDE;
        let Some(&b) = per_actor_table.get(idx) else {
            continue;
        };
        if b == PAUSE_SENTINEL {
            continue;
        }
        actor.move_id = u16::from(b);
        actor.move_id_mirror = u16::from(b);
        kicked += 1;
    }
    kicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(entries: &[(u16, u8)]) -> Vec<u8> {
        let max_id = entries.iter().map(|&(id, _)| id).max().unwrap_or(0);
        // Seed every record's first byte with the sentinel, like the retail
        // variant-swap preamble does.
        let mut t = vec![0u8; (max_id as usize + 1) * PAUSE_TABLE_STRIDE];
        for chunk in t.chunks_mut(PAUSE_TABLE_STRIDE) {
            chunk[0] = PAUSE_SENTINEL;
        }
        for &(id, b) in entries {
            t[id as usize * PAUSE_TABLE_STRIDE] = b;
        }
        t
    }

    fn pausable(actor_id: u16) -> PauseKickActor {
        PauseKickActor {
            flags: MOVING_CLASS | 0x400, // extra bits must not defeat the AND-test
            has_motion_stream: true,
            actor_id,
            move_id: 0,
            move_id_mirror: 0,
        }
    }

    #[test]
    fn pausable_actor_gets_both_timers_reloaded() {
        let mut actors = [pausable(3)];
        let table = table_with(&[(3, 0x12)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 1);
        assert_eq!(actors[0].move_id, 0x12);
        assert_eq!(actors[0].move_id_mirror, 0x12);
    }

    #[test]
    fn non_moving_class_actor_is_skipped() {
        let mut actors = [PauseKickActor {
            flags: 0x400, // moving-class bit clear
            ..pausable(3)
        }];
        let table = table_with(&[(3, 0x12)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 0);
        assert_eq!(actors[0].move_id, 0);
        assert_eq!(actors[0].move_id_mirror, 0);
    }

    #[test]
    fn actor_without_motion_stream_is_skipped() {
        let mut actors = [PauseKickActor {
            has_motion_stream: false, // +0x80 == 0
            ..pausable(3)
        }];
        let table = table_with(&[(3, 0x12)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 0);
        assert_eq!(actors[0].move_id, 0);
        assert_eq!(actors[0].move_id_mirror, 0);
    }

    #[test]
    fn sentinel_record_is_skipped() {
        let mut actors = [PauseKickActor {
            move_id: 7,
            move_id_mirror: 7,
            ..pausable(2)
        }];
        let table = table_with(&[(2, PAUSE_SENTINEL)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 0);
        // Untouched, not zeroed.
        assert_eq!(actors[0].move_id, 7);
        assert_eq!(actors[0].move_id_mirror, 7);
    }

    #[test]
    fn out_of_range_actor_id_is_skipped() {
        let mut actors = [pausable(100)];
        let table = table_with(&[(3, 0x12)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 0);
        assert_eq!(actors[0].move_id, 0);
    }

    #[test]
    fn mixed_list_only_kicks_eligible_actors() {
        let mut actors = [
            pausable(0),
            PauseKickActor {
                flags: 0,
                ..pausable(1)
            },
            pausable(2), // sentinel record
            pausable(3),
        ];
        let table = table_with(&[(0, 0x05), (1, 0x06), (3, 0x22)]);
        assert_eq!(motion_pause_kick(&mut actors, &table), 2);
        assert_eq!(actors[0].move_id, 0x05);
        assert_eq!(actors[1].move_id, 0); // gate-blocked despite table entry
        assert_eq!(actors[2].move_id, 0); // sentinel
        assert_eq!(actors[3].move_id, 0x22);
        assert_eq!(actors[3].move_id_mirror, 0x22);
    }
}
