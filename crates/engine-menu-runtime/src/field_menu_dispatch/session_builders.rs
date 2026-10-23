//! Builders for each pause-menu sub-session from the live `World`.

use super::*;

/// The members a pause-menu picker lists, as `(roster id, record)`: the
/// **present party** in party-list order, not every roster record.
///
/// Retail's item and spell flows hand the cursor kernel the party count
/// (`lbu a1,0x454(s0)` over `0x80084140`, i.e. `0x80084594`) and resolve the
/// picked row through the roster id bytes (`lbu a2,0x458(v0)`, `0x80084598 +
/// cursor`) before calling the applier - `FUN_801D8308` at `0x801D83C0` /
/// `0x801D851C` for an item, `FUN_801D9594` at `0x801D964C` / `0x801D96BC`
/// for a spell. The New Game template seeds every record, so a roster walk
/// offered Noa, Gala and Terra as heal targets to a Vahn travelling alone,
/// and an item used on one of them was spent on a member not in the party.
// REF: FUN_801D8308, FUN_801D9594
fn present_members(world: &World) -> Vec<(usize, &legaia_save::CharacterRecord)> {
    world
        .present_party_list()
        .into_iter()
        .filter_map(|slot| {
            let i = usize::from(slot);
            let member = world.party.roster.members.get(i)?;
            (member.hp_mp_sp().hp_max != 0).then_some((i, member))
        })
        .collect()
}

pub(super) fn build_spell_session(world: &World, catalog: &SpellCatalog) -> SpellMenuSession {
    let names = world.party.roster_names();
    let party: Vec<SpellCasterSlot> = present_members(world)
        .into_iter()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            let list = member.spell_list();
            let n = list.count as usize;
            SpellCasterSlot {
                slot: i as u8,
                name: names.get(i).cloned().unwrap_or_default(),
                hp: hms.hp_cur,
                mp: hms.mp_cur,
                hp_max: hms.hp_max,
                mp_max: hms.mp_max,
                level: member.magic_rank(),
                spells: list.ids[..n].to_vec(),
                spell_levels: list.levels[..n].to_vec(),
                // Per-caster MP-cost ability bits: retail's kernel reads the
                // caster's own record `+0xF4` word (`0x800353B4`), so this is
                // keyed by the roster record, not by battle ordinal (the
                // `character_ability_bits` mirror is ordinal-indexed and
                // names a different member once `active_party` reorders).
                ability_bits: legaia_engine_battle::spells::record_ability_word(member),
                // Retail resolves the Ra-Seru slot through the
                // per-character offset table at 0x8007B424; the engine's
                // roster always carries the Ra-Seru equipped, so the
                // gate stays open here.
                ra_seru_missing: false,
            }
        })
        .collect();
    let targets: Vec<SpellTargetRow> = present_members(world)
        .into_iter()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            SpellTargetRow {
                slot: i as u8,
                name: names.get(i).cloned().unwrap_or_default(),
                hp: hms.hp_cur,
                hp_max: hms.hp_max,
            }
        })
        .collect();
    // Retail's list build asks the spell-record broadcast `FUN_8003053C`
    // whether each spell would affect anybody (`0x80031210`), and both cast
    // flows ask again before they commit (`0x801D954C` / `0x801D98B4`).
    let mut unaffected: Vec<u8> = Vec::new();
    for c in &party {
        for &id in &c.spells {
            if crate::menu_validator::spell_affects_anyone(world, id) == Some(false)
                && !unaffected.contains(&id)
            {
                unaffected.push(id);
            }
        }
    }
    SpellMenuSession::new(party, targets, catalog.clone()).with_unaffected_spells(unaffected)
}

fn build_inventory_session(world: &World) -> InventoryUseSession {
    let names = world.party.roster_names();
    // Retail's Use-list row order when the on-disc effect table is installed
    // (`WorldBagRows::bag_use_rows`, the SCUS content-id-3 builder over the bag's
    // active window): slot walk, three-buffer grouping, field context. The
    // id-sorted fallback is what a disc-free host gets - with no descriptors
    // there is nothing to group by. The paired PauseItemRow list is built in
    // the same order; keep these in lockstep.
    let (items, bag_slots): (Vec<u8>, Vec<u8>) = match world.bag_use_rows(false) {
        Some(rows) => (
            rows.iter().map(|r| r.id).collect(),
            rows.iter().map(|r| r.slot).collect(),
        ),
        None => {
            let mut v: Vec<u8> = world
                .party
                .inventory
                .iter()
                .filter_map(|(id, qty)| if *qty > 0 { Some(*id) } else { None })
                .collect();
            v.sort_unstable();
            (v, Vec::new())
        }
    };
    let targets: Vec<InvTargetRow> = present_members(world)
        .into_iter()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            let mut row = InvTargetRow::new(i as u8, names.get(i).cloned().unwrap_or_default())
                .with_stats(hms.hp_cur, hms.hp_max, hms.mp_cur, hms.mp_max)
                .with_statuses(
                    world
                        .battle
                        .status_effects
                        .statuses(i as u8)
                        .iter()
                        .map(|s| s.kind),
                );
            // A fallen ally (HP 0) gates revive items in / heals out.
            row.alive = !(hms.hp_cur == 0 && hms.hp_max > 0);
            row
        })
        .collect();
    InventoryUseSession::new(
        world.tables.item_catalog.clone(),
        items,
        targets,
        InventoryContext::Field,
    )
    // The row payloads: retail's list entries carry a bag slot, and Use /
    // Throw Out remove from the slot the row named.
    .with_bag_slots(bag_slots)
}

