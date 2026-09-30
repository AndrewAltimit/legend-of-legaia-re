//! A save crystal's clip comes from its spawn prologue, not its MAN header.
//!
//! Every save crystal placement (`model 0xF3`) ships header anim byte `0`;
//! its record's prologue sets `actor[+0x5C]` to `22`, the PROT 0874
//! locomotion bundle's savepoint clip (record 21). Retail's
//! `conc_field_card_boot` capture holds `0x16` at the crystal actor's `+0x5C`
//! with draw kind `1` (posed). A host that read the header byte drew the
//! crystal's three objects unposed on the origin; one that read the mesh
//! out of the shared global pool drew the effect-model library's slot 3.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;

#[test]
fn conc_save_crystal_takes_the_savepoint_clip_from_its_prologue() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("conc", 0).expect("enter conc");
    let scene = host.scene.as_ref().expect("scene");
    let man = scene
        .field_man_payload(&host.index)
        .expect("man read")
        .expect("conc has a MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("MAN parses");
    let crystals: Vec<_> = mf
        .actor_placements(&man)
        .into_iter()
        .filter(|p| p.model_index == 0xF3)
        .collect();
    assert!(!crystals.is_empty(), "conc places a save crystal");
    for p in &crystals {
        assert_eq!(p.anim_id, 0, "the header anim byte is 0");
        let live = host.world.field_npc_live_anim(p.index);
        assert_eq!(
            live,
            Some(legaia_asset::character_pack::LOCOMOTION_SAVEPOINT_RECORD as u8 + 1),
            "P1[{}]: the prologue sets the savepoint clip (record 21 = id 22)",
            p.index
        );
    }
    // The crystal's mesh is the player bank's slot 3 (PROT 0874 §0, three
    // objects) - not whatever the effect-model library left in the shared
    // pool's slot 3.
    let crystal = host.world.field_head_pool[3]
        .as_ref()
        .expect("the player bank seeds slot 3");
    assert_eq!(crystal.tmd.objects.len(), 3, "the 3-object save crystal");
    println!(
        "conc: {} save crystal(s), live clip id {:?}",
        crystals.len(),
        host.world.field_npc_live_anim(crystals[0].index)
    );
}
