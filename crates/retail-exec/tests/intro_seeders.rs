//! The field-to-battle transition's seeders off PROT 0979, the bearing
//! `FUN_80019B28` and the root `FUN_8005AF0C` live beneath them and `rand()`
//! answered by one stream on both sides, each against its port over the
//! production `IntroEnv` (its trig and root) with the same stream: the
//! particle seeders `FUN_801CFBB4` / `FUN_801D0164` against
//! `seed_particle_grid` (every record of the `0x488`-record grid), the tile
//! seeder `FUN_801D081C` against `seed_tile_grid` at every sub-style (the
//! corner grid and every tile record), and the swirl build `FUN_801D1564`
//! against `build_swirl_mesh` (bands, vertices and texels). All compared
//! whole.

use crate::common::{diff_offsets, disc, scus_cpu};
use legaia_engine_system::rand::world_rand;
use legaia_render_kernels::battle_intro_particles::{
    IntroEnv, IntroParticle, PARTICLE_COUNT, PARTICLE_STRIDE, ParticleEnv, STYLE_CFBB4,
    STYLE_D0164, SeedOutcome, seed_particle_grid,
};
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SEED_A: u32 = 0x801C_FBB4;
const SEED_B: u32 = 0x801D_0164;
const ALLOC: u32 = 0x8001_7888;
const RAND: u32 = 0x8005_6798;
const ENTITY: u32 = 0x8010_0000;
const GRID: u32 = 0x8011_0000;
const FILL: u8 = 0xA5;

/// The production environment's trig and root, with the test's stream in
/// place of its stand-in generator.
struct Env {
    trig: IntroEnv,
    seed: u32,
}

impl ParticleEnv for Env {
    fn heading(&mut self, x: i32, z: i32) -> i32 {
        self.trig.heading(x, z)
    }
    fn sin(&mut self, h: i32) -> i16 {
        self.trig.sin(h)
    }
    fn cos(&mut self, h: i32) -> i16 {
        self.trig.cos(h)
    }
    fn sqrt(&mut self, v: i32) -> i32 {
        self.trig.sqrt(v)
    }
    fn rand(&mut self) -> i32 {
        world_rand(&mut self.seed) as i32
    }
}

fn lay_out(grid: &[IntroParticle]) -> Vec<u8> {
    let mut out = vec![FILL; PARTICLE_COUNT * PARTICLE_STRIDE];
    for (k, p) in grid.iter().enumerate() {
        let at = k * PARTICLE_STRIDE;
        out[at + 4..at + 8].copy_from_slice(&p.tint.to_le_bytes());
        for (o, v) in [
            (0x08, p.trans.0),
            (0x0A, p.trans.1),
            (0x0C, p.trans.2),
            (0x10, p.rot.0),
            (0x12, p.rot.1),
            (0x14, p.rot.2),
            (0x16, p.field_16),
            (0x18, p.spin.0),
            (0x1A, p.spin.1),
            (0x1C, p.spin.2),
            (0x1E, p.delay),
            (0x20, p.trans_vel.0),
            (0x22, p.trans_vel.1),
            (0x24, p.trans_vel.2),
            (0x28, p.texel_page),
            (0x2A, p.texel_v),
        ] {
            out[at + o..at + o + 2].copy_from_slice(&(v as u16).to_le_bytes());
        }
    }
    out
}

