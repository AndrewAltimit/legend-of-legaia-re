//! Disc + library-gated: every sounding **cue** voice (libsnd owner `0x21`)
//! of every retail mednafen state in the library is one the engine programs
//! register for register - pitch, ADSR words, left / right volume, reverb
//! send.
//!
//! A cue voice's note record (`0x801CDB50 + voice * 0x36`) keeps what the key
//! resolved to: `+0x0E` the note, `+0x14` the program, `+0x16` the tone,
//! `+0x18` the VAB slot. The slot's header (`*(0x80091508 + slot * 12)`)
//! names the bank, matched on disc by its size and counts (`+0x0C..+0x20`);
//! the engine uploads that PROT entry the way the hosts do and keys the tone
//! through the cue path. The velocity is not on the record. It is the volume
//! pair the caller hands `FUN_80065034`, and there are two callers:
//!
//! - **the cue ring and the direct keys** pass the voice-volume word
//!   `_DAT_80084580` halved, on both sides;
//! - **a positional emitter** (`legaia_engine_vm::sfx_emitter`, the part
//!   tick's render-mode-5 arm) writes its raw volume to the voices every tick
//!   after the key, so the register is the emitter law over the part as
//!   captured - read off the actor lists `0x8007C34C..0x8007C36C` - and the
//!   stored camera focus.
//!
//! Skip-pass: `LEGAIA_DISC_BIN` unset, `extracted/` or the save library
//! missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_audio::Spu;
use legaia_engine_audio::spu::ram::SpuAllocator;
use legaia_engine_audio::vab_bind::VabBank;
use legaia_engine_scene::scene::ProtIndex;
use legaia_engine_vm::sfx_emitter::{EmitterListener, emitter_volume};
use legaia_mednafen::SaveState;
use legaia_parity::audio_trace_oracle::load_runtime_audio_trace_from_save;
use legaia_ram_image::ScenarioManifest;

fn first_dir(cands: &[&str], probe: &str) -> Option<PathBuf> {
    cands
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join(probe).exists())
}

/// One emitter part as captured: its first voice, voice span and the raw
/// volume the law gives it at the captured focus.
struct CapturedEmitter {
    voice: i32,
    span: i32,
    volume: Option<(u16, u16)>,
}

