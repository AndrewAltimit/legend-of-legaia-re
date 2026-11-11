//! Status-screen projections: the active leader, per-member status snapshots and roster names.

use super::*;

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Resolve the slot of the active leader ([`World::field_leader`]: the
/// leader word, else the party's head, else slot 0).
pub fn active_leader_slot(world: &World) -> u8 {
    world.field_leader()
}

/// Build a [`StatusSnapshot`] for every member of the **present party**
/// (retail's `DAT_80084594` count over the `0x80084598` member list,
/// [`World::present_party_list`]), in that list's order, skipping a record
/// with a zero max-HP. The retail status list is the present party, not the
/// roster: the New Game template seeds all four records, so a roster walk
/// lists Noa, Gala and Terra beside a Vahn who is still travelling alone.
pub fn status_snapshots(world: &World) -> Vec<StatusSnapshot> {
    let names = world.party.roster_names();
    let mut out = Vec::new();
    for slot in world.present_party_list() {
        let i = usize::from(slot);
        let Some(member) = world.party.roster.members.get(i) else {
            continue;
        };
        let hms = member.hp_mp_sp();
        if hms.hp_max == 0 {
            continue;
        }
        let xp = member.cumulative_xp();
        // Retail LV is the record's own +0x130 byte (FUN_801D33D8); fall back
        // to base-curve inference for records that never had it stamped.
        let level = match member.magic_rank() {
            l @ 1..=99 => l,
            _ => legaia_save::level_for_cumulative_xp(xp),
        };
        let xp_to_next = xp_to_next_level(member, level);
        // The retail 3x2 derived-stat grid: live values from the `+0x110`
        // window, growth values (the parenthesised number) from the
        // `+0x122..+0x12D` record window, ordered ATK/UDF/LDF | SPD/INT/AGL
        // (docs/subsystems/field-menu.md).
        let growth = member.record_stats();
        let block = status_stat_block(world, member, i as u8, hms.hp_max, hms.mp_max);
        let word = |w: usize| block[w].clamp(0, 999) as u16;
        let stat_pairs: [(u16, u16); 6] = [
            (word(3), growth.atk),
            (word(4), growth.udf),
            (word(5), growth.ldf),
            (word(6), growth.spd),
            (word(7), growth.int),
            (word(2), growth.agl),
        ];
        // The grid's seven rows read the record through retail's own offset
        // tables (FUN_801D33D8 `0x801D3B4C..`): row 0 the per-character
        // weapon byte (`DAT_8007B42C`), rows 1..6 `DAT_801E43E8[row]` -
        // head, body, footwear, Goods x3. The record's `+0x196` order is
        // `[body, head, weapon, weapon, footwear, goods x3]`, so reading it
        // in place put body armour on the weapon row.
        let engine_slots = legaia_engine_menus::equip_session::engine_equip_from_record(
            member.equipment().slots,
            i as u8,
        );
        let equip_views: Vec<EquipSlotView> = legaia_engine_menus::equip_session::BROWSE_SLOT_ORDER
            .iter()
            .map(|&s| EquipSlotView {
                label: equip_slot_label(s),
                // Empty slots stay blank (retail draws only the slot
                // pictogram); an occupied slot prints the item's name out of
                // the SCUS item table (`PTR_DAT_8007436C`), the raw id only
                // when no table was staged.
                item_name: match engine_slots[usize::from(s)] {
                    // An expansion pack's worn gear leaves the record's
                    // byte `0`; its row names the pack item.
                    0 => world
                        .pack_worn_item_name(
                            i as u8,
                            legaia_engine_menus::equip_session::record_byte_for_engine_slot(
                                s, i as u8,
                            ) as u8,
                        )
                        .unwrap_or_default(),
                    id => world
                        .menu
                        .text
                        .as_ref()
                        .and_then(|t| t.item_name(id))
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("#{id:02X}")),
                },
            })
            .collect();
        out.push(StatusSnapshot {
            slot: i as u8,
            name: names.get(i).cloned().unwrap_or_else(|| format!("Slot {i}")),
            level,
            xp,
            xp_to_next,
            hp: hms.hp_cur,
            hp_max: hms.hp_max,
            hp_base: growth.hp_max,
            mp: hms.mp_cur,
            mp_max: hms.mp_max,
            mp_base: growth.mp_max,
            // The status page's AP gauge reads the persistent char-record
            // AP at `+0x10E` (`hp_mp_sp().sp_cur` under the (max, cur) pair
            // order - 0 on a fresh party; the new-game seed zeroes it), NOT
            // the battle ApGauge's base AP.
            // REF: FUN_801D33D8 (docs/subsystems/field-menu.md).
            ap: hms.sp_cur.min(u8::MAX as u16) as u8,
            ap_max: 100,
            attack: world.battle.attack.get(i).copied().unwrap_or(0),
            defense: world.battle.defense.get(i).copied().unwrap_or(0),
            stats: stat_pairs,
            stat_labels: legaia_engine_menus::status_screen::RETAIL_STAT_LABELS,
            equip: equip_views,
            elements: default_element_views(),
            // Record `+0x12E` - the packed ailment word. Retail keeps it as
            // a per-frame mirror of the battle actor's `+0x16E`
            // (`FUN_80047430`), so out of battle the record still carries
            // whatever condition the party walked away with; the engine's
            // equivalent latch is the never-cleared `World::battle.status_effects`
            // tracker, whose `display_flags` packs the same bit word. Party
            // seats and roster indices coincide for `i < party_count`
            // (`World::enter_battle` seats the roster in order).
            status_flags: world.battle.status_effects.display_flags(i as u8),
            seru_level: status_seru_level(member, i),
            skills: status_skills(world, member),
            spells: status_spells(world, member, i),
            moves: status_moves(world, member, i),
        });
    }
    out
}

