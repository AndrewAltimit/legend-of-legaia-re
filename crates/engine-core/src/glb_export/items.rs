//! Equipment item export (`export-glb --items`) and its manifest.

use super::*;

// ---------------------------------------------------------------------------
// Equipment item export (`export-glb --items`)
// ---------------------------------------------------------------------------

/// One exported equipment item: the two `.glb` flavours the characters page
/// offers per record (the item **alone** with its grip repaired, and the
/// record-keeping palette cut **with its host limb**), plus the manifest
/// facts a consumer needs to trust the cut.
pub struct ItemGlb {
    /// Character label (`Vahn` / `Noa` / `Gala` / `Terra`).
    pub character: &'static str,
    pub cslot: usize,
    /// Player-file section index (order differs per character).
    pub section: usize,
    /// Human section label derived from the SCUS equipment stat table.
    pub section_label: String,
    /// Equipment id in the player file's descriptor table.
    pub id: u32,
    /// SCUS item-table display name, when the executable was readable.
    pub name: Option<String>,
    /// Suggested relative file stem (`vahn/s2_005_fire-blade`).
    pub file_stem: String,
    /// The item-alone glb (empty when the cut kept nothing).
    pub glb_alone: Vec<u8>,
    /// The item + host-limb glb (empty when the section contributed none).
    pub glb_with_limb: Vec<u8>,
    /// How the palette cut classed the record (`own-object` / `separate` /
    /// `welded` / `fused`).
    pub class: String,
    /// `false` = the grip is open on the record itself (the shaft inside the
    /// closed fist was never modelled).
    pub complete: bool,
    /// How the item-alone cut decided (`colour-diff` / `identity` / `whole` /
    /// `palette`).
    pub mode: String,
    /// Whether a committed `equip-isolation.toml` rule touched the record.
    pub curated: bool,
    /// Grip-repair bridges the alone mesh carries (0 = none needed/found).
    pub bridges: usize,
    /// Clips baked into both files (action bank + weapon swings).
    pub clip_count: usize,
}

/// Every equippable record of every player battle file, exported.
pub struct ItemsExport {
    pub items: Vec<ItemGlb>,
    /// Tolerated decode degradations, labelled by character/loadout.
    pub notes: Vec<String>,
}

fn item_file_stem(character: &str, section: usize, id: u32, name: Option<&str>) -> String {
    let slug: String = name
        .unwrap_or("item")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{}/s{}_{:03}_{}",
        character.to_lowercase(),
        section,
        id,
        slug
    )
}

/// Assemble each character wearing exactly one item per equippable record
/// and bake both per-item `.glb` flavours through the shared loadout kernel
/// (`legaia_actor_formats::battle_char_assembly::loadout` - the same code the
/// browser characters page runs). `scus` (the `SCUS_942.54` bytes) supplies
/// item display names and section labels; without it both fall back to ids.
///
/// Arts clips are not baked here - an item file carries the action bank and
/// the equipment-spliced weapon swings, which is what moves a held piece.
pub fn export_equipment_item_glbs(
    index: &ProtIndex,
    scus: Option<&[u8]>,
) -> Result<ItemsExport, String> {
    use legaia_actor_formats::battle_char_assembly as bca;
    use legaia_actor_formats::battle_char_assembly::loadout;

    let names = scus.and_then(legaia_game_tables::item_names::ItemNameTable::from_scus);
    let stats = scus.and_then(legaia_game_tables::equip_stats::EquipStatTable::from_scus);
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for (cslot, character) in loadout::CHARACTER_LABELS.iter().enumerate() {
        let prot_index = loadout::PLAYER_FILE_BASE + cslot as u32;
        let raw = index
            .entry_bytes(prot_index)
            .map_err(|e| format!("player file (PROT {prot_index}): {e}"))?;
        let pack = legaia_actor_formats::battle_data_pack::parse(&raw)
            .map_err(|e| format!("player file {prot_index}: {e}"))?;
        let cats = loadout::section_catalog(&pack);
        let labels = loadout::section_labels(&cats, stats.as_ref(), cslot);
        for (section, cat) in cats.iter().enumerate() {
            for &id in &cat.ids {
                let Ok(id_u8) = u8::try_from(id) else {
                    notes.push(format!("{character} s{section} id {id}: out of u8 range"));
                    continue;
                };
                let mut equipped = [0u8; bca::SECTION_COUNT];
                equipped[section] = id_u8;
                let c = match loadout::build(&raw, cslot, equipped, false, &[]) {
                    Ok(c) => c,
                    Err(why) => {
                        notes.push(format!("{character} s{section} id {id}: {why}"));
                        continue;
                    }
                };
                notes.extend(c.notes.iter().map(|n| format!("{character}: {n}")));
                let Some(it) = c.items.iter().find(|i| i.section == section) else {
                    notes.push(format!(
                        "{character} s{section} id {id}: section contributed no geometry"
                    ));
                    continue;
                };
                let name = u8::try_from(it.id)
                    .ok()
                    .and_then(|i| names.as_ref()?.name(i))
                    .map(str::to_string);
                items.push(ItemGlb {
                    character,
                    cslot,
                    section,
                    section_label: labels[section].clone(),
                    id: it.id,
                    file_stem: item_file_stem(character, section, it.id, name.as_deref()),
                    name,
                    glb_alone: loadout::item_only_glb(&c, section, names.as_ref()),
                    glb_with_limb: loadout::item_glb(&c, section, names.as_ref()),
                    class: it.partition.class.tag().to_string(),
                    complete: it.partition.class.is_complete(),
                    mode: it.isolation.mode.tag().to_string(),
                    curated: it.isolation.curated,
                    bridges: it.alone.bridges.len(),
                    clip_count: c.clips.len(),
                });
            }
        }
    }
    Ok(ItemsExport { items, notes })
}

/// The `items/manifest.json` document for one [`ItemsExport`].
pub fn items_manifest(e: &ItemsExport) -> Value {
    json!({
        "generator": "legaia-engine export-glb --items",
        "source": "player battle files data\\battle\\PLAYER1..4 (extraction PROT 863..866)",
        "conventions": {
            "units": "raw PSX model units (same as the site's character downloads) - scale instances by your world export's `scale`",
            "clips": "each glb carries the character's battle action bank + the weapon's spliced direction swings; clip 0 frame 0 is the rest pose",
            "alone_vs_with_limb": "the `_alone` file is the opinionated item-alone cut (grip repaired); `_with_limb` is the exact palette cut with its ground-truth host limb",
        },
        "items": e.items.iter().map(|i| json!({
            "character": i.character,
            "slot": i.cslot,
            "section": i.section,
            "section_label": i.section_label,
            "id": i.id,
            "name": i.name,
            "alone": (!i.glb_alone.is_empty()).then(|| format!("{}_alone.glb", i.file_stem)),
            "with_limb": (!i.glb_with_limb.is_empty()).then(|| format!("{}_with_limb.glb", i.file_stem)),
            "class": i.class,
            "complete": i.complete,
            "isolation_mode": i.mode,
            "curated": i.curated,
            "grip_bridges": i.bridges,
            "clips": i.clip_count,
        })).collect::<Vec<_>>(),
    })
}
