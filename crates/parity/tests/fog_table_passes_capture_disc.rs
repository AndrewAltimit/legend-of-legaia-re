//! Retail capture: how many pool passes the fog table still in a state's RAM
//! sits behind the state's fog pool, per emulator.
//!
//! The field render pass draws each fog record from its position before the
//! pass moves it (`FUN_8003F3FC`), so the table a pass built is the pool
//! rolled back by that pass and every later one. Rolled back by
//! `retail_compare::FOG_TABLE_PASSES` frame steps and run through the
//! integer fog kernel (`FogPool::render_step` over `FieldGteView` built from
//! the state's camera globals), the pool reproduces the fog packets of the
//! state's ordering table corner for corner (all but one on the Mednafen
//! state) - and no other
//! roll-back comes close. The
//! frame on screen is the table before that one, which is what the
//! retail-compare fog seat (`retail_compare::retail_fog`) rolls back to.
//!
//! Skips (and passes) when the manifest or the save library is missing.

use crate::retail_compare::FOG_TABLE_PASSES;
use legaia_engine_field::fog_particles::{
    FOG_CLUT, FOG_POOL_SLOTS, FOG_TPAGE, FogFrameEnv, FogPool, FogView,
};
use legaia_render_kernels::gte::{FIELD_OFX, FieldGteView};
use psx_ram_image::ScenarioManifest;
use psx_ram_image::prim_pool::{self, Prim};

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & 0x1F_FFFF) as usize;
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

fn i16_at(ram: &[u8], va: u32) -> i16 {
    let o = (va & 0x1F_FFFF) as usize;
    i16::from_le_bytes([ram[o], ram[o + 1]])
}

/// The fog quads the integer kernel draws from the pool rolled back `passes`
/// frame steps, as corner lists.
fn kernel_table(ram: &[u8], scratch: &[u8], passes: i32) -> Vec<[(i16, i16); 4]> {
    let dt = i32::from(scratch[0x393]).max(1);
    let back = passes * dt;
    let overworld = scratch[0x394] & 1 != 0;
    let angles = [0x8007_B790u32, 0x8007_B792, 0x8007_B794].map(|a| i16_at(ram, a) as u16);
    let eye = [0x8008_40B8u32, 0x8008_40BC, 0x8008_40C0].map(|a| u32_at(ram, a) as i32);
    let neg_focus = [0x8008_9118u32, 0x8008_911C, 0x8008_9120].map(|a| u32_at(ram, a) as i32);
    let h = i16_at(ram, 0x8007_B6F4) as u16;
    let gte = FieldGteView::from_globals(
        angles,
        eye,
        neg_focus,
        &FieldGteView::field_base(),
        h,
        FIELD_OFX,
        legaia_engine_camera::projection::GTE_OFY as i32,
    );
    let view =
        FogView::from_vp(std::array::from_fn(|i| f32::from(u8::from(i % 5 == 0)))).with_gte(gte);
    let mut pool = FogPool::new();
    pool.overworld = overworld;
    let pool_va = u32_at(ram, 0x8007_B7E0);
    for i in 0..FOG_POOL_SLOTS {
        let o = ((pool_va + 0xA4 + i as u32 * 0x18) & 0x1F_FFFF) as usize;
        let r = &ram[o..o + 0x18];
        let rec = &mut pool.records[i];
        rec.alive = r[5] != 0;
        rec.slot = r[4];
        if !rec.alive {
            continue;
        }
        rec.rate = u16::from_le_bytes([r[2], r[3]]);
        rec.age =
            u16::from_le_bytes([r[0], r[1]]).wrapping_sub((i32::from(rec.rate) * back) as u16);
        rec.vx = r[6] as i8;
        rec.vz = r[7] as i8;
        rec.x = i32::from_le_bytes([r[8], r[9], r[10], r[11]]) - i32::from(rec.vx) * back;
        rec.z = i32::from_le_bytes([r[12], r[13], r[14], r[15]]) - i32::from(rec.vz) * back;
        rec.y = i16::from_le_bytes([r[16], r[17]]);
        rec.grey = r[0x14];
    }
    let pl = u32_at(ram, 0x8007_C364);
    let env = FogFrameEnv {
        dt: dt as u8,
        player: [
            i16_at(ram, pl + 0x14).into(),
            i16_at(ram, pl + 0x16).into(),
            i16_at(ram, pl + 0x18).into(),
        ],
        dpad_held: false,
        y_offset: u32_at(ram, 0x8007_BCAC) as i32,
        tint: [0x80; 3],
        window: [
            scratch[0x384],
            scratch[0x385],
            scratch[0x386],
            scratch[0x387],
        ],
    };
    pool.render_step(&view, &env).iter().map(|q| q.xy).collect()
}

