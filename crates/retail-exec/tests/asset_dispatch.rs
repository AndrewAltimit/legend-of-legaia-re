//! The asset dispatcher `FUN_8001F05C` off the disc with its registrars live
//! beneath it - the raw copy `FUN_8001A8B0`, the TMD registrar `FUN_80026B4C`
//! and the VDF registrar `FUN_8001FBCC` - against the container layer and the
//! scene loader.
//!
//! Retail turns a chunk into a heap buffer plus, for the pack types, a
//! pointer table; the port keeps the decoded body and indexes it on demand.
//! What both must agree on is what a consumer then reads: the bytes
//! `decode` returns, the members `parse_pack` finds, the record `vdf_entry`
//! opens, and the order models enter the pool (`SceneModelBank`).

use crate::common::{Rng, disc, scus_cpu};
use crate::walk_on::index;
use legaia_container_formats::pack::parse_pack;
use legaia_container_formats::{AssetType, DecodeMode, Descriptor, decode};
use legaia_disc_audit::categorize::Class;
use legaia_engine_scene::model_bank::{ModelSource, SceneModelBank};
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene::scene_bundle::{find_bundle, find_vdf_buffer, vdf_entry};
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const DISPATCH: u32 = 0x8001_F05C;
const RAW_COPY: u32 = 0x8001_A8B0;
const TMD_REGISTER: u32 = 0x8002_6B4C;
const VDF_REGISTER: u32 = 0x8001_FBCC;
const PAIRS: u32 = 0x8002_0224;
const DRIVER: u32 = 0x8002_541C;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const UPLOAD: u32 = 0x8001_98E0;
/// `DAT_8007C018` and its counter, `0x80083E58` and its counter.
const TMD_POOL: u32 = 0x8007_C018;
const TMD_COUNT: u32 = 0x8007_B774;
const VDF_TABLE: u32 = 0x8008_3E58;
const VDF_COUNT: u32 = 0x8007_B7EC;
/// The load buffer the walkers read, `_DAT_8007B85C`.
const BUFFER_PTR: u32 = 0x8007_B85C;
const ERROR_WORD: u32 = 0x8007_B828;
const SRC: u32 = 0x800B_0000;
const ARENA_END: u32 = 0x801F_0000;

/// Where each buffer type's pointer lands.
fn install_global(ty: u8) -> Option<u32> {
    Some(match ty {
        3 => 0x8007_B898,
        4 => 0x8007_B8A8,
        5 => 0x8007_B888,
        6 => 0x8007_B7C8,
        7 => 0x8007_B7DC,
        8 => 0x8007_BA00,
        0xB => 0x8007_B840,
        _ => return None,
    })
}

#[derive(Default)]
struct Heap {
    next: u32,
    /// `(pointer, size)` of every live block, newest last.
    blocks: Vec<(u32, u32)>,
    allocs: u32,
    full: bool,
    uploads: Vec<u32>,
    frees: Vec<u32>,
}

/// A CPU whose allocator is a stack of blocks above `Heap::next`.
fn dispatch_cpu() -> Option<(Cpu, Rc<RefCell<Heap>>)> {
    let mut cpu = scus_cpu()?;
    let heap: Rc<RefCell<Heap>> = Rc::default();
    for va in [
        0x8005_67A8, // printf
        0x8005_8104, // cache flush
        0x8005_FB84, // VSync
        0x8003_DE7C, // CD wait
        0x8003_D26C, // block copy
    ] {
        cpu.stub(va, 0);
    }
    {
        let heap = heap.clone();
        cpu.hook(ALLOC, move |r, _| {
            let mut h = heap.borrow_mut();
            let size = r.r[A1].next_multiple_of(4);
            h.allocs += 1;
            if h.next + size > ARENA_END {
                h.full = true;
                r.r[V0] = 0;
                return;
            }
            let p = h.next;
            h.next += size;
            h.blocks.push((p, size));
            r.r[V0] = p;
        });
    }
    {
        let heap = heap.clone();
        cpu.hook(FREE, move |r, _| {
            let mut h = heap.borrow_mut();
            let p = r.r[A0];
            h.frees.push(p);
            if h.blocks.last().is_some_and(|b| b.0 == p) {
                h.blocks.pop();
                h.next = p;
            }
            r.r[V0] = 0;
        });
    }
    {
        let heap = heap.clone();
        cpu.hook(UPLOAD, move |r, _| {
            heap.borrow_mut().uploads.push(r.r[A0]);
            r.r[V0] = 0;
        });
    }
    Some((cpu, heap))
}

