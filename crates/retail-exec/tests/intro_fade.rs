//! The transition tick `FUN_801CF5BC` (PROT 0979)'s second switch - the
//! per-style screen fade - against
//! `legaia_render_kernels::battle_intro_styles::intro_fade`: every style
//! selector, both sub-styles and every clock around each ramp, the entity
//! parked past the phase switch (phase `8`) and every call outside the
//! routine stubbed but the fade emitter `FUN_80024EE4`, whose arguments are
//! recorded. A selector past the table (`>= 5`) draws no fade in either.
//!
//! The first switch - the handoff phases - and the post-switch block run
//! against `battle_intro_transition::tick_transition`: every phase, every
//! battle-id class, both load-wait answers and every clock around the three
//! tail gates, each outside call recorded as the effect it ports.

use crate::common::{disc, scus_cpu};
use legaia_render_kernels::battle_intro_styles::{IntroStyle, intro_fade};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801C_F5BC;
const TICK_END: u32 = 0x801C_FBB4;
const FADE: u32 = 0x8002_4EE4;
const TOTAL: u32 = 0x801D_2458;
const STYLE: u32 = 0x801D_2460;
const SUB_STYLE: u32 = 0x801D_2464;
const ENTITY: u32 = 0x8012_0000;

#[test]
fn the_intro_fade_ramps_as_retail_does() {
    differential!(prot(979, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(979).expect("PROT 0979"));
    cpu.stub_outside = Some((TICK, TICK_END));
    let fades = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let f = fades.clone();
    cpu.hook(FADE, move |r, _| {
        f.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]))
    });
    let mut ramped = 0;
    for selector in 0..8 {
        for sub in [0, 2] {
            for total in [0x30, 0x60] {
                for elapsed in -2..total + 0x50 {
                    cpu.mem.load(ENTITY, &[0u8; 0x40]);
                    cpu.mem.wr16(ENTITY + 0x22, 8);
                    cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
                    cpu.mem.wr32(TOTAL, total as u32);
                    cpu.mem.wr32(STYLE, selector as u32);
                    cpu.mem.wr32(SUB_STYLE, sub as u32);
                    fades.borrow_mut().clear();
                    assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned);
                    let want: Vec<(u32, u32, u32)> = IntroStyle::from_selector(selector)
                        .and_then(|style| intro_fade(style, sub, elapsed, total))
                        .map(|f| (u32::from(f.layer), u32::from(f.abr), f.rgb))
                        .into_iter()
                        .collect();
                    ramped += want.len();
                    assert_eq!(
                        *fades.borrow(),
                        want,
                        "style {selector} sub {sub} total {total:#x} clock {elapsed}"
                    );
                }
            }
        }
    }
    assert!(ramped > 500, "{ramped}");
}

const BATTLE_ID: u32 = 0x8007_B880;
const FLAGS: u32 = 0x8007_BD60;
const ASSEMBLY: u32 = 0x8007_B648;
const ALT_BUNDLE: u32 = 0x8007_B64B;
const PHASE5_SELECTOR: u32 = 0x8007_B8C2;
const VOICE: u32 = 0x8007_0536;
const LEVEL: u32 = 0x8007_B910;
const HANDLE: u32 = 0x8007_BAAC;
const FORMATION: u32 = 0x8007_BD0C;
const CUE: u32 = 0x8007_B6D8;
const MODE: u32 = 0x8007_B83C;
const SPIN: [u32; 2] = [0x8007_B92C, 0x8007_B930];

