//! Retail-frame oracles for two shared kernels the hosts draw through: the
//! battle **value readout** (`engine-vm::battle_value_readout` /
//! `engine-ui::battle_numerals`) and the **field follow camera**
//! (`engine-core::camera_view`).
//!
//! Both kernels were pinned from retail captures, and both are re-measured
//! here against the save library directly rather than against the prose that
//! quotes it - a RAM image *is* the frame's display list, so a battle state's
//! own packet stream answers "what geometry did retail emit" without an
//! emulator run, and a walkable state's camera globals answer "what vantage
//! did retail use" the same way.
//!
//! # What each test pins
//!
//! **`retail_readout_quads_name_their_far_corner_inclusively`** - every
//! `7703/0027` packet in the three battle states that carry a readout puts its
//! far corner at `x0 + w - 1` / `y0 + h - 1`, exactly like its texel rect. The
//! `HIT` / `TOTAL` / `DAMAGE` label blits make that unambiguous: they are
//! drawn 1:1, and their screen delta equals their texel delta to the pixel.
//! The port's [`legaia_engine_ui::battle_numerals::readout_quad`] instead
//! emits `x0 + w` (see its `x1` / `y1`), so each engine readout quad spans one
//! more column and row than retail's; the test prints that delta per label
//! rather than asserting it, because fixing it is a host-side change.
//!
//! **`retail_combo_cluster_seats_match_the_port`** - the port's
//! `combo_cluster` seats and texel rects are byte-equal to retail's own, on
//! the settled frame (`player_steal_skeleton_banner`) and on the mid-glide one
//! (`battle_melee_hit_spark`, whose whole cluster is `+40` px - the glide
//! record's own remaining travel).
//!
//! **`retail_field_camera_is_not_three_constants`** - the walkable states'
//! `_DAT_8007B6F4` (GTE `H`) and `_DAT_8007B790` rotation trio. Two of the
//! port's pins reproduce across the whole corpus (`roll = 0`, world-map
//! `H = 368`); the three field-follow fallbacks (`FIELD_H`,
//! `FIELD_PITCH_UNITS`, `FIELD_FOLLOW_YAW_UNITS`) do not, and the test asserts
//! the spread that makes a scene-invariant follow camera impossible. The
//! per-scene camera the spread demands is `engine-core::camera_zone`; its
//! per-state oracle is `field_camera_zone_oracle.rs` beside this file.
//!
//! Skips (passes) unless `scripts/scenarios.toml` and `saves/library` are both
//! present. CI runs without Sony bytes.

use std::path::{Path, PathBuf};

use legaia_engine_vm::battle_value_readout as vr;
use legaia_mednafen::{SaveState, ScenarioManifest};

const RAM_BASE: u32 = 0x8000_0000;
/// GTE `H` (`_DAT_8007B6F4`).
const GTE_H: u32 = 0x8007_B6F4;
/// Camera rotation trio `(pitch, yaw, roll)` in PSX 12-bit units.
const CAM_ROT: u32 = 0x8007_B790;

fn library() -> Option<(ScenarioManifest, PathBuf)> {
    let manifest = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())?;
    let lib = ["saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())?;
    Some((ScenarioManifest::from_path(&manifest).ok()?, lib))
}

fn state(manifest: &ScenarioManifest, lib: &Path, label: &str) -> Option<SaveState> {
    let sc = manifest.scenarios.iter().find(|s| s.label == label)?;
    let path = manifest.library_save_path(sc, lib)?;
    path.exists().then(|| SaveState::from_path(&path).ok())?
}

fn rs16(ram: &[u8], va: u32) -> i16 {
    let off = (va - RAM_BASE) as usize & 0x1F_FFFF;
    i16::from_le_bytes([ram[off], ram[off + 1]])
}

/// One retail readout packet, reduced to the fields the port also carries.
#[derive(Debug, Clone, Copy)]
struct ReadoutPacket {
    xy: [(i16, i16); 4],
    uv: [(u8, u8); 4],
    cmd: u8,
    color: [u8; 3],
}

impl ReadoutPacket {
    /// Screen delta between the near and far corner - `w - 1` under retail's
    /// inclusive convention.
    fn dxy(&self) -> (i32, i32) {
        (
            i32::from(self.xy[1].0) - i32::from(self.xy[0].0),
            i32::from(self.xy[2].1) - i32::from(self.xy[0].1),
        )
    }
    /// Texel delta, which is inclusive beyond doubt (a `u8` corner pair).
    fn duv(&self) -> (i32, i32) {
        (
            i32::from(self.uv[1].0) - i32::from(self.uv[0].0),
            i32::from(self.uv[2].1) - i32::from(self.uv[0].1),
        )
    }
    fn inclusive_uv(&self) -> (u8, u8, u8, u8) {
        (self.uv[0].0, self.uv[0].1, self.uv[1].0, self.uv[2].1)
    }
}

