use std::path::Path;

use anyhow::{Context, Result};

/// Parse the battle-action per-move power table and print its records.
pub(crate) fn mode_table_cmd(input: &Path, json: bool) -> Result<()> {
    let bytes = crate::common::read_input(input)?;
    let Some(table) = legaia_asset::mode_table::ModeTable::from_scus(&bytes) else {
        anyhow::bail!(
            "no game-mode table at VA {:#x} in {} - is this SCUS_942.54?",
            legaia_asset::mode_table::MODE_TABLE_VA,
            input.display(),
        );
    };
    if json {
        let rows: Vec<_> = table
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "index": e.index,
                    "name": e.name,
                    "handler": format!("{:#010x}", e.handler),
                    "param": format!("{:#010x}", e.param),
                    "per_frame": e.is_per_frame(),
                    "shared_handler": e.uses_shared_handler(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!(
        "game-mode table @ VA {:#010x} ({} entries)",
        legaia_asset::mode_table::MODE_TABLE_VA,
        table.entries.len()
    );
    println!(
        "{:>3}  {:<14}  {:<10}  {:<10}  kind",
        "idx", "name", "handler", "param"
    );
    for e in &table.entries {
        let kind = if e.is_per_frame() {
            if e.uses_shared_handler() {
                "per-frame (shared)"
            } else {
                "per-frame"
            }
        } else {
            "init"
        };
        println!(
            "{:>3}  {:<14}  {:#010x}  {:#010x}  {kind}",
            e.index, e.name, e.handler, e.param
        );
    }
    println!(
        "{} of {} per-frame modes share handler {:#010x}",
        table.shared_handler_count(),
        table.entries.iter().filter(|e| e.is_per_frame()).count(),
        legaia_asset::mode_table::SHARED_PER_FRAME_HANDLER,
    );
    Ok(())
}

pub(crate) fn move_power_cmd(input: &Path, json: bool) -> Result<()> {
    let bytes = crate::common::read_input(input)?;
    let Some(table) = legaia_asset::move_power::parse(&bytes) else {
        anyhow::bail!(
            "no move-power table at the pinned offset {:#x} in {} ({} bytes) - \
             is this the raw PROT 0898 battle-action overlay entry?",
            legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    let map = legaia_asset::move_power::parse_id_index_map(&bytes);
    // The move id(s) that resolve to a given power-table index (the move-id
    // space is the spell-table id space).
    let move_ids_of = |idx: usize| -> Vec<u8> {
        match &map {
            None => Vec::new(),
            Some(m) => (0..m.len())
                .filter(|&mid| {
                    legaia_asset::move_power::index_for_move_id(m, mid as u8) == Some(idx as u8)
                })
                .map(|mid| mid as u8)
                .collect(),
        }
    };
    if json {
        let rows: Vec<_> = table
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| {
                serde_json::json!({
                    "index": r.index,
                    "power": r.power(),
                    "power_raw": r.power_raw,
                    "counter_init": r.counter_init(),
                    "phase_duration": r.phase_duration(),
                    "homing_speed": r.homing_speed(),
                    "strike_y_offset": r.strike_y_offset(),
                    "impact_effect": r.impact_effect(),
                    "trail_texture_page": r.trail_texture_page(),
                    "sound_cue_id": r.sound_cue_id(),
                    "list_mode": r.list_mode(),
                    "tag": r.annotation_tag(),
                    "contact_effects": r.contact_effects(),
                    "launch_effects": r.launch_effects(),
                    "move_ids": move_ids_of(r.index),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    // Invert the map: power index -> the move id(s) that resolve to it.
    let move_ids_for = |idx: usize| -> String {
        match &map {
            None => String::new(),
            Some(m) => {
                let ids: Vec<String> = (0..m.len())
                    .filter(|&mid| {
                        legaia_asset::move_power::index_for_move_id(m, mid as u8) == Some(idx as u8)
                    })
                    .map(|mid| format!("{mid:#04x}"))
                    .collect();
                if ids.is_empty() {
                    String::new()
                } else {
                    format!("  <- move {}", ids.join(","))
                }
            }
        }
    };
    println!(
        "move-power table: {} records @ file {:#x} (runtime VA {:#010x}), 26-byte stride; \
         id->index map {}",
        table.len(),
        legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET,
        legaia_asset::move_power::MOVE_POWER_TABLE_VA,
        if map.is_some() { "present" } else { "MISSING" },
    );
    for r in &table {
        if r.is_empty() {
            continue;
        }
        let tag = r
            .annotation_tag()
            .map(|c| c.to_string())
            .unwrap_or_default();
        let contact = r.contact_effects();
        let launch = r.launch_effects();
        let fx = |v: &[u8]| -> String {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.iter()
                    .map(|b| format!("{b:#04x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        println!(
            "  idx {:3}  power {:5} (raw {:#06x})  ctr {:4}  phase {:4}  homing {:#04x}  \
             yoff {:5}  impact {}  trail {}  sfx {:#04x}  list {:#04x}  tag {:1}  \
             contact[{}]  launch[{}]{}",
            r.index,
            r.power(),
            r.power_raw as u16,
            r.counter_init(),
            r.phase_duration(),
            r.homing_speed(),
            r.strike_y_offset(),
            r.impact_effect(),
            r.trail_texture_page(),
            r.sound_cue_id(),
            r.list_mode(),
            tag,
            fx(&contact),
            fx(&launch),
            move_ids_for(r.index),
        );
    }
    Ok(())
}

/// `asset move-power <PROT 0898> --effect-index` - emit the effect-id ->
/// triggering-move inverse index: one row per `(space, id)` effect key, each
/// with the set of moves whose `+0x12` / `+0x16` lists cite it and (for the
/// Proto3D space) the resolved prototype VA + SFX cue from the aux tables.
pub(crate) fn move_power_effect_index_cmd(input: &Path, json: bool) -> Result<()> {
    use legaia_asset::move_power::{self, EffectFired, EffectKey};

    let bytes = crate::common::read_input(input)?;
    let Some(table) = move_power::parse(&bytes) else {
        anyhow::bail!(
            "no move-power table at the pinned offset {:#x} in {} ({} bytes) - \
             is this the raw PROT 0898 battle-action overlay entry?",
            move_power::MOVE_POWER_TABLE_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    let Some(map) = move_power::parse_id_index_map(&bytes) else {
        anyhow::bail!(
            "no id->index map in {} - the effect inverse index needs the map to \
             attribute effects to move ids",
            input.display(),
        );
    };
    // Aux tables resolve Proto3D keys to their prototype VA + SFX cue (optional:
    // an overlay missing them still yields the move-id inverse).
    let aux = move_power::EffectAuxTables::parse(&bytes);
    let index = move_power::effect_trigger_index(&table, &map);

    // Describe a key's (space, id) and, for Proto3D, its resolved aux fields.
    let space_of = |k: &EffectKey| -> &'static str {
        match k {
            EffectKey::Proto3D(_) => "proto3d",
            EffectKey::Efect2D(_) => "efect2d",
            EffectKey::Flash => "flash",
        }
    };
    let id_of = |k: &EffectKey| -> Option<u8> {
        match k {
            EffectKey::Proto3D(id) | EffectKey::Efect2D(id) => Some(*id),
            EffectKey::Flash => None,
        }
    };
    let proto_of = |k: &EffectKey| -> Option<u32> {
        match k {
            EffectKey::Proto3D(id) => aux.as_ref().and_then(|a| a.effect_proto(*id)),
            _ => None,
        }
    };
    let sfx_of = |k: &EffectKey| -> Option<u8> {
        match k {
            EffectKey::Proto3D(id) => aux.as_ref().and_then(|a| a.effect_sfx(*id)),
            _ => None,
        }
    };
    let fired_str = |f: EffectFired| match f {
        EffectFired::Contact => "contact",
        EffectFired::Launch => "launch",
    };

    if json {
        let rows: Vec<_> = index
            .iter()
            .map(|(key, triggers)| {
                let trigs: Vec<_> = triggers
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "move_id": t.move_id,
                            "record_idx": t.record_idx,
                            "fired": fired_str(t.fired),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "space": space_of(key),
                    "id": id_of(key),
                    "proto_va": proto_of(key).map(|v| format!("{v:#010x}")),
                    "sfx": sfx_of(key),
                    "no_triggering_move_id": triggers.iter().all(|t| t.move_id.is_none()),
                    "triggers": trigs,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    println!(
        "effect-id -> triggering-move inverse index ({} effect keys), from PROT 0898 \
         move-power records' +0x12 (contact) / +0x16 (launch) lists; keyed on (space, id)",
        index.len()
    );
    for (key, triggers) in &index {
        let id = id_of(key)
            .map(|i| format!("{i:#04x}"))
            .unwrap_or_else(|| "-".to_string());
        let mut extra = String::new();
        if let Some(va) = proto_of(key) {
            extra.push_str(&format!(" proto={va:#010x}"));
        }
        if let Some(sfx) = sfx_of(key) {
            extra.push_str(&format!(" sfx={sfx:#04x}"));
        }
        let trigs: Vec<String> = triggers
            .iter()
            .map(|t| {
                let who = t
                    .move_id
                    .map(|m| format!("move {m:#04x}"))
                    .unwrap_or_else(|| "no-move".to_string());
                format!("{who}@rec{}({})", t.record_idx, fired_str(t.fired))
            })
            .collect();
        println!(
            "  {:<8} {:<5}{extra}  <- [{}]",
            space_of(key),
            id,
            trigs.join(", ")
        );
    }
    Ok(())
}

/// Parse + print the battle element-affinity matrix and per-character table.
pub(crate) fn element_affinity_cmd(input: &Path, json: bool) -> Result<()> {
    use legaia_asset::element_affinity::{self, Element};
    let bytes = crate::common::read_input(input)?;
    let Some(aff) = element_affinity::parse(&bytes) else {
        anyhow::bail!(
            "no element-affinity tables at the pinned offsets (matrix {:#x}, \
             char table {:#x}) in {} ({} bytes) - is this the raw PROT 0898 \
             battle-action overlay entry?",
            element_affinity::AFFINITY_MATRIX_FILE_OFFSET,
            element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    if json {
        let elements: Vec<&str> = (0..element_affinity::ELEMENT_COUNT)
            .map(|id| Element::from_id(id as u8).map(|e| e.name()).unwrap_or("?"))
            .collect();
        let out = serde_json::json!({
            "elements": elements,
            "matrix": aff.matrix,
            "character_elements": aff.character_elements,
            "summon_power": aff.summon_power,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    let label = |id: usize| -> String {
        Element::from_id(id as u8)
            .map(|e| e.name().to_string())
            .unwrap_or_else(|| format!("?{id}"))
    };
    println!(
        "element-affinity matrix @ file {:#x} (runtime VA {:#010x}); pct = matrix[attacker][defender]",
        element_affinity::AFFINITY_MATRIX_FILE_OFFSET,
        element_affinity::AFFINITY_MATRIX_VA,
    );
    print!("atk\\def ");
    for def in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>8}", label(def));
    }
    println!();
    for atk in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>7} ", label(atk));
        for def in 0..element_affinity::ELEMENT_COUNT {
            print!("{:>8}", aff.matrix[atk][def]);
        }
        println!();
    }
    println!(
        "\nper-character element table @ file {:#x} (runtime VA {:#010x}, 1-based char id):",
        element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET,
        element_affinity::CHARACTER_ELEMENTS_VA,
    );
    let names = ["Vahn", "Noa", "Gala", "Terra"];
    for (i, &elem) in aff.character_elements.iter().enumerate() {
        let who = names.get(i).copied().unwrap_or("");
        println!(
            "  char {:>2} {:<6} -> element {} ({})",
            i + 1,
            who,
            elem,
            label(elem as usize)
        );
    }
    println!(
        "\nper-character summon power-percent @ file {:#x} (runtime VA {:#010x}); \
         pct = row[summon creature element], FUN_801ddb30 stage 5:",
        legaia_asset::element_affinity::SUMMON_POWER_PCT_FILE_OFFSET,
        legaia_asset::element_affinity::SUMMON_POWER_PCT_VA,
    );
    print!("        ");
    for elem in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>8}", label(elem));
    }
    println!();
    for (i, row) in aff.summon_power.iter().enumerate() {
        print!("{:>7} ", names.get(i).copied().unwrap_or(""));
        for pct in row {
            print!("{pct:>8}");
        }
        println!();
    }
    Ok(())
}

pub(crate) fn item_tables_cmd(
    scus: &Path,
    equipment_only: bool,
    consumables_only: bool,
) -> Result<()> {
    use legaia_asset::{equip_stats, item_effect, item_names};

    let bytes = crate::common::read_input(scus)?;
    let names = item_names::ItemNameTable::from_scus(&bytes).context("parse item-name table")?;
    let effects =
        item_effect::ItemEffectTable::from_scus(&bytes).context("parse item-effect table")?;
    let equips =
        equip_stats::EquipStatTable::from_scus(&bytes).context("parse equip-stat table")?;

    println!("id    name                       category");
    for id in 0u8..=u8::MAX {
        let name = names.name(id).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        if let Some(b) = equips.bonus(id) {
            if consumables_only {
                continue;
            }
            let bonuses = b.stat_bonus();
            let ra = if b.is_ra_seru() { " ra-seru" } else { "" };
            println!(
                "0x{id:02X}  {name:26} equip  atk={} udf={} ldf={} mask={:#05b} slot={:?}{ra} \
                 [+0={} +4={}]",
                b.attack(),
                b.def_up(),
                b.def_down(),
                b.equip_mask(),
                b.slot(),
                bonuses[0],
                bonuses[4],
            );
        } else if let Some(e) = effects.effect(id) {
            if equipment_only || !e.is_usable_consumable() {
                continue;
            }
            let mut where_ = String::new();
            if e.field_usable() {
                where_.push('F');
            }
            if e.battle_usable() {
                where_.push('B');
            }
            if e.all_party() {
                where_.push_str(" all-party");
            }
            println!(
                "0x{id:02X}  {name:26} {:?} tier={} [{}]",
                e.category(),
                e.tier,
                where_,
            );
        }
    }
    Ok(())
}

/// `asset spell-names <SCUS>` - dump the static spell name / MP / target
/// table (`legaia_asset::spell_names`, `DAT_800754C8`).
pub(crate) fn spell_names_cmd(scus: &Path, json: bool) -> Result<()> {
    use legaia_asset::spell_names::SpellNameTable;

    let bytes = crate::common::read_input(scus)?;
    let table = SpellNameTable::from_scus(&bytes).context("parse spell-name table")?;
    if json {
        let rows: Vec<_> = (0u8..=u8::MAX)
            .filter_map(|id| {
                let e = table.entry(id)?;
                e.name.as_ref()?;
                Some(serde_json::json!({
                    "id": id,
                    "name": e.name,
                    "mp": e.mp,
                    "target": e.target_shape(),
                }))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let mut named = 0usize;
    println!("id    mp   target           name");
    for id in 0u8..=u8::MAX {
        let Some(e) = table.entry(id) else { continue };
        let name = e.name.as_deref().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        named += 1;
        println!("0x{id:02X}  {:<4} {:<16?} {name}", e.mp, e.target_shape());
    }
    println!("\n{named} named spell ids");
    Ok(())
}

/// `asset steal-table <SCUS>` - dump the static per-monster steal table
/// (`legaia_asset::steal_table`, `DAT_80077828`), joining the stolen item
/// id to its name from the item-name table.
pub(crate) fn steal_table_cmd(scus: &Path, all: bool, json: bool) -> Result<()> {
    use legaia_asset::{item_names::ItemNameTable, steal_table::StealTable};

    let bytes = crate::common::read_input(scus)?;
    let table = StealTable::from_scus(&bytes).context("parse steal table")?;
    let names = ItemNameTable::from_scus(&bytes);
    if json {
        let rows: Vec<_> = (1u16..=255)
            .filter_map(|monster_id| {
                let e = table.entry(monster_id)?;
                if !all && !e.is_stealable() {
                    return None;
                }
                Some(serde_json::json!({
                    "monster_id": monster_id,
                    "chance_pct": e.chance_pct,
                    "item_id": e.item_id,
                    "item_name": names.as_ref().and_then(|n| n.name(e.item_id)),
                }))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("monster  chance  item");
    for monster_id in 1u16..=255 {
        let Some(e) = table.entry(monster_id) else {
            continue;
        };
        if !all && !e.is_stealable() {
            continue;
        }
        let item = names.as_ref().and_then(|n| n.name(e.item_id)).unwrap_or("");
        println!(
            "{monster_id:>5}    {:>3}%    0x{:02X} {item}",
            e.chance_pct, e.item_id
        );
    }
    println!(
        "\n{} stealable of {} entries",
        table.stealable_count(),
        table.len()
    );
    Ok(())
}

/// `asset accessory-passive <SCUS>` - dump the 64-slot accessory ("Goods")
/// passive-effect table (`legaia_asset::accessory_passive`, `0x8007625C`).
pub(crate) fn accessory_passive_cmd(scus: &Path, json: bool) -> Result<()> {
    use legaia_asset::accessory_passive::{AccessoryPassiveTable, stat_boosts};

    let bytes = crate::common::read_input(scus)?;
    let table =
        AccessoryPassiveTable::from_scus(&bytes).context("parse accessory-passive table")?;
    if json {
        let rows: Vec<_> = (0..table.record_count())
            .filter_map(|i| {
                let idx = i as u8;
                let rec = table.record(idx)?;
                let boosts: Vec<_> = stat_boosts(idx)
                    .iter()
                    .map(|(s, p)| serde_json::json!({ "stat": s, "percent": p }))
                    .collect();
                Some(serde_json::json!({
                    "index": idx,
                    "name": rec.name,
                    "party_wide": rec.party_wide(),
                    "boosts": boosts,
                }))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("idx   scope  name                          boosts / effect");
    for i in 0..table.record_count() {
        let idx = i as u8;
        let Some(rec) = table.record(idx) else {
            continue;
        };
        let name = rec.name.as_deref().unwrap_or("");
        let scope = if rec.party_wide() { "party" } else { "self " };
        let boosts = stat_boosts(idx);
        let effect = if boosts.is_empty() {
            String::new()
        } else {
            boosts
                .iter()
                .map(|(s, p)| format!("{s:?}+{p}%"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!("0x{idx:02X}  {scope}  {name:28}  {effect}");
    }
    Ok(())
}

/// `asset sfx-table <SCUS>` - dump the sound-effect descriptor table
/// (`legaia_asset::sfx_table`, `DAT_8006F198`).
pub(crate) fn sfx_table_cmd(scus: &Path, json: bool) -> Result<()> {
    use legaia_asset::sfx_table::SfxTable;

    let bytes = crate::common::read_input(scus)?;
    let table = SfxTable::from_scus(&bytes).context("parse sfx table")?;
    if json {
        let rows: Vec<_> = table
            .active()
            .map(|(id, d)| {
                serde_json::json!({
                    "id": id,
                    "program": d.program,
                    "tone": d.tone,
                    "note": d.note,
                    "voice_count": d.voice_count(),
                    "sustained": d.sustained(),
                    "category": d.category,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("id    prog tone note voices sustained category");
    for (id, d) in table.active() {
        println!(
            "0x{id:02X}  {:>4} {:>4} {:>4} {:>6} {:>9} {:>3}",
            d.program,
            d.tone,
            d.note,
            d.voice_count(),
            d.sustained(),
            d.category,
        );
    }
    println!(
        "\n{} active of {} cues",
        table.active().count(),
        table.len()
    );
    Ok(())
}

/// `asset new-game <SCUS>` - dump the new-game starting-party template
/// (`legaia_asset::new_game`, `0x80078C4C`) + the code-built starting inventory.
pub(crate) fn new_game_cmd(scus: &Path, json: bool) -> Result<()> {
    use legaia_asset::{
        item_names::ItemNameTable,
        new_game::{StartingInventory, StartingParty},
    };

    let bytes = crate::common::read_input(scus)?;
    let party = StartingParty::from_scus(&bytes).context("parse new-game party template")?;
    if json {
        let names = ItemNameTable::from_scus(&bytes);
        let inventory: Vec<_> = StartingInventory::from_scus(&bytes)
            .map(|inv| {
                inv.items()
                    .iter()
                    .map(|(id, qty)| {
                        serde_json::json!({
                            "item_id": id,
                            "count": qty,
                            "name": names.as_ref().and_then(|n| n.name(*id)),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let out = serde_json::json!({
            "party": party.members(),
            "inventory": inventory,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!("Starting party (new-game template):");
    println!("slot  name        HP   MP   AGL  ATK  UDF  LDF  SPD  INT");
    for (i, m) in party.members().iter().enumerate() {
        println!(
            "{i:>4}  {:10} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4}",
            m.name, m.hp_max, m.mp_max, m.agl, m.atk, m.udf, m.ldf, m.spd, m.intel,
        );
    }
    if let Some(inv) = StartingInventory::from_scus(&bytes) {
        let names = ItemNameTable::from_scus(&bytes);
        println!("\nStarting inventory:");
        for (id, qty) in inv.items() {
            let name = names.as_ref().and_then(|n| n.name(*id)).unwrap_or("");
            println!("  0x{id:02X} x{qty}  {name}");
        }
    }
    Ok(())
}

/// `asset level-up <SCUS>` - dump the per-character stat-growth params +
/// XP thresholds (`legaia_asset::level_up_tables`, `DAT_80076918`). Stats are
/// indexed (the on-disc sub-record order); cross-reference the stat-growth doc.
pub(crate) fn level_up_cmd(scus: &Path, json: bool) -> Result<()> {
    use legaia_asset::level_up_tables::{
        GROWTH_CHAR_COUNT, growth_tables_from_scus, xp_thresholds_from_scus,
    };

    let bytes = crate::common::read_input(scus)?;
    let gt = growth_tables_from_scus(&bytes).context("parse stat-growth tables")?;
    const CHARS: [&str; GROWTH_CHAR_COUNT] = ["Vahn", "Noa", "Gala"];
    if json {
        let growth: Vec<_> = CHARS
            .iter()
            .enumerate()
            .filter_map(|(slot, name)| {
                let cp = gt.char_params(slot)?;
                Some(serde_json::json!({ "char": name, "stats": cp.stats }))
            })
            .collect();
        let out = serde_json::json!({
            "growth": growth,
            "xp_thresholds": xp_thresholds_from_scus(&bytes),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!("Per-character stat-growth params (start / max / jitter / row):");
    for (slot, name) in CHARS.iter().enumerate() {
        let Some(cp) = gt.char_params(slot) else {
            continue;
        };
        println!("  {name}:");
        for (i, s) in cp.stats.iter().enumerate() {
            println!(
                "    stat[{i}]  start={:>5} max={:>5} jitter={:>3} row={}",
                s.start, s.max, s.jitter, s.row,
            );
        }
    }
    if let Some(thresholds) = xp_thresholds_from_scus(&bytes) {
        let n = thresholds.len();
        let head: Vec<u32> = thresholds.iter().take(12).copied().collect();
        println!(
            "\nXP thresholds ({n} levels), first {}: {head:?}",
            head.len()
        );
    }
    Ok(())
}
