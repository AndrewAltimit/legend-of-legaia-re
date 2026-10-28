//! Disc-gated: `rayman`'s collapse rumble stops when the beat ends.
//!
//! The bridge collapse beat (`P2[7]`, global `13 + 92 + 7`) fires the
//! scene's effect stagers (`34 30 0C`, then `34 30 02` / `34 30 0A`), whose
//! render-mode-5 parts are positional sound emitters: the rumble (static cue
//! `0x3E`, voices 16..17) and a second loop (runtime cue `0x201`, voice 19).
//! Each loops while its beat flags are up (`0x5A0` / `0x5A1` for the rumble)
//! and halts once the beat clears them. The emitter arm itself never
//! releases a halted part's voices; the actor-list walk does, when it frees
//! the part (`FUN_8002519C`, `0x80025278..0x80025290`: a `FUN_80021DF4`
//! actor with `+0x5A == 5` takes `FUN_800250D4(+0xAC, +0xB0)`). A port that
//! drops the part without that call leaves both loops sounding for good.
//!
//! The SPU is clocked one video frame per tick (735 samples at the internal
//! rate) so the voices' envelopes run as they would under an output.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset.

use legaia_engine_audio::{AudioSink, Phase};
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::pad::PadButton;

type Sink = legaia_engine_audio::AudioOut;

/// The bridge collapse beat, as an op-`0x44` global record index.
const COLLAPSE_BEAT: u8 = 13 + 92 + 7;

#[test]
fn the_collapse_emitters_fall_silent_when_their_parts_halt() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut s = BootSession::<Sink>::open_disc_detached_audio(
        std::path::Path::new(&disc),
        &BootConfig::default(),
    )
    .expect("boot session");
    s.enter_scene_live("rayman", &FieldLiveOpts::default())
        .expect("enter rayman");
    for _ in 0..30 {
        s.host.world.set_pad(0);
        s.tick().expect("tick");
        s.step_audio_frame();
    }
    s.host
        .world
        .field_vm
        .pending_record_spawns
        .push(COLLAPSE_BEAT);
    // Every (cue, voice) an emitter part held, and the frame the last
    // emitter part left the ambient list.
    let mut emitters: Vec<(u32, u16)> = Vec::new();
    let mut gone_at = None;
    for f in 0..3000u32 {
        // A dialogue beat waits for Cross: press 2, release 14.
        let pad = if f % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        s.step_audio_frame();
        s.audio.as_ref().expect("audio enabled").with_spu(|spu| {
            let mut buf = vec![0i16; 735 * 2];
            spu.render_into(&mut buf);
        });
        let live: Vec<(u32, u16)> = s
            .host
            .world
            .ambient
            .fx
            .iter()
            .filter(|p| p.state.move_submode == 5)
            .map(|p| (p.state.actor_u32(0xAC), p.state.actor_u16(0xB0)))
            .collect();
        for e in &live {
            if !emitters.contains(e) {
                emitters.push(*e);
            }
        }
        if live.is_empty() && !emitters.is_empty() && gone_at.is_none() {
            gone_at = Some(f);
        }
        if gone_at.is_some_and(|g| f > g + 120) {
            break;
        }
    }
    eprintln!("[ran] emitters {emitters:x?}, parts gone at frame {gone_at:?}");
    assert!(
        emitters.contains(&(0x3E, 16)),
        "the beat seats the rumble emitter (cue 0x3E on voice 16): {emitters:x?}"
    );
    assert!(gone_at.is_some(), "the emitter parts halt and are freed");
    let audio = s.audio.as_ref().expect("audio enabled");
    for &(cue, voice) in &emitters {
        let count = if cue == 0x3E { 2 } else { 1 };
        for v in voice..voice + count {
            let p = audio.with_spu(|spu| spu.voices[usize::from(v)].adsr.phase);
            assert!(
                matches!(p, Phase::Release | Phase::Off),
                "emitter cue {cue:#x} voice {v} still sounds ({p:?}) after its part was freed"
            );
        }
    }
}