#[test]
fn the_fog_table_in_ram_is_the_pool_rolled_back_by_the_emulators_passes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (manifest, lib) = (
        root.join("scripts/scenarios.toml"),
        root.join("saves/library"),
    );
    if !manifest.exists() || !lib.exists() {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    }
    let manifest = ScenarioManifest::from_path(&manifest).expect("parse manifest");
    let mut checked = 0;
    for label in [
        "keikoku_chest_preload",
        "map01_field_door_hop",
        "map03_field_door_hop",
        "drake_castle_to_worldmap",
    ] {
        let Some(fp) = manifest
            .scenarios
            .iter()
            .find(|s| s.label == label)
            .and_then(|s| s.backup_fingerprint.as_deref())
        else {
            continue;
        };
        let mut found = None;
        for (emulator, _) in FOG_TABLE_PASSES {
            let Some(path) = psx_ram_image::scenarios::library_backup_for(emulator, &lib, fp)
            else {
                continue;
            };
            found = Some(match emulator {
                "pcsx-redux" => {
                    let st = psx_pcsx_redux::SaveState::from_path(&path).expect("state");
                    (
                        emulator,
                        st.main_ram().to_vec(),
                        st.scratchpad().expect("scratch").to_vec(),
                    )
                }
                _ => {
                    let st = psx_mednafen::SaveState::from_path(&path).expect("state");
                    (
                        emulator,
                        st.main_ram().expect("ram").to_vec(),
                        st.scratch_ram().expect("scratch").to_vec(),
                    )
                }
            });
            break;
        }
        let Some((emulator, ram, scratch)) = found else {
            eprintln!("[skip] {label}: no library backup");
            continue;
        };
        let mut retail = Vec::new();
        for ot in prim_pool::find_ot_arrays(&ram, 0x8000_0000, 64) {
            for c in prim_pool::chain_walk(&ram, 0x8000_0000, (ot.head & 0x1F_FFFF) as usize) {
                if let Prim::PolyFt4 {
                    clut, tpage, verts, ..
                } = c.prim
                    && clut == FOG_CLUT
                    && tpage == FOG_TPAGE
                {
                    retail.push(verts);
                }
            }
        }
        assert!(!retail.is_empty(), "{label}: no fog packets in RAM");
        let exact = |passes: i32| {
            let ours = kernel_table(&ram, &scratch, passes);
            retail
                .iter()
                .filter(|v| ours.iter().any(|q| q == *v))
                .count()
        };
        let passes = FOG_TABLE_PASSES
            .iter()
            .find(|(e, _)| *e == emulator)
            .unwrap()
            .1;
        let counts: Vec<usize> = (0..=3).map(exact).collect();
        eprintln!(
            "[ok] {label} ({emulator}): exact packets per roll-back {counts:?} of {}",
            retail.len()
        );
        // All but at most one packet: the Mednafen state keeps one packet
        // the plain roll-back does not reach (unexplained).
        let hit = counts[passes as usize];
        assert!(
            hit + 1 >= retail.len(),
            "{label}: {hit} of {} at {passes}",
            retail.len()
        );
        for (p, &n) in counts.iter().enumerate() {
            if p != passes as usize {
                assert!(n * 2 < hit, "{label}: roll-back {p} reproduces {n} as well");
            }
        }
        checked += 1;
    }
    eprintln!("[ok] {checked} states");
}