/// Resolve one item id's name / description / passive lines through the
/// world's disc text tables, with the curated catalog and a raw-id spelling
/// as fallbacks.
///
/// Lifted out of the Items-screen session builder because it is not the
/// Items screen's: retail's item-info panel (`FUN_801D0F1C`) is opened by
/// window 17 on the Items screen **and** by window 24 on the Equip screen's
/// candidate step, off the same table. Keeping the resolution inside one
/// screen's builder is why the Equip screen showed raw ids where retail
/// shows names.
pub fn item_display_text(world: &World, id: u8) -> ItemDisplayText {
    let text = world.menu.text.as_ref();
    ItemDisplayText {
        name: text
            .and_then(|t| t.item_name(id))
            .map(str::to_string)
            .or_else(|| {
                world
                    .tables
                    .item_catalog
                    .get(id)
                    .map(|e| e.name.to_string())
            })
            .unwrap_or_else(|| format!("Item {id:02X}")),
        desc: text
            .and_then(|t| t.item_desc(id))
            .unwrap_or_default()
            .to_string(),
        passive: text.and_then(|t| t.item_passive_text(id)),
    }
}

/// Build the retail Items screen session: the item-use flow plus the
/// per-row display data (real bag counts; names / descriptions /
/// accessory passive lines resolved through the world's disc text tables
/// with catalog + raw-id fallbacks).
pub fn build_pause_items_session(world: &World) -> PauseItemsSession {
    let inner = build_inventory_session(world);
    let slots = inner.bag_slots.clone();
    // The dim / alt-ink bits each list build gives a slot - the row ink the
    // list kernel stages from them is per list (`list_row_ink`).
    let ink_bits = |rows: Option<Vec<crate::bag_rows::BagRow>>| {
        rows.unwrap_or_default()
            .into_iter()
            .map(|r| {
                let mut bits = 0u16;
                if r.dim {
                    bits |= legaia_engine_menus::menu_list_rows::ROW_DISABLED;
                }
                if r.alt_ink {
                    bits |= legaia_engine_menus::menu_list_rows::ROW_ALT_INK;
                }
                (r.slot, bits)
            })
            .collect::<std::collections::HashMap<u8, u16>>()
    };
    let use_bits = ink_bits(world.bag_use_rows(false));
    let throw_bits = ink_bits(world.bag_throw_out_rows());
    let rows: Vec<PauseItemRow> = inner
        .items
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let t = item_display_text(world, id);
            let slot = slots.get(i).copied().unwrap_or(0);
            PauseItemRow {
                id,
                slot,
                name: t.name,
                count: world.party.inventory.get(&id).copied().unwrap_or(0),
                desc: t.desc,
                passive: t.passive,
                use_bits: use_bits.get(&slot).copied().unwrap_or(0),
                throw_bits: throw_bits.get(&slot).copied().unwrap_or(0),
            }
        })
        .collect();
    // The Throw Out command's own row order (content id 0x22). The screen is
    // built in the Use order (content id 3), and this is a bag-slot sequence,
    // so the command window can permute the rows it already holds.
    let throw_out_slots: Vec<u8> = world
        .bag_throw_out_rows()
        .map(|rows| rows.iter().map(|r| r.slot).collect())
        .unwrap_or_default();
    PauseItemsSession::new(inner, rows)
        .with_arrange_rank(world.menu.arrange_rank.clone())
        .with_warp_destinations(warp_destinations(world))
        .with_throw_out_row_order(throw_out_slots)
        .with_incense_window(world.locomotion.walk_regen_window)
}

