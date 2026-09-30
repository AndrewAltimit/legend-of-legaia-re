//! Save-library-gated: camera-relative (`+0x52 & 0x780`) move-VM parts, placed
//! by the hosts' kernel, land where retail drew them.
//!
//! Every mednafen library state is walked for part-tick nodes
//! (`+0x0C == FUN_80021DF4`) carrying a `0x780` bit. Three legs, each against
//! the retail state's own RAM, scratchpad and display list:
//!
//! 1. **What `+0x14` is.** The node's view-space position `+0x2C` is the full
//!    camera (`0x1F8003C8`, rotation + translation) applied to `+0x14` for a
//!    skip-bit node, and `S_b * (+0x14)` for a `0x400` node - so `+0x14` is a
//!    world position on the skip arm and an eye-space offset on the locked arm,
//!    as `FUN_8001CF50` reads (`docs/subsystems/renderer.md`).
//! 2. **The host camera is retail's.** `S_b * camera_rotation(pitch, yaw,
//!    roll)` over the angle trio `0x8007B790` reproduces the scratchpad camera
//!    matrix, so the hosts' view-projection is the one a `0x780` prefix undoes.
//! 3. **The kernel places the part where retail drew it.** Under a
//!    [`PartCameraPose`] read from the state, [`camera_relative_part`]'s
//!    placement maps onto the retail `+0x2C`; and for a single-quad billboard
//!    part, the four corners projected through the host composition
//!    (`T(pos) * K * N` under the full camera, then `H` / `OFX` / `OFY`) match
//!    a packet in retail's own primitive pool to within a pixel - size and
//!    orientation included, which is what the six-fold literal and the dropped
//!    axes decide.
//!
//! Needs the mednafen library (`LEGAIA_SAVES_LIBRARY`, else `saves/library`);
//! skips and passes without it.

use legaia_engine_ui::gte::{
    CameraRelativePart, PartCameraPose, camera_relative_part, camera_view_rotation,
};
use legaia_engine_vm::psx_camera::camera_rotation;
use legaia_mednafen::SaveState;
use legaia_mednafen::prim_pool;
use std::path::PathBuf;

/// The battle / field actor-list heads the frame passes walk (next at `+0`).
const LIST_HEADS: [u32; 6] = [
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
];
/// A move-VM part's per-frame tick, in its `+0x0C` callback word.
const PART_TICK: u32 = 0x8002_1DF4;
/// The GTE screen centre, constant across the corpus
/// (`docs/subsystems/renderer.md`, "The screen the GTE projects onto").
const OFX: f32 = 160.0;
const OFY: f32 = 114.0;

fn library() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LEGAIA_SAVES_LIBRARY") {
        let p = PathBuf::from(p).join("mednafen");
        if p.is_dir() {
            return Some(p);
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library/mednafen");
    root.is_dir().then_some(root)
}

struct Node {
    flags: u16,
    pos: [i16; 3],
    ang: [i16; 3],
    view: [i32; 3],
    scale: f32,
    /// The model's vertex list, when its first object is one quad.
    quad: Option<[[f32; 3]; 4]>,
}

struct Frame {
    label: String,
    /// On a kingdom overworld (`mapNN`) the curvature table bends every
    /// vertex's `SY` after projection, so a packet-level compare needs it.
    overworld: bool,
    angles: [i16; 3],
    /// Scratchpad camera matrix `0x1F8003C8` (rows, `4096 = 1.0`) + its
    /// translation.
    m: [[f32; 3]; 3],
    t: [f32; 3],
    h: f32,
    nodes: Vec<Node>,
    /// Every packet bound in every primitive pool found in RAM.
    prims: Vec<(i16, i16, i16, i16)>,
}

