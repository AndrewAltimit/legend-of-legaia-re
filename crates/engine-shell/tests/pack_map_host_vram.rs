//! Disc-gated: a pack map's field VRAM is one image on both play hosts.
//!
//! A pack map's textures go into free VRAM of its base scene's build
//! (`legaia_engine_scene::expansion_map::vram_alloc`), whose occupancy
//! counts what the hosts layer on that build: the effect-texture pool under
//! it and the field character atlas over it. The window layers both in its
//! own scene build (`build_window_scene_resources`); the page samples the
//! scene host's resources. A layer one host adds and the other does not
//! would land pack texels on one host only. Both enter the sample pack's
//! Lantern Isles village over a running Rim Elm (`town01`), the window
//! through the session's entry and its render rebuild, the page through
//! its pack library's load and map entry. Skips when `LEGAIA_DISC_BIN` is
//! unset.

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_web_play::runtime::LegaiaRuntime;

const MAP: &str = "isles_village";

#[test]
fn a_pack_maps_vram_is_one_image_on_both_hosts() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let disc = std::path::Path::new(&disc);
    let pack_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/sample");

    let mut win = HeadlessPlayWindow::open(disc, "town01").expect("open the window");
    win.enter_expansion_map(&pack_dir, MAP)
        .expect("the window enters the pack map");

    let mut rt = LegaiaRuntime::new();
    assert!(
        rt.load_disc(std::fs::read(disc).expect("read the disc"), String::new())
            .is_ok(),
        "load the disc"
    );
    rt.enter_field("town01").expect("enter Rim Elm");
    let files = legaia_expansion_format::read_files(&pack_dir).expect("read the pack");
    let bundle = legaia_expansion_format::bundle::write_bundle(&files).expect("bundle");
    let v: serde_json::Value = serde_json::from_str(&rt.load_expansion(&bundle)).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    rt.enter_expansion_map(MAP, "")
        .expect("the page enters the pack map");

    let native = win
        .field_vram()
        .expect("the window's field VRAM")
        .as_bytes();
    let page = rt.field_vram_bytes();
    assert_eq!(native.len(), page.len());
    let differing: Vec<(usize, usize)> = native
        .as_chunks::<2>()
        .0
        .iter()
        .zip(page.as_chunks::<2>().0)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(hw, _)| (hw % 1024, hw / 1024))
        .take(8)
        .collect();
    assert!(
        differing.is_empty(),
        "the hosts' pack-map VRAM differs, first halfwords at {differing:?}"
    );
}
