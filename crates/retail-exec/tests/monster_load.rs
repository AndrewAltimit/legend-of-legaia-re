//! The battle monster loader `FUN_80055468` off the disc, over every monster
//! of the archive (extraction `0867`) at every battle slot, its `LoadImage`
//! answered by a hook that keeps each rectangle and its pixels, against
//! `MonsterMesh::battle_render_mesh`: the texture page and the CLUT row
//! compared as VRAM (the port's `Vram` read back over the rectangle retail
//! uploads), and the relocated primitives compared as the mesh the port draws
//! (`cba_tsb` per vertex, from the bytes retail left against the port's own
//! relocation). The upload gate on the acting seat's queue byte is run both
//! ways.

use crate::common::{disc, scus_cpu};
use legaia_actor_formats::monster_archive::mesh;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const LOAD: u32 = 0x8005_5468;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const DRAW_SYNC: u32 = 0x8005_8104;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8010_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8010_1000;
const BLOCK: u32 = 0x8012_0000;

type Upload = ([u16; 4], Vec<u8>);

/// The block with each object's primitive pointer absolute, as the battle
/// registrar leaves a resident TMD.
fn absolutize(block: &mut [u8], tmd_at: usize, base: u32) {
    let rd = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let nobj = rd(block, tmd_at + 8) as usize;
    for k in 0..nobj {
        let at = tmd_at + 12 + 0x1C * k + 0x10;
        let rel = rd(block, at);
        let abs = base + (tmd_at + 12) as u32 + rel;
        block[at..at + 4].copy_from_slice(&abs.to_le_bytes());
    }
}

#[test]
fn the_monster_loader_matches_battle_render_mesh() {
    differential!(scus(LOAD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    let archive = disc.read_entry(867).expect("monster archive");
    let uploads: Rc<RefCell<Vec<Upload>>> = Rc::default();
    {
        let uploads = uploads.clone();
        cpu.hook(LOAD_IMAGE, move |r, m| {
            let rect = [0, 2, 4, 6].map(|k| m.rd16(r.r[A0] + k));
            let len = usize::from(rect[2]) * usize::from(rect[3]) * 2;
            let bytes = m.read(r.r[A1], len);
            uploads.borrow_mut().push((rect, bytes));
        });
    }
    cpu.stub(DRAW_SYNC, 0);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr8(CTX + 0x13, 0);
    cpu.mem.wr32(ACTORS, ACTOR);
    let (mut monsters, mut gated, mut prims) = (0usize, 0usize, 0usize);
    for id in 1..=0xFFu16 {
        let Ok(Some(m)) = mesh(&archive, id) else {
            continue;
        };
        let Some(pool) = m.texture_pool_bytes() else {
            continue;
        };
        let page_words = (pool.len() - 0x1E0) / 256 / 2;
        let wide = match page_words {
            0x20 => false,
            0x40 => true,
            _ => continue,
        };
        for slot in 0..5u8 {
            let skip = slot == 4;
            cpu.mem.wr8(ACTOR + 0x1DF, if skip { 0x9A } else { 0 });
            let mut block = m.block.clone();
            absolutize(&mut block, m.tmd_offset, BLOCK);
            cpu.mem.load(BLOCK, &block);
            uploads.borrow_mut().clear();
            let pool_at = BLOCK + m.texture_pool_offset as u32;
            let tmd_at = BLOCK + m.tmd_offset as u32;
            let args = [pool_at, tmd_at, u32::from(wide), u32::from(slot)];
            assert_eq!(cpu.call(LOAD, &args), Stop::Returned);
            let ctx = format!("monster {id:#x} slot {slot}");

            let mut vram = psx_tim::Vram::new();
            let port = m.battle_render_mesh(slot, &mut vram).expect("mesh");

            // The uploads, as VRAM.
            let ups = uploads.borrow();
            if skip {
                assert!(ups.is_empty(), "the gate skips the uploads, {ctx}");
                gated += 1;
            } else {
                assert_eq!(ups.len(), 2, "page then CLUT row, {ctx}");
                for (rect, bytes) in ups.iter() {
                    let [x, y, w, h] = rect.map(usize::from);
                    let mut port_bytes = Vec::with_capacity(bytes.len());
                    for row in y..y + h {
                        for col in x..x + w {
                            port_bytes.extend(vram.pixel(col, row).to_le_bytes());
                        }
                    }
                    let first = bytes
                        .chunks(2)
                        .zip(port_bytes.chunks(2))
                        .position(|(a, b)| a != b)
                        .map(|k| (k % w, k / w));
                    assert!(
                        first.is_none(),
                        "upload {rect:?} differs first at (x+{:?}), {ctx}",
                        first
                    );
                }
            }

            // The primitives, as the mesh the port draws.
            let mut after = cpu.mem.read(BLOCK, block.len());
            // Back to relative pointers for the parser.
            let rd = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
            let nobj = rd(&after, m.tmd_offset + 8) as usize;
            for k in 0..nobj {
                let at = m.tmd_offset + 12 + 0x1C * k + 0x10;
                let rel = rd(&after, at) - (tmd_at + 12);
                after[at..at + 4].copy_from_slice(&rel.to_le_bytes());
            }
            let tmd = legaia_tmd::parse(&after[m.tmd_offset..]).expect("relocated TMD parses");
            let (retail_mesh, _) =
                legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &after[m.tmd_offset..]);
            assert_eq!(
                retail_mesh.cba_tsb, port.cba_tsb,
                "relocated CBA / TSB, {ctx}"
            );
            prims += port.cba_tsb.len();
        }
        monsters += 1;
    }
    assert!(
        monsters > 100 && gated > 0 && prims > 0,
        "monsters {monsters} gated {gated}"
    );
}
