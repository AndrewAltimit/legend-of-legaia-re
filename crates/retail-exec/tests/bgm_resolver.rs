//! The BGM swap poller `FUN_800243F0`'s track resolution: from a track word
//! `_DAT_8007BAC8` that differs from the loaded index, the poller's first three
//! calls (stages `0` and `1` with the CD idle, then stage `2`) reach the payload read
//! `FUN_8001FC00`, whose first argument is the raw TOC index it loads. Held
//! against the port's resolution, `music_labels::prot_entry_for_bgm_id` for a
//! global id and the scene-local fallback `SCENE_LOCAL_BGM_FALLBACK_ID` for a
//! local one, through the `+2` raw-to-extraction skew.
//!
//! Past the bank the poller reads on into the `vab_01` block, which carries no
//! score; the port resolves nothing there, and no shipped script asks.
//!
//! The bank base `_DAT_8007BC64` is seated at the raw `music_01` define the
//! running game holds (`990`); the scene block base `0x80084540` at a
//! generated block.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene_host::scene_host::SCENE_LOCAL_BGM_FALLBACK_ID;
use legaia_engine_system::music_labels::{MUSIC_BANK_SLOTS, prot_entry_for_bgm_id};
use legaia_overlay_images::boot_overlay::raw_to_extraction;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const POLLER: u32 = 0x8002_43F0;
const POLLER_END: u32 = 0x8002_4C80;
const PAYLOAD_READ: u32 = 0x8001_FC00;
const CD_SYNC: u32 = 0x8003_DE7C;
const TRACK: u32 = 0x8007_BAC8;
const REQUESTED: u32 = 0x8007_BAB8;
const LOADED: u32 = 0x8007_BA9C;
const BANK_BASE: u32 = 0x8007_BC64;
const SCENE_BASE: u32 = 0x8008_4540;
const TWO_PART_LATCH: u32 = 0x8007_B9B8;
/// `gp + 0x744`, the stage word.
const STAGE: u32 = 0x8007_BA5C;
/// The `music_01` define on a running image.
const MUSIC_01_RAW: u32 = 990;

