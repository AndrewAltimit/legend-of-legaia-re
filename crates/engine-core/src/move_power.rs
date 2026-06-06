//! Engine-side wrapper over the battle-action **move-power table**
//! ([`legaia_asset::move_power`], runtime VA `0x801F4F5C`, PROT entry 0898).
//!
//! The table is the one true per-move power scalar in the battle system: the
//! arts / physical damage kernel `FUN_801dd0ac` reads its `+0` power for the
//! attacker roll (`rand % ((power >> 2) + 1) + … + power`). The asset crate
//! parses it and the `0x801F4E63` id → index map off the raw overlay bytes;
//! this module pairs the two so a live battle actor's chosen move id
//! (`actor[+0x1df]`, carried on the engine side as a battle move id) resolves
//! straight to its power record.
//!
//! Loaded lazily from PROT entry 0898 by [`crate::scene::SceneHost`] and parked
//! on [`crate::world::World::move_power`]; the monster special-attack damage
//! path consumes it (see `World::enemy_move_predamage`). Disc-free / synthetic
//! battles leave it `None` and keep the placeholder damage path, so no
//! determinism trace changes when the table is absent.
//!
//! ## Behavioural fields ([`MoveFx`])
//!
//! `+0` power feeds damage; the rest of the 26-byte record is the move's
//! presentation / timing behaviour (strike-Y offset, phase counters, homing,
//! impact-effect selector, trail texpage, sound cue, and the two effect-id
//! lists). The asset crate decodes every field; [`MovePowerCatalog::fx_for_move_id`]
//! resolves them for a battle move id into a [`MoveFx`], joining the effect-id
//! lists to the auxiliary spawn-prototype / SFX tables and the impact selector to
//! its packed config word.
//!
//! This is the engine-side *descriptor* surface. The render / audio consumers
//! (drawing the trail, spawning the contact / launch effects, playing the SFX
//! cues) are a deliberate follow-up. The `0x01..=0x63` spawn entries resolve to a
//! `0x801F6324` prototype param that is a **move-VM scene-graph record** in the
//! same format the player Seru-magic summons use (`legaia_asset::summon_overlay`,
//! staged by `FUN_80021B04` → the ported move VM, with `model_sel` indexing the
//! `DAT_8007C018` TMD pool). Wiring it reuses that machinery; the `gp[0x754]`
//! additive base for `model_sel` is live-captured = 3 in battle (a move-FX
//! `FUN_80021B04` spawn), so a battle move-FX mesh is `DAT_8007C018[model_sel + 3]`.
//! The high-bit
//! (`0x80`) list bytes instead route to the 2D `efect.dat` pool
//! ([`crate::world::World::effect_pool`] / `spawn_by_ui_id`). See
//! `docs/formats/move-power.md`.

use legaia_asset::move_power::{
    self, EffectAuxTables, EffectListEntry, IMPACT_EFFECT_TABLE_LEN, MOVE_ID_INDEX_MAP_LEN,
    MoveRecord, parse_impact_effect_table,
};

/// PROT / CDNAME index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_ENTRY: u32 =
    move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX as u32;

/// The parsed move-power table + its id → index map, ready for id lookups.
#[derive(Debug, Clone)]
pub struct MovePowerCatalog {
    /// 26-byte power records (index 0 is the unused all-zero slot).
    table: Vec<MoveRecord>,
    /// 128-byte battle-move-id → table-index map (`0x801F4E63`).
    id_index_map: [u8; MOVE_ID_INDEX_MAP_LEN],
    /// The `0x801F6324` spawn-prototype + `0x801F6418` SFX tables the effect-id
    /// lists index. `None` when the overlay slice doesn't reach them (e.g. the
    /// synthetic table-only buffers the unit tests build); the FX descriptor
    /// then leaves [`ResolvedEffect::proto`] / `sfx` unresolved.
    aux: Option<EffectAuxTables>,
    /// The 5-entry `0x801f53d4` impact-effect packed-config table the `+0x0a`
    /// selector indexes (`(selector - 1)`). `None` when out of the slice.
    impact_table: Option<[u32; IMPACT_EFFECT_TABLE_LEN]>,
}

