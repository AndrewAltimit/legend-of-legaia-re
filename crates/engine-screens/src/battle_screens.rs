//! The battle screens both play hosts draw, projected once: the HUD frame
//! (party strip, plaques, popups), its chrome sprites (strip lozenges, the
//! item window, the command chips, the field-side message banner frame), the
//! sparring tutorial's boxes and the arts command-input chrome.
//!
//! Each of these used to live twice - once in the native window
//! (`engine-shell/src/window/hud*.rs`) and once on the browser play page
//! (`web-viewer/src/play_battle*`) - near line for line, and the copies had
//! started to disagree (the arts-input chrome under a dialogue box). What a
//! host still owns is assembling [`BattleScreenInputs`] from its holders, the
//! stage transform's surface, and the upload.

use legaia_engine_battle::battle_hud_state::BattleHud;
use legaia_engine_core::battle_hud as bh;
use legaia_engine_core::world::World;
use legaia_engine_menus::battle_flow::ActiveTutorialBox;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_ui::battle_hud_chrome::BattleBadgeRects;
use legaia_engine_ui::{
    BattleHudDraws, HudPopupView, HudSlotMeta, HudSlotView, SaveMenuAtlasRects, SpriteDraw,
    TextDraw,
};

/// Top-left anchor of the battle HUD's slot-row block, in surface pixels.
///
/// Numerically equal to [`crate::LEVEL_UP_PEN`] and deliberately a separate
/// constant: nothing ties the battle HUD's anchor to the post-battle banner's,
/// and collapsing them would invent a coupling neither host has.
pub const BATTLE_HUD_PEN: (i32, i32) = (8, 60);

/// One on-screen sparring-tutorial box with its stage rect `(x, y, w, h)`.
pub type TutorialStageBox<'a> = ((i32, i32, i32, i32), &'a ActiveTutorialBox);

/// Everything the battle screens read, borrowed from whichever host holds it.
#[derive(Clone, Copy)]
pub struct BattleScreenInputs<'a> {
    /// The live scene world.
    pub world: &'a World,
    /// The host's battle-HUD model (refreshed each tick by the shared
    /// `sync_battle_hud_rows` fold).
    pub hud: &'a BattleHud,
    pub font: &'a legaia_font::Font,
    /// The system-UI chrome atlas rects; `None` keeps the builders' text
    /// fallbacks and drops every atlas-sampled sprite.
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    /// The status / element badge cells out of the baked atlas.
    pub badges: Option<BattleBadgeRects>,
    /// The font atlas' solid-white texel (`font_solid_src`), which a host may
    /// cache since its font never changes.
    pub solid_src: Option<(u32, u32, u32, u32)>,
}

/// Project the HUD model's slot array into the shared builder's view type.
///
/// Every slot is emitted, **including inactive ones** (as empty-name rows the
/// builder skips). That is deliberate: `battle_hud_draws_for` derives both a
/// row's Y and a popup's anchor from the slice index, so the index has to stay
/// the absolute actor-table slot. Compacting to active slots only would shift
/// every monster row up and anchor damage numbers to the wrong actor.
pub fn battle_hud_slot_views(hud: &BattleHud) -> Vec<HudSlotView<'_>> {
    hud.slots
        .iter()
        .map(|s| {
            let (hp_fill, mp_fill) = s.gauge_fill_indices();
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
                hp_fill,
                mp_fill,
                // The single retail-selected status element
                // (`FUN_8002C2E4`'s ladder over the packed `+0x16E` word)
                // plus the level its no-ailment arm draws.
                status_sprite: s.status_sprite(),
                level: s.level,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            HudSlotView::from_plain(meta, name)
        })
        .collect()
}

/// Project the HUD model's popup queue into the shared builder's view type.
pub fn battle_hud_popup_views(hud: &BattleHud) -> Vec<HudPopupView> {
    hud.popup_views()
        .into_iter()
        .map(|p| HudPopupView {
            slot: p.slot,
            amount: p.amount,
            is_heal: p.is_heal,
            is_crit: p.is_crit,
            status_letter: p.status_letter,
            alpha: p.alpha,
        })
        .collect()
}