/// Every readout quad in a state's main RAM, by **signature byte scan**.
///
/// Not by walking the frame's prim pool: the pool finder anchors a run a few
/// words ahead of its first tag word, so a linear decode from there drops the
/// pool's leading packet - and the leading packet is exactly the one the
/// cluster's `TOTAL` / `DAMAGE` label sits in. The readout's own packet shape
/// is unambiguous enough to key on directly (`0x2C808080` at `+4`, the glyph
/// CLUT at `+0x0E`, the glyph page at `+0x16`, the readout's OT bucket in the
/// tag's high byte), so this scans for that and dedupes the double buffer's
/// two copies.
fn readout_packets(ram: &[u8]) -> Vec<ReadoutPacket> {
    let rd16 = |o: usize| u16::from_le_bytes([ram[o], ram[o + 1]]);
    let mut out: Vec<ReadoutPacket> = Vec::new();
    let mut off = 0usize;
    while off + vr::PRIM_BYTES <= ram.len() {
        let code = u32::from_le_bytes([ram[off + 4], ram[off + 5], ram[off + 6], ram[off + 7]]);
        if code == vr::CODE_COLOUR
            && ram[off + 3] == (vr::OT_TAG >> 24) as u8
            && rd16(off + 0x0E) == vr::GLYPH_CLUT
            && rd16(off + 0x16) == vr::GLYPH_TPAGE
        {
            let mut xy = [(0i16, 0i16); 4];
            let mut uv = [(0u8, 0u8); 4];
            for (c, (p, t)) in xy.iter_mut().zip(uv.iter_mut()).enumerate() {
                let b = off + 8 + c * 8;
                *p = (
                    i16::from_le_bytes([ram[b], ram[b + 1]]),
                    i16::from_le_bytes([ram[b + 2], ram[b + 3]]),
                );
                *t = (ram[b + 4], ram[b + 5]);
            }
            let p = ReadoutPacket {
                xy,
                uv,
                cmd: (code >> 24) as u8,
                color: [ram[off + 4 + 2], ram[off + 4 + 1], ram[off + 4]],
            };
            // The double buffer holds two copies of the same frame.
            if !out.iter().any(|q| q.xy == p.xy && q.uv == p.uv) {
                out.push(p);
            }
        }
        off += 4;
    }
    out
}

/// The three states that carry a readout in the catalogued library.
const READOUT_STATES: [&str; 3] = [
    "battle_melee_hit_spark",
    "battle_gimard_tail_fire_a",
    "player_steal_skeleton_banner",
];

#[test]
fn retail_readout_quads_name_their_far_corner_inclusively() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing");
        return;
    };
    let mut labels = 0usize;
    let mut quads = 0usize;
    for label in READOUT_STATES {
        let Some(st) = state(&manifest, &lib, label) else {
            continue;
        };
        let Ok(ram) = st.main_ram() else { continue };
        let packets = readout_packets(ram);
        assert!(
            !packets.is_empty(),
            "{label}: the readout family must be in the frame"
        );
        for p in &packets {
            quads += 1;
            // Opaque textured quad at the passthrough modulation level - the
            // sheet is already the gold ramp, so retail does not tint it.
            assert_eq!(p.cmd & 0xFC, 0x2C, "{label}: readout quads are POLY_FT4");
            assert_eq!(
                p.color,
                [0x80, 0x80, 0x80],
                "{label}: readout quads carry the passthrough colour"
            );
            // Rectangular: both top corners on one row, both left corners in
            // one column. Everything below assumes it.
            assert_eq!(p.xy[0].1, p.xy[1].1, "{label}: top edge is level");
            assert_eq!(p.xy[0].0, p.xy[2].0, "{label}: left edge is plumb");

            // A 1:1 blit is identifiable without knowing which label it is:
            // its texel rect is one of the three the port carries.
            let uv = p.inclusive_uv();
            if [vr::LABEL_HIT, vr::LABEL_TOTAL, vr::LABEL_DAMAGE].contains(&uv) {
                labels += 1;
                let (dx, dy) = p.dxy();
                let (du, dv) = p.duv();
                assert_eq!(
                    (dx, dy),
                    (du, dv),
                    "{label}: a 1:1 label blit puts its far corner where its \
                     far texel is, so the screen rect is inclusive too"
                );
                // What the port would emit for the same label, for the record.
                let w = i32::from(uv.2) - i32::from(uv.0) + 1;
                eprintln!(
                    "[{label}] label uv={uv:?} retail dx={dx} (w-1), port \
                     readout_quad dx={w} (w) - {} px wider",
                    w - dx
                );
            }
        }
    }
    assert!(
        labels >= 3,
        "the three catalogued states carry at least three 1:1 label blits \
         between them; got {labels}"
    );
    eprintln!("[ok] {quads} retail readout quads, {labels} of them 1:1 label blits");
}