#[test]
fn the_poller_loads_the_entry_the_port_resolves() {
    differential!(scus(POLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for (va, ret) in [
        (0x8001_9890, 0),
        (CD_SYNC, 0),
        (0x8002_66E0, 0),
        (0x8002_6520, 0),
        (0x8005_FB84, 0),
        (0x8001_A068, 0),
    ] {
        cpu.stub(va, ret);
    }
    let reads = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let r2 = reads.clone();
    cpu.hook(PAYLOAD_READ, move |r, _| {
        r2.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let mut rng = Rng(0x4247_4D52);
    let mut ids: Vec<u16> = (2000..2000 + MUSIC_BANK_SLOTS as u16).collect();
    ids.extend((0..64).map(|_| rng.below(40) as u16));
    let mut mismatched = Vec::new();
    for id in ids {
        // The ending theme stages its two streams through the field init with
        // the poller latched off (`_DAT_8007B9B8`); the poller never loads it.
        if id == legaia_engine_system::mode_entry_init::FIELD_BGM_TWO_PART_ID as u16 {
            continue;
        }
        cpu.mem.wr32(TWO_PART_LATCH, 0);
        // The dev arm's suppress (`_DAT_8007B868`), the sound flags and the
        // battle-music hold (`_DAT_8007B8B8`), all clear in play.
        for va in [0x8007_B868, 0x8007_B750, 0x8007_B8B8, 0x8007_BA88] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.wr32(0x1F80_0394, 0);
        cpu.mem.wr32(BANK_BASE, MUSIC_01_RAW);
        cpu.mem.wr32(SCENE_BASE, 0x200 + rng.below(0x200));
        cpu.mem.wr32(TRACK, u32::from(id));
        cpu.mem.wr32(LOADED, 0xFFFF);
        cpu.mem.wr32(STAGE, 0);
        reads.borrow_mut().clear();
        // Stages `0` and `1` wait on the drive; stage `2` kicks the read.
        for call in 0..3 {
            assert_eq!(cpu.call(POLLER, &[]), Stop::Returned, "id {id} call {call}");
        }
        let reads = reads.borrow();
        assert_eq!(
            reads.len(),
            1,
            "id {id}: one payload read, calls {:x?}",
            cpu.calls
        );
        let (raw, slot) = reads[0];
        assert_eq!(slot, 1, "id {id}: the BGM sound slot");
        let port_id = if id < 2000 {
            SCENE_LOCAL_BGM_FALLBACK_ID
        } else {
            id
        };
        let retail = raw_to_extraction(raw);
        let port = prot_entry_for_bgm_id(port_id);
        if retail != port {
            mismatched.push((id, retail, port));
        }
        // The change test's requested index: the bank slot for a global id,
        // the scene block's `+6` slot for a local one.
        let want_requested = if id < 2000 {
            cpu.mem.rd32(SCENE_BASE) + 6 + u32::from(id)
        } else {
            MUSIC_01_RAW + u32::from(id) - 2000
        };
        assert_eq!(cpu.mem.rd32(REQUESTED), want_requested, "id {id}");
    }
    assert!(
        mismatched.is_empty(),
        "id, retail extraction entry, port extraction entry: {mismatched:?}"
    );
}

/// The poller's second streaming slot, the side-band bank: with the track
/// settled and the request `_DAT_8007BABC` off its acknowledge
/// `_DAT_8007BAA0`, its stage walk (`0x800248B4..0x80024B30`) streams one
/// bank through `FUN_8001FC00` into slot `3` or `6` and acknowledges the
/// request, raising the forced-channel latch `_DAT_8007BA88` on a slot-`6`
/// load. Held against `side_band_bank_for_request` (the raw index is the
/// extraction entry `+ 2`) and the field host's latch rule (a slot-`6` bank
/// raises it); a park request (`0x1000`) and any request made under the
/// latch acknowledge at once and load nothing, as the port resolves neither.
#[test]
fn the_side_band_slot_streams_the_bank_the_port_resolves() {
    differential!(scus(POLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_core::world::{SIDE_BAND_PARK, VAB_01_RAW_BASE, side_band_bank_for_request};
    const REQUEST: u32 = 0x8007_BABC;
    const ACK: u32 = 0x8007_BAA0;
    const FORCED: u32 = 0x8007_BA88;
    /// `gp + 0x748`, the side-band stage word.
    const SIDE_STAGE: u32 = 0x8007_BA60;
    /// `gp + 0x72C`, the scene-local request a sub-2000 id leaves.
    const LOCAL_REQUEST: u32 = 0x8007_BA44;
    const VAB_01_BASE: u32 = 0x8007_BBE4;
    for (va, ret) in [
        (0x8001_9890, 0),
        (CD_SYNC, 0),
        (0x8002_66E0, 0),
        (0x8002_6520, 0),
        (0x8002_6478, 0),
        (0x8001_E54C, 0),
        (0x8002_630C, 0),
        (0x8002_614C, 0),
        (0x8005_67A8, 0),
        (0x8005_FB84, 0),
        (0x8001_A068, 0),
        (0x8003_E360, 0),
        (0x8005_2770, 1),
    ] {
        cpu.stub(va, ret);
    }
    let reads = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let r2 = reads.clone();
    cpu.hook(PAYLOAD_READ, move |r, _| {
        r2.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let mut rng = Rng(0x5349_4442);
    let (mut slot3, mut slot6, mut parked, mut latched) = (0, 0, 0, 0);
    for round in 0..600 {
        let id = match rng.below(8) {
            0 => SIDE_BAND_PARK,
            1 => rng.below(1_000) as i32,
            2 => 1_000 + rng.below(1_000) as i32,
            3 | 4 => 2_000 + rng.below(40) as i32,
            _ => 3_000 + rng.below(40) as i32,
        };
        let under_latch = rng.below(6) == 0;
        // The track settled: the requested index is the loaded one.
        for va in [0x8007_B868, 0x8007_B750, 0x8007_B8B8, TWO_PART_LATCH] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.wr32(0x1F80_0394, 0);
        cpu.mem.wr32(BANK_BASE, MUSIC_01_RAW);
        cpu.mem.wr32(TRACK, 2000);
        cpu.mem.wr32(LOADED, MUSIC_01_RAW);
        cpu.mem.wr32(STAGE, 0);
        cpu.mem.wr32(SCENE_BASE, 0x200 + rng.below(0x200));
        cpu.mem.wr32(VAB_01_BASE, VAB_01_RAW_BASE);
        cpu.mem.wr32(FORCED, u32::from(under_latch));
        cpu.mem.wr32(SIDE_STAGE, 0);
        cpu.mem.wr32(LOCAL_REQUEST, 0xDEAD);
        cpu.mem.wr32(REQUEST, id as u32);
        cpu.mem.wr32(ACK, 0xFFFF_FFFF);
        // The debug print's gate (`0x80024B38`): it writes nothing the port
        // models.
        cpu.mem.wr32(0x8007_B6D0, rng.below(2));
        reads.borrow_mut().clear();
        let mut calls = 0;
        while cpu.mem.rd32(ACK) != id as u32 {
            assert_eq!(cpu.call(POLLER, &[]), Stop::Returned, "round {round}");
            calls += 1;
            assert!(calls < 16, "round {round}: id {id} never acknowledged");
        }
        assert_eq!(
            cpu.mem.rd32(SIDE_STAGE),
            0,
            "round {round}: the walk is back at 0"
        );
        let port = if under_latch {
            None
        } else {
            side_band_bank_for_request(id)
        };
        let want: Vec<(u32, u32)> = port
            .iter()
            .map(|b| (b.prot_entry + 2, u32::from(b.slot)))
            .collect();
        // A slot-6 bank streams in two reads of the one entry (header, then
        // body with `a3 = 1` at `0x80024A50..0x80024A64`); the port names the
        // entry once.
        let mut got = reads.borrow().clone();
        got.dedup();
        assert_eq!(got, want, "round {round}: id {id}");
        let forced = under_latch || port.is_some_and(|b| b.slot == 6);
        assert_eq!(
            cpu.mem.rd32(FORCED) != 0,
            forced,
            "round {round}: id {id}: the forced-channel latch"
        );
        match port {
            Some(b) if b.slot == 6 => slot6 += 1,
            Some(_) => slot3 += 1,
            None if under_latch => latched += 1,
            None => parked += 1,
        }
        if port.is_some() && id < 2000 {
            assert_eq!(cpu.mem.rd32(LOCAL_REQUEST), id as u32, "round {round}");
        }
    }
    assert!(
        slot3 > 100 && slot6 > 100 && parked > 20 && latched > 40,
        "{slot3} {slot6} {parked} {latched}"
    );
}

/// The holds ahead of the poller's change test: with sound-flag bit 2 up, or
/// the track word at the park word `0x1000`, the poller copies the requested
/// index onto the loaded one (`0x80024560`) and no walk starts; otherwise the
/// two differ and stage 0 runs. Held against
/// `sound_state::bgm_request_hold`. The warm-entry word `_DAT_8007B8B8`
/// stays zero, so the unported battle-set hold is not entered.
#[test]
fn the_poller_holds_the_requests_the_port_holds() {
    differential!(scus(POLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_system::sound_state::{BGM_PARK_WORD, bgm_request_hold};
    const FLAGS: u32 = 0x8007_B750;
    cpu.stub_outside = Some((POLLER, POLLER_END));
    let mut rng = Rng(0x484F_4C44);
    let (mut held, mut walked) = (0, 0);
    for round in 0..2_000 {
        let track = match rng.below(4) {
            0 => BGM_PARK_WORD,
            _ => 2000 + rng.below(80) as u16,
        };
        let flags = rng.next() & 0x1F & !0x1;
        for va in [0x8007_B868, 0x8007_B8B8, TWO_PART_LATCH, 0x8007_BA88] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.wr32(0x1F80_0394, 0);
        cpu.mem.wr32(FLAGS, flags);
        cpu.mem.wr32(BANK_BASE, MUSIC_01_RAW);
        cpu.mem.wr32(TRACK, u32::from(track));
        cpu.mem.wr32(LOADED, 0xFFFF);
        cpu.mem.wr32(STAGE, 0);
        // The side-band slot settled.
        cpu.mem.wr32(0x8007_BABC, 0xFFFF_FFFF);
        cpu.mem.wr32(0x8007_BAA0, 0xFFFF_FFFF);
        assert_eq!(cpu.call(POLLER, &[]), Stop::Returned, "round {round}");
        let retail_held = cpu.mem.rd32(LOADED) == cpu.mem.rd32(REQUESTED);
        let port = bgm_request_hold(flags & 4 != 0, track);
        assert_eq!(
            retail_held,
            port.is_some(),
            "round {round}: {flags:#x} {track:#x}"
        );
        // A walk that starts leaves stage 0 behind it.
        assert_eq!(cpu.mem.rd32(STAGE) != 0, port.is_none(), "round {round}");
        if port.is_some() {
            held += 1;
        } else {
            walked += 1;
        }
    }
    assert!(held > 500 && walked > 500, "{held} {walked}");
}

/// The BGM stage walk from a track change to its install, call by call,
/// against `sound_state::BgmSwapPoller`. The convention: one poller call is
/// one game frame of `s` vsyncs (`DAT_1F800393 = s`); the change call is the
/// port's `start` and every later call one `tick(s, ..)`; the read clock is
/// `s * (4 + R)` - stages 0..3 one call each plus the `R` calls the drive
/// stays busy after the payload read - and the install walk `2 * s`
/// (stages 5 and 6). Compared each call: the install (the loaded index
/// latching onto the requested one at stage 6), the load-settled bit 3
/// rising while a commit is owed (sound-flag bit 0 without the
/// release-ack bit 4), and whether a swap is still in flight. The commit
/// arrives at a random call; the debug word gates a stall print.
#[test]
fn the_stage_walk_installs_when_the_port_installs() {
    differential!(scus(POLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_system::sound_state::{BgmSwapPoller, BgmSwapTick};
    const FLAGS: u32 = 0x8007_B750;
    cpu.stub_outside = Some((POLLER, POLLER_END));
    let busy = Rc::new(RefCell::new(0u32));
    let read_len = Rc::new(RefCell::new(0u32));
    {
        let (busy, read_len) = (busy.clone(), read_len.clone());
        cpu.hook(PAYLOAD_READ, move |_, _| {
            *busy.borrow_mut() = *read_len.borrow();
        });
    }
    {
        let busy = busy.clone();
        cpu.hook(CD_SYNC, move |r, _| {
            let mut b = busy.borrow_mut();
            r.r[psx_retail_exec::V0] = u32::from(*b > 0);
            *b = b.saturating_sub(1);
        });
    }
    let mut rng = Rng(0x5741_4C4B);
    let (mut installs, mut stalls) = (0, 0);
    for round in 0..400 {
        let s = 1 + rng.below(3);
        let r = rng.below(40);
        *read_len.borrow_mut() = r;
        let owed = rng.below(2) == 0;
        let commit_at = 2 + rng.below(60);
        for va in [0x8007_B868, 0x8007_B8B8, TWO_PART_LATCH, 0x8007_BA88] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.wr32(0x1F80_0394, 0);
        // The field (mode 3), with either value of the build halfword
        // `_DAT_8007B8C2`: outside the field init (mode 2) the settle drains
        // by the frame step either way. The already-playing latch
        // `_DAT_8007BAD0` and the script pause (bit 1) only skip the slot's
        // teardown and re-attach calls; neither moves the walk.
        cpu.mem.wr16(0x8007_B83C, 3);
        cpu.mem.wr16(0x8007_B8C2, rng.below(2) as u16);
        cpu.mem.wr32(0x8007_BAD0, rng.below(2));
        cpu.mem.wr32(0x8007_B6D0, rng.below(2));
        cpu.mem.wr32(FLAGS, u32::from(owed) | (rng.below(2) << 1));
        cpu.mem.wr8(0x1F80_0393, s as u8);
        cpu.mem.wr32(BANK_BASE, MUSIC_01_RAW);
        cpu.mem.wr32(TRACK, 2000 + rng.below(80));
        cpu.mem.wr32(LOADED, 0xFFFF);
        cpu.mem.wr32(STAGE, 0);
        cpu.mem.wr32(0x8007_BA80, 0);
        cpu.mem.wr32(0x8007_BABC, 0xFFFF_FFFF);
        cpu.mem.wr32(0x8007_BAA0, 0xFFFF_FFFF);
        *busy.borrow_mut() = 0;
        let mut port = BgmSwapPoller::default();
        let mut installed = false;
        for call in 1..200u32 {
            if owed && call == commit_at {
                let f = cpu.mem.rd32(FLAGS);
                cpu.mem.wr32(FLAGS, f | 0x10);
            }
            let flags = cpu.mem.rd32(FLAGS);
            let owed_now = flags & 1 != 0 && flags & 0x10 == 0;
            let before = cpu.mem.rd32(LOADED);
            assert_eq!(cpu.call(POLLER, &[]), Stop::Returned, "round {round}");
            let after_flags = cpu.mem.rd32(FLAGS);
            let latched = before != cpu.mem.rd32(LOADED);
            let tick = if call == 1 {
                port.start((s * (4 + r)) as u16);
                BgmSwapTick::Busy
            } else {
                port.tick(s as u16, owed_now, (2 * s) as u16)
            };
            assert_eq!(
                latched,
                tick == BgmSwapTick::Install,
                "round {round} call {call}: s {s} read {r} owed {owed} commit {commit_at}"
            );
            assert_eq!(
                flags & 8 == 0 && after_flags & 8 != 0,
                tick == BgmSwapTick::Settled,
                "round {round} call {call}: bit 3"
            );
            assert_eq!(
                cpu.mem.rd32(LOADED) != cpu.mem.rd32(REQUESTED),
                port.in_flight(),
                "round {round} call {call}: in flight"
            );
            stalls += u32::from(tick == BgmSwapTick::Settled);
            if latched {
                installed = true;
                installs += 1;
                break;
            }
        }
        assert!(installed, "round {round}: never installed");
    }
    assert!(installs == 400 && stalls > 50, "{installs} {stalls}");
}