/// The enemy-row half of a target picker's on-screen text.
///
/// Retail draws **one** plaque for the target cursor - placement record
/// `0x29`, seated by `FUN_801D5854`'s target arm at `0xE8 - w/2`, row `162` -
/// and that plaque rides the shared battle-HUD builder
/// (`BattleHudFrame::target_select`, filled from
/// `battle_hud::battle_target_select_plaque`). This returns an empty draw list
/// while the cursor sits on the enemy row, so the caller does not add its text
/// fallback; `None` on the ally / sweep states, which keep their text line.
///
/// The earlier dedup-name strip (the `FUN_801D9D3C` intro-banner layout at
/// stage row 166) is retired: that routine is the battle-intro banner's
/// composer, and its strip overprinted the commit log's target column.
pub fn enemy_target_strip_draws(
    picker: &legaia_engine_battle::target_picker::TargetPickerSession,
) -> Option<Vec<TextDraw>> {
    use legaia_engine_battle::target_picker::{CursorRow, PickerState};
    matches!(
        picker.state(),
        PickerState::Cursor {
            row: CursorRow::Enemy,
            ..
        }
    )
    .then(Vec::new)
}

/// Whether a Muscle Dome leg's battle chrome is on screen: the dome is the
/// mode, the leg's selection is up and no hub screen (the first visit, the
/// leg-open ROUND card) covers it. Retail runs those hub arms before the round
/// driver raises its command cluster.
pub fn dome_battle_chrome_up(world: &World) -> bool {
    world.mode == SceneMode::MuscleDome
        && world.minigames.muscle_dome.is_some()
        && !world.minigames.muscle_hub.covers_leg()
}

/// The message holding retail's top-of-screen banner this frame, if any
/// ([`legaia_engine_core::battle_hud::battle_banner_message`]: the absorb /
/// magic-level element line, then level-up, then Seru capture).
///
/// `None` without the system-UI atlas: there is no frame to put a message in,
/// so a chrome-less host keeps the loose pens instead.
pub fn battle_banner_message(world: &World, chrome: Option<&SaveMenuAtlasRects>) -> Option<String> {
    chrome?;
    bh::battle_banner_message(world)
}

/// The live sparring-tutorial prompt's box rect in 320x240 stage pixels, or
/// `None` when no box is up.
///
/// The width is measured in the host's font (retail measures it with
/// `FUN_80035F04`) and the engine applies the emitter's placement + sizing
/// arithmetic. Shared by the text layer and the chrome layer so the frame and
/// the rows cannot disagree.
pub fn battle_tutorial_stage_rect(
    world: &World,
    font: &legaia_font::Font,
) -> Option<(i32, i32, i32, i32)> {
    let tbox = world.battle_tutorial_box()?;
    let width = legaia_engine_ui::battle_tutorial_text_width(font, &tbox.text);
    let (x, y, w, h) = tbox.rect(width)?;
    Some((x as i32, y as i32, w as i32, h as i32))
}

/// Every tutorial box on screen with its stage rect - the front group of the
/// world's box queue (one retail hook dispatch registers all of its boxes
/// together, so the group draws together).
pub fn battle_tutorial_stage_boxes<'a>(
    world: &'a World,
    font: &legaia_font::Font,
) -> Vec<TutorialStageBox<'a>> {
    world
        .battle_tutorial_boxes_on_screen()
        .filter_map(|tbox| {
            let width = legaia_engine_ui::battle_tutorial_text_width(font, &tbox.text);
            let (x, y, w, h) = tbox.rect(width)?;
            Some(((x as i32, y as i32, w as i32, h as i32), tbox))
        })
        .collect()
}

/// Sparring-tutorial prompt-box chrome: the same gradient fill + gold 9-slice
/// frame the dialog reading box wears, at the rect the retail emitter
/// registers the prompt's text actor with, plus Koru's timed-fight strip
/// (both are text actors registered with an explicit rect and style word
/// `0x44`).
pub fn battle_tutorial_chrome_draws(
    world: &World,
    font: &legaia_font::Font,
    rects: &SaveMenuAtlasRects,
    origin: (i32, i32),
    scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    for (rect, tbox) in battle_tutorial_stage_boxes(world, font) {
        out.extend(legaia_engine_ui::battle_tutorial_chrome_draws_for(
            rects,
            rect,
            tbox.waits_for_input,
            origin,
            scale,
        ));
    }
    if legaia_engine_core::world_views::timed_fight::timed_fight_strip(world).is_some() {
        out.extend(legaia_engine_ui::timed_fight_strip_chrome_draws(
            rects, origin, scale,
        ));
    }
    out
}