fn read_frame(path: &std::path::Path) -> Option<Frame> {
    let st = SaveState::from_path(path).ok()?;
    let ram = st.main_ram().ok()?;
    let sp = st.scratch_ram().ok()?;
    let o = |va: u32| (va & 0x001F_FFFF) as usize;
    let u32_at = |va: u32| u32::from_le_bytes(ram[o(va)..o(va) + 4].try_into().unwrap());
    let u16_at = |va: u32| u16::from_le_bytes(ram[o(va)..o(va) + 2].try_into().unwrap());
    let in_ram = |va: u32| (0x8000_0000..0x801F_FFF0).contains(&va);
    let mut nodes = Vec::new();
    for head in LIST_HEADS {
        let mut a = u32_at(head);
        let mut n = 0;
        while a != 0 && in_ram(a) && n < 600 {
            let flags = u16_at(a + 0x52);
            if u32_at(a + 0x0C) == PART_TICK && flags & 0x780 != 0 {
                let tri = |k: u32| [k, k + 2, k + 4].map(|x| u16_at(a + x) as i16);
                // Model -> first object -> (vertex pointer, vertex count).
                let quad =
                    (|| {
                        let model = u32_at(a + 0x44);
                        if !in_ram(model) {
                            return None;
                        }
                        let obj = u32_at(model + 4);
                        if !in_ram(obj) || u32_at(obj + 4) != 4 {
                            return None;
                        }
                        let verts = u32_at(obj);
                        if !in_ram(verts) {
                            return None;
                        }
                        Some([0u32, 1, 2, 3].map(|i| {
                            [0u32, 2, 4].map(|k| f32::from(u16_at(verts + i * 8 + k) as i16))
                        }))
                    })();
                nodes.push(Node {
                    flags,
                    pos: tri(0x14),
                    ang: tri(0x24),
                    view: [0x2C, 0x30, 0x34].map(|k| u32_at(a + k) as i32),
                    scale: f32::from(u16_at(a + 0x72)) / 4096.0,
                    quad,
                });
            }
            a = u32_at(a);
            n += 1;
        }
    }
    if nodes.is_empty() {
        return None;
    }
    let s16 = |off: usize| f32::from(i16::from_le_bytes([sp[off], sp[off + 1]]));
    let s32 = |off: usize| i32::from_le_bytes(sp[off..off + 4].try_into().unwrap()) as f32;
    let b = 0x3C8;
    let m = [0usize, 1, 2].map(|r| [0usize, 1, 2].map(|c| s16(b + (r * 3 + c) * 2) / 4096.0));
    let t = [s32(b + 0x14), s32(b + 0x18), s32(b + 0x1C)];
    let mut prims = Vec::new();
    for pool in prim_pool::find_pools(ram, 0x8000_0000, 64) {
        let bytes = &ram[o(pool.start)..o(pool.end)];
        prims.extend(
            prim_pool::decode(bytes, pool.start)
                .iter()
                .map(|p| p.bounds()),
        );
    }
    let label = path
        .file_name()
        .map(|f| f.to_string_lossy()[..8].to_string())
        .unwrap_or_default();
    let scene = &ram[o(0x8007_050C)..o(0x8007_050C) + 8];
    Some(Frame {
        label,
        overworld: scene.starts_with(b"map"),
        angles: [0x8007_B790, 0x8007_B792, 0x8007_B794].map(|va| u16_at(va) as i16),
        m,
        t,
        h: f32::from(u16_at(0x8007_B6F4)),
        nodes,
        prims,
    })
}

fn rad(a: i16) -> f32 {
    f32::from(a) / 4096.0 * std::f32::consts::TAU
}

/// Row-major `R` from the column-major `camera_rotation`.
fn full_rotation(p: f32, y: f32, r: f32) -> [[f32; 3]; 3] {
    let c = camera_rotation(p, y, r);
    [0, 1, 2].map(|row| [0, 1, 2].map(|col| c[col * 4 + row]))
}

fn mul_v(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| (0..3).map(|j| m[i][j] * v[j]).sum())
}

/// The host's eye-space point for a world point: `S_b * (R (w - focus) +
/// tr_unit)`.
fn host_eye(cam: &PartCameraPose, r: &[[f32; 3]; 3], w: [f32; 3]) -> [f32; 3] {
    let d = [0, 1, 2].map(|i| w[i] - cam.focus[i]);
    let e = mul_v(r, d);
    [0, 1, 2].map(|i| cam.base_scale * (e[i] + cam.tr_unit[i]))
}

