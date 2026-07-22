//! Read-only inspection subcommands: each `cmd_*` reads the user's disc and
//! prints the current state of a randomizable table (drops, chests, shops,
//! casino, monster stats, move powers, affinity, spell costs, equip bonuses,
//! weapon specialty, doors, arts, steals, starting items).

use std::path::Path;

use anyhow::{Context, Result};

use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;

use crate::util::load_image;

pub(crate) fn cmd_shops(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let shops = apply::current_shops(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u8| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    for s in &shops {
        println!(
            "[entry {:>4}] {} ({} items):",
            s.entry_idx,
            s.name,
            s.items.len()
        );
        for &id in &s.items {
            println!("    {:>3} (0x{id:02x})  {}", id, nm(id));
        }
    }
    println!("{} town shop(s) on the disc", shops.len());
    Ok(())
}

pub(crate) fn cmd_casino(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u16| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id as u8))
            .unwrap_or("?")
            .to_string()
    };
    match apply::current_casino(&patcher)? {
        Some(ex) => {
            for (b, block) in ex.blocks.iter().enumerate() {
                println!("block {b}:");
                for r in block {
                    let gate = if r.gate == 0 {
                        String::new()
                    } else {
                        format!("  [gated 0x{:02x}]", r.gate)
                    };
                    println!("    {:<16} {:>6} coins{gate}", nm(r.item_id), r.price);
                }
            }
        }
        None => println!("casino prize table not found"),
    }
    Ok(())
}

pub(crate) fn cmd_monster_stats(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_patcher::disc::MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    println!(
        "{:>3}  {:<16} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
        "id", "name", "hp", "mp", "atk", "def+", "def-", "int", "spd"
    );
    for r in &records {
        println!(
            "{:>3}  {:<16} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
            r.id,
            r.name,
            r.hp,
            r.mp,
            r.attack(),
            r.defense_high(),
            r.defense_low(),
            r.intelligence(),
            r.speed()
        );
    }
    println!("{} populated monster records", records.len());
    Ok(())
}

pub(crate) fn cmd_move_powers(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let records =
        legaia_asset::move_power::parse(&entry).context("parse move-power table (PROT 0898)")?;

    // Tag each power-table index with the spell-table name of a move id that
    // resolves to it (the move-id space is the spell-table id space).
    let map = legaia_asset::move_power::parse_id_index_map(&entry);
    let spells = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::spell_names::SpellNameTable::from_scus(&scus));
    let label = |idx: usize| -> String {
        let (Some(map), Some(spells)) = (map.as_ref(), spells.as_ref()) else {
            return String::new();
        };
        for move_id in 0u8..=0x7F {
            if legaia_asset::move_power::index_for_move_id(map, move_id) != Some(idx as u8) {
                continue;
            }
            if let Some(name) = spells.name(move_id).filter(|n| !n.is_empty()) {
                return name.to_string();
            }
        }
        String::new()
    };

    println!("{:>3}  {:>6}  example move", "idx", "power");
    for (i, r) in records.iter().enumerate() {
        println!("{:>3}  {:>6}  {}", i, r.power(), label(i));
    }
    println!("{} move-power records", records.len());
    Ok(())
}

pub(crate) fn cmd_affinity(input: &Path) -> Result<()> {
    use legaia_asset::element_affinity::{ELEMENT_COUNT, Element, ElementAffinity};
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_asset::element_affinity::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let aff =
        ElementAffinity::parse(&entry).context("parse element-affinity matrix (PROT 0898)")?;

    print!("{:>8}", "atk\\def");
    for d in 0..ELEMENT_COUNT {
        print!(
            " {:>7}",
            Element::from_id(d as u8).map(|e| e.name()).unwrap_or("?")
        );
    }
    println!();
    for (a, row) in aff.matrix.iter().enumerate() {
        print!(
            "{:>8}",
            Element::from_id(a as u8).map(|e| e.name()).unwrap_or("?")
        );
        for cell in row {
            print!(" {:>7}", cell);
        }
        println!();
    }
    Ok(())
}

pub(crate) fn cmd_spell_costs(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    match apply::current_spell_costs(&patcher)? {
        Some(spells) => {
            for s in &spells {
                println!("  {:>3}  {:<16} {:>3} MP", s.id, s.name, s.mp);
            }
            println!("{} named, costed spells", spells.len());
        }
        None => println!("spell table not found"),
    }
    Ok(())
}

/// Render an equip-character mask (`+6`) as a compact `V/N/G` string
/// (bit `1` Vahn, `2` Noa, `4` Gala); `7` prints `any`.
fn equip_mask_label(mask: u8) -> String {
    if mask & 0x7 == 0x7 {
        return "any".to_string();
    }
    let mut s = String::new();
    for (bit, ch) in [(1u8, 'V'), (2, 'N'), (4, 'G')] {
        if mask & bit != 0 {
            s.push(ch);
        }
    }
    if s.is_empty() { "-".to_string() } else { s }
}

