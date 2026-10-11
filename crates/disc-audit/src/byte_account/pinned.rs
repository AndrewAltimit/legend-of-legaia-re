//! Overlay sub-assets and tables pinned by consumer, and the slot-B module walker.

use super::*;

mod battle_overlay;
mod consumer_tables;
mod field_world_overlays;
mod menu_overlay;
mod minigame_tables;
mod str_overlay;

pub(super) use battle_overlay::*;
pub(super) use consumer_tables::*;
pub(super) use field_world_overlays::*;
pub(super) use menu_overlay::*;
pub(super) use minigame_tables::*;
pub(super) use str_overlay::*;

/// Sub-assets an overlay image carries at an offset this workspace has pinned.
///
/// The dump corpus is the parser for a code image's code and says nothing about
/// its data segment, so an asset sitting in that segment falls to the magic
/// sweep - which finds it, tags the claim `scan`, and thereby reports "found by
/// guessing" for something a module here already reads at a named constant.
/// That gap is a binding, not a format: the offsets below are the constants,
/// and the extents come from the TIM headers rather than from this table.
pub(super) fn claim_pinned_overlay_assets(buf: &[u8], sink: &mut Sink, prot_index: u32) {
    const MENU_OVERLAY: u32 = 899;
    if prot_index == MENU_OVERLAY {
        // The option-node list's extent is its own zero-word terminator, so
        // it is measured here rather than carried as a fixed row.
        if let Some(len) = legaia_game_tables::menu_windows::options_node_list_len(buf) {
            let off = legaia_game_tables::menu_windows::OPTIONS_NODE_LIST_OFFSET;
            sink.claim(
                off,
                off + len,
                OWNER_RECORD,
                "options row-descriptor list (menu_windows)",
            );
        }
        for (off, what) in [
            (
                legaia_ui_formats::title_pak::OVERLAY_SAVE_MENU_TIM_OFFSET,
                "save-menu UI atlas",
            ),
            (
                legaia_ui_formats::save_icon::PROT_ENTRY_OFFSET,
                "save-slot icon sheet",
            ),
        ] {
            match legaia_container_formats::tim_scan::parse_at(buf, off) {
                Some(h) => sink.claim(
                    off,
                    (off + h.byte_len).min(buf.len()),
                    OWNER_TIM,
                    format!("{what}, {}x{} {}bpp", h.width, h.height, h.bpp),
                ),
                None => sink.note(format!("no TIM at the pinned {what} offset {off:#x}")),
            }
        }
    }
    for (off, len, owner, what) in pinned_overlay_tables(prot_index) {
        let end = off + len;
        if end > buf.len() {
            sink.note(format!(
                "pinned {what} at {off:#x} + {len} runs past this entry"
            ));
            continue;
        }
        sink.claim(off, end, owner, what);
    }
    if prot_index == 898 {
        claim_effect_proto_records(buf, sink);
        claim_subdraw_records(buf, sink);
        claim_unread_affinity_block(buf, sink);
        claim_battle_overlay_strings(buf, sink);
        claim_battle_jump_tables(buf, sink);
        claim_side_effect_banners(buf, sink);
    }
    if prot_index == STR_OVERLAY_PROT_INDEX {
        claim_str_overlay_tables(buf, sink);
    }
    if prot_index == WORLD_MAP_RENDER_PROT_INDEX {
        claim_world_map_prim_dispatch(buf, sink);
    }
    if prot_index == MENU_OVERLAY {
        claim_save_text_slots(buf, sink);
    }
    if prot_index == DEBUG_MENU_PROT_INDEX {
        claim_value_monitor_name_tables(buf, sink);
    }
    if prot_index == legaia_minigame_formats::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32 {
        claim_slot_dev_labels(buf, sink);
    }
    claim_consumer_pinned_tables(buf, sink, prot_index);
    if prot_index == ARENA_PROT_INDEX {
        claim_arena_course_ladder(buf, sink);
    }
    if prot_index == legaia_minigame_formats::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32 {
        claim_fishing_species_names(buf, sink);
    }
    if prot_index == crate::field_probe_tables::OVERLAY_PROT_INDEX {
        claim_field_probe_tables(buf, sink);
        claim_follow_ease_list(buf, sink);
    }
    if prot_index == crate::other3_roster::OVERLAY_PROT_INDEX {
        claim_other3_roster(buf, sink);
    }
}

