//! Binding a freshly entered scene's actors to the world: the one step both
//! play hosts run after scene entry, before they build any mesh.
//!
//! A host that draws needs meshes; the **world** needs only what those meshes
//! are bound to - the lead's locomotion clip bank, the clip cursor of every
//! placed NPC, and the floor height a standing actor rests at. That state used
//! to be written from inside each host's mesh upload: the native window wrote
//! it from its GPU upload (so a window with no renderer ran a different
//! simulation), and the browser page wrote it from its own render rebuild.
//! [`bind_scene_actors`] writes all of it once, from the scene and its
//! resources alone, and hands back what a host's draw half reads (the two ANM
//! banks, the NPC catalog and the per-placement clip players).
//!
//! The registered scene actors' own bindings (actor `K` -> scene TMD `K` and
//! its idle clip, the retail `0x8007C018` table) are the scene host's, written
//! at scene entry (`World::init_scene_animations`); nothing here repeats them.
//! A host's mesh upload may re-point a bound slot at its own mesh handle
//! (`Actor::tmd_binding` holds the host's mesh index), but never binds or
//! unbinds a slot, and never replaces a clip.

use std::collections::BTreeMap;

use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_engine_field::field_anim::{FieldClipPlayer, FieldPlayerAnim};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::npc_catalog::{NpcCatalog, catalog_scene_npcs, scene_anm_bundle};
use legaia_engine_scene_host::scene_host::SceneHost;

/// What [`bind_scene_actors`] resolved, for a host's draw half.
pub struct SceneActorBinding {
    /// The scene's ANM bundle (`npc_catalog::scene_anm_bundle`): poses the
    /// MAN NPCs, the bound placed props and, on a kingdom map, the lead.
    pub scene_anm: Option<PlayerAnmBundle>,
    /// The party locomotion bundle (PROT 0874 §1): the lead's clips and the
    /// global-pool specials'.
    pub locomotion_anm: Option<PlayerAnmBundle>,
    /// The scene's NPC catalog (the play shape, global-pool specials
    /// included), or why it could not be built.
    pub catalog: Result<NpcCatalog, String>,
    /// One clip player per catalogued placement whose live clip id names a
    /// record, keyed by placement slot. The world's cursor for each is bound
    /// (`World::bind_npc_clip_cursor`).
    pub clip_players: BTreeMap<u8, FieldClipPlayer>,
    /// Every catalogued placement slot, with whether it is a global-pool
    /// special: the clip targets a host's cue drain re-targets
    /// (`World::drain_field_anim_cues`).
    pub clip_targets: BTreeMap<u8, bool>,
}

/// The party locomotion bundle (PROT 0874 §1), when the disc carries one.
pub fn locomotion_bundle(host: &SceneHost) -> Option<PlayerAnmBundle> {
    host.index
        .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
        .ok()
        .and_then(|b| legaia_actor_formats::character_pack::field_locomotion_anm(&b).ok())
}

