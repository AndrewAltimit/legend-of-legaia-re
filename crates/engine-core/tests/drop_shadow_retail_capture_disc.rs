//! Retail capture: the field drop shadow (`FUN_8001C394`), packet for packet.
//!
//! `v0_1_pre_battle_tetsu` (`town01`, field mode, retail SCUS) is a standing
//! frame: the player idle, the villagers around him idle. Its ordering tables
//! carry the blob packets `FUN_8001C394` linked - `POLY_FT4`, command `0x2E`,
//! CLUT `0x7F86`, page `0x001F`, one group of four per shadowed actor.
//!
//! The test rebuilds them from nothing but the state's own inputs:
//!
//! 1. the actor lists (`0x8007C34C..0x8007C360`), filtered by the gate
//!    `FUN_8001B964` runs (`legaia_engine_core::drop_shadow::casts_shadow`)
//!    and the actor pass's own draw tests (draw kind `1`, the hide bits
//!    `0xA` clear, view depth `+0x34 > 0xA0`);
//! 2. each actor's `+0x14` position through the engine's grid kernel and the
//!    GTE's `RTPT` arithmetic, with the camera matrix the renderer restores
//!    from scratchpad `0x1F8003C8` and `H = _DAT_8007B6F4`;
//! 3. the engine's packet kernel (`drop_shadow::shadow_quads`).
//!
//! Every retail blob packet must be one the kernel emits - vertices, UVs,
//! CLUT and page exact - and the kernel must emit no blob retail did not
//! draw. That pins the grid, the cell walk, the texel rectangle and the gate
//! in one frame; the texture it samples is checked against the state's VRAM
//! by `drop_shadow_blob_is_resident_in_the_scene_vram`. The port's stand-in
//! for draw kind `1` - a non-zero clip id at `+0x5C` - is checked against
//! every class-bit actor in the lists on the way.
//!
//! Skips (and passes) when the scenario manifest or the save library is
//! missing.

use legaia_engine_core::drop_shadow::{
    SHADOW_CLUT, SHADOW_TPAGE, ShadowVertex, casts_shadow, shadow_grid, shadow_quads,
};
use legaia_mednafen::prim_pool::{self, Prim};
use legaia_mednafen::{SaveState, ScenarioManifest};
use std::collections::BTreeSet;
use std::path::PathBuf;

const RAM_MASK: u32 = 0x001F_FFFF;
const LIST_HEADS: [u32; 6] = [
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
];
/// `_DAT_8007B6F4`: GTE `H`.
const GTE_H: u32 = 0x8007_B6F4;
/// Scratchpad offset of the camera matrix `FUN_8001B964` restores.
const VIEW_MATRIX: usize = 0x3C8;
const OFX: i64 = 160;

fn find(rel: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{rel}")))
        .find(|p| p.exists())
}

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

fn load(label: &str) -> Option<SaveState> {
    let (manifest_path, library) = (find("scripts/scenarios.toml")?, find("saves/library")?);
    let manifest = ScenarioManifest::from_path(&manifest_path).ok()?;
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let path = manifest.library_save_path(scn, library.as_path())?;
    path.exists()
        .then(|| SaveState::from_path(&path).expect("parse save state"))
}

/// `RTPS` with `sf = 1`, `lm = 0`, as the GTE runs it: `SZ` saturated to
/// `0..=0xFFFF`, the UNR divide saturated to `0x1FFFF`, `SX/SY` to
/// `-0x400..=0x3FF`.
struct Gte {
    r: [[i64; 3]; 3],
    t: [i64; 3],
    h: i64,
    ofy: i64,
}