fn run(entry: u32, style: &legaia_render_kernels::battle_intro_particles::IntroParticleStyle) {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(979).expect("PROT 0979"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    cpu.hook(ALLOC, |r, _| r.r[V0] = GRID);
    let stream = Rc::new(Cell::new(0u32));
    {
        let stream = stream.clone();
        cpu.hook(RAND, move |r, _| {
            let mut s = stream.get();
            r.r[V0] = world_rand(&mut s);
            stream.set(s);
        });
    }
    for seed in [1u32, 0x1234_5678, 0xDEAD_BEEF] {
        stream.set(seed);
        cpu.mem
            .load(GRID, &vec![FILL; PARTICLE_COUNT * PARTICLE_STRIDE]);
        assert_eq!(cpu.call(entry, &[ENTITY]), Stop::Returned);
        assert_eq!(cpu.mem.rd32(ENTITY + 0x48), GRID);
        let mut env = Env {
            trig: IntroEnv::new(0),
            seed,
        };
        let SeedOutcome::Seeded(grid) = seed_particle_grid(style, true, &mut env) else {
            panic!("seeded");
        };
        let want = lay_out(&grid);
        let got = cpu.mem.read(GRID, want.len());
        let first = got
            .iter()
            .zip(&want)
            .position(|(a, b)| a != b)
            .map(|i| (i / PARTICLE_STRIDE, i % PARTICLE_STRIDE));
        assert!(
            got == want,
            "seed {seed:#x}: first differing (record, offset) {first:x?}: {:?}",
            diff_offsets(&got, &want)
        );
        assert_eq!(stream.get(), env.seed, "rand draws, seed {seed:#x}");
    }
}

#[test]
fn the_diagonal_seeder_matches_seed_particle_grid() {
    differential!(prot(979, SEED_A));
    run(SEED_A, &STYLE_CFBB4);
}

#[test]
fn the_radial_seeder_matches_seed_particle_grid() {
    differential!(prot(979, SEED_B));
    run(SEED_B, &STYLE_D0164);
}

const SEED_TILES: u32 = 0x801D_081C;
const BUILD_SWIRL: u32 = 0x801D_1564;
const SUB_STYLE: u32 = 0x801D_2464;
const TILES_PTR: u32 = 0x801D_2468;
const VERTS_PTR: u32 = 0x801D_246C;
const BANDS_PTR: u32 = 0x801D_247C;
const SWIRL_VERTS_PTR: u32 = 0x801D_2474;
const SWIRL_TEXELS_PTR: u32 = 0x801D_2478;
const ALLOC_A: u32 = 0x8011_0000;
const ALLOC_B: u32 = 0x8013_0000;
const ALLOC_C: u32 = 0x8015_0000;

/// A CPU with the transition overlay, the LUT pointers and a bump allocator
/// handing out `ALLOC_A`, `ALLOC_B`, `ALLOC_C` in call order.
fn seeder_cpu(stream: &Rc<Cell<u32>>) -> Option<(psx_retail_exec::Cpu, Vec<u8>)> {
    let mut cpu = scus_cpu()?;
    let image = disc()?.read_entry(979).ok()?;
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    let next = Rc::new(Cell::new(0usize));
    {
        let next = next.clone();
        cpu.hook(ALLOC, move |r, _| {
            let k = next.get();
            next.set(k + 1);
            r.r[V0] = [ALLOC_A, ALLOC_B, ALLOC_C][k % 3];
        });
    }
    {
        let stream = stream.clone();
        cpu.hook(RAND, move |r, _| {
            let mut s = stream.get();
            r.r[V0] = world_rand(&mut s);
            stream.set(s);
        });
    }
    Some((cpu, image))
}

#[test]
fn the_tile_seeder_matches_seed_tile_grid() {
    use legaia_render_kernels::battle_intro::parse_tile_corner_table;
    use legaia_render_kernels::battle_intro_tiles::{
        TileSeedOutcome, TileSubStyle, seed_tile_grid,
    };
    differential!(prot(979, SEED_TILES));
    let stream = Rc::new(Cell::new(0u32));
    let Some((mut cpu, image)) = seeder_cpu(&stream) else {
        return;
    };
    let corners = parse_tile_corner_table(&image, OVERLAY_BASE).expect("corner table");
    for (word, sub) in [
        (0u32, TileSubStyle::NegSpinRandomDelay),
        (1, TileSubStyle::PosSpinRandomDelay),
        (2, TileSubStyle::RadialDelayWithTumble),
    ] {
        // A word past 2 runs no arm, and retail then leaves the delay and the
        // spin pad as the pool held them, where the port's record holds 0.
        // The transition's style pick never stores one.
        for seed in [7u32, 0xC0FF_EE00] {
            stream.set(seed);
            cpu.mem.wr32(SUB_STYLE, word);
            cpu.mem.load(ALLOC_A, &vec![FILL; 0x908]);
            cpu.mem.load(ALLOC_B, &vec![FILL; 0x5C00]);
            assert_eq!(cpu.call(SEED_TILES, &[ENTITY]), Stop::Returned);
            let verts = cpu.mem.rd32(VERTS_PTR);
            let tiles = cpu.mem.rd32(TILES_PTR);
            let mut env = Env {
                trig: IntroEnv::new(0),
                seed,
            };
            let TileSeedOutcome::Seeded(grid) = seed_tile_grid(sub, true, corners, &mut env) else {
                panic!("seeded");
            };
            let ctx = format!("sub-style {word} seed {seed:#x}");
            let mut want = vec![FILL; 0x908];
            for (k, v) in grid.vertices.iter().enumerate() {
                for (j, c) in [v.x, v.y, v.z].iter().enumerate() {
                    want[8 * k + 2 * j..8 * k + 2 * j + 2]
                        .copy_from_slice(&(*c as u16).to_le_bytes());
                }
            }
            let got = cpu.mem.read(verts, want.len());
            // The fourth halfword of a grid vertex is never written.
            let got: Vec<u8> = got
                .iter()
                .enumerate()
                .map(|(i, b)| if i % 8 >= 6 { FILL } else { *b })
                .collect();
            assert_eq!(
                got,
                want,
                "corner grid, {ctx}: {:?}",
                diff_offsets(&got, &want)
            );
            let mut want = vec![FILL; 0x5C00];
            for (k, t) in grid.tiles.iter().enumerate() {
                let at = 0x5C * k;
                let mut h = |o: usize, v: i16| {
                    want[at + o..at + o + 2].copy_from_slice(&(v as u16).to_le_bytes())
                };
                h(0, t.progress);
                h(2, t.tpage);
                h(4, t.pos.0);
                h(6, t.pos.1);
                h(8, t.pos.2);
                h(0xA, t.delay);
                h(0xC, t.angles.0);
                h(0xE, t.angles.1);
                h(0x10, t.angles.2);
                for (j, c) in t.front.iter().chain(&t.back).enumerate() {
                    let o = 0x14 + 8 * j;
                    h(o, c.x);
                    h(o + 2, c.y);
                    h(o + 4, c.z);
                    h(o + 6, c.pad);
                }
                for (j, c) in t.front.iter().enumerate() {
                    want[at + 0x54 + 2 * j] = c.uv.0 as u8;
                    want[at + 0x55 + 2 * j] = c.uv.1 as u8;
                }
            }
            let got = cpu.mem.read(tiles, want.len());
            // `+0x12` is never written either, nor the pads of front corner 3
            // and back corners 2 and 3, which carry no rate.
            let got: Vec<u8> = got
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if matches!(
                        i % 0x5C,
                        0x12 | 0x13 | 0x32 | 0x33 | 0x4A | 0x4B | 0x52 | 0x53
                    ) {
                        want[i]
                    } else {
                        *b
                    }
                })
                .collect();
            let first = got
                .iter()
                .zip(&want)
                .position(|(a, b)| a != b)
                .map(|i| (i / 0x5C, i % 0x5C));
            assert!(
                got == want,
                "tiles, {ctx}: first (record, offset) {first:x?}: {:?}",
                diff_offsets(&got, &want)
            );
            assert_eq!(stream.get(), env.seed, "rand draws, {ctx}");
        }
    }
}

