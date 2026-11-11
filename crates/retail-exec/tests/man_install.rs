//! The MAN installer `FUN_8003AEB0`, run whole over a generated MAN with its
//! spawners and overlay calls stubbed, against the pieces of it the port
//! carries, in the order it runs them:
//!
//! - the entry flag stores (`World::man_load_entry_flags`),
//! - the save permission off the header's second byte
//!   (`World::install_scene_save_permission`'s source bit),
//! - the back-from-battle arm's flag stores and game-over hand-off
//!   (`battle_return_flags::apply_battle_return_flags`) and the escape
//!   clock's penalty (`EscapeTimer::field_return_penalty`),
//! - the place-name banner and the flag it consumes
//!   (`place_name_banner::man_load_place_name_banner`), behind the
//!   map-install tail (`World::seat_map_install_tail`).
//!
//! Compared: the first four bytes of the system flag bank as the routine
//! leaves them, the mode word, the clock, and the banner actor's name,
//! position and display total. The scene control block reset `FUN_8003A024`
//! and the map-install tail `FUN_8003ADAC` run live beneath it.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_return_flags::{FlagBank, apply_battle_return_flags};
use legaia_engine_core::world::World;
use legaia_engine_dialog::text_balloon::TextBalloon;
use legaia_engine_field::escape_timer::EscapeTimer;
use legaia_engine_scene_host::place_name_banner::man_load_place_name_banner;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const INSTALL: u32 = 0x8003_AEB0;
const ALLOC: u32 = 0x8001_7888;
const ALLOC_NODE: u32 = 0x8002_0DE0;
const TEXT_WIDTH: u32 = 0x8003_5F04;
const BANNER_DESCRIPTOR: u32 = 0x8007_431C;
/// Everything else the installer calls: spawners, the overlay's scene
/// hooks, the stat rebuild, the floor sampler.
const STUBS: [u32; 27] = [
    0x8002_71A8,
    0x801D_E37C,
    0x8003_A110,
    0x8004_2558,
    0x801D_E190,
    0x8003_53E0,
    0x801D_9C3C,
    0x801D_E004,
    0x8005_67A8,
    0x8002_66E0,
    0x801E_565C,
    0x8001_80EC,
    0x801D_E3E0,
    0x8003_A1E4,
    0x8003_A55C,
    0x8003_A9D4,
    0x801D_596C,
    0x801D_E478,
    0x8001_9278,
    0x801D_B8EC,
    0x801D_AA50,
    0x8001_72C0,
    0x8003_D344,
    0x8003_AB2C,
    0x801D_5A24,
    0x8002_4E80,
    0x8001_FA00,
];

const MAN_PTR: u32 = 0x8007_B898;
const MAP_PTR: u32 = 0x1F80_03EC;
const FLAG_BANK: u32 = 0x8008_5758;
const BOOT_LATCH: u32 = 0x8007_B71C;
const SAVE_PERMISSION: u32 = 0x8007_B6A8;
const FIELD_ENTRY: u32 = 0x8007_B8B8;
const GAME_MODE: u32 = 0x8007_B83C;
const STANDING: u32 = 0x8007_BD60;
const CLOCK_ARMED: u32 = 0x8008_45B8;
const CLOCK: u32 = 0x8008_45A0;
const PLAYER_PTR: u32 = 0x8007_C364;
const LISTS: [u32; 3] = [0x8007_C34C, 0x8007_C350, 0x8007_C354];

const MAN: u32 = 0x8012_0000;
const MAP: u32 = 0x8014_0000;
const BLOCK: u32 = 0x8017_0000;
const PLAYER: u32 = 0x8017_1000;
const EMPTY_LIST: u32 = 0x8017_2000;
const NODE: u32 = 0x8017_3000;

struct Bank<'a>(&'a mut World);
impl FlagBank for Bank<'_> {
    fn test(&self, idx: u16) -> bool {
        self.0.system_flag_test(idx)
    }
    fn set(&mut self, idx: u16) {
        self.0.system_flag_set(idx)
    }
    fn clear(&mut self, idx: u16) {
        self.0.system_flag_clear(idx)
    }
}