/// One battle-HUD frame from the shared builder: the party strip, the
/// top-left plaque and the popups, in surface pixels.
///
/// Both halves come from one call so a host's two draw slots cannot drift:
/// the text half goes into its glyph layer, the sprite half into the
/// system-UI atlas layer through [`battle_chrome_sprite_draws`].
pub fn battle_hud_frame_draws(
    inputs: &BattleScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> BattleHudDraws {
    let w = inputs.world;
    // The result screen draws neither the party card nor the pill (retail
    // `noa_levelup_banner`: the two framed windows over the bare battle
    // scene) - the readout comes down with the last action.
    if w.battle_result_screen_active() {
        return BattleHudDraws {
            text: Vec::new(),
            sprites: Vec::new(),
        };
    }
    let slots = battle_hud_slot_views(inputs.hud);
    let popups = battle_hud_popup_views(inputs.hud);
    // Every per-phase decision is the engine's (`battle_hud`'s predicates
    // carry retail's sub-draw script + action-SM rule).
    let plaque = bh::battle_active_actor(w);
    let third_tab = bh::battle_breadcrumb_third_tab(w);
    let target_plaque = bh::battle_target_plaque(w);
    let target_select = bh::battle_target_select_plaque(w);
    let move_name = bh::battle_move_name(w);
    let message_bar = bh::battle_message_bar(w);
    let commit_log = bh::battle_commit_log(w);
    // The battle-intro enemy-name banner (retail flow `0x0A`) and a
    // battle-stage module's boss-name banner (the Cort arrival), laid out
    // with the host's font.
    let intro_names = bh::battle_intro_names(w, inputs.font);
    let stage_banner = bh::battle_stage_banner(w, inputs.font);
    let banner = battle_banner_message(w, inputs.chrome);
    let tutorial_rect = battle_tutorial_stage_rect(w, inputs.font);
    legaia_engine_ui::battle_hud_draws_for(
        inputs.font,
        &legaia_engine_ui::BattleHudFrame {
            slots: &slots,
            popups: &popups,
            log: &[],
            solid_src: inputs.solid_src,
            surface: (surface_w, surface_h),
            chrome: inputs.chrome,
            // The actor-name plaque shares its top-left seat with the item
            // window's Begin | <name> | Item breadcrumb trail, and retail
            // parks the plaque while that window is up (the
            // battle_item_window capture shows the crumbs alone), so the
            // frame draws one or the other, never both.
            plaque: plaque
                .as_ref()
                .filter(|_| w.battle.item_menu.is_none())
                .map(|(_, n)| n.as_str()),
            plaque_badge: bh::battle_plaque_element_badge(w),
            banner: banner.as_deref(),
            // The sparring-tutorial prompt is a box the host draws itself,
            // and its rect starts on the plaque's own content pen - so while
            // it is up the plaque must not draw, or the two text runs land on
            // the same pixels.
            plaque_seat_taken: tutorial_rect.is_some()
                || w.dialog.current.is_some()
                || w.dialog.inline.is_some()
                // Koru's timed-fight strip: retail draws it OVER the plaque
                // (text actor key 1 walks before the plaque's 0x23 on the
                // same ordering-table slot), so the plaque sits under the
                // strip's fill. Neither host can put the plaque's name under
                // the strip's frame in its layer order - it parks it.
                || legaia_engine_core::world_views::timed_fight::timed_fight_strip(w).is_some(),
            badges: inputs.badges.as_ref(),
            // The same box, tested against the party surfaces' own rows: a
            // bottom-anchored prompt lands on the active-actor bar (188..208)
            // and inside the roster panels (164..212), so the builder parks
            // whichever one it covers.
            host_box: tutorial_rect,
            active_slot: bh::battle_readout_bar_slot(w),
            panels_parked: !bh::battle_panels_visible(w),
            begin_tab: bh::battle_begin_tab_visible(w),
            begin_tab_offset: bh::battle_begin_tab_offset(w),
            third_tab: third_tab.as_deref(),
            move_name: move_name.as_deref(),
            target_plaque: target_plaque.as_ref().map(|(n, b)| (n.as_str(), *b)),
            plaque_dy: bh::battle_action_plaque_dy(w),
            target_plaque_dy: bh::battle_target_plaque_dy(w),
            bar_dy: bh::battle_readout_bar_dy(w),
            panels_dy: bh::battle_round_prompt_panels_dy(w),
            target_select: target_select.as_ref().map(|(n, b)| (n.as_str(), *b)),
            message_bar: message_bar.as_deref(),
            ap_plate_value: bh::battle_ring_ap_plate_value(w),
            ap_plate_dx: bh::battle_ring_ap_plate_dx(w),
            commit_log: &commit_log,
            intro_names: &intro_names,
            stage_banner: stage_banner.as_ref().map(|(l, x, y)| (l.as_str(), *x, *y)),
            diag: legaia_engine_ui::diag_hud_enabled(),
        },
        BATTLE_HUD_PEN,
    )
}

/// The battle HUD's chrome sprites (strip + plaque lozenges, gold `HP` / green
/// `MP` label cells), the battle item window's frames, and the command menu's
/// chip plates + D-pad glyph when a menu is up - surface pixels, for the
/// system-UI atlas layer.
///
/// Outside battle this narrows to one surface: the frame of the **message
/// banner** carrying a level-up / Seru-capture line, which the port raises
/// after the fight has already handed the frame back to the field. Its text
/// half rides [`crate::banner_stage_draws`].
pub fn battle_chrome_sprite_draws(
    inputs: &BattleScreenInputs<'_>,
    surface_w: u32,
    surface_h: u32,
) -> Vec<SpriteDraw> {
    let w = inputs.world;
    if w.mode == SceneMode::MuscleDome && !dome_battle_chrome_up(w) {
        return Vec::new();
    }
    let (origin, scale) = legaia_engine_ui::pause_menu::stage_transform(surface_w, surface_h);
    if !matches!(w.mode, SceneMode::Battle | SceneMode::MuscleDome) {
        let (Some(rects), Some(message)) = (inputs.chrome, battle_banner_message(w, inputs.chrome))
        else {
            return Vec::new();
        };
        use legaia_engine_ui::battle_hud_chrome as bhc;
        return bhc::message_banner_chrome_draws_for(
            rects,
            bhc::message_banner_content(inputs.font, &message),
            origin,
            scale,
        );
    }
    let mut out = battle_hud_frame_draws(inputs, surface_w, surface_h).sprites;
    // The battle item window's chrome (both packet-pinned 9-slice windows,
    // the breadcrumb tabs and the hand cursor) rides the same atlas layer as
    // the rest of the menu chrome.
    if let (Some(rects), Some(model)) = (inputs.chrome, w.battle_item_menu_model()) {
        out.extend(crate::with_battle_item_frame(&model, |frame| {
            legaia_engine_ui::battle_item_ui::battle_item_window_sprites(
                inputs.font,
                rects,
                frame,
                origin,
                scale,
            )
        }));
    }
    // The command chips sample the same blue plate 3-slice the party bar
    // does, so they ride this list rather than a second one.
    if let (Some(rects), Some(battle), Some((chips, cursor, phase))) = (
        inputs.chrome,
        inputs.chrome.and_then(|r| r.battle),
        crate::battle_command::battle_command_menu_chips(w),
    ) {
        use legaia_engine_ui::battle_command_ui as bcu;
        let views = bcu::command_chip_views(&chips);
        // The plates, and on top of them every mark the ring wears - the red
        // cross-outs the special word raises (the Rim Elm ambush / monster
        // `0xAF`) and the Rot / Curse marks over the arms the acting member's
        // status refuses.
        out.extend(bcu::battle_command_menu_sprites(
            &battle,
            // Widget record 2's gold run, the highlighted arm's plate.
            Some([rects.tab_cap_l, rects.tab_body, rects.tab_cap_r]),
            &bcu::BattleCommandMenuFrame {
                chips: &views,
                cursor,
                phase,
                offsets: bh::battle_chip_offsets(w),
                dpad: bh::battle_chip_dpad_visible(w),
            },
            bh::battle_ring_marks(w),
            origin,
            scale,
        ));
    }
    out
}

/// Project the simulation's arts-input phase onto the presentation crate's.
/// Two enums because `engine-ui` is a leaf that does not link `engine-core`.
fn arts_input_screen(
    p: legaia_engine_battle::arts_command_input::ArtsInputScreen,
) -> legaia_engine_ui::arts_input::ArtsInputScreen {
    use legaia_engine_battle::arts_command_input::ArtsInputScreen as Sim;
    use legaia_engine_ui::arts_input::ArtsInputScreen as Ui;
    match p {
        Sim::Entering => Ui::Entering,
        Sim::Review => Ui::Review,
        Sim::Targeting => Ui::Targeting,
    }
}

/// The **arts command-input** chrome sprites - the four direction chips +
/// D-pad glyph, the input bar with its committed pennants, the Rot stamps and
/// the AP plate - while a party member owns the pad in the retail-model entry
/// session (or a Spirit turn raises the bar + plate pair). Empty otherwise.
/// Stage `origin` / `scale` place them on the surface.
///
/// An in-battle dialogue box owns the screen while it is up, and the entry
/// chrome comes down under it: both hosts' plain-text arts list carried that
/// gate, while the native window's chrome sprites did not and drew the command
/// buffer, the AP gauge and the chip row straight through the narration box.
pub fn arts_input_chrome_draws(
    world: &World,
    chrome: Option<&SaveMenuAtlasRects>,
    origin: (i32, i32),
    scale: u32,
) -> Vec<SpriteDraw> {
    use legaia_engine_ui::arts_input as ai;
    if world.dialogue_owns_input() {
        return Vec::new();
    }
    if world.mode == SceneMode::MuscleDome && !dome_battle_chrome_up(world) {
        return Vec::new();
    }
    let Some(view) = world
        .arts_input_view()
        .or_else(|| world.spirit_gauge_view())
    else {
        return Vec::new();
    };
    let frame = ai::ArtsInputFrame {
        buffer: view.pennants,
        spent: view.pennant_spent,
        chip_costs: view.costs,
        chip_icons: view.chip_icons,
        pool: view.pool,
        pool_max: view.pool_max,
        plate_value: view.plate_value,
        list_page: view.list_page,
        phase: arts_input_screen(view.phase),
    };
    let mut out =
        ai::arts_input_chrome_draws(&ai::ArtsInputAtlasRects::BAKED, &frame, origin, scale);
    // The Rot stamp over each direction the caster's rotted limbs refuse, on
    // top of the chips (retail draws it right after the D-pad glyph).
    if let Some(rot) = chrome.and_then(|r| r.battle).and_then(|b| b.rot_stamp) {
        out.extend(ai::arts_input_rot_stamp_draws(
            rot,
            &frame,
            view.status,
            origin,
            scale,
        ));
    }
    // The AP plate is the status screen's own AP-gauge widget, so it reuses
    // the pieces the atlas already carries.
    if let Some(r) = chrome {
        out.extend(ai::arts_input_ap_plate_draws(
            &ai::ApPlateRects {
                cap: r.gauge_cap,
                trough: r.gauge_trough,
                fill: r.gauge_fill,
                box_: r.gauge_box,
                digits: r.gauge_digits,
                tip: r.gauge_tip,
                full_mark: r.gauge_100,
            },
            &frame,
            origin,
            scale,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slot views keep the absolute actor-table index: every slot is
    /// projected, active or not, because the builder anchors rows and popups
    /// on the slice index.
    #[test]
    fn slot_views_keep_every_slot() {
        let hud = BattleHud::new();
        assert_eq!(battle_hud_slot_views(&hud).len(), hud.slots.len());
    }

    /// Outside battle, with no system-UI atlas, there is no banner frame to
    /// draw and no battle chrome either.
    #[test]
    fn no_chrome_outside_battle_without_an_atlas() {
        let world = World::new();
        let hud = BattleHud::new();
        let font = legaia_font::Font::placeholder();
        let inputs = BattleScreenInputs {
            world: &world,
            hud: &hud,
            font: &font,
            chrome: None,
            badges: None,
            solid_src: None,
        };
        assert!(battle_banner_message(&world, None).is_none());
        assert!(battle_chrome_sprite_draws(&inputs, 640, 480).is_empty());
    }

    /// No arts-input session and no Spirit turn: the entry chrome is down.
    #[test]
    fn arts_input_chrome_is_down_without_a_session() {
        let world = World::new();
        assert!(arts_input_chrome_draws(&world, None, (0, 0), 2).is_empty());
    }
}
