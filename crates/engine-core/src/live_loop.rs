//! The one place a host arms the live gameplay loop on a freshly-entered
//! scene.
//!
//! Both hosts used to carry their own copy of this block - the native
//! `BootSession::enter_field_live` and the browser's
//! `LegaiaRuntime::arm_live_battles` - and they drifted: the browser's copy
//! omitted the scene label and the Battle<->Field BGM swap, which is why
//! battle music was silent in the browser while it worked natively. A
//! renderer-free kernel here is what keeps them one implementation.
//!
//! What stays out: the equipment / spell / item catalogs. Those are
//! disc-derived on native and fetched differently in the browser, so each
//! host installs its own before calling in.

use crate::world::{SceneMode, World};

/// How much of the live gameplay loop to arm.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveLoopOpts {
    /// Arm the field side of the Field<->Battle round trip
    /// ([`World::live_gameplay_loop`]): walking a scene rolls step-driven
    /// random encounters.
    ///
    /// This does **not** gate battle *driving* - a battle the world is in is
    /// always driven to resolution (see [`World::live_gameplay_loop`]) - and
    /// it is **independent of `player_battle`**. The two used to be
    /// entangled ("player battle implies the loop"), which made a
    /// "player-driven battles, no random encounters" configuration
    /// unexpressible: the implication silently re-armed the roll.
    pub live_loop: bool,
    /// Make battles player-driven ([`World::battle_player_driven`]): each
    /// party turn opens the command menu instead of auto-attacking, and the
    /// Seru-learning registry a player-driven battle needs is installed.
    /// Orthogonal to `live_loop` - it decides how a battle is *played*, not
    /// whether one starts.
    pub player_battle: bool,
    /// Battle<->Field BGM swap track id, resolved through the scene's own BGM
    /// table by the live loop. `None` leaves the field track playing through
    /// the fight.
    pub battle_bgm: Option<u16>,
}

impl LiveLoopOpts {
    /// The shipped default: the full loop, player-driven, no BGM swap
    /// configured (a host that knows its scene's battle track sets it).
    pub fn playable() -> Self {
        Self {
            live_loop: true,
            player_battle: true,
            battle_bgm: None,
        }
    }
}

impl World {
    /// Arm the live gameplay loop on `scene`, which the caller has already
    /// entered.
    ///
    /// - Tags the world with the scene label.
    /// - Falls back to the synthetic encounter registry **only** when the
    ///   scene's own MAN carried no encounter table (towns resolve to a 0 %
    ///   trigger rate there, so this invents no town fights).
    /// - Applies `opts`: the field-side encounter roll, player-driven
    ///   battles + the Seru registry, and the battle BGM swap.
    pub fn arm_live_loop(&mut self, scene: &str, opts: &LiveLoopOpts) {
        self.set_active_scene_label(scene);

        if self.encounter.is_none() && matches!(self.mode, SceneMode::Field) {
            self.set_formation_table(
                crate::monster_catalog::vanilla_formation_table(),
                crate::monster_catalog::vanilla_monster_catalog(),
            );
            let registry = crate::encounter_registry::vanilla_encounter_registry();
            self.install_encounter_for_scene(&registry, scene);
        }

        if opts.live_loop {
            self.live_gameplay_loop = true;
        }
        self.set_battle_bgm(opts.battle_bgm);
        if opts.player_battle {
            self.battle_player_driven = true;
            self.set_seru_registry(crate::seru_learning::SeruRegistry::retail());
        }
        self.refresh_encounter_rollable();
        if self.live_gameplay_loop && !self.scene_encounters_rollable {
            self.scene_encounter_hint_frames = Self::ENCOUNTER_HINT_FRAMES;
            log::info!(
                "live loop armed on '{scene}', but the scene rolls no random encounters \
                 (all regions rate 0 or shadowed) - this is retail scene data, not a fault"
            );
        } else {
            self.scene_encounter_hint_frames = 0;
        }
    }

    /// How long the "no random encounters in this scene" hint stays up after
    /// entering such a scene, in sim ticks (~5 s at the 100 Hz sim clock).
    /// Bounded rather than permanent - it is an orientation aid, not a
    /// persistent HUD element.
    pub const ENCOUNTER_HINT_FRAMES: u16 = 500;

