//! Disc-parsed static tables installed at boot / scene load (items, spells, arts, monsters, formations, move power, equipment, thresholds, CDNAME map).
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Disc-parsed static tables installed at boot / scene load (items, spells, arts, monsters, formations, move power, equipment, thresholds, CDNAME map).
pub struct DiscTables {
    /// The static `SCUS_942.54` win-pose table (`0x800788A0`) the results
    /// frame picks the leader's victory pose from. Installed at boot by the
    /// shell (`legaia_asset::victory_pose`); `None` on a disc-free build,
    /// where the pose actor simply keeps its idle.
    pub victory_pose_table: Option<legaia_asset::victory_pose::VictoryPoseTable>,
    /// Item catalog used by item-action resolution. Populated at battle
    /// init from [`crate::items::ItemCatalog::vanilla`] (or a custom
    /// catalog set by [`crate::world::World::set_item_catalog`]); empty by default so
    /// the field VM doesn't trigger item effects in non-battle scenes.
    pub item_catalog: crate::items::ItemCatalog,
    /// Real on-disc item-effect descriptor table ([`legaia_asset::item_effect`],
    /// `DAT_800752C0`), if the boot source's `SCUS_942.54` was readable. When
    /// present, [`crate::world::World::set_item_catalog`] applies its field/battle usability
    /// flags onto the installed catalog so item-menu gating matches retail.
    pub item_effects: Option<legaia_asset::item_effect::ItemEffectTable>,
    /// Spell catalog used by the player-driven battle Magic submenu to resolve
    /// spell ids → names / MP cost / effect. Populated at battle init from
    /// [`crate::spells::SpellCatalog::vanilla`] (or a custom catalog via
    /// [`crate::world::World::set_spell_catalog`]); empty by default.
    pub spell_catalog: crate::spells::SpellCatalog,
    /// Art-record catalog used by the player-driven battle Arts submenu to
    /// resolve a saved chain → its real per-strike power profile. Keyed by
    /// `(character, art constant)`; populated from disc art data (PROT entry
    /// `0x05C4`) via [`crate::world::World::set_art_record`] when available. Empty by
    /// default - the Arts submenu then falls back to a synthetic power profile
    /// derived from the chain's directional commands
    /// (see [`crate::battle_arts::synthetic_power`]).
    pub art_records: std::collections::HashMap<
        (legaia_art::Character, legaia_art::ActionConstant),
        legaia_art::ArtRecord,
    >,
    /// Per-actor character max MP. The retail `BattleActor` holds only
    /// the running `mp` value (not the cap); the cap lives on the
    /// character record at `+0x140`. Engines populate this from the
    /// character record at battle init.
    pub character_max_mp: Vec<u16>,
    /// Optional formation table - engines install this at boot via
    /// [`crate::world::World::set_formation_table`] so triggered encounters can resolve
    /// their `formation_id` into concrete monster slot definitions.
    pub formation_table: crate::monster_catalog::FormationTable,
    /// Optional monster catalog - paired with `formation_table`. Engines
    /// look up [`crate::monster_catalog::MonsterDef`] by id when
    /// initialising the [`crate::battle_session::BattleSession`].
    pub monster_catalog: crate::monster_catalog::MonsterCatalog,
    /// Optional battle-action move-power table (PROT 0898, runtime VA
    /// `0x801F4F5C`). When present, the monster special-attack damage path
    /// resolves each move id to its real per-move power scalar and rolls
    /// damage through the faithful arts/physical kernel
    /// (`crate::world::World::enemy_move_predamage`); when `None` (disc-free /
    /// synthetic battles) that path falls back to the MP-scaled placeholder, so
    /// the RNG stream and determinism trace are unchanged. Installed lazily from
    /// the disc by [`crate::scene::SceneHost`].
    pub move_power: Option<crate::move_power::MovePowerCatalog>,
    /// Raw bytes of the battle-action overlay (PROT 0898) the [`move_power`]
    /// catalog was parsed from, retained so the move-FX render path can read the
    /// `0x801f6324` prototype records' move-VM bytecode (the catalog holds only
    /// the parsed tables). Installed alongside [`move_power`] by
    /// [`crate::scene::SceneHost`]; `None` in disc-free battles (move FX simply
    /// don't spawn). `Arc` so cloning `World` stays cheap.
    pub move_power_overlay: Option<Arc<[u8]>>,
    /// Battle **element-affinity** tables ([`legaia_asset::element_affinity`],
    /// matrix `0x801F53E8` + per-character element table `0x801F5480`). When
    /// present, the monster special-attack damage path scales the attacker roll
    /// by `matrix[enemy_element][party_member_element]` (`FUN_801dd864`);
    /// `None` (disc-free / synthetic battles) keeps the neutral 100% multiplier,
    /// so the damage and determinism trace are unchanged. Installed lazily from
    /// the same PROT 0898 overlay as [`crate::world::DiscTables::move_power`] by
    /// [`crate::scene::SceneHost`].
    pub element_affinity: Option<legaia_asset::element_affinity::ElementAffinity>,
    /// Per-character battle-camera height table
    /// ([`legaia_asset::battle_camera_table`], runtime VA `0x801F4D2C`) - the
    /// `TR.y` the submenu close-up framing (`FUN_801D5854` case `0`) reads for
    /// whichever character is acting. Installed from the same PROT 0898
    /// overlay as [`crate::world::DiscTables::move_power`] by [`crate::scene::SceneHost`]; `None`
    /// on disc-free hosts, which leaves the camera on its single traced
    /// fallback height so an unpinned character frames like the measured case
    /// instead of jumping.
    pub battle_camera_heights: Option<legaia_asset::battle_camera_table::BattleCameraHeights>,
    /// Seru-magic **side-effect** table
    /// ([`legaia_asset::seru_side_effect`], runtime VA `0x801F6870`,
    /// `[element][band]`): the percent a levelled player Seru cast's
    /// secondary debuff shaves off the target's stat on every hit, and the
    /// cure class light stages instead.
    ///
    /// Sibling static data in the same PROT 0898 overlay as
    /// [`crate::world::DiscTables::move_power`], installed by
    /// [`crate::scene::SceneHost`]. `None` on a disc-free host, which is the
    /// gate that keeps the stager
    /// ([`crate::world::World::stage_seru_side_effect`]) from running at
    /// all - so a synthetic battle stages nothing and draws no `rand()`.
    pub seru_side_effects: Option<legaia_asset::seru_side_effect::SeruSideEffectTable>,
    /// Player Seru spell id (`0x81..=0x8B`) -> the **summon creature's**
    /// record element (`+0x1D`), the byte the side-effect stager switches on
    /// and the affinity scale reads as the attacker element.
    ///
    /// Resolved at scene entry from the monster archive through
    /// [`crate::summon::summon_creature_id`]. It is a separate table from the
    /// monster catalog on purpose: the catalog only ever holds the *scene's*
    /// own monsters, so a summon creature is absent from it in almost every
    /// fight, and the catalog-by-name lookup
    /// (`World::summon_attacker_element`) answers `None` there. Empty on a
    /// disc-free host.
    pub summon_elements: std::collections::HashMap<u8, u8>,
    /// Static `SCUS_942.54` per-monster **steal** table
    /// ([`legaia_asset::steal_table`], `DAT_80077828 + monster_id * 2`, fields
    /// `[chance, item]`). Install via
    /// [`crate::world::World::set_steal_table`]; `None` on a disc-free host,
    /// which is what keeps a synthetic battle from granting a steal.
    ///
    /// Two consumers: [`crate::world::World::apply_steal`]'s no-argument
    /// sibling, and PROT 0941's `0x51` Steal body, whose **monster-seat** leg
    /// resolves off this table rather than off the bag
    /// (`docs/formats/steal-table.md`; the table is NOT in the PROT 867
    /// monster record).
    pub steal_table: Option<legaia_asset::steal_table::StealTable>,
    /// Per-item battle-stat modifier table (weapon / armor / accessory
    /// bonuses). Empty by default; install via [`crate::world::World::set_equipment_table`]
    /// so [`crate::world::World::seed_party_battle_stats`] folds equipped gear onto each
    /// party combatant's attack / defense at battle entry.
    pub equipment_table: crate::battle_stats::EquipmentTable,
    /// The **raw** static equipment stat-bonus table (`DAT_80074F68`, stride 8)
    /// as parsed off `SCUS_942.54`, kept alongside the derived modifier table
    /// because two retail readers want the record bytes rather than the
    /// modifiers: the Throw Out list builder reads each record's `+7` flags
    /// byte, and the item-detail window reads `+5`. Install via
    /// [`crate::world::World::set_equip_stats`]; `None` on a disc-free load,
    /// where the Throw Out list simply dims nothing.
    pub equip_stats: Option<legaia_asset::equip_stats::EquipStatTable>,
    /// Accessory ("Goods") passive-effect catalog: item id → passive index +
    /// per-index party-wide scope, decoded from the executable. Empty by
    /// default; install via [`crate::world::World::set_accessory_passives`].
    /// [`crate::world::World::refresh_party_ability_bits`] derives each member's ability
    /// bitfield from it, and
    /// [`crate::battle_stats::compute_battle_stats_with_passives`] applies the
    /// percent stat boosts inside [`crate::world::World::seed_party_battle_stats`].
    pub accessory_passives: crate::accessory_passives::AccessoryPassives,
    /// CDNAME `#define` map (raw in-RAM PROT TOC index → block name),
    /// installed once by the scene host from the disc's `CDNAME.TXT`
    /// ([`crate::world::World::install_scene_toc_names`]). This is the id space a
    /// quick-travel placement record's `scene_id` lives in - the on-disc
    /// values (`0x55` map01 / `0xF4` map02 / `0x187` map03 / `0x162` son /
    /// `0x215` korout) are the destination scenes' own `#define` numbers,
    /// the same words the world-map arrival kernel matches `0x80084628`
    /// against (`FUN_801EE328`). Empty on a PROT.DAT-only load; the menu
    /// warp drain then reports retail's `UNFIND MAP NUMBER` diagnostic
    /// instead of warping.
    pub scene_toc_names: legaia_prot::cdname::IndexMap,
    /// Magic-XP threshold table from `SCUS_942.54` (`0x8007656C`, 8 ascending
    /// u16 steps). Installed at boot via
    /// [`crate::world::World::install_magic_xp_thresholds`]; while `None` (disc-free) summon
    /// casts still accrue spell XP but never level the spell up.
    pub magic_xp_thresholds: Option<[u16; crate::magic_xp::THRESHOLD_STEPS]>,
    /// Seru-trade config from the patched disc (the randomizer's `--seru-trade`
    /// blob: enabled flag + master seed + offer cap). Installed at boot via
    /// [`crate::world::World::install_seru_trade_config`]; `None` (or `enabled == false`)
    /// disables vendor seru trading. See [`crate::seru_trade`].
    pub seru_trade_config: Option<legaia_asset::seru_trade::SeruTradeConfig>,
    /// The battle draw's per-character Rot limb object ranges
    /// (`SCUS_942.54` `0x80077998`, [`legaia_engine_vm::battle_actor_draw`]).
    /// Installed by [`crate::world::World::install_menu_text`]; while `None`
    /// (disc-free) no rotted limb dims.
    pub rot_limb_table: Option<legaia_engine_vm::battle_actor_draw::RotLimbTable>,
}

impl DiscTables {
    pub fn new() -> Self {
        Self {
            item_catalog: crate::items::ItemCatalog::default(),
            item_effects: None,
            spell_catalog: crate::spells::SpellCatalog::default(),
            art_records: std::collections::HashMap::new(),
            magic_xp_thresholds: None,
            seru_trade_config: None,
            character_max_mp: Vec::new(),
            formation_table: crate::monster_catalog::FormationTable::new(),
            monster_catalog: crate::monster_catalog::MonsterCatalog::new(),
            move_power: None,
            move_power_overlay: None,
            element_affinity: None,
            battle_camera_heights: None,
            seru_side_effects: None,
            summon_elements: std::collections::HashMap::new(),
            steal_table: None,
            equipment_table: crate::battle_stats::EquipmentTable::new(),
            equip_stats: None,
            accessory_passives: Default::default(),
            scene_toc_names: legaia_prot::cdname::IndexMap::new(),
            victory_pose_table: None,
            rot_limb_table: None,
        }
    }
}

impl Default for DiscTables {
    fn default() -> Self {
        Self::new()
    }
}
