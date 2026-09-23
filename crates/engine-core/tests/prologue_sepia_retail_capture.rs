//! Retail capture of the prologue sepia's **packet half**: the two field-VM
//! `4C E6` ops `opdeene` issues (partition 1 record 0) walk every resident
//! TMD through `FUN_801D8280` -> `FUN_801D5E20` and rewrite each baked colour
//! word through the SCUS HSV pair. Composed, a word ends as a function of
//! its `max` alone:
//!
//! `(V, V * 246 >> 8, V * 112 >> 8)`, `V = min(max, 0xF8) - 30` (floored at 0)
//!
//! which is the law `ColorGrade`'s docs and the renderer's
//! `prologue_sepia_word` carry. The state is `s1_newgame_field`
//! (`scripts/scenarios.toml`, retail SCUS, `opdeene` at field-run): this test
//! walks the resident-object table `DAT_8007C018[0..=DAT_8007BB38]` the
//! walker walks, reads every colour word the walker would rewrite (group
//! `flags >> 1` in `12..=19`, counts `[1, 1, 3, 4, 1, 1, 3, 4]` from the
//! field overlay's table at `0x801F26F0`), and asserts every one lies on the
//! curve.
//!
//! Capture-gated: skip-passes without `extracted/` (the state reader's
//! main-RAM anchor search reads `SCUS_942.54`) or the save library.

use std::path::PathBuf;

const S1_STATE: &str = "e3d6b6f329cdcf23e28ac3a4f4e0bd6ef6b5abee99a354de48f9ce207e49f7e0";
/// `DAT_8007C018` - the resident-object (TMD) table `FUN_801D8280` walks.
const RESIDENT_TABLE: u32 = 0x8007_C018;
/// `DAT_8007BB38` - the last issued id (the walk bound is `+ 1`).
const RESIDENT_LAST: u32 = 0x8007_BB38;
/// Colour words per primitive by `flags >> 1`, from `12` (`0x801F26F0`).
const COLOUR_COUNT: [u32; 8] = [1, 1, 3, 4, 1, 1, 3, 4];

fn sepia(m: u32) -> [u8; 3] {
    let v = m.min(0xF8).saturating_sub(30);
    [v as u8, ((v * 246) >> 8) as u8, ((v * 112) >> 8) as u8]
}

#[test]
fn opdeene_resident_colour_words_lie_on_the_sepia_curve() {
    let Some(lib) = ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}saves/library/pcsx-redux")))
        .find(|d| d.is_dir())
    else {
        eprintln!("[skip] saves/library/pcsx-redux missing (capture-gated)");
        return;
    };
    let Some(extracted) = ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}extracted")))
        .find(|d| d.join("SCUS_942.54").exists())
    else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let path = lib.join(format!("{S1_STATE}.sstate"));
    if !path.exists() {
        eprintln!("[skip] {} not in the library", path.display());
        return;
    }
    let st = legaia_pcsxr::SaveState::from_path(&path).expect("load the S1 state");
    assert_eq!(st.scene_name(), "opdeene");
    let in_ram = |a: u32| (0x8000_0000..0x8020_0000).contains(&a);
    let last = st.u32_at(RESIDENT_LAST);
    let (mut words, mut tmds, mut off_curve) = (0usize, 0usize, Vec::new());
    for id in 0..=last {
        let tmd = st.u32_at(RESIDENT_TABLE + 4 * id);
        if !in_ram(tmd) {
            continue;
        }
        tmds += 1;
        for o in 0..st.u32_at(tmd + 8).min(1024) {
            let mut p = st.u32_at(tmd + 0x0C + o * 0x1C + 0x10);
            if !in_ram(p) {
                continue;
            }
            // `FUN_801D5E20`'s walk: groups until a zero header word; the
            // stride add runs once per primitive and once more per group.
            let mut guard = 0;
            while st.u32_at(p) != 0 && guard < 4096 {
                let count = u32::from(st.u16_at(p));
                let sel = u32::from(st.u16_at(p + 2)) >> 1;
                let stride = u32::from(st.u8_at(p + 5)) * 4;
                p += 8;
                let k = if (12..=19).contains(&sel) {
                    COLOUR_COUNT[(sel - 12) as usize]
                } else {
                    0
                };
                for _ in 0..count {
                    for j in 0..k {
                        let c = [
                            st.u8_at(p + 4 * j),
                            st.u8_at(p + 4 * j + 1),
                            st.u8_at(p + 4 * j + 2),
                        ];
                        words += 1;
                        if sepia(u32::from(c[0]) + 30) != c && c != [0, 0, 0] {
                            off_curve.push(c);
                        }
                    }
                    p += stride;
                }
                p += stride;
                guard += 1;
            }
        }
    }
    eprintln!(
        "[ok] {tmds} resident TMDs, {words} baked colour words, {} off the curve",
        off_curve.len()
    );
    assert!(words > 1000, "the walk reaches the resident meshes");
    assert!(
        off_curve.is_empty(),
        "every resident word is (V, V*246>>8, V*112>>8): {:?}",
        &off_curve[..off_curve.len().min(8)]
    );
}