/// The stat block the status grid prints (`DAT_801EF080`, seeded by
/// `FUN_801CF5D0` and summed by `FUN_801CF650`): the record's words plus the
/// worn equipment's bonuses.
fn status_stat_block(
    world: &World,
    member: &legaia_save::CharacterRecord,
    slot: u8,
    hp_max: u16,
    mp_max: u16,
) -> [i32; 8] {
    let mut record = legaia_engine_menus::pause_screens::stat_record_from_character(member);
    record.equip = legaia_engine_menus::equip_session::engine_equip_from_record(record.equip, slot);
    legaia_engine_menus::pause_screens::menu_stat_block(
        &record,
        &world.tables.equipment_table,
        hp_max,
        mp_max,
    )
}

/// The record's Ra-Seru byte's slot per character (`0x8007B424`).
const RA_SERU_SLOT: [usize; 3] = [3, 2, 3];
/// The first item id of each character's Ra-Seru run (`0x8007B434`).
const RA_SERU_BASE: [u8; 3] = [0x01, 0x0A, 0x13];

/// The worn Ra-Seru's level: the item id's place in its character's run
/// (`0x801D3524..0x801D35C8`).
fn status_seru_level(member: &legaia_save::CharacterRecord, char_slot: usize) -> Option<u8> {
    let id = member.equipment().slots[*RA_SERU_SLOT.get(char_slot)?];
    (id != 0).then(|| id.wrapping_sub(RA_SERU_BASE[char_slot]).wrapping_add(1))
}

/// The Skills page's rows: the record's three Goods bytes, each whose item
/// grants a passive under `0x40` (`0x801D3EBC..0x801D3F38`).
fn status_skills(
    world: &World,
    member: &legaia_save::CharacterRecord,
) -> Vec<legaia_engine_menus::status_screen::StatusSkillRow> {
    let Some(text) = world.menu.text.as_ref() else {
        return Vec::new();
    };
    let Some(passives) = text.passives.as_ref() else {
        return Vec::new();
    };
    member.equipment().slots[5..8]
        .iter()
        .filter_map(|&id| {
            let (_, record) = passives.passive(id)?;
            Some(legaia_engine_menus::status_screen::StatusSkillRow {
                item_name: text.item_name(id).unwrap_or_default().to_string(),
                effect_name: record.name.clone().unwrap_or_default(),
                description: record.description.clone().unwrap_or_default(),
            })
        })
        .collect()
}