    /// Whether a host should be drawing the "this scene rolls no random
    /// encounters" hint this frame.
    pub fn show_encounter_hint(&self) -> bool {
        self.scene_encounter_hint_frames > 0
            && self.live_gameplay_loop
            && !self.scene_encounters_rollable
            && matches!(self.mode, SceneMode::Field | SceneMode::WorldMap)
    }

    /// Recompute the cached [`Self::scene_encounters_rollable`] answer.
    ///
    /// Called whenever the encounter tables change (scene entry's region
    /// routing, and [`Self::arm_live_loop`]). The underlying scan is a pass
    /// over the region AABBs, so it is cached rather than re-run per frame by
    /// a HUD.
    pub fn refresh_encounter_rollable(&mut self) {
        // Resolve each installed region table's story-flag group first: which
        // regions exist at all is a function of the live flag bank, so a
        // rollability answer taken against a stale group is an answer about a
        // different story state.
        if let Some(mut t) = self.field_region_tracker.take() {
            t.select_group(|flag| self.system_flag_test(flag));
            self.field_region_tracker = Some(t);
        }
        if let Some(mut t) = self.world_map_region_tracker.take() {
            t.select_group(|flag| self.system_flag_test(flag));
            self.world_map_region_tracker = Some(t);
        }
        self.scene_encounters_rollable = self.scene_can_roll_encounters();
    }

    /// Restore every roster record's HP / MP to its maximum and re-seed the
    /// party actors' battle mirrors from it.
    ///
    /// Same record fields as the world-map panel's party-restore arm
    /// (`FUN_801EE90C`: `hp_max -> hp_cur`, `mp_max -> mp_cur`), plus the
    /// live mirrors so the field HUD agrees. The game-over "Retry" row needs
    /// it: now that a battle's HP survives the fight
    /// ([`Self::finish_battle`]), dropping a wiped party straight back into
    /// the field would just re-wipe on the next encounter.
    pub fn revive_party_full(&mut self) {
        for rec in self.roster.members.iter_mut() {
            let mut hms = rec.hp_mp_sp();
            hms.hp_cur = hms.hp_max;
            hms.mp_cur = hms.mp_max;
            rec.set_hp_mp_sp(hms);
        }
        self.resync_party_actors_from_roster();
    }

    /// Whether the scene the world is currently in can produce a random
    /// encounter at all.
    ///
    /// Answers the question in the same order the roll paths consult their
    /// tables: the overworld asks its region tracker, a field scene asks its
    /// per-region tracker when one is routed (only the story-flag group the
    /// condition walk selected counts, and regions shadow in group order -
    /// see [`crate::region_encounter::RegionEncounterTable::any_rollable`])
    /// and otherwise falls back to the aggregated mean-rate session. Either
    /// field path additionally needs the session installed, because the
    /// session owns the transition / grace bracketing a trigger goes through.
    ///
    /// The answer is per (scene, story state), not per scene. Several retail
    /// scenes legitimately answer `false` in the state you are in - notably
    /// `town01`, the scene the binary boots into, whose default group is
    /// entirely rate 0 (Rim Elm only fights back once its "under attack" flag
    /// is set). That is scene data, and the port keeps it; a host surfaces the
    /// answer so "I walked for a minute and nothing happened" reads as the
    /// scene's design rather than as a broken engine.
    pub fn scene_can_roll_encounters(&self) -> bool {
        if matches!(self.mode, SceneMode::WorldMap) {
            return self
                .world_map_region_tracker
                .as_ref()
                .is_some_and(|t| t.table().any_rollable());
        }
        // The region tracker answers first: its session bracket is installed
        // lazily on the first roll, so an `encounter.is_none()` early return
        // ahead of this branch reports "no encounters in this scene" for a
        // region scene that has not rolled yet.
        if let Some(t) = self.field_region_tracker.as_ref() {
            return t.table().any_rollable();
        }
        if self.encounter.is_none() {
            return false;
        }
        self.encounter
            .as_ref()
            .is_some_and(|s| !s.tracker().table().is_empty())
    }
}