/// Bind the entered scene's actors to the world. See the module docs.
///
/// In order:
///
/// 1. **The lead** ([`bind_lead_rig`]).
/// 2. **Placed NPCs.** Each catalogued placement whose live clip id (actor
///    `+0x5C` after the spawn prologue, else the header byte) names a record
///    of its bank (`World::npc_clip_party_bank`) gets a clip player and the
///    world's cursor for it.
/// 3. **Floor snap** ([`snap_bound_actors_to_floor`]).
pub fn bind_scene_actors(host: &mut SceneHost) -> SceneActorBinding {
    let scene_anm = host.scene.as_ref().and_then(scene_anm_bundle);
    let locomotion_anm = locomotion_bundle(host);

    bind_lead_rig(host, locomotion_anm.as_ref());

    let catalog = match (host.scene.as_ref(), host.resources.as_ref()) {
        (Some(scene), Some(res)) => catalog_scene_npcs(
            &host.index,
            &scene.name,
            res,
            Some(&host.world.field_head_pool),
        ),
        _ => Err("no scene loaded".to_string()),
    };
    let catalog = catalog.map(|mut cat| {
        for e in &mut cat.entries {
            if let Some(a) = host.world.field_npc_live_anim(e.placement.index) {
                e.placement.anim_id = a;
            }
        }
        cat
    });
    let mut clip_players = BTreeMap::new();
    let mut clip_targets = BTreeMap::new();
    if let Ok(cat) = &catalog {
        for e in &cat.entries {
            let slot = e.placement.index as u8;
            clip_targets.insert(slot, e.special);
            let Some(rec) = (e.placement.anim_id as usize).checked_sub(1) else {
                continue;
            };
            let bundle = if host.world.npc_clip_party_bank(slot, e.special) {
                locomotion_anm.as_ref()
            } else {
                scene_anm.as_ref()
            };
            if let Some(player) = bundle.and_then(|b| FieldClipPlayer::from_record(b, rec)) {
                host.world
                    .bind_npc_clip_cursor(slot, e.placement.anim_id, &player);
                clip_players.insert(slot, player);
            }
        }
    }

    snap_bound_actors_to_floor(host);
    SceneActorBinding {
        scene_anm,
        locomotion_anm,
        catalog,
        clip_players,
        clip_targets,
    }
}

/// Bind the lead's locomotion clips: on a field or kingdom map whose lead
/// resolves a rig mesh (`SceneHost::player_rig_mesh` - the lead's field form,
/// or the model a `CC F8 50` re-staged the player onto), the roster lead's
/// whole bank (Vahn / Noa / Gala only; `FieldPlayerAnim::from_locomotion_bank`),
/// from which the world's settle tail picks idle / walk / run / hop each tick.
/// Anything else clears the clips. The lead's slot is marked bound.
///
/// Runs at scene entry, and again when a script re-stages the lead's model
/// (`World::take_player_rig_change`), on both hosts.
// REF: FUN_8001E890
pub fn bind_lead_rig(host: &mut SceneHost, locomotion_anm: Option<&PlayerAnmBundle>) {
    let rig = host.player_rig_mesh().is_some();
    let world = &mut host.world;
    world.set_field_player_anim(None);
    if !matches!(world.mode, SceneMode::Field | SceneMode::WorldMap) {
        return;
    }
    let Some(pslot) = world.player_actor_slot else {
        return;
    };
    if !rig {
        log::warn!(
            "scene actors: no rig mesh for the lead (model {:?})",
            world.locomotion.player_live_model
        );
        return;
    }
    // Bound so the lead counts as a modelled actor; a drawing host re-points
    // the slot at its own mesh of the rig.
    if world
        .actors
        .get(pslot as usize)
        .is_some_and(|a| a.tmd_binding.is_none())
    {
        world.set_actor_tmd_binding(pslot as usize, pslot as usize);
    }
    let roster_lead = usize::from(world.field_leader());
    let anim = locomotion_anm
        .filter(|_| roster_lead <= 2)
        .and_then(|bundle| FieldPlayerAnim::from_locomotion_bank(bundle, roster_lead));
    world.set_field_player_anim(anim);
}

/// Every bound actor still at the flat default `world_y` takes the floor
/// height under it. Locomotion re-samples the floor only on a step, so an
/// actor standing still since spawn would otherwise rest at `0` while the
/// ground sits at a LUT-elevated tier.
pub fn snap_bound_actors_to_floor(host: &mut SceneHost) {
    let world = &mut host.world;
    if !world.locomotion.follow_terrain_height
        || !matches!(world.mode, SceneMode::Field | SceneMode::WorldMap)
    {
        return;
    }
    for i in 0..world.actors.len() {
        if world.actors[i].tmd_binding.is_none() || world.actors[i].move_state.world_y != 0 {
            continue;
        }
        let ms = &world.actors[i].move_state;
        let y = world.sample_field_floor_height(ms.world_x as i32, ms.world_z as i32);
        world.actors[i].move_state.world_y = y as i16;
    }
}
