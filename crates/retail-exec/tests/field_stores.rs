//! Four small field-overlay routines whose whole effect is a handful of
//! stores, each held store for store against its port: the draw-context
//! primer `FUN_801DE37C` against `field_draw_context`, the CARD request leaf
//! `FUN_801D84B4` against `request_card_mode`, the menu-button state pick
//! `FUN_801F1F4C` against `state_pick`, and the clip-fraction fade tick
//! `FUN_801D820C` against `clip_fraction_step`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_state_pick::{StatePickInputs, state_pick};
use legaia_engine_mode::card_request::request_card_mode;
use legaia_engine_system::mode_entry_init::field_draw_context;
use legaia_engine_vm::actor_tick::{ActorPhysics, clip_fraction_step};
use psx_retail_exec::{Cpu, Stop};

const PRIMER: u32 = 0x801D_E37C;
const CARD: u32 = 0x801D_84B4;
const STATE_PICK: u32 = 0x801F_1F4C;
const CLIP_FADE: u32 = 0x801D_820C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTOR: u32 = 0x8013_0000;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8014_0000;
const FRAME_STEP: u32 = 0x1F80_0393;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

#[test]
fn the_draw_context_primer_and_the_card_request_store_what_retail_stores() {
    differential!(prot(897, PRIMER), prot(897, CARD));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for at in (0x8008_40B8..0x8008_40C4).step_by(4) {
        cpu.mem.wr32(at, 0xAAAA_AAAA);
    }
    assert_eq!(cpu.call(PRIMER, &[]), Stop::Returned);
    let c = field_draw_context();
    assert_eq!(
        (
            cpu.mem.rd32(0x8008_40B8),
            cpu.mem.rd32(0x8008_40BC),
            cpu.mem.rd32(0x8008_40C0)
        ),
        c.camera_block
    );
    assert_eq!(
        (
            cpu.mem.rd16(0x8007_B790),
            cpu.mem.rd16(0x8007_B792),
            cpu.mem.rd16(0x8007_B794)
        ),
        c.screen_triple
    );
    let w = cpu.mem.read(0x1F80_03E8, 4);
    assert_eq!(
        (w[0] as i8, w[1] as i8, w[2] as i8, w[3] as i8),
        c.view_window
    );

    assert_eq!(cpu.call(CARD, &[]), Stop::Returned);
    let r = request_card_mode();
    assert_eq!(cpu.mem.rd16(0x8007_B83C) as i16, r.game_mode);
    assert_eq!(cpu.mem.rd32(0x8007_BB00), r.flag);
}

#[test]
fn the_menu_button_state_pick_matches_retail() {
    differential!(prot(897, STATE_PICK));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SCENE_PTR, SCENE);
    let mut rng = Rng(0x5049_434B);
    let mut debug = 0;
    for _ in 0..4000 {
        let inputs = StatePickInputs {
            script_resume_slot: if rng.below(3) == 0 { rng.next() } else { 0 },
            debug_mode: if rng.below(2) == 0 { rng.next() } else { 0 },
            pad_mask: rng.next() & !0x100 | if rng.below(2) == 0 { 0x100 } else { 0 },
        };
        let state = rng.next() as u16;
        cpu.mem.wr32(0x8007_B450, inputs.script_resume_slot);
        cpu.mem.wr32(0x8007_B98C, inputs.debug_mode);
        cpu.mem.wr32(0x8007_B850, inputs.pad_mask);
        cpu.mem.wr16(ACTOR + 0x50, state);
        cpu.mem.wr16(ACTOR + 0x54, rng.next() as u16);
        cpu.mem.wr16(SCENE + 0x2E, rng.next() as u16);
        cpu.mem.wr16(SCENE + 0x40, rng.next() as u16);
        assert_eq!(cpu.call(STATE_PICK, &[ACTOR]), Stop::Returned);
        let w = state_pick(inputs, state);
        assert_eq!(
            cpu.mem.rd16(SCENE + 0x2E) as i16,
            w.scene_slot_2e,
            "{inputs:?}"
        );
        assert_eq!(
            cpu.mem.rd16(SCENE + 0x40),
            w.scene_saved_state,
            "{inputs:?}"
        );
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), w.actor_state, "{inputs:?}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), w.actor_substate, "{inputs:?}");
        debug += u32::from(w.actor_state == 0x13);
    }
    assert!(debug > 200, "{debug}");
}

#[test]
fn the_clip_fraction_fade_matches_retail() {
    differential!(prot(897, CLIP_FADE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x434C_4950);
    let mut retired = 0;
    for run in 0..2000 {
        let mut p = ActorPhysics {
            timer: [0x10i16, 0x40, 0x200, 0x1000, -0x40, rng.next() as i16][rng.below(6) as usize],
            focal_envelope: rng.below(0x1000) as i16,
            status_flags: rng.next() & !8,
            ..Default::default()
        };
        cpu.mem.wr16(ACTOR + 0x54, p.timer as u16);
        cpu.mem.wr16(ACTOR + 0x78, p.focal_envelope as u16);
        cpu.mem.wr32(ACTOR + 0x10, p.status_flags);
        for frame in 0..30 {
            let dt = [0u8, 1, 2, 3, 0xFF][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, dt);
            assert_eq!(cpu.call(CLIP_FADE, &[ACTOR]), Stop::Returned);
            retired += u32::from(clip_fraction_step(&mut p, dt));
            let label = format!("run {run} frame {frame}");
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x78) as i16,
                p.focal_envelope,
                "{label}"
            );
            assert_eq!(cpu.mem.rd32(ACTOR + 0x10), p.status_flags, "{label}");
        }
    }
    assert!(retired > 2000, "{retired}");
}
