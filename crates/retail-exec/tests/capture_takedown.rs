//! The capture takedown `FUN_801E7824` against
//! `World::battle_capture_takedown`: every monster seat of every formation,
//! a record whose clip list holds the walk tag `1` at any entry or not at all,
//! and the seat's live fields beforehand. Compared: the queued clip `+0x1DA`,
//! the flag byte `+0x1DC` (a raw `+1`), live HP `+0x14C` and its readout
//! `+0x172`, the facing `+0x46` and the target byte `+0x1DD`. The capture
//! counter `+0x227` the port keeps as its capture list, and the name copy and
//! the banner `FUN_801D8DE8(0x43, 0)` are the host's; the test holds that
//! retail reaches all three.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_engine_battle_vm::battle_action::ActorFlags;
use legaia_engine_battle_vm::battle_slot::SeatMap;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const TAKEDOWN: u32 = 0x801E_7824;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
/// Monster records, monster `k` (seat `3 + k`) at `+k*4`.
const RECORDS: u32 = 0x801C_9348;
const NAME_COPY: u32 = 0x8003_CA78;
const NAME_CAT: u32 = 0x8003_CAC4;
const BANNER: u32 = 0x801D_8DE8;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

fn clip(tag: u8) -> Option<MonsterAnimation> {
    Some(MonsterAnimation {
        variant_track: Default::default(),
        action_id: tag,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 1,
        frames: vec![vec![Default::default()]],
    })
}

#[test]
fn the_capture_takedown_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for va in [NAME_COPY, NAME_CAT, BANNER] {
        cpu.stub(va, 0);
    }
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
    }
    let mut rng = Rng(0x5441_4B45);
    let (mut failures, mut walked, mut none) = (Vec::new(), 0, 0);
    for round in 0..20_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let seats = SeatMap::new(party);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        let e = party + rng.below(u32::from(monsters)) as u8;
        let r = u32::from(seats.retail(e).get());
        let k = r - 3;
        let count = 1 + rng.below(8);
        let tags: Vec<u8> = (0..count)
            .map(|_| [0u8, 1, 2, 4, 5, 0x0B][rng.below(6) as usize])
            .collect();
        let rec = 0x8014_0000 + k * 0x200;
        cpu.mem.wr32(RECORDS + k * 4, rec);
        cpu.mem.wr8(rec + 0x4A, count as u8);
        for (c, &t) in tags.iter().enumerate() {
            let entry = 0x8015_0000 + k * 0x1000 + c as u32 * 0x100;
            cpu.mem.wr32(rec + 0x4C + c as u32 * 4, entry);
            cpu.mem.wr8(entry, t);
        }
        if tags.contains(&1) {
            walked += 1;
        } else {
            none += 1;
        }
        let queued = rng.next() as u8;
        let flags = rng.next() as u8;
        let hp = [0u16, 1, 300][rng.below(3) as usize];
        let shown = [0u16, 1, 300][rng.below(3) as usize];
        let facing = rng.next() as u16 & 0xFFF;
        let target = rng.below(9) as u8;
        let a = actor(r);
        cpu.mem.wr8(a + 0x1DA, queued);
        cpu.mem.wr8(a + 0x1DC, flags);
        cpu.mem.wr16(a + 0x14C, hp);
        cpu.mem.wr16(a + 0x172, shown);
        cpu.mem.wr16(a + 0x46, facing);
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x227, 0);
        cpu.calls.clear();
        assert_eq!(cpu.call(TAKEDOWN, &[r]), Stop::Returned, "round {round}");
        for va in [NAME_COPY, NAME_CAT, BANNER] {
            assert!(
                cpu.calls.contains(&va),
                "round {round}: {va:#x} not reached"
            );
        }
        assert_eq!(cpu.mem.rd8(a + 0x227), 1);
        let retail = (
            cpu.mem.rd8(a + 0x1DA),
            cpu.mem.rd8(a + 0x1DC),
            cpu.mem.rd16(a + 0x14C),
            cpu.mem.rd16(a + 0x172),
            cpu.mem.rd16(a + 0x46),
            cpu.mem.rd8(a + 0x1DD),
        );

        let m = &mut w.actors[usize::from(e)];
        m.battle_monster_id = Some(0x20);
        let b = &mut m.battle;
        b.queued_anim = queued;
        b.flag_bits = ActorFlags(flags);
        b.hp = hp;
        b.liveness = hp;
        b.hp_display = Some(shown);
        b.facing_angle = facing;
        b.active_target = target;
        let clips: Vec<_> = tags.iter().map(|&t| clip(t)).collect();
        w.set_actor_battle_action_clips(usize::from(e), std::sync::Arc::new(clips));
        w.battle_capture_takedown(e);
        let b = &w.actors[usize::from(e)].battle;
        let port = (
            b.queued_anim,
            b.flag_bits.0,
            b.hp,
            b.hp_display.unwrap_or(b.hp),
            b.facing_angle,
            b.active_target,
        );
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} seat {e} tags {tags:?}: retail {retail:x?} port {port:x?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(walked > 2_000 && none > 2_000, "{walked} {none}");
}
