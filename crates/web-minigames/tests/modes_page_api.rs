//! The modes page's runtime (`LegaiaModes`): every mode of the built-in
//! practice set and of the Rebirth pack starts, plays a stretch of pad input
//! and draws, with no disc.

use legaia_engine_system::pad::PadButton;
use legaia_web_minigames::LegaiaModes;
use serde_json::Value;

fn modes(rt: &LegaiaModes) -> Vec<(String, String)> {
    let v: Value = serde_json::from_str(&rt.modes_json()).unwrap();
    v["modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_str().unwrap().to_string(),
                m["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn frame(rt: &LegaiaModes) -> (usize, Vec<String>) {
    let v: Value = serde_json::from_str(&rt.frame_json()).unwrap();
    let prims = v["prims"].as_array().unwrap();
    for p in prims {
        let p = p.as_array().unwrap();
        assert_eq!(p.len(), 16);
        // A gradient carries its four corners; a flat fill carries none.
        assert!(p[14].is_null() || p[14].as_array().unwrap().len() == 16);
        // A card face carries its rectangle on the card page.
        assert!(p[15].is_null() || p[15].as_array().unwrap().len() == 4);
    }
    let labels = v["labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l[0].as_str().unwrap().to_string())
        .collect();
    (prims.len(), labels)
}

/// Press each button in turn (a press, then a release frame), cycling
/// through every control a mode reads, but never Start.
fn play(rt: &mut LegaiaModes, frames: usize) {
    let buttons = [
        PadButton::Cross,
        PadButton::Right,
        PadButton::Cross,
        PadButton::Up,
        PadButton::Left,
        PadButton::Cross,
        PadButton::Down,
        PadButton::Square,
        PadButton::Triangle,
        PadButton::R1,
    ];
    for f in 0..frames {
        let pad = if f % 6 < 3 {
            buttons[(f / 6) % buttons.len()].mask()
        } else {
            0
        };
        // A session that ends early stops on its results card.
        if rt.tick(pad) != "playing" {
            return;
        }
    }
}

fn exercise(rt: &mut LegaiaModes) {
    let all = modes(rt);
    assert!(all.len() >= 5, "{all:?}");
    for kind in ["Poker", "Grid Duel", "Golf", "Race", "Prop Hunt"] {
        assert!(all.iter().any(|(_, k)| k == kind), "no {kind} in {all:?}");
    }
    for (id, kind) in &all {
        let err = rt.enter(id);
        assert_eq!(err, "", "{id} would not start");
        assert_eq!(rt.tick(0), "playing");
        let (prims, labels) = frame(rt);
        assert!(
            prims > 4 && !labels.is_empty(),
            "{id} ({kind}) draws nothing"
        );
        play(rt, 900);
        let (prims, _) = frame(rt);
        assert!(prims > 0, "{id} stopped drawing");
        // Start raises the quit prompt, Cross confirms it: the results card.
        if rt.tick(0) == "playing" {
            rt.tick(PadButton::Start.mask());
            rt.tick(0);
            let st: Value = serde_json::from_str(&rt.state_json()).unwrap();
            assert_eq!(st["quit_prompt"], true, "{id}");
            rt.tick(PadButton::Cross.mask());
        }
        assert_eq!(rt.tick(0), "results", "{id}");
        rt.tick(PadButton::Cross.mask());
        assert_eq!(rt.tick(0), "closed", "{id}");
    }
}

#[test]
fn builtin_set_plays_every_mode() {
    let mut rt = LegaiaModes::new();
    rt.set_seed(7.0);
    exercise(&mut rt);
}

#[test]
fn picker_enters_a_mode_and_backs_out() {
    let mut rt = LegaiaModes::new();
    rt.open_picker();
    assert_eq!(rt.tick(0), "picker");
    assert!(frame(&rt).1.iter().any(|l| l == "Practice modes"));
    rt.tick(PadButton::Down.mask());
    rt.tick(0);
    rt.tick(PadButton::Cross.mask());
    assert_eq!(rt.tick(PadButton::Cross.mask()), "playing");
    rt.leave();
    assert_eq!(rt.tick(0), "closed");
    rt.open_picker();
    rt.tick(PadButton::Circle.mask());
    assert_eq!(rt.tick(0), "closed");
}

#[test]
fn rebirth_tree_plays_every_mode() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let mut files = legaia_expansion_format::PackFiles::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if legaia_expansion_format::is_known_path(&rel) {
                    files.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
    }
    let mut rt = LegaiaModes::new();
    assert_eq!(rt.load_pack_files(&files), "");
    let v: Value = serde_json::from_str(&rt.modes_json()).unwrap();
    assert_eq!(v["source"], "pack");
    exercise(&mut rt);
    rt.use_builtin();
    let v: Value = serde_json::from_str(&rt.modes_json()).unwrap();
    assert_eq!(v["source"], "builtin");
}

#[test]
fn a_duel_hands_the_page_its_card_faces() {
    let mut rt = LegaiaModes::new();
    rt.set_seed(3.0);
    let duel = modes(&rt)
        .into_iter()
        .find(|(_, k)| k == "Grid Duel")
        .expect("a duel")
        .0;
    let info: Value = serde_json::from_str(&rt.card_page_info()).unwrap();
    assert_eq!(info["key"], "", "no page before a duel");
    assert!(rt.card_page_rgba().is_empty());
    assert_eq!(rt.enter(&duel), "");
    rt.tick(0);
    let info: Value = serde_json::from_str(&rt.card_page_info()).unwrap();
    let (w, h) = (
        info["w"].as_u64().unwrap() as usize,
        info["h"].as_u64().unwrap() as usize,
    );
    assert_ne!(info["key"], "");
    let rgba = rt.card_page_rgba();
    assert_eq!(rgba.len(), w * h * 4);
    // Faces are drawn (opaque texels) with transparent windows over the
    // owner's colour.
    let opaque = rgba.chunks(4).filter(|p| p[3] == 255).count();
    let clear = rgba.chunks(4).filter(|p| p[3] == 0).count();
    assert!(opaque > w * h / 4 && clear > 0, "{opaque} {clear}");
    // Every card the screen draws off the page names a rectangle inside it.
    let v: Value = serde_json::from_str(&rt.frame_json()).unwrap();
    let tex: Vec<_> = v["prims"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p[15].as_array().cloned())
        .collect();
    assert!(tex.len() >= 10, "both hands draw: {}", tex.len());
    for t in tex {
        let r: Vec<u64> = t.iter().map(|x| x.as_u64().unwrap()).collect();
        assert!(r[0] < r[2] && r[1] < r[3] && (r[2] as usize) < w && (r[3] as usize) < h);
    }
}