impl Gte {
    fn rtps(&self, v: [i32; 3]) -> ShadowVertex {
        let mac = |row: usize| {
            (self.t[row] << 12)
                + self.r[row][0] * i64::from(v[0])
                + self.r[row][1] * i64::from(v[1])
                + self.r[row][2] * i64::from(v[2])
        };
        let ir = |row: usize| (mac(row) >> 12).clamp(-0x8000, 0x7FFF);
        let sz = (mac(2) >> 12).clamp(0, 0xFFFF);
        let q = if sz > 0 && self.h < sz * 2 {
            ((self.h * 0x20000 / sz + 1) / 2).min(0x1FFFF)
        } else {
            0x1FFFF
        };
        let sx = ((OFX << 16) + ir(0) * q) >> 16;
        let sy = ((self.ofy << 16) + ir(1) * q) >> 16;
        ShadowVertex {
            sx: sx.clamp(-0x400, 0x3FF) as i16,
            sy: sy.clamp(-0x400, 0x3FF) as i16,
            sz: sz as u16,
            depth: None,
        }
    }
}

type Packet = ([(i16, i16); 4], [(u8, u8); 4]);

#[test]
fn drop_shadow_packets_match_the_retail_town_frame() {
    check("v0_1_pre_battle_tetsu", false);
}

/// The kingdom overworld: the same packets, every corner's `SY` bent by the
/// curvature entry at the cell's mean depth.
#[test]
fn drop_shadow_packets_match_the_retail_overworld_frames() {
    check("keikoku_chest_preload", true);
    check("karisto_overworld_resident", true);
}

fn check(label: &str, overworld: bool) {
    let Some(state) = load(label) else {
        eprintln!("[skip] {label} state / manifest missing");
        return;
    };
    let ram = state.main_ram().expect("main RAM");
    let sp = state.scratch_ram().expect("scratchpad");
    let m = &sp[VIEW_MATRIX..VIEW_MATRIX + 0x20];
    let r = [
        [i16_at(m, 0), i16_at(m, 2), i16_at(m, 4)],
        [i16_at(m, 6), i16_at(m, 8), i16_at(m, 10)],
        [i16_at(m, 12), i16_at(m, 14), i16_at(m, 16)],
    ]
    .map(|row| row.map(i64::from));
    let t = [0x14, 0x18, 0x1C]
        .map(|o| i64::from(i32::from_le_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]])));
    let h = i64::from(u32_at(ram, GTE_H) as i32);

    // Retail side: every blob packet of every ordering table in RAM.
    let mut retail: Vec<BTreeSet<Packet>> = Vec::new();
    for ot in prim_pool::find_ot_arrays(ram, 0x8000_0000, 64) {
        let chain = prim_pool::chain_walk(ram, 0x8000_0000, (ot.head & RAM_MASK) as usize);
        let mut set = BTreeSet::new();
        for c in chain {
            if let Prim::PolyFt4 {
                cmd,
                color,
                verts,
                uvs,
                clut,
                tpage,
            } = c.prim
                && clut == SHADOW_CLUT
                && tpage == SHADOW_TPAGE
            {
                assert_eq!(cmd, 0x2E, "textured, semi-transparent, texture-blended");
                assert_eq!(color, [0x80; 3], "neutral modulation");
                set.insert((verts, uvs));
            }
        }
        if !set.is_empty() {
            retail.push(set);
        }
    }
    assert!(!retail.is_empty(), "{label}: the frame draws no blob");

    // Engine side: the gate over the actor lists, the grid through the GTE.
    let mut casters = Vec::new();
    for head in LIST_HEADS {
        let mut p = u32_at(ram, head);
        let mut guard = 0;
        while p != 0 && guard < 512 {
            let o = (p & RAM_MASK) as usize;
            let flags = u32_at(ram, p + 0x10);
            let kind = u16::from_le_bytes([ram[o + 0x56], ram[o + 0x57]]);
            let vz = u32_at(ram, p + 0x34) as i32;
            // The port reads draw kind `1` off the channel's clip id: an
            // animated actor names a clip at `+0x5C`, a kind-`5` one none.
            let clip = i16_at(ram, o + 0x5C);
            if casts_shadow(flags) && matches!(kind, 1 | 5) {
                assert_eq!(
                    kind == 1,
                    clip != 0,
                    "{label}: actor 0x{p:08X} kind {kind} clip {clip}"
                );
            }
            if casts_shadow(flags) && kind == 1 && flags & 0xA == 0 && vz > 0xA0 {
                casters.push((
                    p,
                    flags,
                    [
                        i32::from(i16_at(ram, o + 0x14)),
                        i32::from(i16_at(ram, o + 0x16)),
                        i32::from(i16_at(ram, o + 0x18)),
                    ],
                ));
            }
            p = u32_at(ram, p);
            guard += 1;
        }
    }
    // OFY is a GTE control word the state does not carry; take the one of
    // the two field-plausible centres that explains retail's packets.
    let mut best: Option<(i64, BTreeSet<Packet>)> = None;
    for ofy in [120, 114] {
        let gte = Gte { r, t, h, ofy };
        let mut set = BTreeSet::new();
        for &(_, flags, pos) in &casters {
            let pts = shadow_grid(pos);
            let grid = pts.map(|v| gte.rtps(v));
            for q in shadow_quads(&grid, overworld, 0, flags & 0x80_0000 != 0) {
                set.insert((q.xy, q.uv));
            }
        }
        let hits = retail.iter().map(|r| r.intersection(&set).count()).max();
        if best
            .as_ref()
            .is_none_or(|(_, b)| hits > retail.iter().map(|r| r.intersection(b).count()).max())
        {
            best = Some((ofy, set));
        }
    }
    let (ofy, engine) = best.unwrap();
    eprintln!(
        "[info] {label}: H {h}, OFY {ofy}: {} casters pass the gate, {} engine packets; \
         retail tables carry {:?} blob packets",
        casters.len(),
        engine.len(),
        retail.iter().map(BTreeSet::len).collect::<Vec<_>>()
    );
    // At least one table was built from the state's own positions (the
    // standing frame): it must be the engine's set exactly.
    let exact = retail.iter().filter(|r| **r == engine).count();
    for (i, r) in retail.iter().enumerate() {
        let missing: Vec<_> = r.difference(&engine).collect();
        let extra: Vec<_> = engine.difference(r).collect();
        eprintln!(
            "[info] table {i}: {} retail packets, {} unmatched, {} engine-only",
            r.len(),
            missing.len(),
            extra.len()
        );
        for p in missing.iter().take(4) {
            eprintln!("    retail only {p:?}");
        }
        for p in extra.iter().take(4) {
            eprintln!("    engine only {p:?}");
        }
    }
    assert!(
        exact >= 1,
        "{label}: no retail ordering table's blob packets equal the engine kernel's"
    );
    eprintln!("[ok] {label}: {exact} ordering table(s) match the engine's blob packets exactly");
}