#[test]
fn retail_combo_cluster_seats_match_the_port() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing");
        return;
    };
    let mut reproduced = 0usize;
    for label in READOUT_STATES {
        let Some(st) = state(&manifest, &lib, label) else {
            continue;
        };
        let Ok(ram) = st.main_ram() else { continue };
        let packets = readout_packets(ram);
        if packets.is_empty() {
            continue;
        }
        // A state's RAM holds both halves of the double buffer, and the
        // cluster is mid-glide in one of them, so group by the slide each
        // label implies rather than assuming one frame.
        let mut slides: Vec<i32> = packets
            .iter()
            .filter_map(|p| match p.inclusive_uv() {
                u if u == vr::LABEL_HIT => Some(i32::from(p.xy[0].0) - vr::COMBO_HIT_LABEL_SEAT.0),
                u if u == vr::LABEL_DAMAGE => {
                    Some(i32::from(p.xy[0].0) - vr::COMBO_DAMAGE_LABEL_SEAT.0)
                }
                _ => None,
            })
            .collect();
        slides.sort_unstable();
        slides.dedup();
        assert!(!slides.is_empty(), "{label}: the cluster carries a label");

        for slide in slides {
            let at = |x: i32, y: i32, uv: (u8, u8)| {
                packets
                    .iter()
                    .find(|p| {
                        i32::from(p.xy[0].0) == x && i32::from(p.xy[0].1) == y && p.uv[0] == uv
                    })
                    .copied()
            };
            // The value row: 16-px cells at a 16-px pitch, right edge 304.
            let row: Vec<&ReadoutPacket> = packets
                .iter()
                .filter(|p| i32::from(p.xy[0].1) == vr::COMBO_VALUE_Y)
                .filter(|p| {
                    let x = i32::from(p.xy[0].0);
                    (x - (vr::COMBO_VALUE_RIGHT_X + slide)) % vr::COMBO_VALUE_CELL as i32 == 0
                        && x < vr::COMBO_VALUE_RIGHT_X + slide
                })
                .collect();
            if row.is_empty() {
                continue;
            }
            // Reconstruct the value the row shows and hand it back to the
            // port's own layout, then require every seat to match.
            let mut digits: Vec<(i32, u8)> = row
                .iter()
                .map(|p| {
                    let d = (p.uv[0].0 / vr::DIGIT_CELL + 1) % 10;
                    (i32::from(p.xy[0].0), d)
                })
                .collect();
            digits.sort_unstable();
            let total: u32 = digits.iter().fold(0u32, |a, (_, d)| a * 10 + u32::from(*d));
            // The hit count is the 24-px run whose last cell's left edge is
            // `264 + slide - 24`, one pitch (`24 + 1`) apart. Keying on the
            // pitch is what keeps the OTHER buffered frame's count out.
            let count_left = vr::COMBO_COUNT_RIGHT_X + slide - i32::from(vr::DIGIT_CELL);
            let pitch = i32::from(vr::DIGIT_CELL) + vr::DIGIT_GAP;
            let mut counts: Vec<(i32, u8)> = packets
                .iter()
                .filter(|p| i32::from(p.xy[0].1) == vr::COMBO_COUNT_Y)
                .filter(|p| {
                    let x = i32::from(p.xy[0].0);
                    x <= count_left && (count_left - x) % pitch == 0
                })
                .map(|p| (i32::from(p.xy[0].0), (p.uv[0].0 / vr::DIGIT_CELL + 1) % 10))
                .collect();
            counts.sort_unstable();
            let count = if counts.is_empty() {
                None
            } else {
                Some(counts.iter().fold(0u16, |a, (_, d)| a * 10 + u16::from(*d)))
            };
            let style = if count.is_some() {
                vr::ComboStyle::HitTotal
            } else {
                vr::ComboStyle::Damage
            };
            let cluster = vr::combo_cluster(style, count.unwrap_or(0), total, slide);
            for l in &cluster.labels {
                assert!(
                    at(l.x, l.y, (l.uv.0, l.uv.1)).is_some(),
                    "{label}: no retail quad for the {} label at ({}, {}) \
                     with slide {slide}",
                    l.word,
                    l.x,
                    l.y
                );
            }
            for c in &cluster.cells {
                let q = at(c.x, c.y, (c.u, c.v)).unwrap_or_else(|| {
                    panic!(
                        "{label}: no retail quad for digit {} at ({}, {}) \
                         with slide {slide}",
                        c.digit, c.x, c.y
                    )
                });
                // Retail's drawn extent is one less than the port's `w`.
                let (dx, dy) = q.dxy();
                assert_eq!(
                    (dx + 1, dy + 1),
                    (c.w as i32, c.h as i32),
                    "{label}: digit {} drawn size at slide {slide}",
                    c.digit
                );
            }
            reproduced += 1;
            eprintln!(
                "[ok] {label}: cluster reproduced at slide {slide} \
                 ({style:?}, count {count:?}, total {total})"
            );
        }
    }
    assert!(reproduced > 0, "at least one catalogued cluster frame");
}