impl MovePowerCatalog {
    /// Parse the table + map out of the raw PROT 0898 (battle-action overlay)
    /// entry bytes. Returns `None` if either structural guard fails (the pinned
    /// offsets no longer land on the table — e.g. a different build).
    ///
    /// The auxiliary effect tables ([`EffectAuxTables`]) and the impact-effect
    /// config table are parsed best-effort: they live further into the same
    /// overlay than the power table, so a real boot fills them but a table-only
    /// fixture leaves them `None` (the FX descriptor still resolves every field
    /// the record itself carries, only the cross-table joins go unresolved).
    pub fn from_overlay_0898(overlay_0898: &[u8]) -> Option<Self> {
        let table = move_power::parse(overlay_0898)?;
        let id_index_map = move_power::parse_id_index_map(overlay_0898)?;
        let aux = EffectAuxTables::parse(overlay_0898);
        let impact_table = parse_impact_effect_table(overlay_0898);
        Some(Self {
            table,
            id_index_map,
            aux,
            impact_table,
        })
    }

    /// The power record for a battle move id (`actor[+0x1df]`), via the id →
    /// index map. `None` for ids the map marks as having no power record
    /// (`0`/`0xFF`) or out of the map's `0x00..=0x7F` range.
    pub fn record_for_move_id(&self, move_id: u8) -> Option<&MoveRecord> {
        move_power::record_for_move_id(&self.table, &self.id_index_map, move_id)
    }

    /// The roll-modulus base power `FUN_801dd0ac` derives from a move id's
    /// record (`(i16)power >> 2`), or `None` when the id has no record. This is
    /// the `power` fed to [`legaia_engine_vm::battle_formulas::arts_physical_predamage`].
    pub fn power_for_move_id(&self, move_id: u8) -> Option<i32> {
        self.record_for_move_id(move_id).map(|r| r.power())
    }

    /// The auxiliary spawn-prototype / SFX tables, when the overlay reached
    /// them. Used by [`Self::fx_for_move_id`] to resolve effect-list entries.
    pub fn aux_tables(&self) -> Option<&EffectAuxTables> {
        self.aux.as_ref()
    }

    /// The `0x801f53d4` impact-effect packed-config table, when present.
    pub fn impact_table(&self) -> Option<&[u32; IMPACT_EFFECT_TABLE_LEN]> {
        self.impact_table.as_ref()
    }

    /// Resolve a battle move id to its full presentation / timing descriptor.
    ///
    /// Bundles every behavioural field of the move's record (everything past the
    /// `+0` power scalar) and resolves the cross-table joins: the `+0x0a` impact
    /// selector to its packed config word, and each `+0x12` (on-contact) /
    /// `+0x16` (launch) effect-id-list byte to a [`ResolvedEffect`] carrying its
    /// [`EffectListEntry`] classification plus the spawn prototype / SFX cue for
    /// [`EffectListEntry::Spawn`] entries. `None` for ids with no power record.
    pub fn fx_for_move_id(&self, move_id: u8) -> Option<MoveFx> {
        let rec = self.record_for_move_id(move_id)?;
        let impact_effect = rec.impact_effect();
        // Selector is 1-based; `0` = no impact effect, so it indexes nothing.
        let impact_config = (impact_effect != 0)
            .then(|| {
                self.impact_table
                    .and_then(|t| t.get((impact_effect - 1) as usize).copied())
            })
            .flatten();
        Some(MoveFx {
            move_id,
            record_index: rec.index,
            strike_y_offset: rec.strike_y_offset(),
            counter_init: rec.counter_init(),
            phase_duration: rec.phase_duration(),
            homing_speed: rec.homing_speed(),
            effect_tracks_strike: rec.effect_tracks_strike(),
            impact_effect,
            impact_config,
            // `+0x0b` id → the GP0 texpage word the streak draw helper emits.
            trail_texpage: 0x7700u16 + rec.trail_texture_page() as u16,
            sound_cue_id: rec.sound_cue_id(),
            list_mode: rec.list_mode(),
            contact_effects: self.resolve_effect_list(&rec.contact_effects_raw()),
            launch_effects: self.resolve_effect_list(&rec.launch_effects_raw()),
        })
    }