/// The blob's texels and palette are resident in the field VRAM both hosts
/// sample: the 16x16 cell at `(0xE0, 0)` of page `0x001F` and the 16 entries
/// of CLUT `0x7F86` in the engine's `town01` scene VRAM (the boot system-UI
/// underlay) equal the retail state's VRAM word for word.
#[test]
fn drop_shadow_blob_is_resident_in_the_scene_vram() {
    use legaia_engine_core::scene::SceneHost;
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
    else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(state) = load("v0_1_pre_battle_tetsu") else {
        eprintln!("[skip] v0_1_pre_battle_tetsu state / manifest missing");
        return;
    };
    let gpu = legaia_mednafen::gpu::PsxGpu::new(&state);
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    let vram = &host.resources.as_ref().expect("town01 resources").vram;
    // Page 0x1F: x = 15 * 64, y = 256; 4-bit, so u 0xE0 is halfword 0x38.
    let (px, py) = (960 + 0xE0 / 4, 256);
    let cx = u32::from(SHADOW_CLUT & 0x3F) * 16;
    let cy = u32::from(SHADOW_CLUT >> 6);
    let texels = (0..16).flat_map(|v| (0..4).map(move |h| (px + h, py + v)));
    let clut = (0..16).map(|i| (cx + i, cy));
    let mut words = 0;
    for (x, y) in texels.chain(clut) {
        let retail = gpu.vram_pixel(x, y).expect("state VRAM");
        assert_eq!(
            vram.pixel(x as usize, y as usize),
            retail,
            "VRAM ({x}, {y})"
        );
        words += 1;
    }
    eprintln!("[ok] {words} blob texel / CLUT words equal the retail VRAM");
}
