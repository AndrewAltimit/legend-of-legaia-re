//! The frame's screen-space PSX primitive list, composed in one order for
//! both play hosts.
//!
//! Every primitive lands in one of two lists: `under` (drawn after the 3D
//! scene and before the text layer) and `over` (drawn after it). The
//! ordering-table walk inside `screen_prim::build_geometry` orders each list
//! by bucket, **last-added first within a bucket** (`AddPrim` is LIFO), so
//! the order in which this module appends decides every tie - and the two
//! hosts used to append in different orders: the PROT-0900 screen-effect
//! widgets went in with the field's ordering-table effects natively and at
//! the very end on the page, and the shop's opening fade and the pause
//! wipe sat in opposite orders.
//!
//! The order here is the native window's, which is also the one whose
//! `under` / `over` split follows retail's text bucket: retail links every
//! glyph at bucket `1` (`screen_prim::TEXT_OT_BUCKET`), so the widgets
//! (buckets `4` / `0xC` / `0x10` / `0x1C`, `screen_fx::OT_*`) and every
//! deeper push draw under the text, and only bucket `0` covers it. Ties that
//! cross a host's layering - a page that draws both lists in one sort under
//! its text canvas - are the documented layering difference
//! (`docs/tooling/host-drift.md`), not an ordering one.
//!
//! What a host still builds itself is what needs its own camera or VRAM
//! residency ([`HostScreenPrims`]); everything the world decides alone -
//! the fades, the pushes, the cinematic bars, the widgets, the Arts banner,
//! the Cross Beam, the shop and pause fades - is read here.

use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::world::World;
use legaia_engine_ui::screen_prim::{self, FlatQuad, ScreenPrim, ScreenQuad};

/// The primitives a host projects itself (its camera, its VRAM residency),
/// one field per layer. A layer a host does not draw stays empty.
#[derive(Clone, Debug, Default)]
pub struct HostScreenPrims {
    /// The field-to-battle transition's styles (`battle_intro`).
    pub transition: Vec<ScreenPrim>,
    /// In-battle weapon-trail bands (and, on the page, the move-FX streak).
    pub battle_fx: Vec<ScreenPrim>,
    /// The field fog sheets (`fog_particles`).
    pub field_fog: Vec<ScreenPrim>,
    /// The actor drop shadows (`FUN_8001C394`).
    pub drop_shadows: Vec<ScreenPrim>,
    /// Move-VM strip spans.
    pub move_strips: Vec<ScreenPrim>,
    /// The field VM's attached lights (op `0x34` sub-1).
    pub field_lights: Vec<ScreenPrim>,
    /// PROT 0904's (Theeder) beam packets.
    pub theeder: Vec<ScreenPrim>,
    /// The battle value readout (numerals, hit / total counters).
    pub value_readout: Vec<ScreenPrim>,
    /// The dance count-in banner sprite.
    pub dance_countin: Vec<ScreenPrim>,
    /// The dance HUD frame.
    pub dance_hud: Vec<ScreenPrim>,
    /// The Baka cabinet and round chrome widgets.
    pub baka_hud: Vec<ScreenPrim>,
    /// The slot machine (cabinet, reels, dot matrix, coin HUD).
    pub slot_cabinet: Vec<ScreenPrim>,
    /// The slot machine's paylines.
    pub slot_paylines: Vec<ScreenPrim>,
    /// The fishing line.
    pub fishing_line: Vec<ScreenPrim>,
    /// The fishing HUD's sprites.
    pub fishing_hud: Vec<ScreenPrim>,
    /// The overworld's entity and player markers.
    pub world_map_markers: Vec<ScreenPrim>,
    /// The overworld sky band.
    pub world_map_sky: Vec<ScreenPrim>,
}

/// One frame of the PROT-0900 screen-effect widget family as screen
/// primitives: a variant-for-variant re-wrap of
/// `ScreenFxFrame::draw_quads`, which decides the culling, UVs, colours and
/// the retail ordering-table slot. Both hosts used to carry a copy.
pub fn screen_fx_prims(world: &World) -> Vec<ScreenPrim> {
    use legaia_engine_core::screen_fx::ScreenFxQuad;
    world
        .presentation
        .fx_frame
        .draw_quads()
        .into_iter()
        .map(|q| match q {
            ScreenFxQuad::Flat {
                xy,
                rgba,
                gouraud,
                semi_transparent,
                abr_mode,
                ot,
            } => ScreenPrim::Flat(FlatQuad {
                xy,
                color: rgba,
                gouraud,
                semi_transparent,
                abr_mode,
                ot_index: ot,
                depth: None,
            }),
            ScreenFxQuad::Textured {
                xy,
                uv,
                clut,
                tpage,
                color,
                semi_transparent,
                ot,
            } => ScreenPrim::Textured(ScreenQuad {
                xy,
                uv,
                clut,
                tpage,
                color,
                gouraud: None,
                semi_transparent,
                ot_index: ot,
                depth: None,
            }),
        })
        .collect()
}

/// Whether the slot machine is on its rules page: a full-screen panel its
/// text prints on, so the machine rides the `under` list (the `over` tail
/// would bury the page's text).
fn slot_rules_page(world: &World) -> bool {
    world.minigames.slot_machine.as_ref().is_some_and(|m| {
        matches!(
            m.screen(),
            legaia_engine_core::slot_machine::SlotScreen::Instructions { .. }
        )
    })
}

/// The subtractive full-screen quad the shop's opening fade and the pause
/// wipe both draw (`fade_prim(level * 0x010101, 2, 0)`).
fn level_fade(level: u8) -> ScreenPrim {
    screen_prim::fade_prim(u32::from(level) * 0x01_01_01, 2, 0)
}