    /// Classify each byte of a 4-entry effect-id list (the `FUN_801e09f8`
    /// dispatch order) and resolve [`EffectListEntry::Spawn`] indices through the
    /// aux tables. Stops at the first [`EffectListEntry::Terminator`] (`0x00`).
    fn resolve_effect_list(&self, raw: &[u8; 4]) -> Vec<ResolvedEffect> {
        let mut out = Vec::new();
        for &byte in raw {
            let entry = EffectListEntry::classify(byte);
            if entry == EffectListEntry::Terminator {
                break;
            }
            let (proto, sfx) = match entry {
                EffectListEntry::Spawn(idx) => match self.aux.as_ref() {
                    Some(aux) => (aux.effect_proto(idx), aux.effect_sfx(idx)),
                    None => (None, None),
                },
                _ => (None, None),
            };
            out.push(ResolvedEffect {
                raw: byte,
                entry,
                proto,
                sfx,
            });
        }
        out
    }

    /// Number of parsed records (including the unused slot 0).
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// One resolved entry of a move's `+0x12` / `+0x16` effect-id list: the raw
/// byte, its [`EffectListEntry`] classification, and (for
/// [`EffectListEntry::Spawn`]) the spawn-prototype param + SFX cue resolved
/// through the [`EffectAuxTables`]. `proto` / `sfx` are `None` for non-spawn
/// entries or when the aux tables weren't in the parsed overlay slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedEffect {
    /// The raw effect-id-list byte.
    pub raw: u8,
    /// How `FUN_801e09f8` classifies the byte.
    pub entry: EffectListEntry,
    /// `0x801F6324` spawn-prototype param for a [`EffectListEntry::Spawn`] index.
    pub proto: Option<u32>,
    /// `0x801F6418` SFX cue id for a spawn index (`0` = silent).
    pub sfx: Option<u8>,
}

