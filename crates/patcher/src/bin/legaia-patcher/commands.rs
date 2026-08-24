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

pub(crate) fn cmd_fishing(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let overlay = patcher
        .read_entry(legaia_patcher::fishing_price::OVERLAY_PROT_INDEX)
        .context("read fishing overlay (PROT 972)")?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u32| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id as u8))
            .unwrap_or("?")
            .to_string()
    };
    let venue = |page: usize| if page == 0 { "Buma" } else { "Vidna" };
    let rows =
        legaia_patcher::fishing_price::list_prizes(&overlay).context("parse fishing exchange")?;
    let mut cur_page = usize::MAX;
    for p in rows {
        if p.page != cur_page {
            println!("{} pond:", venue(p.page));
            cur_page = p.page;
        }
        let kind = if p.one_time { "one-time" } else { "repeat  " };
        println!(
            "  row {}  {:<16} {:>7} pts  [{kind}]  (id 0x{:02X})",
            p.row,
            nm(p.item_id),
            p.price,
            p.item_id
        );
    }
    Ok(())
}

pub(crate) fn cmd_earth_egg(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    match legaia_patcher::apply::current_earth_egg(&patcher)? {
        Some(info) => {
            let name = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
                .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus))
                .and_then(|t| t.name(info.item_id).map(str::to_string))
                .unwrap_or_else(|| "Earth Egg".to_string());
            println!("Earth Egg exchange (Sol Tower Prize Counter):");
            println!("  scene bundle: PROT entry {}", info.entry_idx);
            println!("  prize:        {name} (item 0x{:02X})", info.item_id);
            println!(
                "  coins required: {}  (gate = coins > {}; debit = {} on purchase)",
                info.price, info.threshold, info.debit
            );
        }
        None => println!("Earth Egg exchange not found on this disc."),
    }
    Ok(())
}