/// The visible rows of the quick-travel landmark list - the Door of Wind
/// destination list, and the same set the world-map landmark menu shows.
///
/// A faithful walk of `FUN_80030628` case `0x19`
/// (`0x80031870..0x800318dc`) over the disc placement table
/// ([`legaia_game_tables::worldmap_menu`], installed by
/// [`World::install_menu_text`]):
///
/// - records run to the `name_idx == 0xFF` terminator (the parser already
///   stops there);
/// - a record whose `name_idx` repeats the **last accepted** row's is
///   skipped (`0x800318a8`: the compare is against `s0`, which only
///   advances on an accept, so a locked landmark does not shadow the next
///   record that shares its name);
/// - the survivor is gated on system flag `discovery_flag + 0x20`
///   (`0x800318b4`, `FUN_8003CE64` = [`World::system_flag_test`]);
/// - an accepted row is pushed as the string id `0x8000 | record_index`
///   (`0x800318c0`), which is why [`WarpDestination::record_index`] is the
///   record ordinal and not the visible one.
///
/// Empty when the executable was not reachable at boot - the route then
/// opens an empty list rather than an invented one.
///
/// PORT: FUN_80030628 (case 0x19: the landmark-list build)
pub fn warp_destinations(
    world: &World,
) -> Vec<legaia_engine_menus::pause_screens::WarpDestination> {
    let Some(menu) = world.menu.worldmap_menu.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut last_accepted: Option<u8> = None;
    for p in &menu.placements {
        if last_accepted == Some(p.name_idx) {
            continue;
        }
        if !world.system_flag_test(u16::from(p.discovery_flag) + 0x20) {
            continue;
        }
        last_accepted = Some(p.name_idx);
        out.push(legaia_engine_menus::pause_screens::WarpDestination {
            record_index: p.index,
            name: menu
                .names
                .get(p.name_idx as usize)
                .cloned()
                .unwrap_or_default(),
            scene_id: p.scene_id,
            menu_x: p.menu_x,
            menu_y: p.menu_y,
        });
    }
    out
}

pub(super) fn build_equip_session(
    world: &World,
    char_slot: u8,
    equipment: &EquipmentTable,
) -> EquipSession {
    // The session works in the engine's slot order; the record's `+0x196`
    // bytes are retail's, reordered per character on the way in.
    let record_for = |slot: u8| {
        world
            .party
            .roster
            .members
            .get(slot as usize)
            .map(|c| {
                let mut r = stat_record_from_character(c);
                r.equip =
                    legaia_engine_menus::equip_session::engine_equip_from_record(r.equip, slot);
                r
            })
            .unwrap_or_default()
    };
    let session = EquipSession::new(
        record_for(char_slot),
        world.party.inventory.clone(),
        equipment.clone(),
        StatusModifiers::default(),
        Vec::new(),
    );
    // The Best-Equipment chooser's weapon arm scores each candidate through
    // `FUN_801DD0C0` against the menu overlay's category table. Without the
    // table installed the check returns 0 for every weapon and the pick
    // collapses onto raw ATK - which is the routine's own empty-table arm,
    // but not what a retail disc produces. `World::install_menu_overlay_tables`
    // fills it, so both hosts get it from the boot call they already make.
    let session = session
        .with_active_party_slot(char_slot)
        .with_weapon_category(world.menu.item_category.clone());
    // Disc restrictions when the boot parsed them: the candidate list is
    // then the retail one - the `+6` character mask and `+7` category for
    // the four armament rows, and the class-2 Goods index for the three
    // Goods rows - instead of the `id >> 5` placeholder rule, which offers
    // a Goods row the wrong id band entirely. Both hosts reach this one
    // builder, so the screens cannot disagree.
    match world.tables.equip_stats.as_ref() {
        Some(stats) => {
            let mut info = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(stats);
            if let Some(effects) = world.tables.item_effects.as_ref() {
                info.install_goods(effects);
            }
            session.with_restrictions(info, char_slot)
        }
        None => session,
    }
}

/// Per-roster-member reorder rows for the Status screen's confirm, in the
/// order [`status_snapshots`] publishes them.
///
/// The rows are the character's own spell list in **record order** - the
/// same `+0x13D` / `+0x161` pair the Magic screen lists and
/// [`legaia_engine_menus::save_subscreen::sub15_swap_rows`] exchanges - so a swap made on
/// the page is a swap on the Magic screen.
pub(super) fn status_spell_rows(world: &World, catalog: &SpellCatalog) -> Vec<Vec<ListOrderRow>> {
    world
        .party
        .roster
        .members
        .iter()
        .filter(|m| m.hp_mp_sp().hp_max != 0)
        .map(|member| {
            let list = member.spell_list();
            list.ids[..(list.count as usize).min(list.ids.len())]
                .iter()
                .map(|&id| ListOrderRow {
                    id,
                    label: catalog
                        .get(id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| format!("Spell {id}")),
                })
                .collect()
        })
        .collect()
}

/// Open the list-reorder page over the Status screen's shown character.
///
/// The step is the spell list's running twin, which is the one step of the
/// three that carries the swap arm
/// ([`legaia_engine_menus::save_subscreen::sub15_list_source`]).
pub(super) fn open_status_list_order(session: &StatusScreenSession) -> Option<ListOrderSession> {
    let slot = session.current().map(|s| s.slot)?;
    let rows = session.spell_rows_for_cursor().to_vec();
    ListOrderSession::open(LIST_ORDER_STEP_MAGIC, slot, rows)
}