/// A battle move's full presentation / timing descriptor, resolved from its
/// move-power record's behavioural fields (everything past the `+0` power
/// scalar) plus the cross-table joins. Built by
/// [`MovePowerCatalog::fx_for_move_id`].
///
/// This is pure data: the render / audio layers consume it (trail texpage,
/// impact config, effect spawns, sound cue). Effect-spawn wiring is blocked on
/// the `0x801F6324` prototype-struct layout — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveFx {
    /// The battle move id this descriptor was resolved for.
    pub move_id: u8,
    /// The power-table record index the id mapped to.
    pub record_index: usize,
    /// `+0x02` strike-position Y offset.
    pub strike_y_offset: i16,
    /// `+0x04` whole-move timing-window counter.
    pub counter_init: u16,
    /// `+0x06` per-arm phase duration.
    pub phase_duration: u16,
    /// `+0x08` homing / approach speed.
    pub homing_speed: u8,
    /// `+0x09` — the spawned effect tracks the live strike position each frame.
    pub effect_tracks_strike: bool,
    /// `+0x0a` impact-effect selector (`0` = none, else 1-based into the config
    /// table).
    pub impact_effect: u8,
    /// The packed config word `impact_effect` resolves to, when the selector is
    /// non-zero and the impact table is present.
    pub impact_config: Option<u32>,
    /// `+0x0b` trail / afterimage texpage as the GP0 word (`0x7700 + id`).
    pub trail_texpage: u16,
    /// `+0x0d` sound / voice cue id (handed to the cue dispatcher).
    pub sound_cue_id: u8,
    /// `+0x0e` list-mode flag (`0xFF` = broadcast trail to all four arms).
    pub list_mode: u8,
    /// `+0x12` on-contact effect list, resolved.
    pub contact_effects: Vec<ResolvedEffect>,
    /// `+0x16` launch-strike effect list, resolved.
    pub launch_effects: Vec<ResolvedEffect>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    /// Build a synthetic PROT-0898-shaped buffer with a known map + table so the
    /// wrapper can be exercised without a disc.
    fn synthetic_overlay() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        // map[4] = 1 (the structural guard + first mapped id), map[5] = 2.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 5] = 2;
        // table record 1 power 0x02ee (>>2 = 187), record 2 power 0x09c4 (625).
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2] = 0xc4;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2 + 1] = 0x09;
        buf
    }

    #[test]
    fn resolves_move_ids_through_the_map() {
        let cat = MovePowerCatalog::from_overlay_0898(&synthetic_overlay()).expect("parses");
        assert_eq!(cat.power_for_move_id(4), Some(187));
        assert_eq!(cat.power_for_move_id(5), Some(625));
        // Unmapped ids (map byte 0) resolve to no record.
        assert_eq!(cat.power_for_move_id(6), None);
        assert_eq!(cat.power_for_move_id(0), None);
        assert!(cat.record_for_move_id(4).is_some());
    }

    #[test]
    fn rejects_a_buffer_that_misses_the_table() {
        // All zeros: map[4] != 1 guard fails, so no catalog.
        let buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        assert!(MovePowerCatalog::from_overlay_0898(&buf).is_none());
        // Too short.
        assert!(MovePowerCatalog::from_overlay_0898(&[0u8; 16]).is_none());
    }

    /// The table-only synthetic overlay doesn't reach the aux tables (they live
    /// further into the real overlay), so the catalog parses but leaves the
    /// cross-table joins unresolved — and the FX descriptor still decodes every
    /// field carried by the record itself.
    #[test]
    fn fx_descriptor_resolves_without_aux_tables() {
        let cat = MovePowerCatalog::from_overlay_0898(&synthetic_overlay()).expect("parses");
        assert!(cat.aux_tables().is_none(), "table-only buffer has no aux");
        assert!(cat.impact_table().is_none());
        let fx = cat.fx_for_move_id(4).expect("move id 4 has a record");
        assert_eq!(fx.record_index, 1);
        // Record 1 carried only power in this fixture; behavioural fields zero.
        assert_eq!(fx.trail_texpage, 0x7700, "trail id 0 -> base texpage word");
        assert_eq!(fx.impact_effect, 0);
        assert_eq!(fx.impact_config, None);
        assert!(fx.contact_effects.is_empty());
        assert!(fx.launch_effects.is_empty());
        // No record for an unmapped id.
        assert!(cat.fx_for_move_id(6).is_none());
    }

    /// A full-size overlay reaching the aux + impact tables exercises every
    /// behavioural field and both cross-table joins.
    #[test]
    fn fx_descriptor_resolves_behavioural_fields_and_effect_lists() {
        use legaia_asset::move_power::{
            EFFECT_AUX_TABLE_LEN, EFFECT_PROTO_TABLE_FILE_OFFSET, EFFECT_SFX_TABLE_FILE_OFFSET,
            IMPACT_EFFECT_TABLE_FILE_OFFSET,
        };

        // Size past the SFX table (the furthest of the three tables).
        let mut buf = vec![0u8; EFFECT_SFX_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN];
        // map[4] = 1 -> move id 4 resolves to record index 1.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;

        // Record 1's 26 bytes: power + the behavioural fields.
        let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
        buf[rec] = 0xee; // +0 power 0x02ee (>>2 = 187)
        buf[rec + 1] = 0x02;
        buf[rec + 2] = 0xf8; // +0x02 strike_y = -8
        buf[rec + 3] = 0xff;
        buf[rec + 4] = 0x20; // +0x04 counter = 0x20
        buf[rec + 6] = 0x10; // +0x06 phase = 0x10
        buf[rec + 8] = 0x18; // +0x08 homing speed
        buf[rec + 9] = 0x01; // +0x09 effect tracks strike
        buf[rec + 0x0a] = 0x03; // +0x0a impact selector 3 -> impact_table[2]
        buf[rec + 0x0b] = 0x05; // +0x0b trail id -> texpage 0x7705
        buf[rec + 0x0d] = 0x2a; // +0x0d sound cue
        buf[rec + 0x0e] = 0x00; // +0x0e list mode
        // +0x12 on-contact list: Spawn(2), FixedFlash(0x64), terminator.
        buf[rec + 0x12] = 0x02;
        buf[rec + 0x13] = 0x64;
        // +0x16 launch list: AltEffect(1)=0x81, Skip(0xFF), Spawn(3), terminator.
        buf[rec + 0x16] = 0x81;
        buf[rec + 0x17] = 0xff;
        buf[rec + 0x18] = 0x03;

        // Aux tables: proto/sfx for spawn indices 2 and 3.
        let put_u32 = |buf: &mut [u8], off: usize, v: u32| {
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        put_u32(
            &mut buf,
            EFFECT_PROTO_TABLE_FILE_OFFSET + 2 * 4,
            0xDEAD_BEEF,
        );
        put_u32(
            &mut buf,
            EFFECT_PROTO_TABLE_FILE_OFFSET + 3 * 4,
            0x0000_CAFE,
        );
        buf[EFFECT_SFX_TABLE_FILE_OFFSET + 2] = 0x11;
        buf[EFFECT_SFX_TABLE_FILE_OFFSET + 3] = 0x22;
        // Impact config table: selector 3 -> index 2.
        put_u32(
            &mut buf,
            IMPACT_EFFECT_TABLE_FILE_OFFSET + 2 * 4,
            0x0000_ABCD,
        );

        let cat = MovePowerCatalog::from_overlay_0898(&buf).expect("parses");
        assert!(cat.aux_tables().is_some());
        assert!(cat.impact_table().is_some());

        let fx = cat.fx_for_move_id(4).expect("record present");
        assert_eq!(fx.record_index, 1);
        assert_eq!(fx.strike_y_offset, -8);
        assert_eq!(fx.counter_init, 0x20);
        assert_eq!(fx.phase_duration, 0x10);
        assert_eq!(fx.homing_speed, 0x18);
        assert!(fx.effect_tracks_strike);
        assert_eq!(fx.impact_effect, 3);
        assert_eq!(
            fx.impact_config,
            Some(0x0000_ABCD),
            "selector 3 -> table[2]"
        );
        assert_eq!(fx.trail_texpage, 0x7705);
        assert_eq!(fx.sound_cue_id, 0x2a);

        // On-contact: Spawn(2) with resolved proto/sfx, then FixedFlash, then stop.
        assert_eq!(fx.contact_effects.len(), 2);
        assert_eq!(fx.contact_effects[0].entry, EffectListEntry::Spawn(2));
        assert_eq!(fx.contact_effects[0].proto, Some(0xDEAD_BEEF));
        assert_eq!(fx.contact_effects[0].sfx, Some(0x11));
        assert_eq!(fx.contact_effects[1].entry, EffectListEntry::FixedFlash);
        assert_eq!(fx.contact_effects[1].proto, None);

        // Launch: AltEffect(1), Skip (0xFF does not terminate), Spawn(3).
        assert_eq!(fx.launch_effects.len(), 3);
        assert_eq!(fx.launch_effects[0].entry, EffectListEntry::AltEffect(1));
        assert_eq!(fx.launch_effects[1].entry, EffectListEntry::Skip);
        assert_eq!(fx.launch_effects[2].entry, EffectListEntry::Spawn(3));
        assert_eq!(fx.launch_effects[2].proto, Some(0x0000_CAFE));
        assert_eq!(fx.launch_effects[2].sfx, Some(0x22));
    }
}
