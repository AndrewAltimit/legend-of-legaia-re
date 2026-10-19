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
        let live = member.live_stats();
        let growth = member.record_stats();
        let stat_pairs: [(u16, u16); 6] = [
            (live.atk, growth.atk),
            (live.udf, growth.udf),
            (live.ldf, growth.ldf),
            (live.spd, growth.spd),
            (live.int, growth.int),
            (live.agl, growth.agl),
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
                    0 => String::new(),
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
            mp: hms.mp_cur,
            mp_max: hms.mp_max,
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
        });
    }
    out
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
