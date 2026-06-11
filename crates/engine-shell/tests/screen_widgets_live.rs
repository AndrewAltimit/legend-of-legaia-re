//! Disc + save-library gated: the PROT-0900 screen-widget family's live
//! actor state matches the disc operand census + the `screen_fx` model.
//!
//! The ending-sequence captures (the only retail consumer of the widget
//! family - field-VM op `0x43` sub-ops) hold live widget actors on the
//! effect-actor list family at `0x8007C34C..0x8007C36C` (next at node
//! `+0x00`, per-frame handler at `+0x0C`, kill-flag bit 8 at `+0x10` -
//! `FUN_8003CF04`'s walk). Per capture this test asserts:
//!
//! - the **mask** widget's current + target rect channels (`+0x14..` /
//!   `+0x3C..`) hold the exact rects the disc census records install
//!   (mask-to-black `[0x20,0x20,0x20,0x20]`, top/bottom border
//!   `[0, 0x10, 0x140, 0xCC]`), latched (`+0x9C/+0x9E = 0` - the census
//!   records all carry duration 0);
//! - the **panel** widget sits at the move-record position with its
//!   `+0xB8` base sizes intact and its live size = base x scale `0x700`
//!   (4.12 fixed) - the `FUN_801F8E6C` scale math, live;
//! - the between-vignettes state has an **empty** widget list (per-scene
//!   teardown).
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `scripts/scenarios.toml` /
//! `saves/library`.

use std::path::PathBuf;

use legaia_mednafen::{SaveState, ScenarioManifest};

const SPRITE: u32 = 0x801F_7A9C;
const MASK: u32 = 0x801F_811C;
const PANEL: u32 = 0x801F_849C;
const LETTERBOX: u32 = 0x801F_8A34;
const LIST_HEADS: u32 = 0x8007_C34C;

fn manifest_path() -> Option<PathBuf> {
    [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

fn library_dir() -> Option<PathBuf> {
    ["saves/library", "../saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.is_dir())
}

fn ru32(ram: &[u8], va: u32) -> u32 {
    let off = (va as usize - 0x8000_0000) & 0x1F_FFFF;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}

fn ri16s<const N: usize>(ram: &[u8], va: u32) -> [i16; N] {
    let off = (va as usize - 0x8000_0000) & 0x1F_FFFF;
    let mut out = [0i16; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = i16::from_le_bytes(ram[off + i * 2..off + i * 2 + 2].try_into().unwrap());
    }
    out
}

/// Walk every effect-actor list head; return live (non-killed) widget
/// actors as `(handler, node)` pairs, deduplicated by node address.
fn live_widgets(ram: &[u8]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for head in 0..9u32 {
        let mut node = ru32(ram, LIST_HEADS + head * 4);
        let mut hops = 0;
        while node != 0 && hops < 256 {
            hops += 1;
            let h = ru32(ram, node + 0xC);
            if [SPRITE, MASK, PANEL, LETTERBOX].contains(&h)
                && ru32(ram, node + 0x10) & 8 == 0
                && !out.iter().any(|&(_, n)| n == node)
            {
                out.push((h, node));
            }
            node = ru32(ram, node);
        }
    }
    out
}

fn load_state(label: &str) -> Option<Vec<u8>> {
    let manifest = ScenarioManifest::from_path(&manifest_path()?).ok()?;
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let library = library_dir()?;
    let path = manifest
        .mednafen_save_path(scn, Some(library.as_path()))
        .ok()?;
    if !path.exists() {
        return None;
    }
    let state = SaveState::from_path(&path).ok()?;
    state.main_ram().ok().map(|r| r.to_vec())
}

#[test]
fn ending_widget_states_match_census_and_model() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let mut checked = 0usize;

    // Banner over black: mask latched at the census mask-to-black rect.
    if let Some(ram) = load_state("ending_banner_mask_closed") {
        let widgets = live_widgets(&ram);
        let mask = widgets.iter().find(|&&(h, _)| h == MASK).map(|&(_, n)| n);
        let mask = mask.expect("banner state: mask widget live");
        assert_eq!(ri16s::<4>(&ram, mask + 0x14), [0x20, 0x20, 0x20, 0x20]);
        assert_eq!(ri16s::<4>(&ram, mask + 0x3C), [0x20, 0x20, 0x20, 0x20]);
        assert_eq!(ri16s::<2>(&ram, mask + 0x9C), [0, 0], "duration-0 snap");
        assert!(
            widgets.iter().filter(|&&(h, _)| h == SPRITE).count() >= 2,
            "banner sprites live"
        );
        checked += 1;
    }

    // Fullscreen vignette (and the Biron sibling): the border rect.
    for label in ["ending_vignette_fullscreen", "ending_vignette_biron"] {
        if let Some(ram) = load_state(label) {
            let widgets = live_widgets(&ram);
            let mask = widgets.iter().find(|&&(h, _)| h == MASK).map(|&(_, n)| n);
            let mask = mask.unwrap_or_else(|| panic!("{label}: mask widget live"));
            assert_eq!(
                ri16s::<4>(&ram, mask + 0x14),
                [0, 0x10, 0x140, 0xCC],
                "{label}: census border rect"
            );
            checked += 1;
        }
    }

    // Panel shrunk to the corner: move-record position, base x 0x700 size.
    if let Some(ram) = load_state("ending_panel_corner") {
        let widgets = live_widgets(&ram);
        let panel = widgets.iter().find(|&&(h, _)| h == PANEL).map(|&(_, n)| n);
        let panel = panel.expect("corner state: panel widget live");
        let [x, y, w, h] = ri16s::<4>(&ram, panel + 0x14);
        let [bw, bh, _bpw] = ri16s::<3>(&ram, panel + 0xB8);
        assert_eq!((bw, bh), (0x140, 0xE0), "panel base sizes intact");
        // The census move records: x in {0x10, 0xA4}, y in {0x20, 0x70},
        // scale 0x700 (4.12 fixed) applied to the base sizes.
        assert!([0x10, 0xA4].contains(&x) && [0x20, 0x70].contains(&y));
        assert_eq!(w as i32, (bw as i32 * 0x700) >> 12, "width = base x 0.4375");
        assert_eq!(
            h as i32,
            (bh as i32 * 0x700) >> 12,
            "height = base x 0.4375"
        );
        checked += 1;
    }

    // Between vignettes: clean teardown, no live widgets.
    if let Some(ram) = load_state("ending_scene_load_gap") {
        assert!(
            live_widgets(&ram).is_empty(),
            "scene-load gap: widget list torn down"
        );
        checked += 1;
    }

    if checked == 0 {
        eprintln!("[skip] no ending-sequence captures available");
    } else {
        eprintln!("validated {checked} ending widget state(s) against the census + scale math");
    }
}
