//! Disc-gated: the Baka Fighter impact pair's effect parts
//! ([`legaia_engine_core::baka_impact_fx`]) over the real disc.
//!
//! Reads the four spawn templates out of PROT 0976, runs each one to its
//! `HALT` inside the snapshot window, checks the sprite templates build a
//! textured quad and the mesh templates name a stage-pack TMD the duel
//! surface loaded, then plays a match through the world tick and asserts a
//! decided exchange seats parts that the surface draws (its attribute
//! generation moves). Structure only - no Sony bytes are asserted. Skips +
//! passes when `LEGAIA_DISC_BIN` is absent.

use legaia_asset::baka_opponents;
use legaia_asset::static_overlay;
use legaia_engine_core::baka_duel_scene::{BakaDuelAssets, BakaDuelSurface};
use legaia_engine_core::baka_fighter::{BakaFight, roster_clip_headers};
use legaia_engine_core::baka_impact_fx::{
    ImpactDraw, ImpactPart, ImpactTemplates, TEMPLATE_WINDOW_WORDS,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

#[test]
fn the_impact_templates_run_and_draw_on_the_duel_surface() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
        .expect("baka overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0976");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let t = ImpactTemplates::from_overlay(&loaded).expect("templates read");
    for (k, tpl) in t.a.iter().enumerate() {
        assert_eq!(tpl.model_sel, -1, "template A{k} is a transform node");
    }
    for (k, tpl) in t.b.iter().enumerate() {
        assert_eq!(
            tpl.model_sel,
            k as i16 + 1,
            "template B{k} names model {}",
            k + 1
        );
    }
    let read = |i: usize| host.index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    let assets = BakaDuelAssets::load(read);
    assert!(
        assets.impact_models.iter().all(|m| m.is_some()),
        "stage TMDs 1 and 2 decode"
    );

    for tpl in t.a.iter().chain(t.b.iter()) {
        let mut p = ImpactPart::spawn(tpl, [0, 0, 0], [0, 0x400, 0], 0x1000);
        let mut ticks = 0;
        let mut sprite_cells = std::collections::BTreeSet::new();
        while !p.halted && ticks < 400 {
            match p.draw() {
                Some(ImpactDraw::Sprite { quad, colour, .. }) => {
                    assert!(quad.tpage != 0 && quad.clut != 0, "a textured quad");
                    assert!(colour.semi, "drawn semi-transparent");
                    sprite_cells.insert(quad.uvs[0]);
                }
                Some(ImpactDraw::Mesh { model, colour, .. }) => {
                    assert_eq!(model, (tpl.model_sel) as usize);
                    assert!(colour.semi);
                }
                None => panic!("template {:#X} draws nothing", tpl.va),
            }
            p.tick(8);
            ticks += 1;
        }
        assert!(p.halted, "template {:#X} halts", tpl.va);
        assert!(
            (p.state.pc as usize) < TEMPLATE_WINDOW_WORDS,
            "template {:#X} halts inside its window",
            tpl.va
        );
        if tpl.model_sel < 0 {
            assert!(sprite_cells.len() > 4, "the flip-book steps its cells");
        }
        eprintln!(
            "[ok] template {:#X}: {ticks} ticks, {} cells",
            tpl.va,
            sprite_cells.len()
        );
    }

    // Through the world tick: a decided exchange seats parts the surface draws.
    let opponents = baka_opponents::parse(&loaded).expect("roster parses");
    let actions = baka_opponents::parse_actions(&loaded).expect("actions parse");
    let opponent = legaia_engine_core::baka_fighter::first_rung_roster();
    let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0x5EED)
        .expect("fight builds")
        .with_roster_clip_headers(roster_clip_headers(read))
        .with_impact_overlay(&loaded);
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_baka_fighter(fight);
    let mut surface = BakaDuelSurface::default();
    let mut saw_parts = false;
    let mut attr_moved = false;
    let mut attr0 = None;
    for frame in 0..3000 {
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        if f.match_over() {
            break;
        }
        let pad = if f.can_choose(0) && frame % 60 == 0 {
            PadButton::Square.mask()
        } else {
            0
        };
        world.set_pad(pad);
        let _ = world.tick();
        let f = world.minigames.baka_fighter.as_ref().unwrap();
        if !f.impact_fx().parts().is_empty() {
            saw_parts = true;
        }
        let scene = surface.frame(read, Some(f)).expect("surface live");
        let g = scene.attr_generation();
        if attr0.is_some_and(|a| a != g) {
            attr_moved = true;
        }
        attr0.get_or_insert(g);
        if saw_parts && attr_moved {
            eprintln!("[ok] parts seated and drawn by frame {frame}");
            break;
        }
    }
    assert!(saw_parts, "a decided exchange seats impact parts");
    assert!(attr_moved, "the surface animates their attributes");
}