pub(crate) fn cmd_equip_bonuses(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u8| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    match apply::current_equip_bonuses(&patcher)? {
        Some(rows) => {
            // Group consecutive same-slot rows for a readable, category-first table.
            let mut cur = "";
            for r in &rows {
                if r.slot != cur {
                    cur = r.slot;
                    println!("\n[{}]", r.slot);
                }
                let [int, atk, udf, ldf, spd] = r.stats;
                let items: Vec<String> = r.items.iter().map(|&id| nm(id)).collect();
                println!(
                    "  row {:>2}  INT {:>3} ATK {:>3} UDF {:>3} LDF {:>3} SPD {:>3}  {:<5}  [{}]",
                    r.row,
                    int,
                    atk,
                    udf,
                    ldf,
                    spd,
                    equip_mask_label(r.mask),
                    items.join(", ")
                );
            }
            let referenced = rows.iter().filter(|r| !r.items.is_empty()).count();
            println!(
                "\n{} bonus rows ({} referenced by equipment - the randomizable population)",
                rows.len(),
                referenced
            );
        }
        None => println!("equipment stat-bonus table not found"),
    }
    Ok(())
}

pub(crate) fn cmd_weapon_specialty(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let cur = apply::current_specialties(&patcher)?;
    if cur.is_empty() {
        println!("player battle files not found");
        return Ok(());
    }
    println!("character  favored weapon class");
    for a in &cur {
        let note = if a.from == a.to {
            String::new()
        } else {
            format!("  (vanilla: {})", a.from)
        };
        println!("  {:<7}  {}{note}", a.character, a.to);
    }
    println!("\n--weapon-specialty permutes these three favored classes among the characters.");
    Ok(())
}

pub(crate) fn cmd_drops(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let drops = apply::current_drops(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let mut n = 0;
    for d in &drops {
        if d.item == 0 {
            continue;
        }
        let name = item_names
            .as_ref()
            .and_then(|t| t.name(d.item))
            .unwrap_or("?");
        println!(
            "monster {:>3}  drop item {:>3} ({:<16})  {:>3}%",
            d.monster_id, d.item, name, d.chance
        );
        n += 1;
    }
    println!("{n} monsters have a drop (of {} slots)", drops.len());
    Ok(())
}

pub(crate) fn cmd_doors(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let doors = apply::current_doors(&patcher)?;
    let mut cur = String::new();
    let mut scenes = 0usize;
    for d in &doors {
        if d.home_scene != cur || cur.is_empty() {
            cur = d.home_scene.clone();
            scenes += 1;
            println!("[{:>4}] {}", d.entry_idx, d.home_scene);
        }
        let class = match d.class {
            apply::DoorSiteClass::WalkDoor => "walk-door",
            apply::DoorSiteClass::ScriptInvoked => "EXCLUDED script",
            apply::DoorSiteClass::WorldMap => "EXCLUDED world-map",
        };
        println!(
            "    -> {:<10} (index {:>4})  entry=({:#04x},{:#04x}) dir={:#04x}  @0x{:x}  [{class}]",
            d.dest_scene, d.index, d.entry_x, d.entry_z, d.dir, d.op_pc
        );
    }
    let pool = doors
        .iter()
        .filter(|d| d.class == apply::DoorSiteClass::WalkDoor)
        .count();
    println!(
        "\n{} doors across {scenes} scenes ({pool} in the shuffle pool; the rest are \
         script/cutscene-invoked or world-map transitions, kept vanilla)",
        doors.len()
    );
    Ok(())
}

pub(crate) fn cmd_house_doors(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let sites = apply::current_house_doors(&patcher)?;
    let cdname = legaia_iso::iso9660::read_file_in_image(patcher.image(), "CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| legaia_prot::cdname::parse_str(&s).ok());
    let scene_of = |idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for(m, idx as u32))
            .unwrap_or("?")
            .to_string()
    };
    let mut cur_entry = usize::MAX;
    let mut scenes = 0usize;
    for (idx, tx, tz) in &sites {
        if *idx != cur_entry {
            cur_entry = *idx;
            scenes += 1;
            println!("[{idx:>4}] {}", scene_of(*idx));
        }
        println!("    door warp -> tile ({tx:>3}, {tz:>3})");
    }
    println!(
        "\n{} intra-town door-warp targets across {scenes} scenes",
        sites.len()
    );
    Ok(())
}

