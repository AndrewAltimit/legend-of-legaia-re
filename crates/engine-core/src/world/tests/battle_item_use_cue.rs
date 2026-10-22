//! Item-use presentation, disc-free: where the cast-cue band's ids go.
//!
//! The ring-commit clip stamp is pinned through the pad in
//! `crates/engine-core/tests/battle_command_arms_reachable.rs`; the
//! disc-gated end-to-end twin (real clips, real cue tracks) is
//! `crates/engine-session/tests/battle_item_use_presentation.rs`.

use super::*;
use crate::world::vm_hosts::BattleHostImpl;
use legaia_engine_battle_vm::battle_action::BattleActionHost;
use legaia_engine_battle_vm::battle_cast_cue::{CastCueOutcome, cast_audio_cue};

/// The party leg of the cast-cue band is `roster_id * 0x10 + 0xF8..0xFC`
/// with a 1-based roster id, so every party cue is `>= 0x108` - a CD-XA clip.
/// The host sends it to the XA channel, not the SFX queue (where both hosts
/// classify it as a voice and decline it).
#[test]
fn the_cast_cue_band_reaches_the_xa_channel() {
    let mut world = World::new();
    world.party.party_count = 1;
    world.actors[0].active = true;
    // A synthetic span table: every entry one sector's worth.
    world.audio.xa_cue_durations = Some(vec![10; 0x200]);

    // Vahn (roster id 1), a class-0 item (the Healing Leaf's heal class).
    let CastCueOutcome::Sfx(id) = cast_audio_cue(
        legaia_engine_battle_vm::battle_slot::RetailSlot::new(0),
        1,
        0,
        0x77,
        0,
    ) else {
        panic!("class 0 has a cue");
    };
    assert_eq!(id, 0x108);
    {
        let mut host = BattleHostImpl { world: &mut world };
        host.one_shot_sfx(id);
    }
    assert!(
        world.audio.battle_sfx_cues.is_empty(),
        "a voice id never reaches the SFX queue"
    );
    let xa = world.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1);
    assert_eq!((xa[0].clip, xa[0].channel), (0x1A, 0));
    assert_eq!(xa[0].duration_sectors, 6, "(10 * 60).div_ceil(100)");

    // The clip holds the drive: a second cue inside its span is declined,
    // as `FUN_8003DE7C(1)` declines it.
    {
        let mut host = BattleHostImpl { world: &mut world };
        host.one_shot_sfx(0x10B);
    }
    assert!(world.drain_battle_xa_cues().is_empty());
}

/// An id below `0x100` goes on the SFX ring as `FUN_8004FCC8` stores it:
/// itself from `0x40`, `id - 1` below.
#[test]
fn a_sub_voice_cast_cue_rings_the_dispatchers_index() {
    use legaia_engine_system::sound_state::SfxRingOp;
    let mut world = World::new();
    world.party.party_count = 1;
    {
        let mut host = BattleHostImpl { world: &mut world };
        host.one_shot_sfx(0xF8);
        host.one_shot_sfx(0x21);
    }
    assert_eq!(
        world.take_sfx_ring_ops(),
        vec![SfxRingOp::Push(0xF8), SfxRingOp::Push(0x20)]
    );
    assert!(world.audio.battle_sfx_cues.is_empty());
    assert!(world.drain_battle_xa_cues().is_empty());
}

/// The Cort stage modules' cues (`0x20A`, `0x20B`) are past `0x100`: the
/// dispatcher starts a CD-XA clip (slot `0x21`, channel `id & 7`), it never
/// keys an SFX row.
#[test]
fn a_stage_module_cue_is_a_voice_clip() {
    let mut world = World::new();
    world.audio.xa_cue_durations = Some(vec![10; 0x200]);
    world.dispatch_battle_cue(legaia_engine_battle::battle_stage_module::ARRIVAL_CUE);
    assert!(world.take_sfx_ring_ops().is_empty());
    let xa = world.drain_battle_xa_cues();
    assert_eq!(xa.len(), 1);
    assert_eq!((xa[0].clip, xa[0].channel), (0x21, 2));
}
