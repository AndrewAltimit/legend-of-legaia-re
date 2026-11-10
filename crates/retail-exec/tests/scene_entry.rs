//! The scene-entry system-script install `FUN_8003AB2C` over every field
//! scene's MAN on the disc, against `ManFile::scene_entry_script` (where the
//! script block starts and its first opcode) and against
//! `region_encounter::encounter_counter_scene_entry_top_up`, the step-counter
//! top-up the install makes on the way (through the field overlay's reroll
//! `FUN_801DDF48`, PROT 0897).
//!
//! The actor it allocates comes back zeroed; the field VM it runs on an
//! opening `0x24` / `0x25` op returns at once, so the compared state is what
//! the install itself left: the script base `+0x90` and pc `+0x9E`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::region_encounter::encounter_counter_scene_entry_top_up;
use psx_retail_exec::{A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const INSTALL: u32 = 0x8003_AB2C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAN_PTR: u32 = 0x8007_B898;
const MAN: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_LIST: u32 = 0x8007_C34C;
const COUNTER: u32 = 0x8007_B5FC;
/// The script a MAN with an empty partition 1 falls back to.
const DEFAULT_SCRIPT: u32 = 0x8007_3EE8;

#[test]
fn every_scene_installs_the_ports_entry_script() {
    differential!(scus(INSTALL));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_scene::vfs::Vfs;
    let vfs = legaia_engine_scene::vfs::DiscVfs::open(std::env::var_os("LEGAIA_DISC_BIN").unwrap())
        .expect("disc");
    let names = vfs
        .read("cdname.txt")
        .or_else(|_| vfs.read("data/cdname.txt"))
        .expect("CDNAME.TXT");
    let names = String::from_utf8(names).expect("text");
    let index = legaia_engine_scene::scene::ProtIndex::from_bytes(
        vfs.read("prot.dat").expect("PROT.DAT"),
        Some(&names),
    )
    .expect("index");
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8002_0DE0, ACTOR);
    cpu.mem.wr32(ACTOR_LIST, 0x8015_0000);
    cpu.mem.wr32(0x8015_0000, 0);
    // The field VM returns the pc it was handed: nothing runs.
    cpu.hook(0x801D_E840, |r, _| r.r[V0] = r.r[A1]);
    let draws: Rc<RefCell<Vec<u32>>> = Rc::default();
    {
        let draws = draws.clone();
        cpu.hook(0x8005_6798, move |r, _| {
            r.r[V0] = draws.borrow_mut().remove(0)
        });
    }
    let mut rng = Rng(0x5343_4E45);
    let (mut scenes, mut topped, mut fallback) = (0, 0, 0);
    for name in legaia_prot::cdname::parse_str(&names)
        .expect("cdname")
        .values()
    {
        let Ok(scene) = legaia_engine_scene::scene::Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(file) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        scenes += 1;
        cpu.mem.load(MAN, &man);
        cpu.mem.wr32(MAN_PTR, MAN);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        let counter = if rng.below(2) == 0 {
            rng.below(0x1E7) as i32
        } else {
            0x1E7 + rng.below(0x400) as i32
        };
        cpu.mem.wr32(COUNTER, counter as u32);
        let pair = [rng.below(0x8000), rng.below(0x8000)];
        *draws.borrow_mut() = pair.to_vec();
        assert_eq!(cpu.call(INSTALL, &[]), Stop::Returned, "{name}");
        let base = cpu.mem.rd32(ACTOR + 0x90);
        let pc = cpu.mem.rd16(ACTOR + 0x9E);
        match file.scene_entry_script(&man) {
            Some((start, pc0)) => {
                assert_eq!(base, MAN + start as u32, "{name}: script base");
                assert_eq!(usize::from(pc), pc0, "{name}: first opcode");
            }
            None => {
                fallback += 1;
                assert_eq!(base, DEFAULT_SCRIPT, "{name}: no entry script");
            }
        }
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), 0xFB, "{name}: channel");
        let mut it = pair.into_iter();
        let want = encounter_counter_scene_entry_top_up(counter, || it.next().expect("a draw"));
        assert_eq!(cpu.mem.rd32(COUNTER) as i32, want, "{name}: step counter");
        topped += i32::from(want != counter);
    }
    eprintln!("{scenes} scenes, {topped} topped up, {fallback} without a script");
    assert!(scenes > 80 && topped > 20, "{scenes} {topped}");
}