pub(crate) fn cmd_locations(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let inv = legaia_patcher::apply::list_locations(&patcher).context("read place names")?;

    println!("Quick-travel landmark cells (SCUS_942.54; --rename-location INDEX=NAME)");
    for (idx, name) in &inv.landmarks {
        println!("  {idx:>2}  {name}");
    }

    println!("\nWorld-map labels (kingdom MANs; --rename-location \"NAME=NEW\")");
    const KINGDOM: [&str; 3] = ["Drake", "Sebucus", "Karisto"];
    for (region, x, y, name) in &inv.world_map {
        let kingdom = KINGDOM.get(*region as usize).copied().unwrap_or("?");
        println!("  {kingdom:<8} ({x:>3},{y:>3})  {name}");
    }

    println!("\nScene-entry banners (per-scene MANs; scenes carrying each name)");
    for (name, count) in &inv.scene_banners {
        println!("  {count:>2}x  {name}");
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
        // Join the per-strike damage-power bytes (record0 +0x24) for this char.
        let power_by_combo: std::collections::HashMap<Vec<u8>, Vec<u8>> = patcher
            .read_entry(legaia_patcher::arts_power::player_entry_index(ch))
            .ok()
            .and_then(|entry| legaia_patcher::arts_power::labeled_art_powers(&scus, &entry, ch))
            .map(|list| {
                list.into_iter()
                    .map(|a| (a.combo.iter().map(|c| c.as_byte()).collect(), a.power))
                    .collect()
            })
            .unwrap_or_default();
        println!("{}:", ch.name());
        for e in entries.iter().filter(|e| e.character == ch) {
            let combo = legaia_patcher::arts::pretty_combo(&e.commands);
            let key: Vec<u8> = e.commands.iter().map(|c| c.as_byte()).collect();
            let power = power_by_combo.get(&key);
            let tiers = match power {
                Some(p) if !p.is_empty() => p
                    .iter()
                    .map(|&b| {
                        legaia_patcher::arts_power::power_tier(b)
                            .map(|(u, m)| format!("{}x{m}", if u { "U" } else { "L" }))
                            .unwrap_or_else(|| format!("{b:02X}"))
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                _ => "-".into(),
            };
            let tag = if e.is_miracle { "  [Miracle]" } else { "" };
            println!(
                "  {:>2}  ap{:>3}  {:<11}  power [{:<12}]  {}{}",
                e.index,
                e.ap,
                if combo.is_empty() { "-".into() } else { combo },
                tiers,
                e.name,
                tag
            );
            if !e.is_miracle {
                regular += 1;
            }
        }
        // Super Arts. They carry no combo and no arts-name-table row, so they
        // are listed from their own record0 records (located by finisher
        // constant, name-validated) with the chain that triggers them.
        let supers = patcher
            .read_entry(legaia_patcher::super_art_power::player_entry_index(ch))
            .ok()
            .and_then(|entry| legaia_patcher::super_art_power::super_art_powers(&scus, &entry, ch))
            .unwrap_or_default();
        if !supers.is_empty() {
            println!("  -- Super Arts (no combo, no AP; the chain arts pay) --");
        }
        for s in &supers {
            let tiers = if s.power.is_empty() {
                "-".to_string()
            } else {
                s.power
                    .iter()
                    .map(|&b| {
                        legaia_patcher::arts_power::power_tier(b)
                            .map(|(u, m)| format!("{}x{m}", if u { "U" } else { "L" }))
                            .unwrap_or_else(|| format!("{b:02X}"))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            // Project the trigger pattern onto the named arts it chains.
            let chain: Vec<String> = legaia_patcher::super_art_power::super_arts_for(ch)
                .into_iter()
                .find(|t| t.finisher == s.finisher)
                .map(|t| {
                    t.art_sequence()
                        .into_iter()
                        .map(|c| {
                            entries
                                .iter()
                                .find(|e| {
                                    e.character == ch && u16::from(e.index) + 0x1B == u16::from(c)
                                })
                                .map(|e| e.name.clone())
                                .unwrap_or_else(|| format!("{c:#04X}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            println!(
                "  {:02X}  ap  -  {:<11}  power [{:<12}]  {}",
                s.finisher, "-", tiers, s.name
            );
            if !chain.is_empty() {
                println!("            chain: {}", chain.join(" > "));
            }
        }
    }
    println!(
        "\n{} arts total. `--arts-power COMBO=VALUE` rebalances an art's damage \
         (power byte 0x0C..=0x1F = tier, or 0 to disable). `--arts-ap-grant \
         COMBO=AMOUNT` makes an art grant AP (Spirit) instead of costing it; the \
         leftmost number is the arts-table index, which is the shared config row \
         - AP-grant applies to every character's art at that same index. Super \
         Arts have no combo, so they are addressed by name: \
         `--super-art-power \"Tri-Somersault\"=0x1A`. A Super Art also costs no \
         AP of its own - the chain arts printed under it pay it, so set the AP \
         of a Super Art's setup on those arts' own combos.",
        entries.len()
    );
    let _ = regular;
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

/// Print the Delilas-dome SCUS-side injection as a `LEGAIA_POKES` string for
/// the PCSX-Redux probes (`autorun_delilas_dome_course.lua` /
/// `autorun_delilas_reward_trace.lua`). Static data - no disc required.
pub(crate) fn cmd_delilas_pokes(custom_items: bool) -> Result<()> {
    use legaia_patcher::{custom_items as ci, delilas_dome};

    let mut writes = delilas_dome::probe_ram_writes();
    if custom_items {
        writes.extend(ci::probe_ram_writes());
    }
    let mut pokes: Vec<String> = Vec::new();
    for (va, bytes) in writes {
        anyhow::ensure!(
            bytes.len() % 4 == 0,
            "probe write at {va:#x} is not word-aligned"
        );
        for (i, w) in bytes.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(w.try_into().unwrap());
            pokes.push(format!("0x{:08X}:0x{:08X}", va + (i as u32) * 4, word));
        }
    }
    // Course-unlock story flag (COURSE_FLAG): the seed routine tests it via
    // the retail flag reader (base 0x80085758, MSB-first bit order). The
    // whole byte is overwritten - its other bits are the retail course
    // unlocks 0x538..0x53F, which the koin1 arm clears anyway.
    let flag = u32::from(delilas_dome::COURSE_FLAG);
    let flag_addr = 0x8008_5758 + (flag >> 3);
    let flag_bit = 0x80u32 >> (flag & 7);
    pokes.push(format!("0x{flag_addr:08X}:0x{flag_bit:02X}:b"));
    println!("{}", pokes.join(","));
    eprintln!(
        "{} pokes: SCUS cave + stream hooks + PRG ERR print gate + course flag {:#x}.",
        pokes.len(),
        flag
    );
    eprintln!(
        "To reproduce the balance-wipe report, also seed the winnings counter: 0x80084440:123456"
    );
    Ok(())
}

/// Static verdict on a patched image's `--delilas-party` build: is the
/// swap present, and do the rebuilt player battle files carry the current
/// invariants? Run this on the exact `.bin` about to be play-tested.
///
/// The failure class this exists for: a rom patched by a *stale* build
/// (browser-cached wasm, an old local server) reproduces bugs that are
/// already fixed in the tree, and nothing in the play-test distinguishes
/// "fix does not work" from "fix is not on this disc". Every check here is
/// a property of the disc bytes alone, so the verdict lands in seconds.
///
/// Checks per rebuilt player file (863/864/865):
/// - **No `0xFE` equipment extras** in any equipment assembly. The swap
///   emits none by construction; a surviving extra re-enables the
///   variant-pair ordinal overrun (`ctx+0x240` past the 2-pair snapshot)
///   whose out-of-range per-frame pin installs a foreign object pointer -
///   the Spirit-streak / idle-artifact class.
/// - **Hand seat**: each hand object's local centroid stays near its
///   wrist pivot (retail hands measure ~21-36 units; the un-seated bake
///   defect measured 60-150).
/// - Every skeleton part carries geometry in every assembly.
pub(crate) fn cmd_delilas_verify(input: &Path) -> Result<()> {
    use legaia_asset::battle_char_assembly as bca;
    use legaia_asset::{battle_data_pack, monster_archive, party_swap};

    /// A baked hand's local-centroid magnitude ceiling (units). Retail
    /// hands sit 21-36 from the wrist pivot; the un-seated sibling fists
    /// measured 60 (Gi armA) and 150 (Che hammer).
    const HAND_SEAT_MAX: f32 = 48.0;

    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let archive = patcher
        .read_entry_footprint(867)
        .context("read monster archive (PROT 867)")?;

    // Swap detection: an applied `--delilas-party` renames each sibling's
    // monster block to the host character it now depicts.
    let hosts = ["Vahn", "Noa", "Gala"];
    let mut mapping: Vec<(usize, u16, String)> = Vec::new();
    for id in [162u16, 163, 164] {
        let name = monster_archive::record(&archive, id)?
            .map(|r| r.name)
            .unwrap_or_default();
        if let Some(slot) = hosts.iter().position(|h| name == *h) {
            mapping.push((slot, id, name));
        } else {
            println!("monster {id}: named {name:?} (not a swapped block)");
        }
    }
    if mapping.len() != 3 {
        anyhow::bail!(
            "delilas party swap NOT detected: {} of 3 sibling blocks are \
             hero-named. This image was not patched with --delilas-party \
             (or was patched by a build older than the swap).",
            mapping.len()
        );
    }
    for (slot, id, name) in &mapping {
        println!("monster {id} wears {name:?} -> player slot {slot} rebuilt");
    }

    let rigs = [
        &party_swap::RIG_VAHN_GALA,
        &party_swap::RIG_NOA,
        &party_swap::RIG_VAHN_GALA,
    ];
    let mut failures = 0usize;
    for &(slot, _, _) in &mapping {
        let who = hosts[slot];
        let rig = rigs[slot];
        let file = patcher
            .read_entry_footprint(863 + slot)
            .with_context(|| format!("read player file PROT {}", 863 + slot))?;
        let pack = battle_data_pack::parse(&file)
            .with_context(|| format!("{who}: parse player battle file"))?;

        // Group section record ids: a record id of 0 closes a section.
        let mut sections: Vec<Vec<u8>> = vec![Vec::new()];
        for rec in &pack.records {
            if rec.index == 0 {
                continue;
            }
            sections.last_mut().unwrap().push(rec.id as u8);
            if rec.id == 0 && sections.len() < bca::SECTION_COUNT {
                sections.push(Vec::new());
            }
        }

        // Every equipment assembly: default, plus each section id alone.
        let mut loadouts: Vec<[u8; bca::SECTION_COUNT]> = vec![[0; bca::SECTION_COUNT]];
        for (sec, ids) in sections.iter().enumerate() {
            for &id in ids {
                if id == 0 {
                    continue;
                }
                let mut eq = [0u8; bca::SECTION_COUNT];
                eq[sec] = id;
                loadouts.push(eq);
            }
        }

        let mut fe_hits = 0usize;
        let mut seat_worst: f32 = 0.0;
        let mut empty_bones = 0usize;
        // Weapon-fusion presence: a single-weapon loadout should carry at
        // least one flat-colour (untextured) primitive - the fused host
        // weapon's signature. Zero across every weapon record = a rom
        // from a build older than the fusion.
        let mut weapon_loadouts = 0usize;
        let mut fused_loadouts = 0usize;
        for eq in &loadouts {
            let Ok(asm) = bca::assemble_character(&file, &pack, eq) else {
                continue;
            };
            // 0xFE extras assemble with bone tags 100..200.
            fe_hits += asm
                .bone_tags
                .iter()
                .filter(|&&t| (100..200).contains(&t))
                .count();
            let Ok(tmd) = legaia_tmd::parse(&asm.tmd) else {
                continue;
            };
            let skeleton = bca::battle_animations(&file)
                .ok()
                .and_then(|a| a.first().map(|s| s.part_count))
                .unwrap_or(0);
            for (i, o) in tmd.objects.iter().enumerate() {
                let tag = asm.bone_tags[i];
                if (tag as usize) < skeleton
                    && rig.hair_channel != Some(tag)
                    && o.vertices.is_empty()
                {
                    empty_bones += 1;
                }
            }
            // Hand seat (canonical 5 and 8), measured over the vertices
            // the TEXTURED primitives reference: the fist itself. The
            // host's fused weapon (`weapon_fuse`) is flat-colour geometry
            // welded into the same object, legitimately authored far from
            // the wrist (a blade runs 250 units), and must not count
            // against the fist's seat.
            for c in [5usize, 8] {
                let ch = rig.channel_for_canonical[c];
                let Some(oi) = asm.bone_tags.iter().position(|&t| t == ch) else {
                    continue;
                };
                let o = &tmd.objects[oi];
                let mut corners = std::collections::BTreeSet::new();
                for pr in bca::equip_isolate::object_prim_refs(&tmd, &asm.tmd, oi) {
                    if !pr.uvs.is_empty() {
                        corners.extend(pr.corners.iter().copied());
                    }
                }
                if corners.is_empty() {
                    continue;
                }
                let n = corners.len() as f32;
                let s = corners
                    .iter()
                    .filter_map(|&ci| o.vertices.get(ci))
                    .fold([0f32; 3], |a, v| {
                        [a[0] + v.x as f32, a[1] + v.y as f32, a[2] + v.z as f32]
                    });
                let mag = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt() / n;
                if mag > seat_worst {
                    seat_worst = mag;
                }
            }
            if (2..=3).any(|sec| eq[sec] > 0x18) {
                weapon_loadouts += 1;
                let flat = asm
                    .bone_tags
                    .iter()
                    .enumerate()
                    .filter(|&(_, &tag)| tag < 100)
                    .map(|(oi, _)| {
                        bca::equip_isolate::object_prim_refs(&tmd, &asm.tmd, oi)
                            .iter()
                            .filter(|pr| pr.uvs.is_empty())
                            .count()
                    })
                    .sum::<usize>();
                if flat > 0 {
                    fused_loadouts += 1;
                }
            }
        }
        let fe_ok = fe_hits == 0;
        let seat_ok = seat_worst <= HAND_SEAT_MAX;
        let bones_ok = empty_bones == 0;
        let fuse_ok = weapon_loadouts == 0 || fused_loadouts > 0;
        if !fe_ok || !seat_ok || !bones_ok || !fuse_ok {
            failures += 1;
        }
        println!(
            "{who}: {} assemblies | 0xFE extras {} ({fe_hits}) | hand seat {} \
             (worst {seat_worst:.1} <= {HAND_SEAT_MAX}) | skeleton geometry {} \
             ({empty_bones} empty) | weapon fusion {} ({fused_loadouts}/{weapon_loadouts})",
            loadouts.len(),
            if fe_ok { "OK" } else { "FAIL" },
            if seat_ok { "OK" } else { "FAIL" },
            if bones_ok { "OK" } else { "FAIL" },
            if fuse_ok { "OK" } else { "FAIL" },
        );
    }
    if failures > 0 {
        anyhow::bail!(
            "delilas-verify FAILED for {failures} player file(s) - this \
             image was patched by a build missing current fixes. Re-patch \
             with the current patcher (hard-refresh the web page, or use \
             this CLI's `randomize --delilas-party`)."
        );
    }
    println!("delilas-verify PASS: swap present, all invariants hold.");
    Ok(())
}

/// Emit RAM pokes that bring a resident SCUS in line with a patched disc:
/// one `0xADDR:0xWORD` line per 32-bit word where the two discs' SCUS
/// images differ.
///
/// Why this exists: a PCSX-Redux save state carries the WHOLE RAM,
/// including the boot-loaded `SCUS_942.54` - so a probe that loads a
/// field state from one disc era and then triggers a battle on a NEWER
/// patched disc runs fresh overlay code (loaded from the disc) against a
/// STALE resident SCUS. A patched overlay `jal` into the SCUS injection
/// arena then executes whatever bytes the state was carrying - observed
/// as a per-frame "Unknown instruction for dynarec" fault at
/// `0x8007782C` that wedges the whole battle. Applying these pokes right
/// after the state load makes the resident SCUS byte-match the disc
/// under test.
///
/// The poke set is the patched-vs-baseline DIFF (not the whole SCUS):
/// the data segment holds live game state a blanket copy would corrupt,
/// while the differing words are exactly the patcher's own edits - hook
/// sites in text and dead-region arenas - which are safe to (re)write.
pub(crate) fn cmd_scus_pokes(patched: &Path, baseline: &Path) -> Result<()> {
    const SCUS_BASE_VA: u32 = 0x8001_0000;
    const HEADER: usize = 0x800; // PSX-EXE header before the loaded image
    let read_scus = |path: &Path| -> Result<Vec<u8>> {
        let image = load_image(path)?;
        let (lba, size) = legaia_iso::iso9660::find_file_in_image(&image, "SCUS_942.54")
            .ok_or_else(|| anyhow::anyhow!("{}: SCUS_942.54 not found", path.display()))?;
        let mut out = Vec::with_capacity(size as usize);
        for b in 0..size as usize {
            let sec = lba as usize + b / 2048;
            let at = sec * 2352 + 0x18 + b % 2048;
            out.push(
                *image
                    .get(at)
                    .ok_or_else(|| anyhow::anyhow!("{}: image truncated", path.display()))?,
            );
        }
        Ok(out)
    };
    let a = read_scus(patched)?;
    let b = read_scus(baseline)?;
    let n = a.len().min(b.len());
    let mut count = 0usize;
    for off in (HEADER..n.saturating_sub(3)).step_by(4) {
        if a[off..off + 4] != b[off..off + 4] {
            let va = SCUS_BASE_VA + (off - HEADER) as u32;
            let word = u32::from_le_bytes(a[off..off + 4].try_into().unwrap());
            println!("0x{va:08X}:0x{word:08X}");
            count += 1;
        }
    }
    eprintln!("{count} differing SCUS words (patched vs baseline)");
    Ok(())
}