/// Data-segment tables an overlay image carries at an offset a parser in this
/// workspace already reads, with the extent that parser's own `count * stride`.
///
/// Every row is a **binding**, not a discovery: each offset and each length is
/// a `pub const` of the module named in the detail, so nothing here is a new
/// claim about the disc and nothing here can be tuned to buy percentage points
/// - widening a row means widening the parser that reads it. The rows are
///   asserted against the parsers' constants by the disc-gated tests, so a
///   parser that re-pins a table moves this table with it or the test fails.
///
/// What this closes is the gap the sweep kept reporting as unwalked format: an
/// overlay's code is credited from the dump corpus and its data segment from
/// nothing, so a table with a named constant and a decoded record layout ranked
/// beside a format nobody had opened.
pub fn pinned_overlay_tables(prot_index: u32) -> Vec<(usize, usize, &'static str, &'static str)> {
    use legaia_game_tables::{
        battle_attack_camera_table as atkcam, battle_camera_table as camh,
        element_affinity as elem, menu_windows as menu, move_power as mp, seru_side_effect as seru,
    };
    use legaia_minigame_formats::{
        baka_opponents as baka, dance_art, dance_cast, dance_chart, minigame_art as art,
        minigame_slot_scene as slot, muscle_dome as dome, slot_payout as payout,
    };
    use legaia_minigame_formats::{fishing_exchange as fex, fishing_species as fish};
    use legaia_ui_formats::battle_ui_strings as bui;
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| (va - SLOT_A) as usize;
    match prot_index {
        898 => vec![
            (
                at(dome::DECK_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome deck move-index table (muscle_dome)",
            ),
            (
                at(dome::HAND_SPRITE_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome hand sprite-id table (muscle_dome)",
            ),
            (
                at(bui::RASERU_LABEL_TABLE_VA),
                (bui::RASERU_LABEL_MAX as usize + 1) * bui::RASERU_LABEL_STRIDE as usize,
                OWNER_STRING,
                "Ra-Seru magic-command labels (battle_ui_strings)",
            ),
            (
                camh::CAMERA_HEIGHT_FILE_OFFSET,
                camh::CAMERA_HEIGHT_LEN * 2,
                OWNER_RECORD,
                "battle camera-height table (battle_camera_table)",
            ),
            (
                at(dome::SUBDRAW_PTR_TABLE_VA),
                dome::SUBDRAW_PTR_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome sub-draw record pointers (muscle_dome)",
            ),
            (
                at(dome::VICTORY_MSG_TABLE_VA),
                dome::VICTORY_MSG_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome victory-message pointers (muscle_dome)",
            ),
            (
                atkcam::ATTACK_CAMERA_FILE_OFFSET,
                atkcam::ATTACK_CAMERA_LEN,
                OWNER_RECORD,
                "per-art attack-camera tracks (battle_attack_camera_table)",
            ),
            (
                mp::MOVE_ID_INDEX_MAP_FILE_OFFSET,
                mp::MOVE_ID_INDEX_MAP_LEN,
                OWNER_RECORD,
                "move-id to record-index map (move_power)",
            ),
            (
                mp::MOVE_POWER_TABLE_FILE_OFFSET,
                mp::MOVE_POWER_TABLE_LEN * mp::MOVE_POWER_RECORD_STRIDE,
                OWNER_RECORD,
                "move power + behaviour table (move_power)",
            ),
            (
                mp::IMPACT_EFFECT_TABLE_FILE_OFFSET,
                mp::IMPACT_EFFECT_TABLE_LEN * 4,
                OWNER_RECORD,
                "impact-effect config table (move_power)",
            ),
            (
                elem::AFFINITY_MATRIX_FILE_OFFSET,
                elem::ELEMENT_COUNT * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "element-affinity matrix (element_affinity)",
            ),
            (
                elem::SUMMON_POWER_PCT_FILE_OFFSET,
                elem::SUMMON_POWER_PCT_ROWS * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "summon power-percent table (element_affinity)",
            ),
            (
                elem::CHARACTER_ELEMENTS_FILE_OFFSET,
                elem::CHARACTER_ELEMENTS_LEN,
                OWNER_RECORD,
                "per-character element table (element_affinity)",
            ),
            (
                mp::EFFECT_PROTO_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN * 4,
                OWNER_TOC,
                "move effect-prototype pointers (move_power)",
            ),
            (
                mp::EFFECT_CLUT_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN,
                OWNER_RECORD,
                "move effect CLUT ids (move_power)",
            ),
            (
                mp::CUE_GROUP_TABLE_FILE_OFFSET,
                mp::CUE_GROUP_TABLE_LEN * mp::CUE_GROUP_STRIDE,
                OWNER_RECORD,
                "move cue-group table (move_power)",
            ),
            (
                seru::SIDE_EFFECT_TABLE_FILE_OFFSET,
                seru::SIDE_EFFECT_ELEMENTS
                    * seru::SIDE_EFFECT_BANDS
                    * seru::SIDE_EFFECT_RECORD_STRIDE,
                OWNER_RECORD,
                "Seru-magic side-effect table (seru_side_effect)",
            ),
        ],
        899 => vec![
            (
                menu::EQUIP_BROWSE_MAP_OFFSET,
                menu::EQUIP_BROWSE_MAP_LEN,
                OWNER_RECORD,
                "equip browse-row to equip-byte map (menu_windows)",
            ),
            (
                menu::EQUIP_BROWSE_MAP_OFFSET + menu::EQUIP_BROWSE_MAP_LEN,
                1,
                OWNER_PAD,
                "pad byte between the browse map and the equip mask",
            ),
            (
                menu::CHARACTER_EQUIP_MASK_OFFSET,
                menu::CHARACTER_EQUIP_MASK_LEN,
                OWNER_RECORD,
                "per-character equip mask (menu_windows)",
            ),
            (
                menu::SLOT_PICTOGRAM_OFFSET,
                menu::SLOT_PICTOGRAM_LEN * 2,
                OWNER_RECORD,
                "equip slot pictogram ids (menu_windows)",
            ),
            (
                menu::MENU_WINDOW_TABLE_OFFSET,
                menu::MENU_WINDOW_COUNT * menu::MENU_WINDOW_RECORD_STRIDE,
                OWNER_RECORD,
                "pause-menu window descriptor table (menu_windows)",
            ),
            (
                menu::OPTIONS_LAYOUT_OFFSET,
                menu::OPTIONS_LAYOUT_ROWS * menu::OPTIONS_LAYOUT_STRIDE,
                OWNER_RECORD,
                "options display-layout table (menu_windows)",
            ),
            (
                menu::PRIZE_TABLE_OFFSET,
                menu::PRIZE_TABLE_BLOCKS * menu::PRIZE_BLOCK_BYTES,
                OWNER_RECORD,
                "casino prize table (menu_windows)",
            ),
        ],
        975 => vec![
            (
                slot::MESSAGE_TABLE_OFFSET,
                slot::MESSAGE_COUNT * slot::MESSAGE_STRIDE,
                OWNER_RECORD,
                "slot-machine message table (minigame_slot_scene)",
            ),
            (
                payout::SLOT_PAYOUT_FILE_OFFSET,
                payout::SLOT_SYMBOL_COUNT,
                OWNER_RECORD,
                "per-symbol payout ladder (slot_payout)",
            ),
            (
                slot::LANDING_LINE_TABLE_OFFSET,
                slot::LANDING_LINE_ROWS * slot::LANDING_LINE_STRIDE,
                OWNER_RECORD,
                "landing-line table, rand%5 rows (minigame_slot_scene)",
            ),
            (
                slot::PAYLINE_TABLE_OFFSET,
                slot::PAYLINE_COUNT * 16,
                OWNER_RECORD,
                "payline geometry table (minigame_slot_scene)",
            ),
            (
                slot::MARQUEE_TABLE_OFFSET,
                slot::MARQUEE_COUNT * 16,
                OWNER_RECORD,
                "marquee cell table (minigame_slot_scene)",
            ),
            (
                slot::MEDALLION_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline medallion positions (minigame_slot_scene)",
            ),
            (
                slot::LAMP_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline lamp positions (minigame_slot_scene)",
            ),
            (
                art::SLOT_HUD_TABLE_OFFSET,
                art::SLOT_HUD_RECORDS * art::SLOT_HUD_STRIDE,
                OWNER_RECORD,
                "slot-machine HUD sprite records (minigame_art)",
            ),
        ],
        976 => vec![
            (
                baka::HUD_WIDGET_TABLE_FILE_OFFSET,
                baka::HUD_WIDGET_COUNT * baka::HUD_WIDGET_STRIDE,
                OWNER_RECORD,
                "Baka Fighter HUD widget table (baka_opponents)",
            ),
            (
                baka::ACTOR_PROTOTYPE_TABLE_FILE_OFFSET,
                baka::ACTOR_PROTOTYPE_COUNT * baka::ACTOR_PROTOTYPE_STRIDE,
                OWNER_RECORD,
                "Baka Fighter actor prototypes (baka_opponents)",
            ),
            (
                baka::OPPONENT_TABLE_FILE_OFFSET,
                baka::OPPONENT_COUNT * baka::OPPONENT_RECORD_STRIDE,
                OWNER_RECORD,
                "Baka Fighter opponent roster (baka_opponents)",
            ),
            (
                (baka::ACTION_PTR_TABLE_VA - SLOT_A) as usize,
                baka::OPPONENT_COUNT * 4,
                OWNER_TOC,
                "Baka Fighter per-fighter action-table pointers (baka_opponents)",
            ),
            (
                baka::BLIT_RECT_TABLE_FILE_OFFSET,
                baka::BLIT_RECT_COUNT * baka::BLIT_RECT_STRIDE,
                OWNER_RECORD,
                "Baka Fighter blit source rects (baka_opponents)",
            ),
        ],
        980 => vec![
            (
                (dance_art::WIDGET_TABLE_VA - SLOT_A) as usize,
                dance_art::WIDGET_COUNT * dance_art::WIDGET_STRIDE,
                OWNER_RECORD,
                "dance widget table (dance_art)",
            ),
            (
                (dance_cast::KIND_TABLE_VA - SLOT_A) as usize,
                dance_cast::KIND_COUNT * dance_cast::KIND_STRIDE,
                OWNER_RECORD,
                "dance cast kind table (dance_cast)",
            ),
            (
                dance_chart::DANCE_CHART_FILE_OFFSET,
                dance_chart::DANCE_CHART_ROWS * dance_chart::BEATS_PER_ROW,
                OWNER_RECORD,
                "dance step chart (dance_chart)",
            ),
            (
                (dance_chart::DANCE_BONUS_VA - SLOT_A) as usize,
                dance_chart::DANCE_SKILL_ROWS * dance_chart::DANCE_BONUS_LANES * 4,
                OWNER_RECORD,
                "dance sequence-bonus table (dance_chart)",
            ),
            (
                (dance_chart::DANCE_TRIANGLE_SCHEDULE_VA - SLOT_A) as usize,
                dance_chart::DANCE_SKILL_ROWS * dance_chart::DANCE_SCHEDULE_SLOTS * 4,
                OWNER_RECORD,
                "dance AI triangle schedule (dance_chart)",
            ),
            (
                (dance_cast::SPAWN_QUALIFIER_VA - SLOT_A) as usize,
                3 * 0x10,
                OWNER_RECORD,
                "dance qualifier spawn table (dance_cast)",
            ),
            (
                (dance_cast::SPAWN_FINALS_VA - SLOT_A) as usize,
                3 * 0x10,
                OWNER_RECORD,
                "dance finals spawn table (dance_cast)",
            ),
            (
                (dance_cast::SPAWN_FREEPLAY_VA - SLOT_A) as usize,
                6 * 0x10,
                OWNER_RECORD,
                "dance free-play spawn table (dance_cast)",
            ),
        ],
        972 => vec![
            (
                fish::SPECIES_TABLE_FILE_OFFSET,
                fish::SPECIES_COUNT * fish::SPECIES_RECORD_STRIDE,
                OWNER_RECORD,
                "fishing species table (fishing_species)",
            ),
            (
                (fish::SPAWN_TABLE_VA_PAGE0 - SLOT_A) as usize,
                fish::SPAWN_RODS * fish::SPAWN_BANDS * 4,
                OWNER_RECORD,
                "fishing venue-0 spawn table (fishing_species)",
            ),
            (
                (fish::SPAWN_TABLE_VA_PAGE1 - SLOT_A) as usize,
                fish::SPAWN_RODS * fish::SPAWN_BANDS * 4,
                OWNER_RECORD,
                "fishing venue-1 spawn table (fishing_species)",
            ),
            (
                (fish::CADENCE_TEMPLATE_VA - SLOT_A) as usize,
                fish::CADENCE_TEMPLATE_COUNT * fish::CADENCE_TEMPLATE_STRIDE,
                OWNER_RECORD,
                "fishing reel-cadence templates (fishing_species)",
            ),
            (
                (legaia_minigame_formats::fishing_sprites::FISHING_SPRITE_TABLE_VA - SLOT_A)
                    as usize,
                legaia_minigame_formats::fishing_sprites::FISHING_SPRITE_COUNT
                    * legaia_minigame_formats::fishing_sprites::FISHING_SPRITE_STRIDE,
                OWNER_RECORD,
                "fishing HUD sprite records (fishing_sprites)",
            ),
            (
                (fex::EXCHANGE_TABLE_VA_PAGE0 - SLOT_A) as usize,
                fex::EXCHANGE_ROWS * fex::EXCHANGE_ROW_STRIDE,
                OWNER_RECORD,
                "fishing point-exchange page 0 (fishing_exchange)",
            ),
            (
                (fex::EXCHANGE_TABLE_VA_PAGE1 - SLOT_A) as usize,
                fex::EXCHANGE_ROWS * fex::EXCHANGE_ROW_STRIDE,
                OWNER_RECORD,
                "fishing point-exchange page 1 (fishing_exchange)",
            ),
        ],
        _ => Vec::new(),
    }
}

/// A slot-B module image: the dump corpus for its code, plus the image's own
/// structural regions - the head jump table and the spawn-record band.
///
/// The record claims need no dump directory: both ends of every one of them are
/// addresses the module's own code computes and hands to `FUN_80021B04` /
/// `FUN_80050ED4`. See [`legaia_overlay_images::slot_b_module`] and
/// [`docs/formats/slot-b-module-layout.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/slot-b-module-layout.html).
pub(super) fn walk_slot_b_module(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    // The record walk is cut at the inherited tail for the same reason the
    // band-level measurement is: a spawn call site up there belongs to the
    // donor whose bytes those are, and so does the record pointer it forms.
    let layout = legaia_overlay_images::slot_b_module::parse_with_tail(
        buf,
        base_for(opts),
        inherited_tail_start(buf, opts),
    );
    if let Some(h) = layout.head_table.clone() {
        sink.claim(
            h.start,
            h.end,
            OWNER_TOC,
            format!("head jump table, {} arms", (h.end - h.start) / 4),
        );
    }
    for (i, r) in layout.records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!("spawn record {i} (model_sel {})", r.model_sel),
        );
    }
    // Records above the highest consumer-credited one. Their starts come from
    // the move-VM program walk rather than from a pointer, so the reason line
    // says which evidence the claim rests on.
    for (i, r) in layout.chained_records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!(
                "spawn record {} (model_sel {}, chained from the record below by \
                 its program's terminator)",
                layout.records.len() + i,
                r.model_sel
            ),
        );
    }
    sink.note(format!(
        "slot-B module: {} framed functions, code ends at {:#x}; {} spawn sites, \
         {} records claimed ({} bytes) + {} chained ({} bytes){}",
        layout.functions.len(),
        layout.code_end(),
        layout.spawn_sites,
        layout.records.len(),
        layout.record_bytes(),
        layout.chained_records.len(),
        layout
            .chained_records
            .iter()
            .map(legaia_overlay_images::slot_b_module::RecordSpan::len)
            .sum::<usize>(),
        match layout.unbounded_record {
            Some(o) => format!(
                "; the highest record at {o:#x} has no boundary above it and \
                 its program does not terminate, so it stays residue"
            ),
            None => String::new(),
        }
    ));
    // The dump corpus is the parser for the code half, and it is optional here
    // - `--funcs` absent still gives the structural regions.
    if opts.funcs_dir.is_some() {
        walk_overlay_code(buf, sink, opts);
    }
}
