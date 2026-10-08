//! Battle presentation decisions both play hosts take in their per-tick
//! effect step, so neither re-spells them.

use legaia_engine_audio::{CueDispatch, classify_cue};
use legaia_engine_core::world::World;

/// The production move-FX trigger, spawn to sound: take the world's pending
/// move-FX spawn (a non-summon spell cast or enemy special whose move-power
/// record carries a spawnable effect list), seat it through
/// [`World::spawn_move_fx`], and resolve the move's sound cue through the
/// retail dispatch decode (`classify_cue` = `FUN_8004FCC8`).
///
/// Returns the SFX ring value to enqueue - the `SfxBank` descriptor id
/// (`docs/formats/sfx-table.md`) the host hands its director's
/// `enqueue_sfx`, which resolves the cue's own `+4` category bank. `None`
/// when nothing spawned or the move carries no cue.
///
/// The cue is a byte (`World::take_pending_move_fx_cue`), so it always lands
/// in the dispatcher's ring band below `0x100`; the `Voice` band (a streamed
/// CD-XA trigger, `id >= 0x100`) cannot arise here, and the two hosts used to
/// differ only in whether they logged that impossible arm. The full ring
/// value goes through: cue `0` resolves to ring value `0xFFFF`, which a
/// `u8` narrowing used to drop on the play page.
pub fn spawn_pending_move_fx(world: &mut World) -> Option<u16> {
    let (move_id, origin) = world.take_pending_move_fx_spawn()?;
    if !world.spawn_move_fx(move_id, origin) {
        return None;
    }
    let cue = world.take_pending_move_fx_cue()?;
    match classify_cue(u32::from(cue)) {
        CueDispatch::Ring { ring_value, .. } => Some(ring_value),
        CueDispatch::Voice { .. } => None,
    }
}