/// Compose this frame's `(under, over)` screen-primitive lists.
///
/// `pause_wipe` is the session's pause-menu wipe level
/// (`PauseWipe::fade_level`). Under a menu-overlay screen with the field
/// faded out (`MenuRuntime::covers_field`) the field's own effects are
/// dropped: the field is not drawn at all, so none of them may survive onto
/// the black backdrop.
pub fn compose_screen_prims(
    world: &World,
    menu: &MenuRuntime,
    pause_wipe: Option<u8>,
    host: HostScreenPrims,
) -> (Vec<ScreenPrim>, Vec<ScreenPrim>) {
    let HostScreenPrims {
        transition,
        battle_fx,
        field_fog,
        drop_shadows,
        move_strips,
        field_lights,
        theeder,
        value_readout,
        dance_countin,
        dance_hud,
        baka_hud,
        slot_cabinet,
        slot_paylines,
        fishing_line,
        fishing_hud,
        world_map_markers,
        world_map_sky,
    } = host;

    // The field scene's own ordering-table effects, sorted as one list
    // under the text: fog sheets, drop shadows, move strips, attached
    // lights and the screen-effect widgets.
    let mut under = field_fog;
    under.extend(drop_shadows);
    under.extend(move_strips);
    under.extend(field_lights);
    under.extend(screen_fx_prims(world));
    if menu.covers_field() {
        under.clear();
    }
    let mut over = transition;
    over.extend(battle_fx);
    // PROT 0948's Cross Beam (OT `2` / `0x400`, both behind the text) and
    // PROT 0904's beam.
    if let Some(c) = world.cross_beam_draw() {
        under.extend(legaia_engine_ui::cast_beam::cross_beam_prims(c));
    }
    under.extend(theeder);
    // The world's one live full-screen fade, split at the text layer by its
    // ordering-table id: the summon band's flashes (id `1`) under the HUD,
    // a field warp's fade (id `0`) over it.
    for (rgb, abr, ot) in world.screen_fade_draws() {
        let p = screen_prim::fade_prim(rgb, abr, ot);
        let ot = i16::try_from(p.ot_index()).unwrap_or(i16::MAX);
        if screen_prim::push_covers_text(ot) {
            over.push(p);
        } else {
            under.push(p);
        }
    }
    // A shop opening's fade to black, then the pause menu's wipe.
    if let Some(level) = menu.shop_fade_level() {
        over.push(level_fade(level));
    }
    if let Some(level) = pause_wipe {
        over.push(level_fade(level));
    }
    // The field overlay's screen-effect washes (op `0x34` sub-0 ->
    // `FUN_80024EE4`), split at the text layer.
    let (pushes_under, pushes_over) =
        screen_prim::screen_effect_push_prims_split(&world.screen_tint_push_args());
    under.extend(pushes_under);
    over.extend(pushes_over);
    // The cinematic wipe (`0x43 0C` -> `FUN_801DD784`).
    over.extend(screen_prim::cinematic_bar_prims(
        world.presentation.cinematic_bar,
        screen_prim::PSX_DISPLAY_H,
    ));
    over.extend(value_readout);
    over.extend(legaia_engine_ui::battle_numerals::arts_banner_prims(
        &world.battle_arts_banner_quads(),
        legaia_engine_ui::battle_numerals::VALUE_READOUT_OT,
    ));
    over.extend(dance_countin);
    over.extend(dance_hud);
    over.extend(baka_hud);
    if slot_rules_page(world) {
        under.extend(slot_cabinet);
    } else {
        over.extend(slot_cabinet);
    }
    over.extend(slot_paylines);
    over.extend(fishing_line);
    over.extend(fishing_hud);
    over.extend(world_map_markers);
    over.extend(world_map_sky);
    (under, over)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-prim layer tagged by its ordering-table index, so the composed
    /// lists can be read back as tags.
    fn tag(ot: u32) -> Vec<ScreenPrim> {
        vec![screen_prim::fade_prim(0, 0, ot)]
    }

    fn tags(v: &[ScreenPrim]) -> Vec<u32> {
        v.iter().map(ScreenPrim::ot_index).collect()
    }

    /// The append order both hosts now share: the field's own effects under
    /// the text in one run, the transition / readout / minigame tail over it.
    #[test]
    fn layers_append_in_one_order() {
        let w = World::new();
        let menu = MenuRuntime::new(std::env::temp_dir());
        let host = HostScreenPrims {
            transition: tag(100),
            battle_fx: tag(101),
            field_fog: tag(1),
            drop_shadows: tag(2),
            move_strips: tag(3),
            field_lights: tag(4),
            theeder: tag(5),
            value_readout: tag(102),
            dance_countin: tag(103),
            dance_hud: tag(104),
            baka_hud: tag(105),
            slot_cabinet: tag(106),
            slot_paylines: tag(107),
            fishing_line: tag(108),
            fishing_hud: tag(109),
            world_map_markers: tag(110),
            world_map_sky: tag(111),
        };
        let (under, over) = compose_screen_prims(&w, &menu, None, host);
        assert_eq!(tags(&under), vec![1, 2, 3, 4, 5]);
        assert_eq!(
            tags(&over),
            vec![100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111]
        );
    }

    /// The pause wipe lands in the `over` tail, after the transition.
    #[test]
    fn pause_wipe_is_a_bucket_zero_fade_over_the_text() {
        let w = World::new();
        let menu = MenuRuntime::new(std::env::temp_dir());
        let host = HostScreenPrims {
            transition: tag(100),
            ..Default::default()
        };
        let (under, over) = compose_screen_prims(&w, &menu, Some(0x40), host);
        assert!(under.is_empty());
        assert_eq!(tags(&over), vec![100, 0]);
    }
}