#[test]
fn retail_cue_voices_match_the_engine_register_for_register() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir(
        &["extracted", "../extracted", "../../extracted"],
        "PROT.DAT",
    ) else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(root) = first_dir(&[".", "..", "../.."], "scripts/scenarios.toml") else {
        eprintln!("[skip] scripts/scenarios.toml missing");
        return;
    };
    let lib = root.join("saves/library");
    if !lib.join("mednafen").is_dir() {
        eprintln!("[skip] saves/library/mednafen missing");
        return;
    }
    let manifest = ScenarioManifest::from_path(root.join("scripts/scenarios.toml")).unwrap();
    let index = ProtIndex::open_extracted(&extracted).expect("prot index");

    // Every VAB header on the disc, keyed by its size + counts.
    let mut banks: BTreeMap<Vec<u8>, (u32, usize)> = BTreeMap::new();
    for e in 0..index.entry_count() as u32 {
        let Ok(b) = index.entry_bytes(e) else {
            continue;
        };
        let mut i = 0;
        while let Some(p) = b[i..].windows(4).position(|w| w == b"pBAV") {
            let o = i + p;
            if let Some(h) = b.get(o + 12..o + 0x20) {
                banks.entry(h.to_vec()).or_insert((e, o));
            }
            i = o + 4;
        }
    }
    let mut uploaded: BTreeMap<(u32, usize), Option<(Spu, VabBank)>> = BTreeMap::new();

    let (mut checked, mut ring, mut emitted) = (0usize, 0usize, 0usize);
    let mut misses = Vec::new();
    for scn in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(scn, &lib).filter(|p| p.exists()) else {
            continue;
        };
        let Ok(state) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = state.main_ram() else {
            continue;
        };
        let at = |va: u32| (va & 0x1F_FFFF) as usize;
        let rd16 = |va: u32| u16::from_le_bytes([ram[at(va)], ram[at(va) + 1]]);
        let rd32 = |va: u32| {
            let o = at(va);
            u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
        };
        let retail = load_runtime_audio_trace_from_save(&path).expect("retail SPU");
        let voice_volume = rd32(0x8008_4580) as i32;
        let mono = rd32(0x8008_46BC) != 0;

        // The emitter parts on the actor lists.
        let listener = EmitterListener {
            focus_stored: (rd32(0x8008_9118) as i32, rd32(0x8008_9120) as i32),
            voice_volume,
            mono,
            scale: (0x80, 0x80),
            field_mode: true,
            stream_settled: true,
            release_all: false,
            frame_step: 1,
            speed: 0x10,
        };
        let mut emitters = Vec::new();
        for head in (0x8007_C34C..0x8007_C370).step_by(4) {
            let mut node = rd32(head);
            for _ in 0..256 {
                if !(0x8000_0000..0x8020_0000).contains(&node) {
                    break;
                }
                if rd32(node + 0xC) == 0x8002_1DF4 && rd16(node + 0x5A) == 5 {
                    let cue = rd32(node + 0xAC) as i32;
                    // The descriptor's `+3 & 0x1F` voice count: the static
                    // table below `0x200`, the runtime bank `_DAT_8007B8D0`
                    // (row base `+2` halfword rounded to even) above it.
                    let row = if cue < 0x200 {
                        0x8006_F198 + cue as u32 * 8
                    } else {
                        let base = rd32(0x8007_B8D0);
                        let n = i32::from(rd16(base + 2) as i16);
                        base.wrapping_add((n / 2 * 2) as u32)
                            .wrapping_add((cue as u32 - 0x200) * 8)
                    };
                    let span = i32::from(ram[at(row + 3)] & 0x1F);
                    let s = |o: u32| i32::from(rd16(node + o) as i16);
                    emitters.push(CapturedEmitter {
                        voice: s(0xB0),
                        span: span.max(1),
                        volume: emitter_volume(
                            (s(0x14), s(0x18)),
                            (s(0x90), s(0x92)),
                            [0x9C, 0xA0, 0xA4, 0xA8].map(|o| rd32(node + o) as i32),
                            &listener,
                        ),
                    });
                }
                node = rd32(node);
            }
        }

        for (i, v) in retail.voices.iter().enumerate() {
            let rec = 0x801C_DB50 + i as u32 * 0x36;
            if rd16(rec + 0x10) != 0x21 || v.env_level.unwrap_or(0) == 0 {
                continue;
            }
            checked += 1;
            let f = |o: u32| rd16(rec + o);
            let (note, prog, tone, slot) = (f(0x0E), f(0x14), f(0x16), f(0x18));
            let hdr = at(rd32(0x8009_1508 + u32::from(slot) * 12));
            let Some(&(e, off)) = banks.get(&ram[hdr + 12..hdr + 0x20]) else {
                misses.push(format!(
                    "{} voice {i}: slot {slot} holds no disc bank",
                    scn.label
                ));
                continue;
            };
            let Some((spu, bank)) = uploaded
                .entry((e, off))
                .or_insert_with(|| {
                    let b = index.entry_bytes(e).ok()?;
                    let report = legaia_vab::parse(&b, off).ok()?;
                    let mut spu = Spu::new();
                    let mut alloc = SpuAllocator::new(0x1010, 0x7_0000);
                    let bank = VabBank::upload(&mut spu, &mut alloc, &report, &b);
                    Some((spu, bank))
                })
                .as_mut()
            else {
                misses.push(format!("{} voice {i}: PROT {e} does not parse", scn.label));
                continue;
            };
            let emitter = emitters
                .iter()
                .find(|em| (em.voice..em.voice + em.span).contains(&(i as i32)));
            let half = ((voice_volume as u32) << 15) as i32 >> 16;
            bank.play_tone_pair(
                spu,
                0,
                usize::from(prog),
                usize::from(tone),
                note as u8,
                half as i16,
                half as i16,
            );
            let ev = spu.voices[0].clone();
            let (mut l, mut r) = (ev.vol_left, ev.vol_right);
            if let Some(em) = emitter {
                emitted += 1;
                match em.volume {
                    Some((el, er)) => (l, r) = (el as i16, er as i16),
                    None => {
                        misses.push(format!("{} voice {i}: emitter out of range", scn.label));
                        continue;
                    }
                }
            } else {
                ring += 1;
                if mono {
                    let m = l.max(r);
                    (l, r) = (m, m);
                }
            }
            let adsr = ev.adsr_cfg.raw.0 as u32 | ((ev.adsr_cfg.raw.1 as u32) << 16);
            let engine = (
                Some(ev.pitch),
                Some(adsr),
                Some(l),
                Some(r),
                Some(ev.reverb_send),
            );
            let want = (
                v.pitch,
                v.adsr_control,
                v.vol_left,
                v.vol_right,
                v.reverb_send,
            );
            if engine != want {
                misses.push(format!(
                    "{} voice {i} (PROT {e} prog {prog} tone {tone} note {note}, emitter {}): \
                     retail {want:?} engine {engine:?}",
                    scn.label,
                    emitter.is_some()
                ));
            }
        }
    }
    eprintln!(
        "[ran] {checked} cue voices ({ring} ring / direct, {emitted} emitter), {} unmatched",
        misses.len()
    );
    assert!(checked > 0, "no cue voice in the library");
    assert!(misses.is_empty(), "{misses:#?}");
}

