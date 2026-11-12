//! The TMD renderer `FUN_8002735C` over every object of every Legaia TMD the
//! raw PROT entries carry, registered by `FUN_800268DC` first, against the
//! primitive decode the port draws from (`legaia_tmd::legaia_prims::iter_groups`
//! through `descriptor::Descriptor::for_flags`): for each primitive the
//! renderer reaches, the corners it loads into the GTE. The renderer reads a
//! group's row of the descriptor table `DAT_8007326C` for the index offset
//! (`0x8002744C..0x800274F0` for quads, the row's byte 4 for triangles), so a
//! corner read from the wrong word of the primitive shows here.
//!
//! The one known split is pinned rather than skipped: on the light-source
//! rows' quads (descriptor rows 0 / 1, byte 3 outside `0..=3`) this renderer
//! takes `byte4 + 2`, which lands on the normal block; the port reads the
//! four vertices at byte `12`, where the packets and the light-source
//! renderer `FUN_80029888` (which the field sends those rows to) put them.
//!
//! The packet calls (`FUN_8003D1A4`, `FUN_80027F00`, `FUN_80027C6C`) are
//! stubbed; the corners are read by probes at the vertex loads, ahead of the
//! back-face test.

use crate::common::{disc, scus_cpu};
use legaia_container_formats::tmd_scan::scan_buffer;
use legaia_tmd::descriptor::Descriptor;
use legaia_tmd::legaia_prims::iter_groups_lenient;
use psx_retail_exec::Stop;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

const RENDER: u32 = 0x8002_735C;
const REGISTER: u32 = 0x8002_68DC;
const QUAD_LOAD: u32 = 0x8002_751C;
const FOURTH_LOAD: u32 = 0x8002_7598;
const TRI_LOAD: u32 = 0x8002_78EC;
const TMD: u32 = 0x8010_0000;
const S2: usize = 18;
const S5: usize = 21;
const S6: usize = 22;

/// What the renderer loaded for one primitive: the index word offset it
/// used and the corners, as byte offsets into the vertex pool.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Load {
    offset: u32,
    corners: Vec<u32>,
}

#[test]
fn the_renderer_loads_the_corners_the_port_decodes() {
    differential!(scus(RENDER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for va in [0x8003_D1A4, 0x8002_7F00, 0x8002_7C6C] {
        cpu.stub(va, 0);
    }
    let loads: Rc<RefCell<BTreeMap<u32, Load>>> = Rc::default();
    for va in [QUAD_LOAD, TRI_LOAD] {
        let l = loads.clone();
        cpu.probe(va, move |r, _, _| {
            let base = r.r[S6];
            let corners = [4usize, 3, 2]
                .iter()
                .map(|&k| r.r[k].wrapping_sub(base))
                .collect();
            l.borrow_mut().insert(
                r.r[S2] - TMD,
                Load {
                    offset: r.r[S5] * 2,
                    corners,
                },
            );
        });
    }
    let l = loads.clone();
    cpu.probe(FOURTH_LOAD, move |r, _, _| {
        let prim = r.r[S2] - TMD;
        let v = r.r[2].wrapping_sub(r.r[S6]);
        l.borrow_mut()
            .get_mut(&prim)
            .expect("the quad's first three")
            .corners
            .push(v);
    });
    cpu.step_budget = 50_000_000;

    let (mut tmds, mut prims, mut fourths, mut lit_quads) = (0usize, 0usize, 0usize, 0usize);
    let mut differ: BTreeMap<u16, (usize, String)> = BTreeMap::new();
    for entry in 0..1233 {
        let Ok(buf) = d.read_entry(entry) else {
            continue;
        };
        for hit in scan_buffer(&buf).into_iter().take(64) {
            let bytes = &buf[hit.offset..hit.offset + hit.byte_len];
            if bytes.len() > 0xF_0000 {
                continue;
            }
            let Ok(tmd) = legaia_tmd::parse(bytes) else {
                continue;
            };
            cpu.mem.load(TMD, bytes);
            assert_eq!(cpu.call(REGISTER, &[TMD]), Stop::Returned);
            for (k, o) in tmd.objects.iter().enumerate() {
                let obj = TMD + 12 + 0x1C * k as u32;
                if cpu.mem.rd32(obj + 0x14) == 0 {
                    continue;
                }
                loads.borrow_mut().clear();
                let stop = cpu.call(RENDER, &[obj, 0, 0]);
                assert_eq!(stop, Stop::Returned, "entry {entry} object {k}");
                let got = loads.borrow().clone();
                let groups = iter_groups_lenient(
                    bytes,
                    o.primitives_byte_offset,
                    bytes.len() - o.primitives_byte_offset,
                );
                for g in &groups {
                    let Some(desc) = Descriptor::for_flags(g.header.flags) else {
                        continue;
                    };
                    for p in &g.prims {
                        let Some(load) = got.get(&(p.bytes_offset as u32)) else {
                            continue;
                        };
                        prims += 1;
                        let port: Vec<u32> =
                            p.vertex_indices_raw.iter().map(|&v| u32::from(v)).collect();
                        let lit_quad = desc.is_quad && desc.raw_byte3 > 3;
                        if lit_quad {
                            // The pinned split: this renderer's `byte4 + 2`.
                            assert_eq!(
                                load.offset,
                                (u32::from(desc.raw_byte4) + 2) * 2,
                                "flags {:#x}",
                                g.header.flags
                            );
                            assert_eq!(desc.vertex_offset, 12);
                            lit_quads += 1;
                            continue;
                        }
                        fourths += usize::from(load.corners.len() == 4);
                        let n = load.corners.len();
                        if load.offset as usize != desc.vertex_offset
                            || load.corners[..] != port[..n]
                        {
                            let e = differ.entry(g.header.flags).or_default();
                            e.0 += 1;
                            if e.0 == 1 {
                                e.1 = format!(
                                    "entry {entry} object {k} prim {:#x}: retail +{:#x} {:x?}, port +{:#x} {:x?}",
                                    p.bytes_offset,
                                    load.offset,
                                    load.corners,
                                    desc.vertex_offset,
                                    port
                                );
                            }
                        }
                    }
                }
            }
            tmds += 1;
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(f, (n, ex))| format!("flags {f:#x}: {n}, e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(
        tmds > 900 && prims > 100_000 && fourths > 50_000 && lit_quads > 100,
        "{tmds} {prims} {fourths} {lit_quads}"
    );
}