#[test]
fn the_man_installer_is_retails() {
    differential!(scus(INSTALL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for s in STUBS {
        cpu.stub(s, 0);
    }
    cpu.stub(ALLOC, BLOCK);
    let spawned = Rc::new(RefCell::new(Vec::new()));
    let s = spawned.clone();
    cpu.hook(ALLOC_NODE, move |r, m| {
        let mut s = s.borrow_mut();
        let node = NODE + 0x100 * s.len() as u32;
        m.load(node, &[0xEEu8; 0x100]);
        s.push((r.r[A0], node));
        r.r[V0] = node;
    });
    let width = Rc::new(RefCell::new((0u32, Vec::new())));
    let w = width.clone();
    cpu.hook(TEXT_WIDTH, move |r, _| {
        let mut w = w.borrow_mut();
        w.1.push(r.r[A0]);
        r.r[V0] = w.0;
    });
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    for l in LISTS {
        cpu.mem.wr32(l, EMPTY_LIST);
    }

    let mut rng = Rng(0x4D41_4E49);
    let (mut banners, mut game_overs, mut returns, mut failures) = (0, 0, 0, Vec::new());
    for case in 0..4000 {
        let bank: [u8; 4] = std::array::from_fn(|_| rng.next() as u8 & rng.next() as u8);
        let header = rng.next() as u8;
        let entry = [0u32, 1, 2, 2][rng.below(4) as usize];
        let survived = rng.below(2) == 0;
        let armed = rng.below(2) == 0;
        let clock = rng.below(20_000) as i32;
        let name: Vec<u8> = match rng.below(3) {
            0 => vec![],
            _ => (0..1 + rng.below(12))
                .map(|_| 0x41 + rng.below(26) as u8)
                .collect(),
        };
        let text_width = rng.below(0x140);

        // A MAN whose three counts are zero: the header, then the section
        // chain of 24-bit lengths. Section 2's body is the name.
        let mut man = vec![0u8; 0x2B];
        man[0] = rng.next() as u8;
        man[1] = header;
        for len in [1usize, 1, name.len() + 1, 1, 1, 1] {
            man.extend([len as u8, 0, 0]);
            if len == name.len() + 1 && man.len() == 0x2B + 3 * 3 + 2 {
                man.extend(&name);
                man.push(0);
            } else {
                man.extend(std::iter::repeat_n(1u8, len));
            }
        }
        man.extend([0u8; 16]);
        cpu.mem.load(MAN, &man);
        cpu.mem.load(MAP + 0x12000, &[0u8; 0x10]);
        cpu.mem.load(FLAG_BANK, &bank);
        let latch = rng.below(4) == 0;
        cpu.mem.wr32(BOOT_LATCH, u32::from(latch));
        cpu.mem.wr32(FIELD_ENTRY, entry);
        cpu.mem.wr16(GAME_MODE, 2);
        cpu.mem.wr8(STANDING, if survived { 0x80 } else { 0 });
        cpu.mem.wr32(CLOCK_ARMED, u32::from(armed) * 0x1000);
        cpu.mem.wr32(CLOCK, clock as u32);
        cpu.mem.load(PLAYER, &[0u8; 0x100]);
        spawned.borrow_mut().clear();
        *width.borrow_mut() = (text_width, Vec::new());
        assert_eq!(
            cpu.call(INSTALL, &[0, 0x8017_4000]),
            Stop::Returned,
            "case {case}"
        );
        assert_eq!(
            cpu.regs.r[V0],
            u32::from(man[0]),
            "the header's first byte comes back"
        );

        let mut w = World::new();
        for (i, b) in bank.iter().enumerate() {
            for bit in 0..8u16 {
                if b & (0x80 >> bit) != 0 {
                    w.system_flag_set(8 * i as u16 + bit);
                }
            }
        }
        w.man_load_entry_flags();
        let mut timer = EscapeTimer {
            remaining: clock,
            warn_threshold: 0,
            armed,
        };
        let mut game_over = false;
        if entry == 2 {
            game_over = apply_battle_return_flags(&mut Bank(&mut w), survived).game_over;
            timer.field_return_penalty();
            returns += 1;
        }
        // The map-install tail runs live on retail's side and takes flag `0x1B`.
        w.seat_map_install_tail();
        let seated = man_load_place_name_banner(&mut w, &name);

        let ctx = format!(
            "case {case}: bank {bank:02x?} entry {entry} survived {survived} name {name:02x?}"
        );
        // Flag `0x16` is the installer's "first scene since boot" mark:
        // cleared here, raised under the boot latch. The port carries
        // neither, so it is held to retail on its own and left out of the
        // comparison.
        let mut retail_bank = cpu.mem.read(FLAG_BANK, 4);
        assert_eq!(retail_bank[2] & 2 != 0, latch, "{ctx}: flag 0x16");
        assert_eq!(cpu.mem.rd32(BOOT_LATCH), 0, "{ctx}: the latch is spent");
        retail_bank[2] = retail_bank[2] & !2 | bank[2] & 2;
        let port_bank: Vec<u8> = (0..4u16)
            .map(|i| {
                (0..8u16).fold(0u8, |b, bit| {
                    b | if w.system_flag_test(8 * i + bit) {
                        0x80 >> bit
                    } else {
                        0
                    }
                })
            })
            .collect();
        if retail_bank != port_bank && failures.len() < 8 {
            failures.push(format!(
                "{ctx}\n retail {retail_bank:02x?}\n port   {port_bank:02x?}"
            ));
        }
        assert_eq!(cpu.mem.rd8(SAVE_PERMISSION), header & 1, "{ctx}");
        assert_eq!(
            cpu.mem.rd16(GAME_MODE) == 0x16,
            game_over,
            "{ctx}: the mode word"
        );
        assert_eq!(
            cpu.mem.rd32(CLOCK) as i32,
            timer.remaining,
            "{ctx}: the escape clock"
        );
        // The banner: the one spawn off its descriptor.
        let banner: Vec<u32> = spawned
            .borrow()
            .iter()
            .filter(|(d, _)| *d == BANNER_DESCRIPTOR)
            .map(|(_, n)| *n)
            .collect();
        assert_eq!(banner.len(), usize::from(seated), "{ctx}");
        if let (Some(&node), Some(b)) = (banner.first(), w.cutscene.text_balloon.as_ref()) {
            let text = cpu.mem.rd32(node + 0x90);
            assert_eq!(cpu.mem.read(text, name.len()), b.text, "{ctx}");
            assert_eq!(
                width.borrow().1,
                vec![text],
                "the width is measured off the name"
            );
            let measured = TextBalloon::spawn_measured(&name, text_width as i16);
            assert_eq!(
                (
                    cpu.mem.rd16(node + 0x14) as i16,
                    cpu.mem.rd16(node + 0x16) as i16,
                    cpu.mem.rd16(node + 0x54) as i16,
                    cpu.mem.rd16(node + 0x9C) as i16,
                    cpu.mem.rd32(node + 0x94) != 0,
                ),
                (measured.x.unwrap(), b.y, b.timer, b.total, b.parent_link),
                "{ctx}"
            );
            banners += 1;
        }
        game_overs += usize::from(game_over);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        banners > 500 && game_overs > 100 && returns > 1500,
        "{banners} {game_overs} {returns}"
    );
}
