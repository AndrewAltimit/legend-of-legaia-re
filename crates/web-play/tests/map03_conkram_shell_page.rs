//! Disc-gated: the play page poses Conkram's shell on the Karisto overworld.
//!
//! The page resolves the overworld landmarks without binds (unbound draws,
//! unit scale), so the clip id that poses a bound multi-object landmark has to
//! come off the walk map's binds. `map03` record 458 (pack slot 16) is
//! Conkram's shell lid, bound to clip 4; drawn unposed it lies over the town
//! instead of standing open behind it (the native window draws it through its
//! posed-prop pass). The page hands the anim id to `field_mesh_posed` and the
//! live pose key to `field_mesh_posed_frame_positions`, so both have to be
//! there for the landmark.

use legaia_web_play::runtime::LegaiaRuntime;

/// The shell lid's env-pack slot.
const SHELL_SLOT: u32 = 16;

#[test]
fn the_page_poses_conkrams_shell() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("map03").expect("enter map03");
    assert_eq!(rt.scene_mode(), "WorldMap", "map03 is the overworld");
    for _ in 0..4 {
        rt.tick_frame().expect("tick_frame");
    }
    let slots = rt.field_placement_slots();
    let anims = rt.field_placement_anim_ids();
    let frames = rt.field_placement_frames();
    let decorations = rt.field_decoration_start() as usize;
    let shell: Vec<usize> = (0..decorations.min(slots.len()))
        .filter(|&i| slots[i] == SHELL_SLOT)
        .collect();
    assert_eq!(shell.len(), 1, "one shell landmark");
    let i = shell[0];
    assert_eq!(anims[i], 4, "the landmark carries its bind's clip");
    assert_eq!(frames[i], 0, "posed at frame 0 with flag 0x4C8 clear");
    // The decorations stay unbound and unposed.
    assert!(anims[decorations..].iter().all(|&a| a == 0));
}
