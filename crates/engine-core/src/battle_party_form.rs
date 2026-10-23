//! A party member's **battle form**: the `World` adapter.
//!
//! The build itself - the player-file assembly, the PROT 1204 fallback, the
//! band pixels and palette, the art bank - is
//! [`legaia_engine_battle::battle_party_form`], which names no `World`.
//! This module is the half that does: it reads each present member's roster
//! slot and equipment off the world, builds every form over the disc index
//! ([`ProtFormDisc`]), and installs the clips, art bank and art records on
//! the actors.
//!
//! Building and installing the forms is the engine's battle-entry duty, not
//! a host's: [`install_party_battle_forms`], run once per fight by
//! `SceneHost::ensure_battle_party_forms` from `SceneHost::tick`, so a
//! headless session fights with the same clips and arts a windowed one does.
//!
//! The native window and the browser play page each once carried the whole
//! build ladder, and the copies had drifted: a missing idle stream fell back
//! to PROT 1204 on the native window and kept the assembled mesh (with no
//! pose source) on the page, and only the native window overlaid the
//! character's battle palette on a fallback mesh. The one copy keeps the
//! native window's choices; the hosts keep only what differs by renderer
//! (the VRAM replay, the GPU mesh upload and the per-frame posing).

use std::sync::Arc;

use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_engine_battle::battle_party_form::{
    BattlePartyForms, PartyBattleForm, PartyFormDisc, PartyFormSources, VramSink, art_character,
};
use legaia_engine_scene::scene::ProtIndex;

/// The engine's disc index as the battle-form build's disc reader.
pub struct ProtFormDisc<'a>(pub &'a ProtIndex);

impl PartyFormDisc for ProtFormDisc<'_> {
    fn entry(&self, prot: u32) -> Result<Arc<Vec<u8>>, String> {
        self.0.entry_bytes(prot).map_err(|e| format!("{e:#}"))
    }
}

/// Build every present member's battle form and install it on its actor -
/// the battle-entry duty every session owes, windowed or headless. The
/// VRAM writes are recorded, not applied; a renderer replays them.
///
/// This is the injection point for a session that holds a [`World`] but no
/// [`legaia_engine_scene_host::scene_host::SceneHost`] (and so no disc index): it calls this itself
/// after [`crate::world::World::enter_battle`]. A world with no disc behind
/// it has no player files to assemble, and its party fights with no clips
/// and no arts.
///
/// [`World`]: crate::world::World
pub fn install_party_battle_forms(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> BattlePartyForms {
    let mut out = BattlePartyForms {
        battle_serial: world.battle.entry_serial,
        ..Default::default()
    };
    // The monsters' side-band pages ride the same replayed log: retail
    // streams them on each monster turn, and the module casts sample them.
    crate::battle_sideband_textures::record_monster_sideband_textures(
        index,
        world,
        &mut out.vram_writes,
    );
    let party_count = usize::from(world.party.party_count).min(3);
    if party_count == 0 {
        return out;
    }
    let Some(sources) = PartyFormSources::load(&ProtFormDisc(index)) else {
        log::warn!("battle party: PROT 1204 fallback pack unavailable; no battle forms");
        return out;
    };
    for member in 0..party_count {
        if let Some(mut form) =
            build_party_battle_form(index, world, &sources, &mut out.vram_writes, member)
        {
            world.install_party_battle_form(&mut form);
            out.forms.push(form);
        }
    }
    out
}

/// Build member `member`'s battle form off the world's roster: the
/// occupying character's roster slot and its equipped item ids (from the
/// canonical roster record; an absent or zeroed record assembles the
/// all-default, unequipped sections), then
/// [`legaia_engine_battle::battle_party_form::build_party_battle_form`].
pub fn build_party_battle_form(
    index: &ProtIndex,
    world: &crate::world::World,
    sources: &PartyFormSources,
    vram: &mut impl VramSink,
    member: usize,
) -> Option<PartyBattleForm> {
    let cslot = world.party_roster_slot(member);
    let equipped: [u8; 5] = world
        .party
        .roster
        .members
        .get(cslot)
        .map(|rec| {
            let s = rec.equipment().slots;
            [s[0], s[1], s[2], s[3], s[4]]
        })
        .unwrap_or_default();
    legaia_engine_battle::battle_party_form::build_party_battle_form(
        &ProtFormDisc(index),
        sources,
        vram,
        member,
        cslot,
        &equipped,
    )
}

impl crate::world::World {
    /// Install a party member's battle form on its actor: the idle clip,
    /// the action-clip set, the art bank, and the art records the live arts
    /// input tokenizes (a fallback form has none).
    ///
    /// Called by [`install_party_battle_forms`] at battle entry, for every
    /// session; the facial animator's registration
    /// ([`PartyBattleForm::face_tracks`]) stays with the host that owns the
    /// band's live VRAM.
    pub fn install_party_battle_form(&mut self, form: &mut PartyBattleForm) {
        let member = form.member;
        if let Some(anim) = &form.idle
            && let Some(player) = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(anim)
        {
            self.set_actor_battle_animation(member, player);
        }
        if let Some(clips) = form.action_clips.take() {
            self.set_actor_battle_action_clips(member, std::sync::Arc::new(clips));
        }
        let bank = std::mem::take(&mut form.art_bank);
        if !bank.is_empty() {
            self.set_actor_battle_art_bank(member, std::sync::Arc::new(bank));
        }
        if let Some(character) = art_character(form.cslot) {
            self.install_art_bank_records(character, &form.art_records);
        }
        // The seat's model table, fresh per fight (retail registers the
        // node and snapshots the pairs at every battle load). The draw set
        // starts as the idle stream's channels until the first pose.
        if let Some(a) = self.actors.get_mut(member) {
            a.battle_model_table = form.model_table.clone();
            a.battle_drawn_objects = form
                .model_table
                .as_ref()
                .map(|t| t.drawn_objects(usize::from(t.idle_parts)));
        }
    }

    /// Install a monster's battle form on its actor: the texture slot its
    /// mesh was relocated into (the posed rebuild re-applies it) and the
    /// archive idle clip, unless one is already playing. The archive-order action clips are the engine's own
    /// to stage (`SceneHost::tick`), so none is installed here.
    pub fn install_monster_battle_form(
        &mut self,
        actor: usize,
        tex_slot: u8,
        idle: Option<&MonsterAnimation>,
    ) {
        if let Some(a) = self.actors.get_mut(actor) {
            a.battle_tex_slot = Some(tex_slot);
        }
        // The scene host seats the idle the tick the battle is up
        // (`SceneHost::install_battle_monster_action_clips`); a render build
        // that lands later does not restart a loop already playing, or the
        // host's run of a fight drifts a tick from a headless one.
        if self
            .actors
            .get(actor)
            .is_some_and(|a| a.battle_animation.is_none())
            && let Some(anim) = idle
            && let Some(player) = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(anim)
        {
            self.set_actor_battle_animation(actor, player);
        }
    }
}