#[test]
fn camera_relative_parts_land_where_retail_drew_them() {
    let Some(lib) = library() else {
        eprintln!("[skip] needs saves/library/mednafen (LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&lib)
        .expect("read library")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mcr"))
        .collect();
    paths.sort();

    let (mut frames, mut incoherent, mut locked, mut skewed) = (0, 0, 0, 0);
    let (mut quads, mut offscreen, mut bent, mut locked_seen, mut undrawn) = (0, 0, 0, 0, 0);
    let (mut control_hits, mut controls) = (0, 0);
    for path in &paths {
        let Some(f) = read_frame(path) else { continue };
        frames += 1;
        // Base scale from the camera matrix's row length (4x battle, 6x field).
        let s_b = (0..3)
            .map(|c| f.m[1][c] * f.m[1][c])
            .sum::<f32>()
            .sqrt()
            .round();
        assert!(s_b == 4.0 || s_b == 6.0, "{}: base scale {s_b}", f.label);
        let [p, y, r] = f.angles.map(rad);
        let cam = PartCameraPose {
            pitch: p,
            yaw: y,
            roll: r,
            focus: [0.0; 3],
            tr_unit: f.t.map(|c| c / s_b),
            base_scale: s_b,
        };

        // Leg 2: the host's full camera is the scratchpad matrix.
        let rot = full_rotation(p, y, r);
        for (i, (host_row, retail_row)) in rot.iter().zip(&f.m).enumerate() {
            for (j, (h, m)) in host_row.iter().zip(retail_row).enumerate() {
                assert!(
                    (s_b * h - m).abs() < 0.01 * s_b,
                    "{}: camera matrix [{i}][{j}] {m} vs host {}",
                    f.label,
                    s_b * h
                );
            }
        }

        // Leg 1: what `+0x14` is, read off retail's own `+0x2C`. A locked
        // node's `+0x2C` ignores the camera, so it must hold exactly; a skip
        // node's is the full camera at the node's own update, which is the
        // scratchpad matrix unless the camera moved between the two writes.
        let leg1 = |n: &Node| -> bool {
            let pos = n.pos.map(f32::from);
            let expect = if n.flags & 0x400 != 0 {
                pos.map(|c| s_b * c)
            } else {
                let e = mul_v(&f.m, pos);
                [0, 1, 2].map(|i| e[i] + f.t[i])
            };
            (0..3).all(|i| (expect[i] - n.view[i] as f32).abs() <= 2.0)
        };
        // A node spawned this frame has not been drawn yet, so its `+0x2C` is
        // whatever the recycled slot last held; those are counted, not
        // compared.
        for n in f.nodes.iter().filter(|n| n.flags & 0x400 != 0) {
            locked_seen += 1;
            if !leg1(n) {
                println!(
                    "{}: locked +0x14 {:?} with +0x2C {:?} - not drawn since spawn",
                    f.label, n.pos, n.view
                );
                undrawn += 1;
            }
        }
        let coherent = f.nodes.iter().filter(|n| n.flags & 0x400 == 0).all(leg1);
        if !coherent {
            println!(
                "{}: camera moved between the node and view snapshots",
                f.label
            );
            incoherent += 1;
        }

        for n in &f.nodes {
            let pos = n.pos.map(f32::from);
            let view = n.view.map(|v| v as f32);
            if n.flags & 0x400 != 0 {
                if !leg1(n) {
                    continue;
                }
                locked += 1;
            } else if coherent {
                skewed += 1;
            } else {
                continue;
            }

            // Leg 3a: the kernel's placement lands on retail's `+0x2C`. The
            // host's f32 rotation differs from retail's 12-bit matrix in the
            // fourth digit, which the overworld's far coordinates amplify.
            let CameraRelativePart { pos: placed, basis } =
                camera_relative_part(n.flags, pos, &cam).expect("0x780 bit => a prefix");
            let eye = host_eye(&cam, &rot, placed);
            let scale = view.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            for i in 0..3 {
                assert!(
                    (eye[i] - view[i]).abs() <= 2.0 + s_b + 0.002 * scale,
                    "{}: flags {:04X}: host eye {eye:?} vs retail +0x2C {:?}",
                    f.label,
                    n.flags,
                    n.view
                );
            }

            // Leg 3b: a single-quad part's corners, projected, are a retail
            // packet. Not on the overworld, whose curvature bend is applied
            // after the projection (`FUN_800271A8`).
            let Some(quad) = n.quad else { continue };
            if f.overworld {
                bent += 1;
                continue;
            }
            let node_rot = camera_view_rotation(0, rad(n.ang[0]), rad(n.ang[1]), rad(n.ang[2]))
                .expect("Euler matrix");
            let nm = node_rot
                .m
                .map(|row| row.map(|e| f32::from(e) / 4096.0 * n.scale));
            // Project the quad with part basis `k` placed at `at`, and look for
            // a retail packet with the same screen bounds.
            let packet_at = |k: &[[f32; 3]; 3], at: [f32; 3]| {
                let corners = quad.map(|v| {
                    let local = mul_v(k, mul_v(&nm, v));
                    let w = [0, 1, 2].map(|i| at[i] + local[i]);
                    let e = host_eye(&cam, &rot, w);
                    (OFX + f.h * e[0] / e[2], OFY + f.h * e[1] / e[2])
                });
                let b = (
                    corners.iter().map(|c| c.0).fold(f32::MAX, f32::min),
                    corners.iter().map(|c| c.1).fold(f32::MAX, f32::min),
                    corners.iter().map(|c| c.0).fold(f32::MIN, f32::max),
                    corners.iter().map(|c| c.1).fold(f32::MIN, f32::max),
                );
                // Each edge within 1.5 px, and the extent within 1.5 px - the
                // extent is what separates the six-fold literal from the base.
                let hit = f.prims.iter().any(|&(x0, y0, x1, y1)| {
                    let (x0, y0, x1, y1) = (x0.into(), y0.into(), x1.into(), y1.into());
                    [(x0, b.0), (y0, b.1), (x1, b.2), (y1, b.3)]
                        .iter()
                        .all(|(r, p): &(f32, f32)| (r - p).abs() <= 1.5)
                        && ((x1 - x0) - (b.2 - b.0)).abs() <= 1.5
                        && ((y1 - y0) - (b.3 - b.1)).abs() <= 1.5
                });
                (b, hit)
            };
            let ((bx0, by0, bx1, by1), hit) = packet_at(&basis, placed);
            // The GPU drawing area is 320x224; a part projected outside it
            // leaves no packet to compare against.
            if bx0 < 0.0 || by0 < 0.0 || bx1 > 320.0 || by1 > 224.0 {
                offscreen += 1;
                continue;
            }
            assert!(
                hit,
                "{}: flags {:04X} quad projects to x[{bx0:.1},{bx1:.1}] y[{by0:.1},{by1:.1}], \
                 no retail packet there",
                f.label, n.flags
            );
            // Controls: the same part drawn as a flag-free node (no prefix,
            // `+0x14` as a world point), and the prefix with the base scale
            // in place of the six-fold literal, must each miss.
            let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            controls += 1;
            if packet_at(&ident, pos).1 {
                control_hits += 1;
            }
            if n.flags & 0x400 == 0 {
                let k = s_b / 6.0;
                controls += 1;
                if packet_at(&basis.map(|r| r.map(|e| e * k)), placed).1 {
                    control_hits += 1;
                }
            }
            quads += 1;
        }
    }
    println!(
        "[ran] {frames} states ({incoherent} with a moved camera): {skewed} skip-arm + {locked} \
         locked parts placed; {quads} on-screen single-quad parts matched a retail packet \
         ({offscreen} off-screen, {bent} overworld-bent); {control_hits} of {controls} \
         control compositions matched"
    );
    assert!(
        frames > 0,
        "no library state carries a camera-relative part"
    );
    assert!(
        incoherent * 4 <= frames,
        "most states' camera snapshots disagree"
    );
    assert!(
        undrawn * 20 <= locked_seen,
        "{undrawn} of {locked_seen} locked parts disagree with S_b * +0x14"
    );
    assert!(quads > 0, "no single-quad part was frame-compared");
    assert_eq!(
        control_hits, 0,
        "a flag-free or base-scaled composition also matched a retail packet"
    );
}
