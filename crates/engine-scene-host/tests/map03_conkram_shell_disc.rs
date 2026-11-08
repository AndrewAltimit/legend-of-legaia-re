//! Disc-gated: Conkram's shell on the Karisto overworld poses from the arm its
//! story flag picks.
//!
//! `map03` record 458 (pack mesh 16, cell `(66, 104)`) is the shell's lid, a
//! one-object prop bound to `P0[0]` with clip 4 (ANM record 3, one bone,
//! 15 frames). Its spawn prologue tests system flag `0x4C8`: clear, it
//! snaps to frame 0 and holds (`4C 35`) - the lid standing open behind the
//! town; set, it snaps to the last frame (`4C 36`). A retail capture beside
//! the town (flag clear) holds the actor at draw kind `1`, `+0x5C = 4`,
//! `+0x62 = 0x1F`, cursor `0`. The port used to read the clip words off both
//! arms, so the last snap won and the lid lay over the town.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// The shell's bind anchor.
const SHELL: (u8, u8) = (66, 104);
/// The story flag its prologue tests.
const SHELL_FLAG: u16 = 0x4C8;

#[test]
fn conkrams_shell_poses_from_the_arm_its_flag_picks() {
    let Some(root) = extracted_root() else { return };
    for (flag, cursor) in [(false, 0), (true, 14 * 16)] {
        let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
        if flag {
            host.world.system_flag_set(SHELL_FLAG);
        }
        host.enter_world_map_scene("map03").expect("enter map03");
        let key = host
            .world
            .props
            .bank
            .pose_key(SHELL)
            .expect("the shell is a banked posed prop");
        assert_eq!(
            key.cursor,
            cursor,
            "flag 0x4C8 {flag}: the shell holds frame {}",
            cursor / 16
        );
        // The bind names clip 4, which is what hands the landmark to the
        // hosts' posed-prop pass instead of the unposed stamp.
        let scene = host.scene.as_ref().expect("scene");
        let binds = scene
            .field_object_binds(&host.index)
            .expect("binds")
            .expect("map03 has binds");
        assert_eq!(binds.get(&SHELL).map(|b| b.anim_id), Some(4));
    }
}