#[test]
fn the_handoff_phases_tick_as_retail_does() {
    use legaia_render_kernels::battle_intro_transition::{
        TransitionEffect as E, TransitionEntity, TransitionGlobals, TransitionResponses,
        tick_transition,
    };
    differential!(prot(979, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(979).expect("PROT 0979"));
    cpu.stub_outside = Some((TICK, TICK_END));
    let calls = Rc::new(RefCell::new(Vec::<E>::new()));
    let busy = Rc::new(std::cell::Cell::new(0u32));
    macro_rules! record {
        ($va:expr, |$r:ident, $m:ident| $e:expr) => {{
            let c = calls.clone();
            cpu.hook($va, move |$r, $m| {
                let _ = (&$r, &$m);
                c.borrow_mut().push($e)
            });
        }};
    }
    record!(0x8006_2004, |r, m| E::ReapplyBgmLevel {
        voice_id: r.r[A0] as i16,
        level: r.r[A1] as i16,
    });
    record!(0x8005_2770, |r, m| E::AssembleBattleMeshes);
    record!(0x8005_67A8, |r, m| E::LoadBattleBgm {
        battle_id: r.r[A1] as i32
    });
    record!(0x8001_FC00, |r, m| {
        assert_eq!((r.r[A1], r.r[A3]), (5, 0), "bundle read arguments");
        E::LoadBattleBundle {
            bundle_id: r.r[A0] as i32,
            flags: m.rd32(r.r[psx_retail_exec::SP] + 0x10),
        }
    });
    record!(0x8003_E6BC, |r, m| E::Phase5Lookup);
    record!(0x8003_EB98, |r, m| E::Phase5AltLookup);
    record!(0x8002_6740, |r, m| {
        assert_eq!(r.r[A0], 0x8007_052C);
        E::StopFieldBgmVoice
    });
    record!(0x8002_66E0, |r, m| {
        assert_eq!(r.r[A0], 0x8007_051C);
        E::ReleaseFieldBgmSlot
    });
    record!(0x8001_E54C, |r, m| {
        assert_eq!((r.r[A0], r.r[A2]), (5, 0));
        E::InstallBattleSequence {
            bundle_handle: r.r[A1],
        }
    });
    record!(0x8002_6478, |r, m| {
        assert_eq!(r.r[A0], 0x8007_056C);
        E::StartBattleBgmVoice
    });
    {
        let (c, b) = (calls.clone(), busy.clone());
        cpu.hook(0x8003_DE7C, move |r, _| {
            assert_eq!(r.r[A0], 1);
            c.borrow_mut().push(E::WaitForLoad);
            r.r[psx_retail_exec::V0] = b.get();
        });
    }
    let mut rng = crate::common::Rng(0x4841_4E44);
    let mut seen = [0usize; 6];
    for case in 0..30_000 {
        let total = [0x84, 0xFC][rng.below(2) as usize];
        let g = TransitionGlobals {
            battle_id: [-2, -1, 0, 1, 0x2A][rng.below(5) as usize],
            battle_flags: [0, 0x80, 0x7F][rng.below(3) as usize],
            assembly_state: [0, 0x80, 0x81][rng.below(3) as usize],
            alt_default_bundle: rng.below(2) as u8,
            phase5_selector: rng.below(2) as i16,
            total_duration: total,
            bgm_voice_id: rng.next() as i16,
            audio_level: rng.below(0x100) as i32,
            bundle_handle: rng.next(),
            formation_slot0: [0, 0xA6, 0xA5][rng.below(3) as usize],
        };
        let near = [0, 3, 4, -0x1F, -0x1E, -0x1D, -7, -6, -5, -1, 0, 1, 2];
        let k = rng.below(near.len() as u32) as usize;
        let elapsed = if k < 3 { near[k] } else { total + near[k] };
        let mut e = TransitionEntity {
            phase: rng.below(10) as i16,
            elapsed: elapsed as i16,
            ready: rng.below(4) as u16,
        };
        let load_busy = rng.below(2);
        cpu.mem.load(ENTITY, &[0u8; 0x40]);
        cpu.mem.wr16(ENTITY + 0x22, e.phase as u16);
        cpu.mem.wr16(ENTITY + 0x1A, e.elapsed as u16);
        cpu.mem.wr16(ENTITY + 0x2A, e.ready);
        cpu.mem.wr32(TOTAL, total as u32);
        cpu.mem.wr32(STYLE, 7);
        cpu.mem.wr32(BATTLE_ID, g.battle_id as u32);
        cpu.mem.wr8(FLAGS, g.battle_flags);
        cpu.mem.wr8(ASSEMBLY, g.assembly_state);
        cpu.mem.wr8(ALT_BUNDLE, g.alt_default_bundle);
        cpu.mem.wr16(PHASE5_SELECTOR, g.phase5_selector as u16);
        cpu.mem.wr16(VOICE, g.bgm_voice_id as u16);
        cpu.mem.wr32(LEVEL, g.audio_level as u32);
        cpu.mem.wr32(HANDLE, g.bundle_handle);
        cpu.mem.wr8(FORMATION, g.formation_slot0);
        cpu.mem.wr16(CUE, 0xBEEF);
        cpu.mem.wr16(MODE, 0);
        for a in SPIN {
            cpu.mem.wr32(a, rng.next());
        }
        busy.set(load_busy);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned, "case {case}");

        let tick = tick_transition(
            &mut e,
            &g,
            &TransitionResponses {
                load_busy: load_busy as i32,
            },
        );
        let tag = format!("case {case}: {g:?} {e:?} busy {load_busy}");
        let (cues, effects): (Vec<E>, Vec<E>) = tick
            .effects
            .iter()
            .cloned()
            .partition(|x| matches!(x, E::SetAudioCue { .. }));
        assert_eq!(*calls.borrow(), effects, "{tag}: calls");
        let cue = cues.last().map_or(0xBEEF, |c| match c {
            E::SetAudioCue { cue } => *cue,
            _ => unreachable!(),
        });
        assert_eq!(cpu.mem.rd16(CUE), cue, "{tag}: cue");
        assert_eq!(cpu.mem.rd16(ENTITY + 0x22) as i16, e.phase, "{tag}: phase");
        assert_eq!(cpu.mem.rd16(ENTITY + 0x2A), e.ready, "{tag}: ready");
        assert_eq!(
            cpu.mem.rd16(MODE) == 0x14,
            tick.entered_battle_mode,
            "{tag}: mode"
        );
        let cleared = g.battle_id != 0 && cpu.mem.rd32(BATTLE_ID) == 0;
        assert_eq!(
            cleared,
            tick.cleared_battle_id && g.battle_id != 0,
            "{tag}: id"
        );
        for a in SPIN {
            assert_eq!(cpu.mem.rd32(a), 0, "{tag}: the spin accumulators");
        }
        seen[0] += usize::from(!effects.is_empty());
        seen[1] += usize::from(tick.entered_battle_mode);
        seen[2] += usize::from(tick.cleared_battle_id);
        seen[3] += usize::from(effects.contains(&E::StopFieldBgmVoice));
        seen[4] += usize::from(effects.contains(&E::StartBattleBgmVoice));
        seen[5] += usize::from(cues.len() == 2);
    }
    assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
}