fn reset(cpu: &mut Cpu, heap: &Rc<RefCell<Heap>>, src_len: usize) {
    *heap.borrow_mut() = Heap {
        next: (SRC + src_len as u32 + 0x100).next_multiple_of(0x100),
        ..Heap::default()
    };
    cpu.mem.wr32(TMD_COUNT, 0);
    cpu.mem.wr32(VDF_COUNT, 0);
    cpu.mem.wr32(ERROR_WORD, 0);
}

/// A pack: `count`, `count` offsets, then the members. `word_offsets` picks
/// the TIM / TMD packs' word units over the VDF's byte units.
fn pack(members: &[Vec<u8>], word_offsets: bool) -> Vec<u8> {
    let mut out = (members.len() as u32).to_le_bytes().to_vec();
    let mut at = 4 + 4 * members.len();
    for m in members {
        let off = if word_offsets { at / 4 } else { at };
        out.extend_from_slice(&(off as u32).to_le_bytes());
        at += m.len();
    }
    for m in members {
        out.extend_from_slice(m);
    }
    out
}

/// A TMD the registrar accepts: the magic, clear flags, `n` objects with no
/// pools.
fn tiny_tmd(rng: &mut Rng) -> Vec<u8> {
    let n = rng.below(3);
    let mut t = Vec::new();
    t.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    t.extend_from_slice(&n.to_le_bytes());
    for _ in 0..n {
        // Vertex / normal / primitive tops and counts, then the scale.
        for w in [0u32, 0, 0, 0, 0, 0, rng.below(4)] {
            t.extend_from_slice(&w.to_le_bytes());
        }
    }
    t
}