/// The Magic page's rows: empty with no Ra-Seru worn (`0x801D412C`), else the
/// record's `+0x13D` / `+0x161` list.
fn status_spells(
    world: &World,
    member: &legaia_save::CharacterRecord,
    char_slot: usize,
) -> Vec<legaia_engine_menus::status_screen::StatusSpellRow> {
    if status_seru_level(member, char_slot).is_none() {
        return Vec::new();
    }
    let names = world
        .menu
        .text
        .as_ref()
        .and_then(|t| t.spell_names.as_ref());
    let ability = legaia_engine_battle::spells::record_ability_word(member);
    let list = member.spell_list();
    list.ids[..usize::from(list.count).min(list.ids.len())]
        .iter()
        .zip(list.levels)
        .map(|(&id, level)| {
            let mp = names.and_then(|n| n.mp(id)).unwrap_or(0);
            legaia_engine_menus::status_screen::StatusSpellRow {
                spell_id: id,
                name: names
                    .and_then(|n| n.name(id))
                    .unwrap_or_default()
                    .to_string(),
                icon: names.and_then(|n| n.icon(id)),
                level,
                mp_cost: legaia_engine_battle_vm::battle_formulas::mp_cost_after_ability_bits(
                    u16::from(mp),
                    legaia_engine_battle_vm::battle_formulas::MpCostModifier::from_ability_flags(
                        ability,
                    ),
                ),
                description: names
                    .and_then(|n| n.desc(id))
                    .unwrap_or_default()
                    .to_string(),
            }
        })
        .collect()
}

/// The Moves page's rows: the record's `+0x186` ids against the arts table,
/// keyed on `[character, id]` (`0x801D4468..0x801D44D4`). The AP halves with
/// the record's `+0xF8` bit `0x800` (`0x801D4520..0x801D4534`).
fn status_moves(
    world: &World,
    member: &legaia_save::CharacterRecord,
    char_slot: usize,
) -> Vec<legaia_engine_menus::status_screen::StatusMoveRow> {
    let Some(text) = world.menu.text.as_ref() else {
        return Vec::new();
    };
    let Some(arts) = text.arts.as_ref() else {
        return Vec::new();
    };
    let halved =
        u32::from_le_bytes(member.ability_bits()[4..8].try_into().unwrap_or([0; 4])) & 0x800 != 0;
    let list = member.displayed_skills();
    list.ids[..usize::from(list.count).min(list.ids.len())]
        .iter()
        .filter_map(|&id| {
            let at = arts
                .iter()
                .position(|e| usize::from(e.character as u8) == char_slot && e.index == id)?;
            let e = &arts[at];
            Some(legaia_engine_menus::status_screen::StatusMoveRow {
                id,
                name_bytes: e.name_bytes.clone(),
                ap: if halved { e.ap >> 1 } else { e.ap },
                glyphs: e.glyphs.clone(),
                description: text
                    .art_descriptions
                    .as_ref()
                    .and_then(|d| d.get(at).cloned())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The Status-menu "Next Level" number. Retail (`FUN_801D33D8`) draws the
/// record's next-level-threshold word (`+0x4`) **verbatim** - the cumulative
/// XP total at which the next level lands, NOT the remaining difference.
/// Records that never had `+0x4` stamped (engine-synthesized rosters) fall
/// back to the base-curve threshold; at L99 retail carries 0 there.
// REF: FUN_801D33D8 (Next Level draw), FUN_801E9504 (threshold writer)
fn xp_to_next_level(member: &legaia_save::CharacterRecord, level: u8) -> u32 {
    match member.next_level_xp() {
        0 if level < 99 => legaia_save::xp_for_level(level + 1),
        threshold => threshold,
    }
}

fn equip_slot_label(slot: u8) -> &'static str {
    const LABELS: [&str; 8] = [
        "Weapon", "Armour", "Helmet", "Ring", "Acc 1", "Acc 2", "Acc 3", "Misc",
    ];
    LABELS.get(slot as usize).copied().unwrap_or("Slot")
}

fn default_element_views() -> Vec<ElementRankView> {
    [
        "Fire", "Water", "Earth", "Wind", "Light", "Dark", "Thunder", "Bio",
    ]
    .iter()
    .map(|l| ElementRankView { label: l, rank: 0 })
    .collect()
}