/// The engine seats the emitter the captured state holds. A live `town01`
/// session's ambient tree carries a render-mode-5 part with the captured
/// river emitter's cue, place, levels and box, keys it and drives its voices;
/// at the `rimelm_wall_press_left` focus its law gives that state's `1706`.
#[test]
fn town01_seats_and_drives_the_captured_river_emitter() {
    use legaia_engine_session::native::NativeSession;
    use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
    use legaia_engine_vm::sfx_emitter::{EMITTER_RENDER_MODE, EmitterOp};
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir(
        &["extracted", "../extracted", "../../extracted"],
        "PROT.DAT",
    ) else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("session");
    s.host.world.seed_free_roam_story_baseline("town01");
    s.enter_field_live("town01", &FieldLiveOpts::default())
        .expect("field live");
    let mut ops = Vec::new();
    for _ in 0..120 {
        s.tick().expect("tick");
        ops.extend(s.host.world.take_sfx_emitter_ops());
    }
    let part = s
        .host
        .world
        .ambient
        .fx
        .iter()
        .map(|p| &p.state)
        .find(|st| st.move_submode == EMITTER_RENDER_MODE)
        .expect("town01 seats a render-mode-5 part");
    let range = [0x9C, 0xA0, 0xA4, 0xA8].map(|o| part.actor_u32(o) as i32);
    assert_eq!(part.actor_u32(0xAC), 0x200, "cue");
    assert_eq!((part.world_x, part.world_z), (3968, 1024), "place");
    assert_eq!((part.tween_src_x, part.tween_src_y), (56, 56), "levels");
    assert_eq!(range, [-10000, 2500, 10000, -2000], "box");
    let listener = EmitterListener {
        focus_stored: (-1838, -2526),
        voice_volume: 200,
        mono: false,
        scale: (0x80, 0x80),
        field_mode: true,
        stream_settled: true,
        release_all: false,
        frame_step: 1,
        speed: 0x10,
    };
    assert_eq!(
        emitter_volume((3968, 1024), (56, 56), range, &listener),
        Some((1706, 1706))
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, EmitterOp::Key { cue: 0x200, .. })),
        "the session keys the emitter: {:?}",
        &ops[..ops.len().min(4)]
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, EmitterOp::Volume { cue: 0x200, .. })),
        "and drives its volume every tick"
    );
}
