//! The three selector inputs the battle-intro style picker reads, resolved
//! from the live scene host.
//!
//! [`legaia_engine_vm::battle_intro_styles::select_intro_style`] takes
//! `IntroStyleInputs { battle_flags, formation_slot0, scene_index }` - the
//! retail globals `DAT_8007BD60`, `DAT_8007BD0C` and `DAT_80084540`. None of
//! the three is a host choice: all three are properties of the rolled
//! formation row and the loaded scene, so resolving them belongs here rather
//! than in each host's emitter arming.
//!
//! Both hosts arm the emitter from their own `arm_battle_intro`, and the
//! resolution drifted between them: the native window carried the live
//! monster-table fallback for `formation_slot0` and the browser play page
//! went straight from the formation-table lookup to the bare row index. The
//! fallback is not decoration - `formation_slot0` is what every id-keyed
//! style override keys on, so a host that answers it with a row index
//! renders the default TileShatter for battles retail gives a Curtain, a
//! Swirl or one of the three ScatterParticles arms.

use super::SceneHost;
use legaia_engine_vm::battle_intro_styles::IntroStyleInputs;

impl SceneHost {
    /// The battle's **first monster id** - retail's `DAT_8007BD0C`, slot 0 of
    /// the resolved formation cell.
    ///
    /// Three sources, in the order retail's own value becomes available:
    ///
    /// 1. the rolled formation row's own slot 0. This is the only source that
    ///    answers while the encounter sits in its `Transition` phase, which is
    ///    when the intro is armed: the world is still in Field mode there and
    ///    [`crate::world::World::battle_monster_slots`] returns empty outside
    ///    `SceneMode::Battle`;
    /// 2. the live actor table, for an in-battle re-arm (a second wave, a
    ///    scripted re-entry) where the formation row is not the authority;
    ///    and
    /// 3. the formation id itself - a **row index**, not a monster id, and
    ///    therefore wrong for the selector. It is the last resort only
    ///    because the emitter must produce some style.
    pub fn battle_intro_slot0(&self, formation_id: u16) -> u8 {
        self.world
            .tables
            .formation_table
            .formation(formation_id)
            .and_then(|d| d.slots.first())
            .map(|s| s.monster_id as u8)
            .or_else(|| {
                self.world
                    .battle_monster_slots()
                    .first()
                    .map(|&(_, id, _)| id as u8)
            })
            .unwrap_or(formation_id as u8)
    }

    /// All three selector inputs for `formation_id`.
    ///
    /// `battle_flags` is `DAT_8007BD60`, of which the selector reads only bit
    /// `0x80`; the entity SM's confirm state ORs that bit in when the rolled
    /// row's `record[+0]` is non-zero (`FUN_801DA51C` at
    /// `0x801DA5F8..0x801DA61C`), which is what makes the scripted / boss arm
    /// reachable at all. `scene_index` is `DAT_80084540`, the loaded scene's
    /// PROT base.
    pub fn battle_intro_style_inputs(&self, formation_id: u16) -> IntroStyleInputs {
        IntroStyleInputs {
            battle_flags: self
                .world
                .tables
                .formation_table
                .formation(formation_id)
                .map(|d| d.per_battle_flags())
                .unwrap_or(0),
            formation_slot0: self.battle_intro_slot0(formation_id),
            scene_index: self.scene.as_ref().map(|s| s.start).unwrap_or(0),
        }
    }
}
