//! Disc-gated: a battle's command surfaces sound their blips, and a
//! sustained cue stops when its clip gives way - on the native play window's
//! own frame body (`HeadlessPlayWindow`), its audio director over a
//! device-less output whose SPU the test clocks a video frame per tick.
//!
//! Every tick logs the director's commands (the cues that came due) and the
//! SPU's key-ons / key-offs (`Spu::note_trace`), so each assertion is about
//! what reached a voice, not what was asked for.
//!
//! - **Command blips.** `FUN_801D0748` calls the cue dispatcher
//!   `FUN_8004FCC8` with `0x21` / `0x22` / `0x23`, which rings `0x20` /
//!   `0x21` / `0x22`; the item window's list kernel rings its own `0x37` on a
//!   cancel. The round prompt's `Begin`, the ring's `Item` arm, the item
//!   list's cancel and the ring's cancel must each key a voice on the tick
//!   of the press.
//! - **Sustained cue.** A cue whose descriptor sets `+3 & 0x20` keys voices
//!   from 7 and loops; retail stops it from the battle anim commit
//!   (`FUN_8004AD80` -> `FUN_80017910` at `0x8004AE68`) once an actor's staged
//!   clip differs from its latched one. The fight must key one and key it
//!   off again, and no cue voice may sound for longer than any clip.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::AudioSink;
use legaia_engine_audio::note_trace::{NoteEvent, NoteEventKind, NoteTrace};
use legaia_engine_audio::spu::voice::VoiceBus;
use legaia_engine_battle_menus::battle_input::CommandPhase;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::audio_director::AudioCommand;
use legaia_engine_shell::window::HeadlessPlayWindow;

const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

/// SPU samples in one 60 Hz video frame.
const SAMPLES_PER_TICK: usize = 735;

/// Longer than any battle clip: a cue voice still sounding after this many
/// ticks is a voice nothing will stop.
const STUCK_TICKS: u32 = 600;

struct Rig {
    w: HeadlessPlayWindow,
    t: u32,
    seen_cmds: u64,
    seen_notes: usize,
    /// Per SPU voice: the tick a cue voice started sounding, while it does.
    sounding_since: [Option<u32>; 24],
    /// The longest unbroken run a cue voice sounded, `(ticks, voice)`.
    longest: (u32, usize),
}

/// What one tick did.
#[derive(Default)]
struct Tick {
    cmds: Vec<AudioCommand>,
    keys: Vec<NoteEvent>,
}

impl Tick {
    fn ring_due(&self, id: i16) -> bool {
        self.cmds.contains(&AudioCommand::RingDue(id))
    }
    fn keyed_on(&self, pred: impl Fn(u8) -> bool) -> bool {
        self.keys
            .iter()
            .any(|e| e.ev == NoteEventKind::On && pred(e.voice))
    }
}

impl Rig {
    fn open(disc: &str) -> Self {
        let w = HeadlessPlayWindow::open(std::path::Path::new(disc), "map03").expect("open");
        w.session()
            .bgm
            .as_ref()
            .expect("audio director")
            .audio()
            .with_spu(|s| s.note_trace = Some(NoteTrace::new()));
        Self {
            w,
            t: 0,
            seen_cmds: 0,
            seen_notes: 0,
            sounding_since: [None; 24],
            longest: (0, 0),
        }
    }

    fn world(&self) -> &legaia_engine_core::world::World {
        &self.w.session().host.world
    }

    fn step(&mut self, pad: u16) -> Tick {
        self.t += 1;
        self.w.display_frame(pad, 1);
        let bgm = self.w.session().bgm.as_ref().expect("audio director");
        let log = bgm.commands();
        let new = (log.count() - self.seen_cmds) as usize;
        self.seen_cmds = log.count();
        let recent: Vec<AudioCommand> = log.recent().copied().collect();
        assert!(
            new <= recent.len(),
            "more commands in a tick than the log keeps"
        );
        let cmds = recent[recent.len() - new..].to_vec();
        let (keys, sounding) = bgm.audio().with_spu(|spu| {
            for _ in 0..SAMPLES_PER_TICK {
                spu.tick();
            }
            let tr = spu.note_trace.as_mut().expect("trace");
            tr.advance(SAMPLES_PER_TICK as u64);
            let keys = tr.events[self.seen_notes..].to_vec();
            let sounding: Vec<bool> = spu
                .voices
                .iter()
                .map(|v| !v.is_off() && v.bus == VoiceBus::Sfx)
                .collect();
            (keys, sounding)
        });
        self.seen_notes += keys.len();
        for (v, on) in sounding.into_iter().enumerate() {
            if on {
                let since = *self.sounding_since[v].get_or_insert(self.t);
                if self.t - since > self.longest.0 {
                    self.longest = (self.t - since, v);
                }
            } else {
                self.sounding_since[v] = None;
            }
        }
        Tick { cmds, keys }
    }

    /// Walk a box on the encounter field, confirming now and then, until a
    /// battle opens.
    fn walk_into_battle(&mut self) {
        while self.world().mode != SceneMode::Battle {
            assert!(self.t < 4000, "no encounter opened a battle");
            let t = self.t + 1;
            let dir = [UP, RIGHT, DOWN, LEFT][((t / 90) % 4) as usize];
            self.step(dir | if t % 24 < 3 { CROSS } else { 0 });
        }
    }

