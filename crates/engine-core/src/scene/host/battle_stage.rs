//! Which PROT entry a battle is fought inside, resolved from the live scene
//! host - the one call both play hosts build the backdrop from.
//!
//! Retail does not look the backdrop up per scene. The region reader
//! `FUN_801D9E1C` stamps the stage variant `_DAT_8007BD60 = region[+8] &
//! 0x1F` for the region the player stands in - every step, and again on the
//! op-`0x3E` scripted-battle install - and battle init `FUN_800513F0` loads
//! entry `scene_index + variant` through `FUN_8001FA88`
//! ([`crate::region_encounter::battle_stage_entry_for_variant`]). So a scene
//! with several sub-area backdrops fights in whichever one the region names.

use super::SceneHost;

impl SceneHost {
    /// The battle-stage backdrop entry for a fight starting now: the entry the
    /// last region setup's stage variant names in the loaded scene, falling
    /// back to [`crate::scene::ProtIndex::battle_stage_entry_for_scene`] when
    /// no region setup has been applied (a scene with no region section) or
    /// the variant does not name a stage stream.
    ///
    /// REF: FUN_800513F0, FUN_801D9E1C
    pub fn battle_stage_entry(&self) -> Option<u32> {
        let scene = self.scene.as_ref()?;
        self.world
            .encounters
            .region_setup
            .and_then(|s| {
                self.index
                    .battle_stage_entry_for_region(&scene.name, s.stage_variant)
            })
            .or_else(|| self.index.battle_stage_entry_for_scene(&scene.name))
    }

    /// Whether battle init keeps the backdrop shell's object 1: retail
    /// `_DAT_8007B64B`, bit 5 of the last long-layout region record's
    /// `+8` byte (`FUN_800513F0` skips its drop-object-1 pass when set,
    /// `0x80051ABC`). `false` - the drop - until a long-layout region says
    /// otherwise.
    pub fn battle_stage_keeps_object_1(&self) -> bool {
        self.world
            .encounters
            .region_setup
            .and_then(|s| s.keep_backdrop_object_1)
            .unwrap_or(false)
    }
}