#[test]
fn every_dispatch_arm_installs_what_the_port_decodes() {
    differential!(scus(DISPATCH));
    let Some((mut cpu, heap)) = dispatch_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4153_4554);
    let mut arms = [0u32; 0x18];
    for round in 0..6_000 {
        let ty = if rng.below(4) == 0 {
            rng.below(0x18) as u8
        } else {
            [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xB][rng.below(11) as usize]
        };
        let raw = rng.below(2) == 1;
        // What the chunk decodes to.
        let members: Vec<Vec<u8>> = (0..rng.below(6))
            .map(|_| match ty {
                2 => tiny_tmd(&mut rng),
                _ => (0..4 * (1 + rng.below(20)))
                    .map(|_| rng.below(7) as u8)
                    .collect(),
            })
            .collect();
        let body = match ty {
            1 | 2 => pack(&members, true),
            7 => pack(&members, false),
            9 => tiny_tmd(&mut rng),
            _ => (0..4 * (1 + rng.below(40)))
                .map(|_| rng.below(5) as u8)
                .collect(),
        };
        let stored = if raw {
            body.clone()
        } else {
            psx_lzs::compress(&body)
        };
        let desc = Descriptor {
            type_byte: ty,
            size: body.len() as u32,
            data_offset: 0,
        };
        let header = (u32::from(ty) << 24) | desc.size;
        cpu.mem.load(SRC, &stored);
        reset(&mut cpu, &heap, stored.len());
        for ty in 0..0x18 {
            if let Some(g) = install_global(ty) {
                cpu.mem.wr32(g, 0);
            }
        }
        assert_eq!(
            cpu.call(DISPATCH, &[SRC, header, 0, u32::from(raw)]),
            Stop::Returned,
            "round {round}"
        );
        let ret = cpu.regs.r[V0];
        let h = std::mem::take(&mut *heap.borrow_mut());
        assert!(!h.full, "round {round}: arena");
        let ctx = format!("round {round} type {ty:#x} raw {raw}");
        arms[usize::from(ty)] += 1;

        let kind = AssetType::from_byte(ty);
        let mode = if raw {
            DecodeMode::Raw
        } else {
            DecodeMode::Lzs
        };
        // The flag types and the unknown ones touch nothing.
        match kind {
            AssetType::Flag(b) => {
                assert_eq!(ret, u32::from(b) << 8, "{ctx}: flag return");
                assert_eq!(h.allocs, 0, "{ctx}");
                continue;
            }
            AssetType::Unknown(_) => {
                assert_eq!((ret, h.allocs), (0, 0), "{ctx}");
                continue;
            }
            _ => assert!(kind.has_data(), "{ctx}"),
        }
        // The accumulator bit each data type raises
        // (`docs/formats/asset-type.md`); a lone TIM, SIN and the single
        // TMD raise none.
        let bit = match ty {
            1 => 0x01,
            2 => 0x02,
            3 => 0x04,
            4 => 0x08,
            5 | 0xB => 0x10,
            6 => 0x20,
            7 => 0x40,
            _ => 0,
        };
        assert_eq!(ret, bit, "{ctx}: return");
        let port = decode(&stored, &desc, mode).expect("decode");
        assert_eq!(port, body, "{ctx}: the port's own decode");
        match kind {
            AssetType::Tim => {
                // Uploaded from the source in place, or from a scratch block
                // the arm frees again.
                assert_eq!(h.uploads.len(), 1, "{ctx}");
                assert!(cpu.mem.read(h.uploads[0], port.len()) == port, "{ctx}: TIM");
                assert_eq!(h.uploads[0] == SRC, raw, "{ctx}");
                // The decoder's ring buffer is the other block freed.
                assert_eq!(
                    h.frees.last().copied(),
                    (!raw).then_some(h.uploads[0]),
                    "{ctx}"
                );
                assert!(h.blocks.is_empty(), "{ctx}: a TIM leaves no block");
            }
            AssetType::TimList => {
                let base = if raw {
                    SRC
                } else {
                    *h.frees.last().expect("scratch")
                };
                assert!(h.blocks.is_empty(), "{ctx}: a TIM pack leaves no block");
                let retail: Vec<usize> = h.uploads.iter().map(|p| (p - base) as usize).collect();
                let port_members: Vec<usize> = parse_pack(&port)
                    .map(|e| e.iter().map(|e| e.byte_offset).collect())
                    .unwrap_or_default();
                assert_eq!(retail, port_members, "{ctx}: TIM pack members");
            }
            AssetType::Tmd | AssetType::Tmd2 => {
                let base = h.blocks[0].0;
                let n = cpu.mem.rd32(TMD_COUNT);
                let retail: Vec<usize> = (0..n)
                    .map(|i| (cpu.mem.rd32(TMD_POOL + 4 * i) - base) as usize)
                    .collect();
                let port_members: Vec<usize> = if kind == AssetType::Tmd {
                    parse_pack(&port)
                        .map(|e| e.iter().map(|e| e.byte_offset).collect())
                        .unwrap_or_default()
                } else {
                    vec![0]
                };
                assert_eq!(retail, port_members, "{ctx}: registered models");
                // Each model's flags word is raised and its pool offsets are
                // pointers; taken back, the body is the port's.
                let mut retail_body = cpu.mem.read(base, port.len());
                for &m in &port_members {
                    assert_eq!(retail_body[m + 4], 1, "{ctx}: flags");
                    let own = unregister(&mut cpu, base + m as u32, port.len() - m);
                    retail_body[m..].copy_from_slice(&own);
                }
                assert!(retail_body == port, "{ctx}: TMD body");
                assert_eq!(cpu.mem.rd32(ERROR_WORD), 0, "{ctx}");
            }
            _ => {
                let global = install_global(ty).expect("a buffer type");
                let p = cpu.mem.rd32(global);
                assert_eq!(p, h.blocks[0].0, "{ctx}: installed pointer");
                assert!(cpu.mem.read(p, port.len()) == port, "{ctx}: installed body");
                if kind == AssetType::Vdf {
                    let n = cpu.mem.rd32(VDF_COUNT);
                    assert_eq!(n as usize, members.len(), "{ctx}: VDF records");
                    for i in 0..n {
                        let retail = (cpu.mem.rd32(VDF_TABLE + 4 * i) - p) as usize;
                        let rec = vdf_entry(&port, i as u8).expect("record");
                        let at = rec.as_ptr() as usize - port.as_ptr() as usize;
                        assert_eq!(retail, at, "{ctx}: VDF record {i}");
                    }
                }
            }
        }
    }
    assert!(arms.iter().all(|&n| n > 30), "{arms:?}");
}