    /// Idle until the command session reads `want`.
    fn wait_for(&mut self, what: &str, want: impl Fn(&CommandPhase) -> bool) {
        let deadline = self.t + 2000;
        loop {
            let phase = self.world().battle.command.as_ref().map(|s| &s.phase);
            if phase.is_some_and(&want) {
                return;
            }
            assert!(self.t < deadline, "never reached {what}: {phase:?}");
            self.step(0);
        }
    }

    /// Press `pad` for one tick, then release; the ticks the press spans.
    fn press(&mut self, pad: u16) -> Vec<Tick> {
        let mut out = vec![self.step(pad)];
        for _ in 0..3 {
            out.push(self.step(0));
        }
        out
    }

    /// Idle long enough for a window's glide to land.
    fn settle(&mut self) {
        for _ in 0..60 {
            self.step(0);
        }
    }
}

/// The press rang `ring` and the same tick keyed a one-shot cue voice
/// (`FUN_80016B6C` keys one-shots from voice 23 down).
fn assert_blip(what: &str, ticks: &[Tick], ring: i16) {
    let hit = ticks.iter().find(|k| k.ring_due(ring));
    let Some(hit) = hit else {
        let cmds: Vec<_> = ticks.iter().flat_map(|k| k.cmds.iter()).collect();
        panic!("{what}: ring {ring:#x} never came due: {cmds:?}");
    };
    assert!(
        hit.keyed_on(|v| v >= 20),
        "{what}: ring {ring:#x} came due but keyed no voice: {:?}",
        hit.keys.iter().map(|e| (e.ev, e.voice)).collect::<Vec<_>>()
    );
}

#[test]
fn battle_command_surfaces_sound_their_blips() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut rig = Rig::open(&disc);
    rig.walk_into_battle();

    rig.wait_for("the round prompt", |p| {
        matches!(p, CommandPhase::RoundPrompt { .. })
    });
    rig.settle();
    // `Begin` (Left): the shared tail `0x801D20D4`, `FUN_8004FCC8(0x21)`.
    let ticks = rig.press(LEFT);
    assert_blip("Begin", &ticks, 0x20);

    rig.wait_for("the ring", |p| matches!(p, CommandPhase::Menu { .. }));
    rig.settle();
    // The `Item` arm (Up): `0x801D13F8 -> 0x801D20D4`.
    let ticks = rig.press(UP);
    assert_blip("the Item arm", &ticks, 0x20);
    assert!(
        rig.world().battle.item_menu.is_some(),
        "the item window opened"
    );

    rig.settle();
    // Cancel out of the item list: the list kernel's `0x37`.
    let ticks = rig.press(CIRCLE);
    assert_blip("the item list's cancel", &ticks, 0x37);

    rig.wait_for("the ring again", |p| matches!(p, CommandPhase::Menu { .. }));
    rig.settle();
    // The ring's cancel: `0x801D11F4 -> 0x801D2968`, `FUN_8004FCC8(0x23)`.
    let ticks = rig.press(CIRCLE);
    assert_blip("the ring's cancel", &ticks, 0x22);
    eprintln!("[ran] four command blips keyed by t={}", rig.t);
}

#[test]
fn a_sustained_battle_cue_is_keyed_off_when_its_clip_gives_way() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut rig = Rig::open(&disc);
    rig.walk_into_battle();
    let start = rig.t;

    // Fight on pulsed confirms (Begin, Attack, Auto, the first target, the
    // round's Begin) until a sustained cue keys from voice 7.
    let mut keyed: Option<(u32, Vec<i16>)> = None;
    let mut released: Option<u32> = None;
    while rig.world().mode == SceneMode::Battle && rig.t < start + 6000 {
        let rel = rig.t - start;
        let tick = rig.step(if rel % 30 < 2 { CROSS } else { 0 });
        if keyed.is_none() && tick.keyed_on(|v| v == 7) {
            let rings = tick
                .cmds
                .iter()
                .filter_map(|c| match c {
                    AudioCommand::RingDue(id) => Some(*id),
                    _ => None,
                })
                .collect();
            keyed = Some((rig.t, rings));
            continue;
        }
        if keyed.is_some()
            && released.is_none()
            && tick
                .keys
                .iter()
                .any(|e| e.ev == NoteEventKind::Off && e.voice == 7)
        {
            assert!(
                tick.cmds.contains(&AudioCommand::SfxReleaseSustained),
                "the key-off is the sustained teardown's"
            );
            released = Some(rig.t);
        }
    }
    let (on_at, rings) = keyed.expect("the fight keyed a sustained cue");
    let off_at = released.unwrap_or_else(|| {
        panic!("the sustained cue keyed at t={on_at} (rings {rings:x?}) was never keyed off")
    });
    eprintln!(
        "[ran] sustained cue (rings {rings:x?}) keyed t={on_at}, released t={off_at}; \
         longest cue voice {} ticks (voice {})",
        rig.longest.0, rig.longest.1
    );
    assert!(off_at - on_at < STUCK_TICKS);
    assert!(
        rig.longest.0 < STUCK_TICKS,
        "cue voice {} sounded {} ticks",
        rig.longest.1,
        rig.longest.0
    );
}