#[test]
fn the_swirl_build_matches_build_swirl_mesh() {
    use legaia_render_kernels::battle_intro_swirl::{SwirlBuildOutcome, build_swirl_mesh};
    differential!(prot(979, BUILD_SWIRL));
    let stream = Rc::new(Cell::new(0u32));
    let Some((mut cpu, _)) = seeder_cpu(&stream) else {
        return;
    };
    cpu.mem.load(ALLOC_A, &vec![FILL; 0x100]);
    cpu.mem.load(ALLOC_B, &vec![FILL; 0x6300]);
    cpu.mem.load(ALLOC_C, &vec![FILL; 0x18C0]);
    assert_eq!(cpu.call(BUILD_SWIRL, &[ENTITY]), Stop::Returned);
    let SwirlBuildOutcome::Built(mesh) = build_swirl_mesh(true, &mut IntroEnv::new(0)) else {
        panic!("built");
    };
    let mut want = Vec::new();
    for b in &mesh.bands {
        for w in [b.phase, b.width, b.angle, b.rate] {
            want.extend((w as u32).to_le_bytes());
        }
    }
    let bands = cpu.mem.rd32(BANDS_PTR);
    let got = cpu.mem.read(bands, want.len());
    assert_eq!(got, want, "bands: {:?}", diff_offsets(&got, &want));
    let verts = cpu.mem.rd32(SWIRL_VERTS_PTR);
    let got = cpu.mem.read(verts, 8 * mesh.vertices.len());
    let mut want = got.clone();
    for (k, v) in mesh.vertices.iter().enumerate() {
        for (j, c) in [v.x, v.y, v.z].iter().enumerate() {
            want[8 * k + 2 * j..8 * k + 2 * j + 2].copy_from_slice(&(*c as u16).to_le_bytes());
        }
    }
    let first = got
        .iter()
        .zip(&want)
        .position(|(a, b)| a != b)
        .map(|i| i / 8);
    assert!(
        got == want,
        "vertices, first vertex {first:?}: {:?}",
        diff_offsets(&got, &want)
    );
    let texels = cpu.mem.rd32(SWIRL_TEXELS_PTR);
    let got = cpu.mem.read(texels, 2 * mesh.texels.len());
    let want: Vec<u8> = mesh
        .texels
        .iter()
        .flat_map(|t| [t.0 as u8, t.1 as u8])
        .collect();
    assert_eq!(got, want, "texels: {:?}", diff_offsets(&got, &want));
}