#[test]
fn the_raw_copy_is_the_raw_decode() {
    differential!(scus(RAW_COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const DST: u32 = 0x8012_0000;
    let mut rng = Rng(0x5241_5743);
    let mut empty = 0;
    for round in 0..2_000 {
        let n = if rng.below(8) == 0 {
            empty += 1;
            0
        } else {
            1 + rng.below(300)
        };
        let skip = rng.below(8);
        let src: Vec<u8> = (0..skip + n + 8).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SRC, &src);
        cpu.mem.load(DST, &vec![0xA5; n as usize + 16]);
        assert_eq!(cpu.call(RAW_COPY, &[DST, SRC + skip, n]), Stop::Returned);
        let desc = Descriptor {
            type_byte: 0,
            size: n,
            data_offset: skip,
        };
        let mut port = decode(&src, &desc, DecodeMode::Raw).expect("decode");
        port.extend_from_slice(&[0xA5; 16]);
        assert!(cpu.mem.read(DST, port.len()) == port, "round {round}");
    }
    assert!(empty > 100);
}

#[test]
fn the_registrars_number_models_and_records_in_call_order() {
    differential!(scus(TMD_REGISTER), scus(VDF_REGISTER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(0x8005_67A8, 0);
    let mut rng = Rng(0x5245_4749);
    for round in 0..300 {
        // Models: the counter stands where the player pack left it, and the
        // n-th model registered is scene-bank id n.
        let base = u32::from(legaia_engine_scene::model_bank::SCENE_BANK_BASE);
        cpu.mem.wr32(TMD_COUNT, base);
        let mut at = SRC;
        for id in 0..1 + rng.below(0xE0) {
            let tmd = tiny_tmd(&mut rng);
            cpu.mem.load(at, &tmd);
            assert_eq!(cpu.call(TMD_REGISTER, &[at]), Stop::Returned);
            let slot = cpu.regs.r[V0];
            let port = legaia_engine_scene::model_bank::resolve_model_id(id as i16);
            assert_eq!(slot, u32::from(port.pool_index), "round {round}: id {id}");
            assert_eq!(cpu.mem.rd32(TMD_POOL + 4 * slot), at, "round {round}");
            // `gp + 0x820`, the id just issued.
            assert_eq!(cpu.mem.rd32(0x8007_BB38), slot, "round {round}");
            at += tmd.len() as u32;
        }
        assert_eq!(cpu.mem.rd32(ERROR_WORD), 0, "round {round}");

        // Records: the dispatcher's type-7 loop hands the registrar
        // `buffer + offset[i]` for each record in table order.
        let members: Vec<Vec<u8>> = (0..rng.below(12))
            .map(|_| {
                (0..4 * (1 + rng.below(20)))
                    .map(|_| rng.next() as u8)
                    .collect()
            })
            .collect();
        let vdf = pack(&members, false);
        cpu.mem.load(SRC, &vdf);
        cpu.mem.wr32(VDF_COUNT, 0);
        for i in 0..cpu.mem.rd32(SRC) {
            let off = cpu.mem.rd32(SRC + 4 + 4 * i);
            assert_eq!(cpu.call(VDF_REGISTER, &[SRC + off]), Stop::Returned);
            // `gp + 0x6FC`, the record just issued.
            assert_eq!(cpu.mem.rd32(0x8007_BA14), i, "round {round}");
        }
        assert_eq!(cpu.mem.rd32(VDF_COUNT) as usize, members.len());
        for (i, m) in members.iter().enumerate() {
            let retail = (cpu.mem.rd32(VDF_TABLE + 4 * i as u32) - SRC) as usize;
            let rec = vdf_entry(&vdf, i as u8).expect("record");
            assert_eq!(retail, rec.as_ptr() as usize - vdf.as_ptr() as usize);
            assert!(rec == &m[..], "round {round}: record {i}");
        }
    }
}

/// The object table's pointer words of a registered TMD made offsets again,
/// and the flags word cleared.
fn unregister(cpu: &mut Cpu, at: u32, len: usize) -> Vec<u8> {
    let mut b = cpu.mem.read(at, len);
    b[4..8].copy_from_slice(&0u32.to_le_bytes());
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    for k in 0..n {
        for field in [0usize, 8, 0x10] {
            let o = 12 + 0x1C * k + field;
            let Some(w) = b.get(o..o + 4) else {
                return b;
            };
            let p = u32::from_le_bytes(w.try_into().unwrap());
            b[o..o + 4].copy_from_slice(&p.wrapping_sub(at + 12).to_le_bytes());
        }
    }
    b
}

#[test]
fn a_scene_registers_its_models_in_the_bank_order() {
    differential!(scus(PAIRS), scus(DRIVER));
    let (Some((mut cpu, heap)), Some(_)) = (dispatch_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // A whole bundle's LZS decodes run inside one call.
    cpu.step_budget = 2_000_000_000;
    let index = index();
    let (mut scenes, mut models, mut skipped, mut vdfs, mut stream_models) = (0, 0, 0, 0, 0);
    let mut stacked = 0;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let bank = SceneModelBank::build(&scene);
        if bank.is_empty() {
            continue;
        }
        // The descriptor table the block's models ride, where it has one.
        let table = match (find_bundle(&scene), bank.sources()[0]) {
            (Some(b), _) => Some((b.entry_idx(), b.table_offset())),
            (None, ModelSource::WalkedPack { entry_idx, .. }) => Some((entry_idx, 0)),
            _ => None,
        };
        let mut src_len = 0usize;
        let mut loads: Vec<(u32, usize, bool)> = Vec::new();
        if let Some((idx, off)) = table {
            let e = scene.entries.iter().find(|e| e.idx == idx).expect("entry");
            cpu.mem.load(SRC, &e.bytes[off..]);
            src_len = e.bytes.len() - off;
            loads.push((SRC, src_len, true));
        }
        for e in &scene.entries {
            if table.is_some_and(|t| t.0 == e.idx) || e.class != Class::DataFieldStreaming {
                continue;
            }
            let at = (SRC + src_len as u32 + 0x10).next_multiple_of(0x10);
            if at as usize + e.bytes.len() > 0x8018_0000 {
                src_len = usize::MAX;
                break;
            }
            cpu.mem.load(at, &e.bytes);
            loads.push((at, e.bytes.len(), false));
            src_len = (at - SRC) as usize + e.bytes.len();
        }
        if src_len == usize::MAX {
            skipped += 1;
            continue;
        }
        reset(&mut cpu, &heap, src_len);
        cpu.mem.wr32(0x8007_B7DC, 0);
        let mut ok = true;
        for &(at, len, is_table) in &loads {
            cpu.mem.wr32(BUFFER_PTR, at);
            let stop = if is_table {
                cpu.call(PAIRS, &[0])
            } else {
                cpu.stub(0x8002_55B8, len as u32);
                cpu.call(DRIVER, &[0x14])
            };
            ok &= stop == Stop::Returned && !heap.borrow().full;
        }
        if !ok {
            skipped += 1;
            continue;
        }
        let n = cpu.mem.rd32(TMD_COUNT) as usize;
        assert_eq!(n, bank.len(), "{name}: models registered");
        for i in 0..n {
            let port = bank.tmd_bytes(&scene, i as i16).expect("model bytes");
            let p = cpu.mem.rd32(TMD_POOL + 4 * i as u32);
            let retail = unregister(&mut cpu, p, port.len());
            // The port's slice runs to the next member; a registered model
            // is held to its own extent.
            let own = legaia_container_formats::tmd_scan::scan_buffer(&port)
                .first()
                .filter(|h| h.offset == 0)
                .map_or(port.len(), |h| h.byte_len.min(port.len()));
            assert!(retail[..own] == port[..own], "{name}: model {i}");
            if matches!(
                bank.sources()[i],
                ModelSource::StreamPack { .. } | ModelSource::StreamSingle { .. }
            ) {
                stream_models += 1;
            }
        }
        models += n;
        // The VDF buffer and its record table.
        let vdf_count = cpu.mem.rd32(VDF_COUNT);
        match find_vdf_buffer(&scene) {
            Some(port) => {
                let p = cpu.mem.rd32(0x8007_B7DC);
                if cpu.mem.read(p, port.len()) != port {
                    for e in &scene.entries {
                        if let Ok(r) = legaia_container_formats::parse_streaming(&e.bytes, 4096) {
                            for c in r.chunks.iter().filter(|c| c.type_byte == 7) {
                                eprintln!(
                                    "{name}: VDF chunk entry {} class {:?} at {:#x} size {:#x} table {:?}",
                                    e.idx, e.class, c.header_offset, c.size, table
                                );
                            }
                        }
                    }
                    panic!("{name}: VDF body");
                }
                // The record table runs on across installs (nothing but a
                // stage init rewinds its counter); the buffer a reader
                // indexes is the last one, and its records close the table.
                let count = u32::from_le_bytes(port[..4].try_into().unwrap());
                assert!(vdf_count >= count, "{name}: VDF records");
                if vdf_count > count {
                    stacked += 1;
                }
                for i in 0..count.min(255) {
                    let slot = VDF_TABLE + 4 * (vdf_count - count + i);
                    let retail = (cpu.mem.rd32(slot) - p) as usize;
                    let rec = vdf_entry(&port, i as u8).expect("record");
                    assert_eq!(
                        retail,
                        rec.as_ptr() as usize - port.as_ptr() as usize,
                        "{name}: VDF record {i}"
                    );
                }
                vdfs += 1;
            }
            None => assert_eq!(vdf_count, 0, "{name}: a VDF the port did not find"),
        }
        scenes += 1;
    }
    eprintln!(
        "scenes {scenes} models {models} stream {stream_models} vdfs {vdfs} stacked {stacked} skipped {skipped}"
    );
    assert!(
        scenes > 80 && models > 5_000 && vdfs >= 5 && stream_models > 100,
        "scenes {scenes} models {models} stream {stream_models} vdfs {vdfs} stacked {stacked} skipped {skipped}"
    );
}