pub(crate) fn cmd_map_doors(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let sites = apply::current_map_doors(&patcher)?;
    let mut cur_entry = usize::MAX;
    let mut scenes = 0usize;
    let mut eligible = 0usize;
    for (idx, scene, s) in &sites {
        if *idx != cur_entry {
            cur_entry = *idx;
            scenes += 1;
            println!("[{idx:>4}] {scene}");
        }
        let class = match s.class {
            legaia_patcher::map_door::MapDoorClass::MainBound => "exit (main-bound)",
            legaia_patcher::map_door::MapDoorClass::PocketBound => "entry (pocket-bound)",
            legaia_patcher::map_door::MapDoorClass::Static => "static (unattributed)",
        };
        if s.class != legaia_patcher::map_door::MapDoorClass::Static {
            eligible += 1;
        }
        println!(
            "    tile ({:>3},{:>3}) -> dest ({:>3},{:>3})  landing tile ({:>3},{:>3})  {class}",
            s.tile.0,
            s.tile.1,
            s.dest.0,
            s.dest.1,
            s.dest_tile().0,
            s.dest_tile().1
        );
    }
    println!(
        "\n{} kind-0 intra-scene teleports across {scenes} scenes ({eligible} shuffle-eligible)",
        sites.len()
    );
    Ok(())
}

pub(crate) fn cmd_chests(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let chests = apply::current_chests(&patcher)?;

    // Resolve item ids -> names (SCUS table) and PROT-entry -> scene name
    // (CDNAME.TXT), both off the user's own disc. Purely for legibility.
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let cdname = legaia_iso::iso9660::read_file_in_image(patcher.image(), "CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| legaia_prot::cdname::parse_str(&s).ok());
    let scene_of = |entry_idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for(m, entry_idx as u32))
            .unwrap_or("?")
            .to_string()
    };

    // Group consecutive chests by scene for a readable table.
    let mut last_entry: Option<usize> = None;
    let mut per_item: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for c in &chests {
        if last_entry != Some(c.entry_idx) {
            println!("\n[entry {:>4}  {}]", c.entry_idx, scene_of(c.entry_idx));
            last_entry = Some(c.entry_idx);
        }
        println!(
            "  item {:>3} (0x{:02x})  {}",
            c.item,
            c.item,
            name_of(c.item)
        );
        *per_item.entry(c.item).or_default() += 1;
    }

    println!(
        "\n{} chest give-item sites across {} scenes, {} distinct items.",
        chests.len(),
        chests
            .iter()
            .map(|c| c.entry_idx)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        per_item.len(),
    );
    println!("\nItem multiset (id  count  name):");
    for (id, count) in &per_item {
        println!(
            "  {:>3} (0x{:02x})  x{:<3}  {}",
            id,
            id,
            count,
            name_of(*id)
        );
    }
    Ok(())
}

pub(crate) fn cmd_arts(input: &Path) -> Result<()> {
    use legaia_art::queue::Character;
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .context("read SCUS_942.54")?;
    let entries =
        legaia_art::arts_table::parse_from_scus(&scus).context("parse arts-name table")?;
    let mut regular = 0usize;
    for ch in Character::all() {
        println!("{}:", ch.name());
        for e in entries.iter().filter(|e| e.character == ch) {
            let combo = legaia_patcher::arts::pretty_combo(&e.commands);
            let tag = if e.is_miracle {
                "  [Miracle, not randomized]"
            } else {
                ""
            };
            println!(
                "  {:>2}  ap{:>3}  {:<11}  {}{}",
                e.index,
                e.ap,
                if combo.is_empty() { "-".into() } else { combo },
                e.name,
                tag
            );
            if !e.is_miracle {
                regular += 1;
            }
        }
    }
    println!(
        "\n{} arts total, {} regular arts the randomizer reassigns (3 Miracle arts left untouched).",
        entries.len(),
        regular
    );
    Ok(())
}

pub(crate) fn cmd_steals(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let steals = apply::current_steals(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let mut per_item: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for s in &steals {
        println!(
            "monster {:>3}  steal item {:>3} (0x{:02x}, {:<16})  {:>3}%",
            s.monster_id,
            s.item,
            s.item,
            name_of(s.item),
            s.chance
        );
        *per_item.entry(s.item).or_default() += 1;
    }
    println!(
        "\n{} monsters are stealable, {} distinct steal items.",
        steals.len(),
        per_item.len()
    );
    Ok(())
}

pub(crate) fn cmd_starting_items(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let items = apply::current_starting_items(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let all_warps = apply::current_all_warps(&patcher)?;
    if items.is_empty() {
        println!("The new game starts with an empty inventory.");
    } else {
        println!("New game starting inventory:");
        for (id, count) in &items {
            println!(
                "  {:>3} x item {:>3} (0x{:02x}, {})",
                count,
                id,
                id,
                name_of(*id)
            );
        }
        println!(
            "\n{} slot(s) seeded (the randomizer can set up to {}).",
            items.len(),
            legaia_patcher::starting_items::MAX_STARTING_ITEMS
        );
    }
    println!(
        "Door-of-Wind all-warps preset: {}",
        if all_warps { "ON" } else { "off" }
    );
    let level = apply::current_starting_level(&patcher)?;
    println!(
        "Starting level: {}{}",
        level,
        if level == 1 { " (vanilla)" } else { "" }
    );
    Ok(())
}