/// Every walkable state's `(scene, H, pitch, yaw, roll)`.
fn walkable_camera_rows(
    manifest: &ScenarioManifest,
    lib: &Path,
) -> Vec<(String, i16, i16, i16, i16)> {
    let mut out = Vec::new();
    for sc in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(sc, lib) else {
            continue;
        };
        if !path.exists() || path.extension().is_some_and(|e| e != "mcr") {
            continue;
        }
        let Ok(st) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = st.main_ram() else { continue };
        let scene = legaia_mednafen::game_anchors::scene_name(ram);
        let mode = legaia_mednafen::game_anchors::game_mode(ram);
        // Field / walkable modes only - a battle writes its own camera.
        if mode != 0x03 {
            continue;
        }
        out.push((
            scene,
            rs16(ram, GTE_H),
            rs16(ram, CAM_ROT),
            rs16(ram, CAM_ROT + 2),
            rs16(ram, CAM_ROT + 4),
        ));
    }
    out
}

#[test]
fn retail_field_camera_is_not_three_constants() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing");
        return;
    };
    let rows = walkable_camera_rows(&manifest, &lib);
    if rows.len() < 8 {
        eprintln!("[skip] only {} walkable states in the library", rows.len());
        return;
    }
    let (maps, fields): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r.0.starts_with("map"));

    // Two port pins the whole corpus reproduces.
    for r in &rows {
        assert_eq!(
            r.4, 0,
            "{}: the walkable camera never rolls, so `roll: 0.0` is right",
            r.0
        );
    }
    for m in &maps {
        assert_eq!(
            f32::from(m.1),
            legaia_engine_core::camera_view::WORLD_MAP_H,
            "{}: the overworld GTE H is the pinned one",
            m.0
        );
    }

    // ... and the three that it does not.
    let uniq = |f: fn(&&(String, i16, i16, i16, i16)) -> i16| {
        let mut v: Vec<i16> = fields.iter().map(f).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let hs = uniq(|r| r.1);
    let pitches = uniq(|r| r.2);
    let yaws = uniq(|r| r.3);
    assert!(
        hs.len() > 1 && pitches.len() > 1 && yaws.len() > 1,
        "the field follow camera moves per scene and per frame: H {hs:?}, \
         pitch {pitches:?}, yaw {yaws:?}"
    );

    let n = fields.len();
    let hits = |want: f32, f: fn(&&(String, i16, i16, i16, i16)) -> i16| {
        fields.iter().filter(|r| f32::from(f(r)) == want).count()
    };
    use legaia_engine_core::camera_view as cv;
    eprintln!(
        "[ok] {n} non-overworld walkable states: FIELD_H={} holds {}/{n}, \
         FIELD_PITCH_UNITS={} holds {}/{n}, FIELD_FOLLOW_YAW_UNITS={} holds {}/{n}; \
         roll=0 and WORLD_MAP_H={} hold everywhere ({} overworld states)",
        cv::FIELD_H,
        hits(cv::FIELD_H, |r| r.1),
        cv::FIELD_PITCH_UNITS,
        hits(cv::FIELD_PITCH_UNITS, |r| r.2),
        cv::FIELD_FOLLOW_YAW_UNITS,
        hits(cv::FIELD_FOLLOW_YAW_UNITS, |r| r.3),
        cv::WORLD_MAP_H,
        maps.len(),
    );
}
