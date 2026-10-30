//! Disc-gated: the Rebirth pack's trading cards against the player's own
//! disc - every card's art names a real monster-archive record, every one of
//! them renders a portrait, and opening the card screen through the scene
//! host builds the portrait cache the binder draws.
//!
//! Set `LEGAIA_CARD_PORTRAIT_DIR` to a scratch directory to have the
//! portraits written there as PNGs to look at (they are disc-derived: never
//! into the repository). Skips and passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeSet;
use std::sync::Arc;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_scene_host::scene_host::expansion_cards::{
    PORTRAIT_H, PORTRAIT_W, render_portraits,
};
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::refs::DiscRef;

fn rebirth() -> Arc<ExpansionPack> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    Arc::new(ExpansionPack::from_dir(&dir).expect("the Rebirth pack loads"))
}

#[test]
fn every_rebirth_card_names_a_real_monster_and_renders_its_portrait() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack = rebirth();
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    let archive = host.monster_archive_bytes().expect("the monster archive");
    let ids: BTreeSet<u16> = pack
        .cards
        .cards
        .iter()
        .filter_map(|c| match c.art {
            Some(DiscRef::Monster { id }) => Some(id),
            _ => None,
        })
        .collect();
    // Two cards may share a portrait (a boss and the legend it echoes).
    assert!(
        pack.cards
            .cards
            .iter()
            .all(|c| matches!(c.art, Some(DiscRef::Monster { .. }))),
        "every card is a monster card"
    );
    for c in &pack.cards.cards {
        let Some(DiscRef::Monster { id }) = c.art else {
            unreachable!()
        };
        let rec = legaia_actor_formats::monster_archive::record(&archive, id)
            .expect("record decodes")
            .unwrap_or_else(|| panic!("card {} names empty slot {id}", c.id));
        assert!(rec.hp > 0 && !rec.name.is_empty(), "card {}: {id}", c.id);
        eprintln!("{:20} -> {id:3} {}", c.id, rec.name);
    }
    let cache = render_portraits(&archive, &ids);
    let missing: Vec<u16> = ids
        .iter()
        .copied()
        .filter(|id| cache.get(*id).is_none())
        .collect();
    assert!(missing.is_empty(), "no portrait for {missing:?}");
    for (id, p) in &cache.by_monster {
        assert_eq!((p.w, p.h), (PORTRAIT_W, PORTRAIT_H));
        let lit = p
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] != 0)
            .count();
        assert!(
            lit * 10 > p.rgba.len() / 4,
            "monster {id}'s portrait fills a tenth of its box ({lit} px)"
        );
    }
    if let Some(dir) = std::env::var_os("LEGAIA_CARD_PORTRAIT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (id, p) in &cache.by_monster {
            let f = std::fs::File::create(dir.join(format!("monster_{id:03}.png"))).unwrap();
            let mut enc = png::Encoder::new(f, u32::from(p.w), u32::from(p.h));
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&p.rgba)
                .unwrap();
        }
    }

    // The scene host builds the same cache when the card screen opens.
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world
        .activate_expansion(pack.clone())
        .expect("pack activates");
    assert!(host.world.expansion.cards.portraits.by_monster.is_empty());
    assert!(host.world.open_card_binder());
    host.tick().expect("tick");
    let built = &host.world.expansion.cards.portraits;
    assert_eq!(built.pack_id, "rebirth");
    assert_eq!(built.by_monster.len(), ids.len());
    // The starter deck arrived with the first opening.
    assert_eq!(
        host.world
            .expansion
            .quests
            .progress
            .cards
            .collection
            .total(),
        pack.cards.starter.len() as u32
    );
}
