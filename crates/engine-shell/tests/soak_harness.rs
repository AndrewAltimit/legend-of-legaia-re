//! Soak / softlock harness: seeded, game-shaped pseudo-random pad input over
//! every playable scene, with automatic failure detectors.
//!
//! Every other progression instrument in this crate walks a route someone
//! chose (`critical_path_replay`, `menu_replay`, `minigame_replay`) or scores
//! one rung per scene (`chapter1_frontier_ladder`). A scripted ladder never
//! steps on the input a player produces by accident - the confirm pressed
//! during a door fade, the menu opened on the frame a battle starts, the
//! direction held into a shop picker. This harness produces that input on
//! purpose and watches for the engine falling over.
//!
//! See `docs/tooling/soak-harness.md` for the policies, the detectors, how to
//! run a long soak, and how to reproduce a finding. In short:
//!
//! - **Driver.** One fresh [`BootSession`] per run (scene, seed): new-game
//!   party, live loop and player-driven battles armed, an empty card rack
//!   mounted, then `frames` ticks of pad input from a seeded [`Policy`]. The
//!   only actuator is `World::set_pad`; FMVs are skipped through the shared
//!   hand-off kernel, as the headless `play` subcommand does.
//! - **Detectors.** Panic (per-tick `catch_unwind`), `tick()` error, unknown
//!   scene id, softlock (the progress digest frozen for a window while input
//!   varies), a pause menu that will not close, a battle that never ends, a
//!   drop to `Title`, non-finite camera floats, absurd values (HP / MP over
//!   max, money, bag invariants), unbounded queue growth, and a hung tick
//!   (wall-clock watchdog).
//! - **Output.** `target/soak/<tag>/report.md` plus one `j-replay-v1` file per
//!   distinct finding signature (pad stream only), truncated to the finding
//!   frame and confirmed to reproduce.
//!
//! Tests:
//!
//! - `soak_smoke_no_panics` - a small fixed budget (a few scenes x one seed x
//!   a few hundred frames) that asserts no panic, tick error or hang. The
//!   CI-shaped gate.
//! - `soak_long` - opt-in, runs only when `LEGAIA_SOAK_SEEDS` or
//!   `LEGAIA_SOAK_FRAMES` is set. Report-only unless `LEGAIA_SOAK_STRICT=1`.
//! - `soak_replay` - opt-in, runs only when `LEGAIA_SOAK_REPLAY=<file>` is set:
//!   replays one saved finding and prints what the detectors see.
//! - `soak_fixtures` - replays every committed fixture under
//!   `scripts/replays/soak/` and reports whether each still reproduces.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset. The
//! assets come from `LEGAIA_EXTRACTED_DIR`, else `<repo>/extracted`, else
//! straight from the disc image.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::hash::Hasher;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use legaia_engine_core::input::PadButton;
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::save_screen::card_port_snapshot;
use legaia_engine_core::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_core::scene::{Scene, SceneTickEvent, is_world_map_scene};
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_shell::replay::{ReplayFile, ReplayMeta};

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn env_u64(key: &str) -> Option<u64> {
    let v = std::env::var(key).ok()?;
    let v = v.trim();
    if let Some(hex) = v.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        v.parse().ok()
    }
}

fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Where the session reads its assets from.
#[derive(Clone, Debug)]
enum Source {
    Extracted(PathBuf),
    Disc(PathBuf),
}

fn source() -> Option<Source> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from);
    let Some(disc) = disc.filter(|p| p.exists()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated convention)");
        return None;
    };
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(repo_root().join("extracted"));
    for d in candidates {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(Source::Extracted(d));
        }
    }
    Some(Source::Disc(disc))
}

fn open_session(src: &Source) -> BootSession {
    let cfg = BootConfig {
        scene: legaia_engine_shell::boot::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    match src {
        Source::Extracted(d) => BootSession::open(d, &cfg).expect("open BootSession (extracted)"),
        Source::Disc(p) => BootSession::open_disc(p, &cfg).expect("open BootSession (disc)"),
    }
}

fn out_dir(tag: &str) -> PathBuf {
    let base = std::env::var_os("LEGAIA_SOAK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target").join("soak"));
    base.join(tag)
}

// ---------------------------------------------------------------------------
// Scene set
// ---------------------------------------------------------------------------

/// Every CDNAME label that resolves to a playable scene (a kingdom overworld,
/// or a scene whose field MAN resolves), unioned with the decoded `0x3F`
/// destinations of those scenes that also resolve.
fn scene_set(session: &BootSession) -> Vec<String> {
    use legaia_engine_core::man_field_scripts::scene_destinations;
    let index = &session.host.index;
    let playable = |name: &str| -> bool {
        if is_world_map_scene(name) {
            return Scene::load(index, name).is_ok();
        }
        Scene::load(index, name)
            .ok()
            .and_then(|s| s.field_man_payload(index).ok().flatten())
            .is_some()
    };
    let mut set: BTreeSet<String> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        if playable(&name) {
            set.insert(name);
        }
    }
    let mut extra = BTreeSet::new();
    for name in &set {
        let Some(man) = Scene::load(index, name)
            .ok()
            .and_then(|s| s.field_man_payload(index).ok().flatten())
        else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        for d in scene_destinations(&mf, &man) {
            if !set.contains(&d.scene_name) && playable(&d.scene_name) {
                extra.insert(d.scene_name);
            }
        }
    }
    set.extend(extra);
    set.into_iter().collect()
}

// ---------------------------------------------------------------------------
// RNG
// ---------------------------------------------------------------------------

/// SplitMix64 - tiny, seedable, and stable across platforms.
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n.max(1))) as u32
    }
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.below(hi - lo + 1)
    }
    fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }
}

/// Per-(scene, seed) policy seed, so one scene's run does not depend on the
/// order the scene list happens to be in.
fn run_seed(scene: &str, seed: u64) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in scene.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const DIRS: [u16; 4] = [UP, RIGHT, DOWN, LEFT];

fn b(p: PadButton) -> u16 {
    p.mask()
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    /// Hold one or two directions; occasionally tap Cross.
    Walk { dirs: u16, confirm_every: u32 },
    /// Alternate Cross / Circle taps.
    Mash,
    /// Press Start once; the menu arm below takes over while it is open.
    OpenMenu,
    /// Neutral pad.
    Idle,
    /// A fresh random mask every few frames, every button but Start.
    Chaos,
}

/// The seeded input source. A pure function of its seed and the observed
/// session, so `(scene, seed, frames)` alone regenerates a run; the recorded
/// pad stream is what the replay file carries.
struct Policy {
    rng: Rng,
    seg: Segment,
    seg_left: u32,
    /// Frames the pause menu has been open this visit.
    menu_frames: u32,
    /// How long this menu visit browses before it starts backing out.
    menu_budget: u32,
    /// Chaos mask currently held and for how long.
    chaos_mask: u16,
    chaos_left: u32,
    /// Tap duty cycle: the last frame's mask.
    last: u16,
}

impl Policy {
    fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            seg: Segment::Idle,
            seg_left: 0,
            menu_frames: 0,
            menu_budget: 0,
            chaos_mask: 0,
            chaos_left: 0,
            last: 0,
        }
    }

    /// A tap: the wanted mask on one frame, neutral on the next. Every UI
    /// surface reads `just_pressed`, so a held mask is one event.
    fn tap(&mut self, want: u16) -> u16 {
        if self.last != 0 { 0 } else { want }
    }

    fn random_face(&mut self) -> u16 {
        match self.rng.below(10) {
            0..=4 => b(PadButton::Cross),
            5..=7 => b(PadButton::Circle),
            8 => b(PadButton::Triangle),
            _ => b(PadButton::Square),
        }
    }

    fn random_dir(&mut self) -> u16 {
        DIRS[self.rng.below(4) as usize]
    }

    fn next_segment(&mut self) {
        let r = self.rng.below(100);
        let (seg, len) = if r < 42 {
            let d0 = self.random_dir();
            let dirs = if self.rng.chance(25) {
                // A diagonal: two adjacent directions.
                let i = DIRS.iter().position(|&d| d == d0).unwrap_or(0);
                d0 | DIRS[(i + 1) % 4]
            } else {
                d0
            };
            let confirm_every = if self.rng.chance(35) {
                self.rng.range(8, 40)
            } else {
                0
            };
            (
                Segment::Walk {
                    dirs,
                    confirm_every,
                },
                self.rng.range(15, 160),
            )
        } else if r < 67 {
            (Segment::Mash, self.rng.range(20, 140))
        } else if r < 77 {
            (Segment::OpenMenu, 2)
        } else if r < 87 {
            (Segment::Idle, self.rng.range(5, 60))
        } else {
            (Segment::Chaos, self.rng.range(20, 120))
        };
        self.seg = seg;
        self.seg_left = len;
    }

    fn pad(&mut self, s: &BootSession, frame: u64) -> u16 {
        let pad = self.pad_inner(s, frame);
        self.last = pad;
        pad
    }

    fn pad_inner(&mut self, s: &BootSession, frame: u64) -> u16 {
        let w = &s.host.world;
        // Pause menu open: browse at random, then back out with Circle.
        if s.field_menu.is_some() {
            if self.menu_frames == 0 {
                self.menu_budget = self.rng.range(30, 420);
            }
            self.menu_frames += 1;
            if self.menu_frames > self.menu_budget {
                return self.tap(b(PadButton::Circle));
            }
            let want = match self.rng.below(10) {
                0..=4 => self.random_dir(),
                5..=6 => b(PadButton::Cross),
                7..=8 => b(PadButton::Circle),
                _ => b(PadButton::Triangle),
            };
            return self.tap(want);
        }
        self.menu_frames = 0;

        match w.mode {
            SceneMode::Battle => {
                if self.rng.chance(75) {
                    let want = fight_pad(s);
                    if want != 0 {
                        return self.tap(want);
                    }
                }
                let want = if self.rng.chance(50) {
                    self.random_dir()
                } else {
                    self.random_face()
                };
                self.tap(want)
            }
            SceneMode::Field | SceneMode::WorldMap => {
                // A dialogue or a picker owns the pad: mostly confirm, some
                // directions (pickers), some cancel.
                if w.dialogue_owns_input() && self.rng.chance(70) {
                    let want = match self.rng.below(10) {
                        0..=5 => b(PadButton::Cross),
                        6..=7 => self.random_dir(),
                        _ => b(PadButton::Circle),
                    };
                    return self.tap(want);
                }
                self.field_pad(frame)
            }
            // Minigames, cutscenes, the title: every face button and held
            // directions, and now and then Start - the minigames' escape
            // (`World::poll_minigame_escape`), so the exit paths are soaked
            // too without cutting every session short.
            _ => {
                if self.rng.below(1500) == 0 {
                    return self.tap(b(PadButton::Start));
                }
                if self.chaos_left == 0 {
                    self.chaos_left = self.rng.range(2, 12);
                    self.chaos_mask = match self.rng.below(4) {
                        0 => self.random_dir(),
                        1 => self.random_face(),
                        2 => 0,
                        _ => self.random_dir() | self.random_face(),
                    };
                }
                self.chaos_left -= 1;
                if self.chaos_mask & 0xF000 != 0 {
                    self.tap(self.chaos_mask)
                } else {
                    self.chaos_mask
                }
            }
        }
    }

    fn field_pad(&mut self, frame: u64) -> u16 {
        if self.seg_left == 0 {
            self.next_segment();
        }
        self.seg_left -= 1;
        match self.seg {
            Segment::Walk {
                dirs,
                confirm_every,
            } => {
                if confirm_every != 0 && frame.is_multiple_of(u64::from(confirm_every)) {
                    dirs | b(PadButton::Cross)
                } else {
                    dirs
                }
            }
            Segment::Mash => {
                let want = if self.rng.chance(70) {
                    b(PadButton::Cross)
                } else {
                    b(PadButton::Circle)
                };
                self.tap(want)
            }
            Segment::OpenMenu => self.tap(b(PadButton::Start)),
            Segment::Idle => 0,
            Segment::Chaos => {
                if self.chaos_left == 0 {
                    self.chaos_left = self.rng.range(2, 8);
                    let mut m = 0u16;
                    for bit in 0..16u16 {
                        let mask = 1u16 << bit;
                        if mask == b(PadButton::Start) {
                            continue;
                        }
                        if self.rng.chance(12) {
                            m |= mask;
                        }
                    }
                    self.chaos_mask = m;
                }
                self.chaos_left -= 1;
                self.chaos_mask
            }
        }
    }
}

/// The battle fighter's wanted press this frame (see `critical_path_replay`'s
/// `FightPolicy`): Begin, Attack, Auto, confirm the target, Cross any message
/// box. `0` when nothing obvious is wanted.
fn fight_pad(s: &BootSession) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    use legaia_engine_core::inventory_use::InventoryUseState;
    let w = &s.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return b(PadButton::Cross);
    }
    if let Some(menu) = w.battle.item_menu.as_ref() {
        return match menu.state {
            InventoryUseState::Browsing { .. } => b(PadButton::Circle),
            InventoryUseState::TargetSelect { .. } => b(PadButton::Cross),
            _ => 0,
        };
    }
    if let Some(session) = w.battle.command.as_ref() {
        return match &session.phase {
            CommandPhase::RoundPrompt { .. } => LEFT,
            CommandPhase::Menu { .. } => LEFT,
            CommandPhase::AttackMode { .. } => LEFT,
            CommandPhase::Targeting { .. } => b(PadButton::Cross),
            CommandPhase::CommitConfirm { .. } => LEFT,
            _ => 0,
        };
    }
    b(PadButton::Cross)
}

// ---------------------------------------------------------------------------
// Progress digest
// ---------------------------------------------------------------------------

/// `fmt::Write` straight into a hasher, so a `Debug` rendering is hashed
/// without being allocated.
struct HashWriter(std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

fn player_slot(s: &BootSession) -> usize {
    s.host.world.player_actor_slot.unwrap_or(0) as usize
}

/// Everything a player could see move. Softlock = this frozen for a whole
/// window while the pad varies. Deliberately broad: a false "frozen" costs a
/// triage, a false "moving" only a missed finding.
fn progress_digest(s: &BootSession) -> u64 {
    let w = &s.host.world;
    // `DefaultHasher::new()` is keyed with fixed zeros, so the digest is
    // stable across runs and processes.
    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
    let _ = write!(h, "{:?}|{}|", w.mode, w.active_scene_label);
    if let Some(a) = w.actors.get(player_slot(s)) {
        let m = &a.move_state;
        let _ = write!(h, "p{},{},{},{}|", m.world_x, m.world_y, m.world_z, m.y_rot);
    }
    let _ = write!(h, "pc{}|", w.field_pc);
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        // A timeline-owned dialog panel pages through a conversation - and a
        // picker loops it back to its own segment - while `tl.pc` stays on
        // the segment's lead byte; the slice counter and the panel's cursor
        // are what move.
        let _ = write!(
            h,
            "tl{}|{}|{:?}|",
            tl.pc,
            tl.frames,
            tl.dialog.as_ref().map(|d| d.pc)
        );
    }
    for hc in &w.field_vm.helper_contexts {
        let _ = write!(h, "hc{}|", hc.pc);
    }
    let _ = write!(h, "{:?}|{:?}|", w.dialog.current, w.dialog.inline.is_some());
    if let Some(inline) = w.dialog.inline.as_ref() {
        let _ = write!(h, "{inline:?}|");
    }
    if let Some(m) = s.field_menu.as_ref() {
        let _ = write!(h, "menu{m:?}|");
    }
    if let Some(sub) = s.field_menu_sub.as_ref() {
        let _ = write!(h, "sub{}|", sub.row().index());
    }
    let _ = write!(h, "{:?}|{}|", w.shops.pending_shop, w.shops.shop_open);
    let _ = write!(h, "{:?}|", w.party.name_entry);
    let _ = write!(h, "{:?}|", w.cutscene.active_fmv);
    // Scripted NPC glides: an actor walking off-screen is still progress.
    for (slot, m) in &w.npcs.motions {
        let _ = write!(
            h,
            "npc{slot}{:?}{:?}|",
            w.npcs.positions.get(slot),
            m.target
        );
    }
    match w.mode {
        SceneMode::Battle => {
            let _ = write!(
                h,
                "{:?}|{:?}|{}|",
                w.battle.command,
                w.battle.item_menu,
                w.battle.tutorial_boxes.len()
            );
            let _ = write!(h, "{:?}|", w.battle_ctx);
            for a in w.actors.iter().take(8) {
                let _ = write!(h, "{},{},{}|", a.battle.hp, a.battle.mp, a.battle.liveness);
            }
        }
        SceneMode::Dance => {
            let _ = write!(h, "{:?}", w.minigames.dance);
        }
        SceneMode::Fishing => {
            let _ = write!(
                h,
                "{:?}|{:?}",
                w.minigames.fishing, w.minigames.fishing_exchange
            );
        }
        SceneMode::SlotMachine => {
            let _ = write!(h, "{:?}", w.minigames.slot_machine);
        }
        SceneMode::BakaFighter => {
            let _ = write!(h, "{:?}", w.minigames.baka_fighter);
        }
        _ => {
            let _ = write!(
                h,
                "{:?}{:?}",
                w.minigames.muscle_dome.is_some(),
                w.minigames.muscle_contest.is_some()
            );
        }
    }
    let _ = write!(h, "${}|bag{}", w.party.money, w.party.inventory.len());
    h.0.finish()
}

/// The shop / prize-counter session's share of the progress digest: its
/// menu context and whatever sub-session it holds.
fn menu_digest(m: &MenuRuntime) -> u64 {
    if !m.is_open() {
        return 0;
    }
    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
    let _ = write!(
        h,
        "{:?}|{:?}|{:?}",
        m.ctx, m.shop_session, m.recipient_session
    );
    h.0.finish()
}

/// Step an open shop / prize-counter / inn session on this frame's edges,
/// then unpark the field script a closed one left suspended - the play
/// window's `tick_menu_runtime_session`.
fn tick_menu_session(
    menu: &mut MenuRuntime,
    world: &mut legaia_engine_core::world::World,
    edge: u16,
) {
    if menu.is_open() {
        let input = legaia_engine_core::menu_runtime::menu_input_from_pad_edges(edge);
        menu.tick(world, input);
    }
    if world.shops.shop_open && !menu.is_open() {
        world.finish_field_shop();
    }
    if world.shops.prize_exchange_open && !menu.is_open() {
        world.finish_prize_exchange();
    }
}

/// What holds the frame, as a short signature fragment.
fn held_by(s: &BootSession) -> String {
    let w = &s.host.world;
    let op_at = |bc: &[u8], pc: usize| bc.get(pc).copied().unwrap_or(0xFF);
    if let Some(m) = s.field_menu.as_ref() {
        let sub = s
            .field_menu_sub
            .as_ref()
            .map(|x| format!("{:?}", x.row()))
            .unwrap_or_else(|| "root".into());
        return format!("pause-menu:{sub}:{}", variant(&format!("{:?}", m.phase())));
    }
    match w.mode {
        SceneMode::Battle => {
            if !w.battle.tutorial_boxes.is_empty() {
                return "battle:message-box".into();
            }
            if let Some(m) = w.battle.item_menu.as_ref() {
                return format!("battle:item-menu:{}", variant(&format!("{:?}", m.state)));
            }
            if let Some(c) = w.battle.command.as_ref() {
                return format!("battle:command:{}", variant(&format!("{:?}", c.phase)));
            }
            // The absorbing HP-bar pair (`hp != hp_display` with a zero
            // accumulator) is what parks the `0x51` bar-drain gate for good;
            // name it so every such park groups under one signature.
            let absorbing = w.actors.iter().take(8).any(|a| {
                a.battle
                    .hp_display
                    .is_some_and(|d| d != a.battle.hp && a.battle.hp_bar_pending == 0)
            });
            return format!(
                "battle:state:{}{}",
                variant(&format!("{:?}", w.battle_ctx.action_state)),
                if absorbing { ":hp-bar-absorbing" } else { "" }
            );
        }
        SceneMode::Field | SceneMode::WorldMap => {}
        // The pond names its sub-screen: an idle shore waiting for a cast
        // and a prize list that will not close are different parks, and a
        // reduction must not trade one for the other.
        SceneMode::Fishing => {
            let Some(f) = w.minigames.fishing.as_ref() else {
                return "mode:Fishing".into();
            };
            let screen = if w.minigames.fishing_exchange.is_some() {
                "exchange".to_string()
            } else if let Some(hub) = f.hub() {
                format!("hub-{}", variant(&format!("{:?}", hub.screen)))
            } else {
                variant(&format!("{:?}", f.phase()))
            };
            return format!("mode:Fishing:{screen}");
        }
        other => return format!("mode:{other:?}"),
    }
    if w.party.name_entry.is_some() {
        return "name-entry".into();
    }
    if w.shops.shop_open || w.shops.pending_shop.is_some() {
        return "shop".into();
    }
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        return format!(
            "timeline@{:#06x}:op{:02x}",
            tl.pc,
            op_at(&tl.bytecode, tl.pc)
        );
    }
    if w.dialog.current.is_some() || w.dialog.inline.is_some() {
        return "dialogue".into();
    }
    if let Some(hc) = w.field_vm.helper_contexts.first() {
        return format!("helper@{:#06x}:op{:02x}", hc.pc, op_at(&hc.bytecode, hc.pc));
    }
    let m = &w.actors[player_slot(s).min(w.actors.len().saturating_sub(1))].move_state;
    format!(
        "free-roam:immobile@tile({},{})",
        (m.world_x - 0x40) >> 7,
        (m.world_z - 0x40) >> 7
    )
}

/// One triage line: mode, scene, holder, player, and the bytes at every live
/// script context's PC.
fn trace_line(s: &BootSession, pad: u16) -> String {
    let w = &s.host.world;
    let bytes = |bc: &[u8], pc: usize| -> String {
        bc.get(pc..(pc + 6).min(bc.len()))
            .map(|b| {
                b.iter()
                    .map(|x| format!("{x:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let m = &w.actors[player_slot(s).min(w.actors.len().saturating_sub(1))].move_state;
    let mut out = format!(
        "pad {pad:04x} {:?} {} held={} player=({},{},{}) field_pc={:#x}",
        w.mode,
        w.active_scene_label,
        held_by(s),
        m.world_x,
        m.world_y,
        m.world_z,
        w.field_pc
    );
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        let _ = write!(out, " tl@{:#x}[{}]", tl.pc, bytes(&tl.bytecode, tl.pc));
    }
    for hc in &w.field_vm.helper_contexts {
        let _ = write!(out, " hc@{:#x}[{}]", hc.pc, bytes(&hc.bytecode, hc.pc));
    }
    if w.dialog.current.is_some() || w.dialog.inline.is_some() {
        out.push_str(" dialog");
    }
    if w.mode == SceneMode::Battle {
        let _ = write!(
            out,
            " ctx(state {:?} active {} timer {} menu_open {} end {:?} game_over {}/{})",
            w.battle_ctx.action_state,
            w.battle_ctx.active_actor,
            w.battle_ctx.frame_timer,
            w.battle_ctx.menu_open,
            w.battle.end,
            w.game_over,
            w.game_over_hold
        );
        for (i, a) in w.actors.iter().take(8).enumerate() {
            if a.battle.max_hp > 0 || a.battle.liveness != 0 {
                let _ = write!(
                    out,
                    " a{i}:{}/{}L{}D{:?}P{}A{}T{}",
                    a.battle.hp,
                    a.battle.max_hp,
                    a.battle.liveness,
                    a.battle.hp_display,
                    a.battle.hp_bar_pending,
                    a.battle.damage_accum,
                    a.battle.active_target
                );
            }
        }
    }
    if let Some(ps) = w.menu.pause_session.as_ref() {
        let _ = write!(
            out,
            " travel({} @ ({},{}) handler {:#04x})",
            ps.target.scene, ps.target.tile_x, ps.target.tile_z, ps.handler_id
        );
    }
    if let Some(f) = w.minigames.fishing.as_ref() {
        let _ = write!(
            out,
            " pond(phase {:?} hub {:?} exchange {} power {} depth {} tension {})",
            f.phase(),
            f.hub(),
            w.minigames.fishing_exchange.is_some(),
            f.cast_power(),
            f.depth(),
            f.tension()
        );
    }
    for (slot, m) in &w.npcs.motions {
        let _ = write!(
            out,
            " npc{slot}@{:?}->({},{})",
            w.npcs.positions.get(slot),
            m.target.0,
            m.target.1
        );
    }
    out
}

/// `Foo { .. }` / `Foo(..)` / `Foo` -> `Foo`.
fn variant(debug: &str) -> String {
    debug
        .split([' ', '{', '(', ','])
        .next()
        .unwrap_or("")
        .to_string()
}

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Finding {
    detector: &'static str,
    /// The scene the run started in.
    start_scene: String,
    /// The scene the world was in when the detector fired.
    scene: String,
    mode: String,
    frame: u64,
    /// Detector-specific location: a panic's `file:line`, a softlock's
    /// holder, a value check's field.
    location: String,
    detail: String,
}

impl Finding {
    fn signature(&self) -> String {
        format!("{}|{}|{}", self.detector, self.scene, self.location)
    }
    /// Rank bucket: lower is worse.
    fn rank(&self) -> u8 {
        match self.detector {
            "panic" | "hang" | "tick_error" => 0,
            "softlock" | "menu_stuck" | "battle_endless" | "dropped_to_title" => {
                if self.location.starts_with("free-roam") {
                    2
                } else {
                    1
                }
            }
            "unknown_scene" => 1,
            "script_stall" => 2,
            "battle_loop" => 1,
            _ => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Panic capture
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_PANIC: std::cell::RefCell<Option<(String, String, String)>> =
        const { std::cell::RefCell::new(None) };
}

static HOOK: Once = Once::new();

fn install_panic_hook() {
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let in_soak = std::thread::current()
                .name()
                .is_some_and(|n| n.starts_with("soak-"));
            if !in_soak {
                default(info);
                return;
            }
            let loc = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "?".into());
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string payload>".into()
            };
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<&str> = bt
                .lines()
                .map(str::trim)
                .filter(|l| l.contains("legaia_") && !l.contains("soak_harness"))
                .take(6)
                .collect();
            LAST_PANIC.with(|p| {
                *p.borrow_mut() = Some((loc, msg, frames.join(" <- ")));
            });
        }));
    });
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct RunSpec {
    scene: String,
    seed: u64,
    frames: u64,
}

#[derive(Default, Clone, Debug)]
struct RunStats {
    frames_run: u64,
    battles: u32,
    battles_won: u32,
    wiped: bool,
    scenes_entered: BTreeSet<String>,
    modes: BTreeSet<String>,
    menu_opens: u32,
    fmvs: u32,
    save_commits: u32,
    /// Pause-menu sub-screens entered and shops opened - what the random
    /// browse actually reached.
    menu_rows: BTreeSet<String>,
    shop_opens: u32,
    round_trips: u32,
}

struct RunOutcome {
    spec: RunSpec,
    findings: Vec<Finding>,
    pads: Vec<u16>,
    stats: RunStats,
    enter_ok: bool,
}

struct Tunables {
    softlock_frames: u64,
    digest_every: u64,
    menu_close_frames: u64,
    battle_frames: u64,
    queue_cap: usize,
    /// `LEGAIA_SOAK_TRACE=N`: print a state line every N frames (replay
    /// triage; 0 = off).
    trace_every: u64,
    /// Distinct pad masks the softlock window must contain. `1` turns the
    /// detector into a neutral-pad control (`LEGAIA_SOAK_MIN_DISTINCT=1`).
    min_distinct: usize,
}

impl Tunables {
    fn from_env() -> Self {
        Self {
            softlock_frames: env_u64("LEGAIA_SOAK_SOFTLOCK_FRAMES").unwrap_or(1800),
            digest_every: 15,
            menu_close_frames: 900,
            battle_frames: env_u64("LEGAIA_SOAK_BATTLE_FRAMES").unwrap_or(18_000),
            queue_cap: 4096,
            trace_every: env_u64("LEGAIA_SOAK_TRACE").unwrap_or(0),
            min_distinct: env_u64("LEGAIA_SOAK_MIN_DISTINCT").unwrap_or(3) as usize,
        }
    }
}

/// Shared with the watchdog: what each worker is doing right now.
#[derive(Clone, Debug, Default)]
struct WorkerStatus {
    tick_started: Option<Instant>,
    scene: String,
    seed: u64,
    frame: u64,
}

type StatusBoard = Arc<Mutex<Vec<WorkerStatus>>>;

fn value_checks(s: &BootSession, t: &Tunables, out: &mut Vec<(&'static str, String, String)>) {
    let w = &s.host.world;
    let cam = &s.camera;
    let floats = [
        ("camera.eye", cam.eye.iter().all(|v| v.is_finite())),
        ("camera.look_at", cam.look_at.iter().all(|v| v.is_finite())),
        (
            "camera.angles",
            cam.yaw.is_finite() && cam.pitch.is_finite() && cam.roll.is_finite(),
        ),
        (
            "camera.follow",
            cam.follow_distance.is_finite() && cam.follow_height.is_finite(),
        ),
        (
            "cutscene.caption_alpha",
            w.cutscene.caption_alpha.is_finite(),
        ),
    ];
    for (field, ok) in floats {
        if !ok {
            out.push(("non_finite", field.to_string(), format!("{cam:?}")));
        }
    }
    for (i, m) in w.party.roster.members.iter().enumerate() {
        let hms = m.hp_mp_sp();
        if hms.hp_max > 0 && hms.hp_cur > hms.hp_max {
            out.push((
                "value",
                format!("roster.hp>max(slot{i})"),
                format!("{}/{}", hms.hp_cur, hms.hp_max),
            ));
        }
        if hms.mp_max > 0 && hms.mp_cur > hms.mp_max {
            out.push((
                "value",
                format!("roster.mp>max(slot{i})"),
                format!("{}/{}", hms.mp_cur, hms.mp_max),
            ));
        }
        // `+0x130` is the character level (`CharacterRecord::level`).
        let lv = m.level();
        if lv == 0 || lv > 99 {
            out.push(("value", format!("roster.level(slot{i})"), format!("{lv}")));
        }
    }
    if w.mode == SceneMode::Battle {
        for (i, a) in w.actors.iter().take(8).enumerate() {
            if a.battle.max_hp > 0 && a.battle.hp > a.battle.max_hp {
                out.push((
                    "value",
                    format!("battle.hp>max(actor{i})"),
                    format!("{}/{}", a.battle.hp, a.battle.max_hp),
                ));
            }
        }
    }
    if w.party.money < 0 || w.party.money > 99_999_999 {
        out.push(("value", "party.money".into(), format!("{}", w.party.money)));
    }
    let slots = w.party.inventory.slots();
    let (lo, hi) = w.party.inventory.window_bounds();
    let mut seen: HashMap<u8, usize> = HashMap::new();
    for (i, &(id, count)) in slots.iter().enumerate() {
        if count > 99 {
            out.push((
                "value",
                "bag.count>99".into(),
                format!("slot{i} id{id} x{count}"),
            ));
        }
        if id == 0 && count != 0 {
            out.push((
                "value",
                "bag.free-slot-count".into(),
                format!("slot{i} x{count}"),
            ));
        }
        if id != 0
            && (lo..hi).contains(&i)
            && let Some(prev) = seen.insert(id, i)
        {
            out.push((
                "value",
                "bag.duplicate-stack".into(),
                format!("id{id} slots {prev},{i}"),
            ));
        }
    }
    let queues = [
        ("pending_field_events", w.pending_field_events.len()),
        ("pending_battle_events", w.pending_battle_events.len()),
        ("pending_actor_spawns", w.pending_actor_spawns.len()),
        ("helper_contexts", w.field_vm.helper_contexts.len()),
        (
            "pending_record_spawns",
            w.field_vm.pending_record_spawns.len(),
        ),
        ("battle.hit_fx", w.battle.hit_fx.len()),
        ("battle.effect_spawns", w.battle.effect_spawns.len()),
        ("debug_effects", w.debug_effects.len()),
        ("actors", w.actors.len()),
    ];
    for (q, n) in queues {
        if n > t.queue_cap {
            let detail = if q == "pending_field_events" {
                let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                for ev in &w.pending_field_events {
                    *kinds.entry(variant(&format!("{ev:?}"))).or_default() += 1;
                }
                format!("len {n}: {kinds:?}")
            } else {
                format!("len {n}")
            };
            out.push(("unbounded_growth", q.to_string(), detail));
        }
    }
}

/// The presentation queues both play hosts drain after every tick (the
/// window's `drain_and_route_field_events` / `drain_and_log_battle_events` /
/// CLUT-stage and effect-spawn routing, the browser runtime's twins).
/// `BootSession::tick` routes only BGM events off the field queue and drains
/// none of the rest, so a headless driver that skipped this would measure its
/// own leak rather than the engine.
fn host_drains(session: &mut BootSession) {
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
    let _ = w.drain_battle_hit_fx();
    let _ = w.drain_battle_hit_events();
    let _ = w.drain_battle_sfx_cues();
    let _ = w.drain_battle_shout_cues();
    let _ = w.drain_battle_xa_cues();
    let _ = w.drain_battle_xa_prestage();
    let _ = w.route_battle_effect_spawns();
    let _ = w.drain_battle_clut_stages();
    let _ = w.drain_minigame_sfx_cues();
}

fn run_one(
    src: &Source,
    known_scenes: &BTreeSet<String>,
    spec: &RunSpec,
    pads_override: Option<&[u16]>,
    t: &Tunables,
    status: Option<(&StatusBoard, usize)>,
) -> RunOutcome {
    let mut session = open_session(src);
    session.begin_new_game();
    session.set_save_rack(
        SaveRack::CardPorts(vec![
            card_port_snapshot(0, Some("MEMORY CARD")),
            card_port_snapshot(1, None),
        ]),
        vec![(0..15).map(SlotSnapshot::empty).collect(), Vec::new()],
    );
    let mut card = SoakCard::new();
    session.host.world.rng_state = (run_seed(&spec.scene, spec.seed) >> 16) as u32;
    let opts = FieldLiveOpts {
        // `LEGAIA_SOAK_NO_ENCOUNTERS=1` disarms the random-encounter roll - a
        // triage control that separates a scripted battle from a random one.
        live_loop: !env_flag("LEGAIA_SOAK_NO_ENCOUNTERS"),
        player_battle: true,
        battle_bgm: None,
    };
    let mut findings: Vec<Finding> = Vec::new();
    let mut stats = RunStats::default();
    let mut pads = Vec::with_capacity(spec.frames as usize);

    let mk = |session: &BootSession,
              detector: &'static str,
              frame: u64,
              location: String,
              detail: String| Finding {
        detector,
        start_scene: spec.scene.clone(),
        scene: session.host.world.active_scene_label.clone(),
        mode: format!("{:?}", session.host.world.mode),
        frame,
        location,
        detail,
    };

    // `<venue>+mg<sub_id>`: enter the venue, then request the mode-24 door
    // warp the op-`0x3E` arm makes (`World::request_minigame_warp`), so the
    // host's next tick loads the minigame overlay through the retail path.
    let (label, shop_visits) = split_shop(&spec.scene);
    let (label, round_trip) = split_round_trip(label);
    let (scene, minigame) = split_minigame(label);
    let entered = catch_unwind(AssertUnwindSafe(|| {
        let r = session.enter_scene_live(scene, &opts);
        if let Some(id) = minigame {
            // The casino doors refuse a player with no coins (the cabinet
            // record's coin compare), so a run arriving through the door
            // arrives with some: the one non-pad precondition these runs set.
            session.host.world.minigames.casino_coins = 500;
            session.host.world.request_minigame_warp(id);
        }
        r
    }));
    let enter_ok = match entered {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            findings.push(mk(
                &session,
                "tick_error",
                0,
                "enter_scene_live".into(),
                format!("{e:#}"),
            ));
            false
        }
        Err(_) => {
            let (loc, msg, frames) = LAST_PANIC
                .with(|p| p.borrow_mut().take())
                .unwrap_or_default();
            findings.push(mk(
                &session,
                "panic",
                0,
                loc,
                format!("[enter] {msg} | {frames}"),
            ));
            false
        }
    };
    if !enter_ok {
        return RunOutcome {
            spec: spec.clone(),
            findings,
            pads,
            stats,
            enter_ok,
        };
    }

    // A round-trip run starts with one save on the card, so the pause
    // menu's Load row (open in every scene; Save is per-scene) has a file
    // to resume - an empty card refuses before any commit.
    if round_trip {
        let sf = session.host.world.save_full();
        let resume = session.current_resume();
        card.insert((0, 0), (sf, resume));
        refresh_rack(&mut session, &card);
    }
    let mut policy = Policy::new(run_seed(&spec.scene, spec.seed));
    let mut menu_rt = MenuRuntime::new(std::env::temp_dir().join("legaia-soak-menu"));
    let mut last_digest = progress_digest(&session) ^ menu_digest(&menu_rt);
    let mut last_change = 0u64;
    // Script stall: the modal timeline (or first helper) parked at one PC for
    // a whole window, even while something else moves.
    let mut park: Option<(String, u64, u64)> = None;
    let mut menu_open_since: Option<u64> = None;
    let mut battle_since: Option<u64> = None;
    let mut prev_mode = session.host.world.mode;
    let mut prev_scene = session.host.world.active_scene_label.clone();
    let mut reported: BTreeSet<String> = BTreeSet::new();
    let mut traced_scripts: BTreeSet<u64> = BTreeSet::new();
    let mut battle_sites: BTreeMap<String, u32> = BTreeMap::new();
    let mut prev_pad = 0u16;

    for frame in 0..spec.frames {
        let pad = match pads_override {
            Some(p) => p.get(frame as usize).copied().unwrap_or(0),
            None => policy.pad(&session, frame),
        };
        pads.push(pad);
        let was_menu_open = session.field_menu.is_some();
        let was_shop_open = session.host.world.shops.shop_open;
        // While a shop / prize counter is up the pad drives it, not the
        // field (the play window's `field_pad`).
        session
            .host
            .world
            .set_pad(if menu_rt.is_open() { 0 } else { pad });
        let menu_suspended = menu_rt.suspends_field();
        if let Some((board, i)) = status {
            let mut b = board.lock().unwrap();
            b[i].tick_started = Some(Instant::now());
            b[i].frame = frame;
        }
        // The name-entry overlay is host-modal on both play hosts: while it
        // is up the field tick is skipped, every pad edge routes into the
        // entry SM, and the frame counter keeps advancing (the window's
        // redraw arm, the browser's `name_entry_input`). `BootSession::tick`
        // does not do this, so the harness does it for it.
        let edge = pad & !prev_pad;
        prev_pad = pad;
        let r = catch_unwind(AssertUnwindSafe(|| {
            if menu_suspended {
                // A shop / prize exchange freezes the field whole; only the
                // menu session steps (the play window's suspended arm).
                tick_menu_session(&mut menu_rt, &mut session.host.world, edge);
                Ok(SceneTickEvent::Stepped)
            } else if session.host.world.name_entry_active() {
                let input = legaia_engine_core::name_entry::NameEntryInput::from_pad_edge(edge);
                session.host.world.step_name_entry(input);
                session.host.world.frame = session.host.world.frame.wrapping_add(1);
                Ok(SceneTickEvent::Stepped)
            } else {
                session.tick()
            }
        }));
        if let Some((board, i)) = status {
            board.lock().unwrap()[i].tick_started = None;
        }
        stats.frames_run = frame + 1;
        let event = match r {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => {
                findings.push(mk(
                    &session,
                    "tick_error",
                    frame,
                    variant(&format!("{e}")),
                    format!("{e:#}"),
                ));
                break;
            }
            Err(_) => {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("{msg} | {frames}"),
                ));
                break;
            }
        };
        // Both play hosts drain the field-event queue after the tick (the
        // window's `drain_and_route_field_events`, the browser runtime's
        // drain); `BootSession::tick` routes only the BGM events and restores
        // the rest, so a headless driver that skipped this would measure its
        // own leak. Checked before the drain, then drained.
        if session.host.world.pending_field_events.len() > t.queue_cap {
            let mut v = Vec::new();
            value_checks(&session, t, &mut v);
            for (det, loc, detail) in v {
                if det == "unbounded_growth" && reported.insert(format!("{det}|{loc}")) {
                    findings.push(mk(&session, det, frame, loc, detail));
                }
            }
        }
        host_drains(&mut session);
        // A shop or prize counter the tick opened goes to the menu runtime
        // and gets this frame's edge; an open session steps here every
        // unsuspended frame (the play window's frame tail).
        if !menu_suspended {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let w = &mut session.host.world;
                if let Some(shop) = w.take_pending_field_shop() {
                    menu_rt.open_shop_menu(shop);
                }
                if let Some(ex) = w.take_pending_prize_exchange() {
                    menu_rt.open_prize_exchange(ex);
                }
                tick_menu_session(&mut menu_rt, w, edge);
            }));
            if r.is_err() {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[menu runtime] {msg} | {frames}"),
                ));
                break;
            }
        }
        match &event {
            SceneTickEvent::SceneEntered { name } => {
                stats.scenes_entered.insert(name.clone());
                if !known_scenes.contains(name) {
                    findings.push(mk(
                        &session,
                        "unknown_scene",
                        frame,
                        format!("label:{name}"),
                        "entered a label outside the playable scene set".into(),
                    ));
                }
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                findings.push(mk(
                    &session,
                    "unknown_scene",
                    frame,
                    format!("map_id:{map_id}"),
                    "scene transition to a map id with no scene".into(),
                ));
            }
            SceneTickEvent::Stepped => {}
        }
        if !was_menu_open && session.field_menu.is_some() {
            stats.menu_opens += 1;
        }
        if let Some(sub) = session.field_menu_sub.as_ref() {
            stats.menu_rows.insert(format!("{:?}", sub.row()));
        }
        if session.host.world.shops.shop_open && !was_shop_open {
            stats.shop_opens += 1;
        }
        // Save screen: both play hosts persist a Save pick and resume a Load
        // pick (`apply_save_commit`); `BootSession` only latches the pick.
        // The harness keeps the card in memory, so a later Load in the same
        // run reads back what an earlier Save wrote.
        if let Some(commit) = session.last_save_commit.take() {
            stats.save_commits += 1;
            let r = catch_unwind(AssertUnwindSafe(|| {
                apply_save_commit(&mut session, &mut card, commit, &opts)
            }));
            if r.is_err() {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[save commit] {msg} | {frames}"),
                ));
                break;
            }
        }
        // `<scene>+shop`: walk up to one of the scene's merchants every
        // `SHOP_VISIT_EVERY` free field frames - the priced session the
        // field VM's op `0x49` stages, handed to the menu runtime as the play
        // window hands it. Random walking almost never reaches a merchant and
        // picks Buy, so without this the shop UI is never soaked.
        if shop_visits
            && !menu_rt.is_open()
            && frame % SHOP_VISIT_EVERY == SHOP_VISIT_EVERY - 1
            && round_trip_ready(&session)
        {
            let w = &mut session.host.world;
            let n = w.shops.scene_shops.len();
            if n > 0
                && let Some(shop) = w.scene_shop_session(stats.shop_opens as usize % n)
            {
                w.shops.shop_open = true;
                menu_rt.open_shop_menu(shop);
                stats.shop_opens += 1;
            }
        }
        // `<scene>+rt`: a save / load round trip at a free field frame every
        // `ROUND_TRIP_EVERY` frames - `save_full`, then `resume_save` of that
        // file the way a Load resumes it, then `save_full` again. What the
        // second save says that the first did not is state the file does not
        // carry, or a resume that does not restore what it read.
        if round_trip
            && frame % ROUND_TRIP_EVERY == ROUND_TRIP_EVERY - 1
            && round_trip_ready(&session)
        {
            stats.round_trips += 1;
            let r = catch_unwind(AssertUnwindSafe(|| {
                let before = session.host.world.save_full();
                let resume = session.current_resume();
                let _ = session.resume_save(before.clone(), &resume.scene, &opts);
                let after = session.host.world.save_full();
                save_diff(&before, &after)
            }));
            match r {
                Ok(Some((field, detail))) => {
                    if reported.insert(format!("save_roundtrip|{field}")) {
                        findings.push(mk(&session, "save_roundtrip", frame, field, detail));
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    let (loc, msg, frames) = LAST_PANIC
                        .with(|p| p.borrow_mut().take())
                        .unwrap_or_default();
                    findings.push(mk(
                        &session,
                        "panic",
                        frame,
                        loc,
                        format!("[round trip] {msg} | {frames}"),
                    ));
                    break;
                }
            }
        }
        // FMV: skip the movie, then run the shared post-play hand-off - the
        // headless `play` subcommand's order.
        let fmv = catch_unwind(AssertUnwindSafe(|| {
            if session.host.world.active_fmv().is_some() {
                session.host.world.finish_cutscene();
                let _ = session.apply_pending_fmv_handoff();
                true
            } else {
                false
            }
        }));
        match fmv {
            Ok(true) => stats.fmvs += 1,
            Ok(false) => {}
            Err(_) => {
                let (loc, msg, frames) = LAST_PANIC
                    .with(|p| p.borrow_mut().take())
                    .unwrap_or_default();
                findings.push(mk(
                    &session,
                    "panic",
                    frame,
                    loc,
                    format!("[fmv hand-off] {msg} | {frames}"),
                ));
                break;
            }
        }

        let w = &session.host.world;
        stats.modes.insert(format!("{:?}", w.mode));
        if w.game_over || w.game_over_hold {
            stats.wiped = true;
            break;
        }
        // Effect residue: a battle-scoped effect (the `efect.dat` pool, a
        // move-FX / effect-script / summon scene-graph, the streak block)
        // still live on the first frame past a battle exit or a scene load.
        // Retail drops every one of them with the actor-pool reset of its
        // per-stage init (`FUN_8001E1B4`); both play hosts draw them with no
        // mode test, so a survivor is on screen in the field.
        let left_battle = prev_mode == SceneMode::Battle
            && matches!(w.mode, SceneMode::Field | SceneMode::WorldMap);
        let scene_changed = w.active_scene_label != prev_scene;
        if (left_battle || scene_changed) && w.mode != SceneMode::Battle {
            let residue = w.battle_effect_residue();
            if !residue.is_empty() {
                findings.push(mk(
                    &session,
                    "effect_residue",
                    frame,
                    if left_battle {
                        "after:battle-exit".into()
                    } else {
                        "after:scene-change".into()
                    },
                    format!("battle effects still live: {}", residue.join(",")),
                ));
            }
        }
        if scene_changed {
            prev_scene = w.active_scene_label.clone();
        }
        // Mode edges.
        if w.mode != prev_mode {
            if w.mode == SceneMode::Battle {
                stats.battles += 1;
                // Battle loop: the same parked timeline PC opening battle
                // after battle. A scripted fight fires once per beat; a
                // record that re-arms it on every return never lets the
                // beat finish.
                if let Some(tl) = w.cutscene.timeline.as_ref() {
                    let site = format!("timeline@{:#06x}", tl.pc);
                    let n = battle_sites.entry(site.clone()).or_insert(0u32);
                    *n += 1;
                    if *n == 3 {
                        findings.push(mk(
                            &session,
                            "battle_loop",
                            frame,
                            site,
                            "three battles opened from one parked timeline PC".into(),
                        ));
                    }
                }
            }
            if prev_mode == SceneMode::Battle
                && matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
            {
                stats.battles_won += 1;
            }
            if w.mode == SceneMode::Title {
                findings.push(mk(
                    &session,
                    "dropped_to_title",
                    frame,
                    format!("from:{prev_mode:?}"),
                    "world mode became Title with no game over".into(),
                ));
                break;
            }
            prev_mode = w.mode;
        }
        // Battle that never ends.
        if w.mode == SceneMode::Battle {
            let since = *battle_since.get_or_insert(frame);
            if frame - since > t.battle_frames {
                findings.push(mk(
                    &session,
                    "battle_endless",
                    frame,
                    held_by(&session),
                    format!("in Battle for {} frames", frame - since),
                ));
                break;
            }
        } else {
            battle_since = None;
        }
        // Pause menu that cannot be closed: the policy starts backing out
        // after at most 420 frames, so a menu open past the close budget
        // refused every Circle in between.
        if session.field_menu.is_some() {
            let since = *menu_open_since.get_or_insert(frame);
            if frame - since > 420 + t.menu_close_frames {
                findings.push(mk(
                    &session,
                    "menu_stuck",
                    frame,
                    held_by(&session),
                    format!("pause menu open {} frames under Circle", frame - since),
                ));
                break;
            }
        } else {
            menu_open_since = None;
        }
        // Script stall. Only field frames count: a battle, the pause menu or
        // a minigame suspends the field VM by design, so the clock pauses
        // (it restarts from zero on the next field frame).
        let field_frame = matches!(
            session.host.world.mode,
            SceneMode::Field | SceneMode::WorldMap
        ) && session.field_menu.is_none()
            // A script parked on a player-owned modal (the name-entry grid, a
            // shop) is waiting for the player, not stalled.
            && !session.host.world.name_entry_active()
            && !session.host.world.shops.shop_open;
        if !field_frame {
            park = None;
        } else if frame % t.digest_every == 0 {
            let w = &session.host.world;
            let site = w
                .cutscene
                .timeline
                .as_ref()
                .map(|tl| ("timeline", tl))
                .or_else(|| w.field_vm.helper_contexts.first().map(|h| ("helper", h)))
                .map(|(k, c)| {
                    let op = c.bytecode.get(c.pc).copied().unwrap_or(0xFF);
                    // The slice count rides along: a conversation a picker
                    // loops back to its own segment re-enters the same PC
                    // every pass, and each pass is a slice. A `0xC7` walk
                    // parks its PC for as long as the walk runs, so for it
                    // the walked bodies' positions ride along too: a long
                    // walk still stepping is the op progressing, not a stall.
                    let mut h = HashWriter(std::collections::hash_map::DefaultHasher::new());
                    let _ = write!(h, "{}|", c.frames);
                    if op == 0xC7 {
                        if let Some(a) = w.actors.get(player_slot(&session)) {
                            let m = &a.move_state;
                            let _ = write!(h, "p{},{}|", m.world_x, m.world_z);
                        }
                        for slot in w.npcs.motions.keys() {
                            let _ = write!(h, "{slot}{:?}|", w.npcs.positions.get(slot));
                        }
                    }
                    (format!("{k}@{:#06x}:op{op:02x}", c.pc), h.0.finish())
                });
            match (site, park.as_ref()) {
                (Some((site, slices)), Some((cur, cur_slices, since)))
                    if *cur == site && *cur_slices == slices =>
                {
                    if frame - since >= t.softlock_frames
                        && reported.insert(format!("script_stall|{site}"))
                    {
                        findings.push(mk(
                            &session,
                            "script_stall",
                            frame,
                            site,
                            format!("script parked at one PC for {} frames", frame - since),
                        ));
                    }
                }
                (Some((site, slices)), _) => park = Some((site, slices, frame)),
                (None, _) => park = None,
            }
        }
        // Softlock.
        if frame % t.digest_every == 0 {
            let d = progress_digest(&session) ^ menu_digest(&menu_rt);
            if d != last_digest {
                last_digest = d;
                last_change = frame;
            } else if frame - last_change >= t.softlock_frames {
                let window = &pads[last_change as usize..];
                let distinct: BTreeSet<u16> = window.iter().copied().collect();
                if distinct.len() >= t.min_distinct {
                    findings.push(mk(
                        &session,
                        "softlock",
                        frame,
                        held_by(&session),
                        format!(
                            "no progress for {} frames under {} distinct pad masks",
                            frame - last_change,
                            distinct.len()
                        ),
                    ));
                    break;
                }
            }
        }
        if t.trace_every != 0 {
            if frame % t.trace_every == 0 {
                eprintln!("[trace {frame:>6}] {}", trace_line(&session, pad));
            }
            // Each distinct script body a timeline / helper runs, once, as
            // hex - the bytes a triage needs to read the park in context.
            // Printed to the terminal only; never written to the report.
            let w = &session.host.world;
            let bodies = w
                .cutscene
                .timeline
                .iter()
                .chain(w.field_vm.helper_contexts.iter())
                .map(|c| &c.bytecode);
            for bc in bodies {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                h.write(bc);
                if traced_scripts.insert(h.finish()) {
                    let hex: Vec<String> = bc
                        .iter()
                        .take(env_u64("LEGAIA_SOAK_TRACE_BYTES").unwrap_or(0x400) as usize)
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    eprintln!(
                        "[trace {frame:>6}] script body ({} bytes): {}",
                        bc.len(),
                        hex.join(" ")
                    );
                }
            }
        }
        // Value checks.
        if frame % 8 == 0 {
            let mut v = Vec::new();
            value_checks(&session, t, &mut v);
            for (det, loc, detail) in v {
                let sig = format!("{det}|{loc}");
                if reported.insert(sig) {
                    findings.push(mk(&session, det, frame, loc, detail));
                }
            }
        }
    }
    RunOutcome {
        spec: spec.clone(),
        findings,
        pads,
        stats,
        enter_ok,
    }
}

// ---------------------------------------------------------------------------
// Replay files
// ---------------------------------------------------------------------------

/// The start scene rides in a header comment (`j-replay-v1` has no scene
/// field; `meta.scenario` names a `scripts/scenarios.toml` label and is left
/// unset).
const SCENE_TAG: &str = "# soak-scene = ";
const SEED_TAG: &str = "# soak-seed = ";
const SIG_TAG: &str = "# soak-signature = ";
const MIN_DISTINCT_TAG: &str = "# soak-min-distinct = ";

fn replay_text(
    spec: &RunSpec,
    pads: &[u16],
    finding: Option<&Finding>,
    min_distinct: Option<usize>,
) -> String {
    let mut rf = ReplayFile::new(
        ReplayMeta::new(pads.len() as u64)
            .with_rng_seed((run_seed(&spec.scene, spec.seed) >> 16) as u32),
    );
    let mut prev = 0u16;
    for (f, &p) in pads.iter().enumerate() {
        if f == 0 || p != prev {
            rf.push_event(f as u64, p);
            prev = p;
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Soak-harness finding (pad input only - no disc bytes)."
    );
    let _ = writeln!(out, "{SCENE_TAG}{:?}", spec.scene);
    let _ = writeln!(out, "{SEED_TAG}{}", spec.seed);
    if let Some(f) = finding {
        let _ = writeln!(out, "{SIG_TAG}{:?}", f.signature());
        let _ = writeln!(out, "# detail: {}", f.detail.replace('\n', " "));
    }
    if let Some(n) = min_distinct {
        // Minimised under the neutral-pad control: the input that fed the
        // frozen window was dropped, so the detector must not ask for it.
        let _ = writeln!(out, "{MIN_DISTINCT_TAG}{n}");
    }
    let _ = writeln!(
        out,
        "# reproduce: LEGAIA_SOAK_REPLAY=<this file> cargo test -p legaia-engine-shell \
         --profile release-test --test soak_harness soak_replay -- --nocapture"
    );
    out.push_str(&rf.to_toml_string().expect("serialise replay"));
    out
}

struct LoadedReplay {
    spec: RunSpec,
    pads: Vec<u16>,
    signature: Option<String>,
    min_distinct: Option<usize>,
}

impl LoadedReplay {
    /// The detector settings this replay was recorded under.
    fn tunables(&self) -> Tunables {
        let mut t = Tunables::from_env();
        if let Some(n) = self.min_distinct {
            t.min_distinct = n;
        }
        t
    }
}

fn parse_tag(text: &str, tag: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let v = l.strip_prefix(tag)?.trim();
        Some(v.trim_matches('"').to_string())
    })
}

fn load_replay(path: &Path) -> LoadedReplay {
    let text = std::fs::read_to_string(path).expect("read replay");
    let rf = ReplayFile::from_toml_str(&text).expect("parse j-replay-v1");
    let scene = parse_tag(&text, SCENE_TAG).expect("replay carries a soak-scene header");
    let seed = parse_tag(&text, SEED_TAG)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut pads = rf.expand_pad_stream();
    pads.truncate(rf.meta.frames as usize);
    LoadedReplay {
        spec: RunSpec {
            scene,
            seed,
            frames: rf.meta.frames,
        },
        pads,
        signature: parse_tag(&text, SIG_TAG),
        min_distinct: parse_tag(&text, MIN_DISTINCT_TAG).and_then(|s| s.parse().ok()),
    }
}

fn slug(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    out.truncate(96);
    out
}

// ---------------------------------------------------------------------------
// Soak driver
// ---------------------------------------------------------------------------

struct SoakConfig {
    tag: String,
    scenes: Vec<String>,
    seeds: Vec<u64>,
    frames: u64,
    jobs: usize,
    confirm: bool,
}

struct SoakResult {
    outcomes: Vec<RunOutcome>,
    hangs: Vec<Finding>,
    wall: Duration,
}

fn soak(src: &Source, known: &BTreeSet<String>, cfg: &SoakConfig) -> SoakResult {
    install_panic_hook();
    let t0 = Instant::now();
    let mut jobs: Vec<RunSpec> = Vec::new();
    for &seed in &cfg.seeds {
        for scene in &cfg.scenes {
            jobs.push(RunSpec {
                scene: scene.clone(),
                seed,
                frames: cfg.frames,
            });
        }
    }
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let board: StatusBoard = Arc::new(Mutex::new(vec![WorkerStatus::default(); cfg.jobs]));
    let results: Arc<Mutex<Vec<RunOutcome>>> = Arc::new(Mutex::new(Vec::new()));
    let hangs: Arc<Mutex<Vec<Finding>>> = Arc::new(Mutex::new(Vec::new()));
    let hang_secs = env_u64("LEGAIA_SOAK_HANG_SECS").unwrap_or(60);
    let out = out_dir(&cfg.tag);
    let _ = std::fs::create_dir_all(&out);

    // Watchdog: a single tick running past `hang_secs` cannot be interrupted,
    // so it is reported (with the spec that regenerates it) and the process
    // exits - a hung worker would otherwise hold the whole soak.
    let wd = {
        let board = board.clone();
        let done = done.clone();
        let out = out.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let b = board.lock().unwrap();
                for w in b.iter() {
                    if let Some(st) = w.tick_started
                        && st.elapsed() > Duration::from_secs(hang_secs)
                    {
                        let msg = format!(
                            "HANG: scene {} seed {} frame {}: one tick ran > {hang_secs}s\n\
                             reproduce: LEGAIA_SOAK_SCENES={} LEGAIA_SOAK_SEEDS={} \
                             LEGAIA_SOAK_FRAMES={}\n",
                            w.scene,
                            w.seed,
                            w.frame,
                            w.scene,
                            w.seed,
                            w.frame + 1
                        );
                        eprintln!("{msg}");
                        let _ = std::fs::write(out.join("HANG.txt"), &msg);
                        std::process::exit(3);
                    }
                }
            }
        })
    };

    let total = jobs.len();
    let mut handles = Vec::new();
    for wi in 0..cfg.jobs {
        let jobs = jobs.clone();
        let next = next.clone();
        let board = board.clone();
        let results = results.clone();
        let src = src.clone();
        let known = known.clone();
        let h = std::thread::Builder::new()
            .name(format!("soak-{wi}"))
            .stack_size(64 << 20)
            .spawn(move || {
                let t = Tunables::from_env();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(spec) = jobs.get(i) else { break };
                    {
                        let mut b = board.lock().unwrap();
                        b[wi] = WorkerStatus {
                            tick_started: None,
                            scene: spec.scene.clone(),
                            seed: spec.seed,
                            frame: 0,
                        };
                    }
                    let st = Instant::now();
                    let o = run_one(&src, &known, spec, None, &t, Some((&board, wi)));
                    eprintln!(
                        "[soak {}/{}] {:>10} seed {:<4} {:>6} fr {:>5.1}s {}{}",
                        i + 1,
                        total,
                        spec.scene,
                        spec.seed,
                        o.stats.frames_run,
                        st.elapsed().as_secs_f32(),
                        if o.findings.is_empty() { "ok" } else { "FIND" },
                        o.findings
                            .iter()
                            .map(|f| format!(" [{}]", f.signature()))
                            .collect::<String>()
                    );
                    results.lock().unwrap().push(o);
                }
            })
            .expect("spawn soak worker");
        handles.push(h);
    }
    for h in handles {
        let _ = h.join();
    }
    done.store(true, Ordering::Relaxed);
    let _ = wd.join();
    let mut outcomes = std::mem::take(&mut *results.lock().unwrap());
    outcomes.sort_by(|a, b| (a.spec.seed, &a.spec.scene).cmp(&(b.spec.seed, &b.spec.scene)));
    let hangs = std::mem::take(&mut *hangs.lock().unwrap());
    let r = SoakResult {
        outcomes,
        hangs,
        wall: t0.elapsed(),
    };
    write_report(src, known, cfg, &r);
    r
}

/// Confirm + minimise one finding: replay the recorded pads truncated to the
/// finding frame, report whether the same signature fires, and then zero out
/// chunks of the pad stream (a bounded delta-debugging pass) while it still
/// does. A softlock's reduction runs with the neutral-pad control
/// (`min_distinct = 1`), so pads that only fed the frozen window drop out and
/// what is left is the input that *caused* the park.
fn confirm(
    src: &Source,
    known: &BTreeSet<String>,
    o: &RunOutcome,
    f: &Finding,
) -> (bool, Vec<u16>, Option<usize>) {
    let n = (f.frame + 1) as usize;
    let pads: Vec<u16> = o.pads[..n.min(o.pads.len())].to_vec();
    let (src, known, scene, seed, sig) = (
        src.clone(),
        known.clone(),
        o.spec.scene.clone(),
        o.spec.seed,
        f.signature(),
    );
    let softlock = matches!(f.detector, "softlock");
    let budget = env_u64("LEGAIA_SOAK_MINIMIZE").unwrap_or(24) as usize;
    std::thread::Builder::new()
        .name("soak-confirm".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let reproduces = |pads: &[u16], relaxed: bool| -> bool {
                let mut t = Tunables::from_env();
                if relaxed {
                    t.min_distinct = 1;
                }
                let spec = RunSpec {
                    scene: scene.clone(),
                    seed,
                    frames: pads.len() as u64,
                };
                run_one(&src, &known, &spec, Some(pads), &t, None)
                    .findings
                    .iter()
                    .any(|g| g.signature() == sig)
            };
            if !reproduces(&pads, false) {
                return (false, pads, None);
            }
            let mut best = pads.clone();
            let mut tries = 0usize;
            let mut chunks = 2usize;
            while tries < budget && chunks <= best.len() {
                let size = best.len().div_ceil(chunks);
                let mut changed = false;
                for c in 0..chunks {
                    if tries >= budget {
                        break;
                    }
                    let (a, b) = (c * size, ((c + 1) * size).min(best.len()));
                    if a >= b || best[a..b].iter().all(|&p| p == 0) {
                        continue;
                    }
                    let mut cand = best.clone();
                    cand[a..b].iter_mut().for_each(|p| *p = 0);
                    tries += 1;
                    if reproduces(&cand, softlock) {
                        best = cand;
                        changed = true;
                    }
                }
                if !changed {
                    chunks *= 2;
                }
            }
            // A softlock whose pads were reduced replays under the relaxed
            // control it was reduced under.
            let relaxed = (softlock && best != pads).then_some(1);
            (true, best, relaxed)
        })
        .expect("spawn confirm")
        .join()
        .unwrap_or((false, Vec::new(), None))
}

/// `(rank, signature, [(outcome index, finding index)])`.
type SigRow = (u8, String, Vec<(usize, usize)>);

fn write_report(src: &Source, known: &BTreeSet<String>, cfg: &SoakConfig, r: &SoakResult) {
    let out = out_dir(&cfg.tag);
    let _ = std::fs::create_dir_all(out.join("replays"));
    // Group by signature.
    let mut groups: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    for (oi, o) in r.outcomes.iter().enumerate() {
        for (fi, f) in o.findings.iter().enumerate() {
            groups.entry(f.signature()).or_default().push((oi, fi));
        }
    }
    let mut rows: Vec<SigRow> = groups
        .into_iter()
        .map(|(sig, v)| {
            let f = &r.outcomes[v[0].0].findings[v[0].1];
            (f.rank(), sig, v)
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.0, std::cmp::Reverse(a.2.len())).cmp(&(b.0, std::cmp::Reverse(b.2.len())))
    });

    let frames_total: u64 = r.outcomes.iter().map(|o| o.stats.frames_run).sum();
    let battles: u32 = r.outcomes.iter().map(|o| o.stats.battles).sum();
    let wins: u32 = r.outcomes.iter().map(|o| o.stats.battles_won).sum();
    let wipes = r.outcomes.iter().filter(|o| o.stats.wiped).count();
    let menus: u32 = r.outcomes.iter().map(|o| o.stats.menu_opens).sum();
    let fmvs: u32 = r.outcomes.iter().map(|o| o.stats.fmvs).sum();
    let enter_fail = r.outcomes.iter().filter(|o| !o.enter_ok).count();
    let saves: u32 = r.outcomes.iter().map(|o| o.stats.save_commits).sum();
    let trips: u32 = r.outcomes.iter().map(|o| o.stats.round_trips).sum();
    let shops: u32 = r.outcomes.iter().map(|o| o.stats.shop_opens).sum();
    let menu_rows: BTreeSet<&String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.stats.menu_rows.iter())
        .collect();
    let mut modes: BTreeMap<String, usize> = BTreeMap::new();
    for o in &r.outcomes {
        for m in &o.stats.modes {
            *modes.entry(m.clone()).or_default() += 1;
        }
    }

    let mut md = String::new();
    let _ = writeln!(md, "# Soak report `{}`\n", cfg.tag);
    let _ = writeln!(
        md,
        "- runs: {} ({} scenes x {} seeds x {} frames), {} frames ticked, wall {:.0}s, {} jobs",
        r.outcomes.len(),
        cfg.scenes.len(),
        cfg.seeds.len(),
        cfg.frames,
        frames_total,
        r.wall.as_secs_f32(),
        cfg.jobs
    );
    let _ = writeln!(
        md,
        "- battles entered {battles}, left to field {wins}, party wipes {wipes}, menu opens {menus}, FMVs skipped {fmvs}, entry failures {enter_fail}"
    );
    let _ = writeln!(
        md,
        "- save-screen commits {saves}, save / load round trips {trips}, shops opened {shops}"
    );
    let _ = writeln!(md, "- pause-menu sub-screens entered: {menu_rows:?}");
    let _ = writeln!(md, "- runs that reached each mode: {modes:?}");
    let moved = r
        .outcomes
        .iter()
        .filter(|o| !o.stats.scenes_entered.is_empty())
        .count();
    let reached: BTreeSet<&String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.stats.scenes_entered.iter())
        .collect();
    let _ = writeln!(
        md,
        "- runs that changed scene {moved}; distinct scenes entered by transition {}",
        reached.len()
    );
    let _ = writeln!(md, "- distinct finding signatures: {}\n", rows.len());
    let _ = writeln!(
        md,
        "| rank | detector | scene | location | hits | repro | first (start scene / seed / frame) | detail |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|");
    let max_confirm = env_u64("LEGAIA_SOAK_CONFIRM_MAX").unwrap_or(40) as usize;
    let mut confirmed = 0usize;
    let mut json = String::from("[\n");
    for (rank, sig, hits) in &rows {
        let (oi, fi) = hits[0];
        let o = &r.outcomes[oi];
        let f = &o.findings[fi];
        let (repro, pads, relaxed) = if cfg.confirm && confirmed < max_confirm {
            confirmed += 1;
            let (hit, pads, relaxed) = confirm(src, known, o, f);
            (if hit { "yes" } else { "NO" }.to_string(), pads, relaxed)
        } else {
            (
                "-".to_string(),
                o.pads[..((f.frame + 1) as usize).min(o.pads.len())].to_vec(),
                None,
            )
        };
        let file = out
            .join("replays")
            .join(format!("{}.replay.toml", slug(sig)));
        let _ = std::fs::write(&file, replay_text(&o.spec, &pads, Some(f), relaxed));
        let detail: String = f.detail.chars().take(220).collect();
        let _ = writeln!(
            md,
            "| {} | {} | {} | `{}` | {} | {} | {} / {} / {} | {} |",
            rank,
            f.detector,
            f.scene,
            f.location.replace('|', "/"),
            hits.len(),
            repro,
            o.spec.scene,
            o.spec.seed,
            f.frame,
            detail.replace('|', "/")
        );
        let starts: BTreeSet<&str> = hits
            .iter()
            .map(|&(oi, _)| r.outcomes[oi].spec.scene.as_str())
            .collect();
        let _ = writeln!(
            json,
            "  {{\"signature\": {:?}, \"rank\": {}, \"hits\": {}, \"repro\": {:?}, \"start_scenes\": {:?}, \"first\": {{\"scene\": {:?}, \"seed\": {}, \"frame\": {}}}, \"mode\": {:?}, \"detail\": {:?}, \"replay\": {:?}}},",
            sig,
            rank,
            hits.len(),
            repro,
            starts,
            o.spec.scene,
            o.spec.seed,
            f.frame,
            f.mode,
            f.detail,
            file.display().to_string()
        );
    }
    if json.ends_with(",\n") {
        json.truncate(json.len() - 2);
        json.push('\n');
    }
    json.push_str("]\n");
    let _ = writeln!(md, "\nReplays: `{}`", out.join("replays").display());
    let _ = std::fs::write(out.join("report.md"), &md);
    let _ = std::fs::write(out.join("findings.json"), &json);
    eprintln!("{md}");
    eprintln!(
        "[soak] report written to {}",
        out.join("report.md").display()
    );
}

fn default_jobs() -> usize {
    env_u64("LEGAIA_SOAK_JOBS")
        .map(|n| n as usize)
        .unwrap_or(4)
        .max(1)
}

/// Minigame pseudo-scenes soaked alongside the scene set: each venue plus the
/// door-warp `sub_id` it requests (fishing 0, slot 3, Baka Fighter 4, Muscle
/// Dome 5, dance 6 - `legaia_engine_core::minigame_entry::MinigameSubId`).
const MINIGAME_RUNS: [&str; 5] = [
    "balden+mg0",
    "koin1+mg3",
    "koin3+mg4",
    "koin3+mg5",
    "koin1+mg6",
];

/// `"town01+rt"` -> `("town01", true)`: the run round-trips a save every
/// [`ROUND_TRIP_EVERY`] frames. A plain label passes through.
fn split_round_trip(label: &str) -> (&str, bool) {
    match label.strip_suffix("+rt") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// `"town01+shop"` -> `("town01", true)`: the run opens one of the scene's
/// shops every [`SHOP_VISIT_EVERY`] free field frames.
fn split_shop(label: &str) -> (&str, bool) {
    match label.strip_suffix("+shop") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// Frames between two shop visits in a `+shop` run.
const SHOP_VISIT_EVERY: u64 = 900;

/// The scenes whose field MAN carries at least one priced gold shop (the
/// list scene entry decodes into `ShopState::scene_shops`), as `+shop`
/// pseudo-scenes. Entered once each in a probe session.
fn shop_scenes(src: &Source, all: &[String]) -> Vec<String> {
    let mut probe = open_session(src);
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: false,
        battle_bgm: None,
    };
    let mut out = Vec::new();
    for name in all {
        if is_world_map_scene(name) {
            continue;
        }
        let entered = catch_unwind(AssertUnwindSafe(|| probe.enter_scene_live(name, &opts)));
        if matches!(entered, Ok(Ok(_))) && !probe.host.world.shops.scene_shops.is_empty() {
            out.push(format!("{name}+shop"));
        }
    }
    out
}

/// Frames between two round trips in a `+rt` run.
const ROUND_TRIP_EVERY: u64 = 600;

/// A frame a player could open the pause menu on and save: free field
/// roam, no modal, no timeline, no dialogue.
fn round_trip_ready(s: &BootSession) -> bool {
    let w = &s.host.world;
    matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
        && s.field_menu.is_none()
        && w.cutscene.timeline.is_none()
        && w.dialog.current.is_none()
        && w.dialog.inline.is_none()
        && !w.name_entry_active()
        && !w.shops.shop_open
        && w.active_fmv().is_none()
}

/// The first field two saves disagree on, named, with a short detail.
fn save_diff(a: &legaia_save::SaveFile, b: &legaia_save::SaveFile) -> Option<(String, String)> {
    if a.party.members.len() != b.party.members.len() {
        return Some((
            "party.len".into(),
            format!("{} -> {}", a.party.members.len(), b.party.members.len()),
        ));
    }
    for (i, (x, y)) in a.party.members.iter().zip(&b.party.members).enumerate() {
        if let Some(off) = x.raw.iter().zip(&y.raw).position(|(p, q)| p != q) {
            return Some((
                format!("party[{i}]+{off:#05x}"),
                format!("{:#04x} -> {:#04x}", x.raw[off], y.raw[off]),
            ));
        }
    }
    macro_rules! field {
        ($name:literal, $x:expr, $y:expr) => {
            if $x != $y {
                let (dx, dy) = (format!("{:?}", $x), format!("{:?}", $y));
                let cut = |s: &str| s.chars().take(160).collect::<String>();
                return Some(($name.into(), format!("{} -> {}", cut(&dx), cut(&dy))));
            }
        };
    }
    field!("ext.story_flags", a.ext.story_flags, b.ext.story_flags);
    field!(
        "ext.story_flag_bits",
        a.ext.story_flag_bits,
        b.ext.story_flag_bits
    );
    field!("ext.money", a.ext.money, b.ext.money);
    field!("ext.inventory", a.ext.inventory, b.ext.inventory);
    field!("ext.item_slots", a.ext.item_slots, b.ext.item_slots);
    field!("ext.minigames", a.ext.minigames, b.ext.minigames);
    field!(
        "ext_v2.active_party",
        a.ext_v2.active_party,
        b.ext_v2.active_party
    );
    field!("ext_v2.per_char", a.ext_v2.per_char, b.ext_v2.per_char);
    field!(
        "ext_v2.saved_chains",
        a.ext_v2.saved_chains,
        b.ext_v2.saved_chains
    );
    field!(
        "ext_v2.field_position",
        a.ext_v2.field_position,
        b.ext_v2.field_position
    );
    field!("ext_v2", a.ext_v2, b.ext_v2);
    field!("ext", a.ext, b.ext);
    None
}

/// The host half of a save-screen pick (the play window's
/// `apply_save_commit`): a Save writes the file into the in-memory card and
/// refreshes the grid's snapshot for that block; a Load resumes the file the
/// block holds, or does nothing for an empty block (the flow refuses those
/// before committing, so that arm is defensive).
fn apply_save_commit(
    session: &mut BootSession,
    card: &mut SoakCard,
    commit: legaia_engine_core::save_screen::SaveCommit,
    opts: &FieldLiveOpts,
) {
    use legaia_engine_core::save_screen::SaveCommitKind;
    let key = (commit.port, commit.cell);
    match commit.kind {
        SaveCommitKind::Save => {
            let sf = session.host.world.save_full();
            let resume = session.current_resume();
            card.insert(key, (sf, resume));
            refresh_rack(session, card);
        }
        SaveCommitKind::Load => {
            if let Some((sf, resume)) = card.get(&key).cloned() {
                let _ = session.resume_save(sf, &resume.scene, opts);
            }
        }
    }
}

/// Re-derive the save grid's port-0 snapshots from the in-memory card.
fn refresh_rack(session: &mut BootSession, card: &SoakCard) {
    let mut ports: Vec<Vec<SlotSnapshot>> =
        vec![(0..15).map(SlotSnapshot::empty).collect(), Vec::new()];
    for (&(port, cell), (sf, resume)) in card.iter() {
        if let Some(slot) = ports
            .get_mut(port as usize)
            .and_then(|p| p.get_mut(cell as usize))
        {
            *slot = legaia_engine_core::save_select::snapshot_for_save(cell, sf, resume);
        }
    }
    let rack = session.save_rack().clone();
    session.set_save_rack(rack, ports);
}

/// The harness's memory card: `(port, cell)` -> the file a Save wrote.
type SoakCard = BTreeMap<(u8, u8), (legaia_save::SaveFile, legaia_save::SaveResume)>;

/// `"koin1+mg3"` -> `("koin1", Some(3))`; a plain label passes through.
fn split_minigame(label: &str) -> (&str, Option<u8>) {
    match label.split_once("+mg") {
        Some((scene, id)) => (scene, id.parse().ok()),
        None => (label, None),
    }
}

fn filter_scenes(all: &[String], extra: &[String]) -> Vec<String> {
    let mut scenes: Vec<String> = match std::env::var("LEGAIA_SOAK_SCENES") {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => all
            .iter()
            .cloned()
            .chain(MINIGAME_RUNS.iter().map(|s| s.to_string()))
            // Every scene again as a save / load round-trip run.
            .chain(all.iter().map(|s| format!("{s}+rt")))
            .chain(extra.iter().cloned())
            .collect(),
    };
    // `LEGAIA_SOAK_SHARD=i/n` splits the scene set so a long soak can be
    // chunked across invocations.
    if let Ok(sh) = std::env::var("LEGAIA_SOAK_SHARD")
        && let Some((i, n)) = sh.split_once('/')
        && let (Ok(i), Ok(n)) = (i.parse::<usize>(), n.parse::<usize>())
        && n > 0
    {
        scenes = scenes
            .into_iter()
            .enumerate()
            .filter(|(k, _)| k % n == i)
            .map(|(_, s)| s)
            .collect();
    }
    scenes
}

fn seed_list(default_count: u64) -> Vec<u64> {
    let base = env_u64("LEGAIA_SOAK_SEED_BASE").unwrap_or(1);
    let n = env_u64("LEGAIA_SOAK_SEEDS").unwrap_or(default_count);
    (base..base + n).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Scenes the fixed-budget smoke run covers: the cold-boot town, the
/// chapter-1 overworld, a dungeon, the casino floor (minigame doors) and a
/// late-game dungeon, plus one save / load round-trip run.
const SMOKE_SCENES: [&str; 6] = ["town01", "map01", "keikoku", "koin1", "jou", "town01+rt"];
const SMOKE_FRAMES: u64 = 900;

#[test]
fn soak_smoke_no_panics() {
    let Some(src) = source() else { return };
    let probe = open_session(&src);
    let all = scene_set(&probe);
    drop(probe);
    let known: BTreeSet<String> = all.iter().cloned().collect();
    let scenes: Vec<String> = SMOKE_SCENES
        .iter()
        .chain(MINIGAME_RUNS.iter())
        .filter(|s| known.contains(split_minigame(split_round_trip(s).0).0))
        .map(|s| s.to_string())
        .collect();
    assert!(
        !scenes.is_empty(),
        "no smoke scene resolves - scene_set broken"
    );
    let cfg = SoakConfig {
        tag: "smoke".into(),
        scenes,
        seeds: vec![1],
        frames: SMOKE_FRAMES,
        jobs: default_jobs(),
        confirm: false,
    };
    eprintln!("[soak-smoke] scenes: {}", all.join(" "));
    eprintln!(
        "[soak-smoke] playable scene set = {} scenes; running {:?} x seed 1 x {} frames from {:?}",
        all.len(),
        cfg.scenes,
        cfg.frames,
        src
    );
    let r = soak(&src, &known, &cfg);
    let fatal: Vec<String> = r
        .outcomes
        .iter()
        .flat_map(|o| o.findings.iter())
        .filter(|f| matches!(f.detector, "panic" | "tick_error"))
        .map(|f| {
            format!(
                "{} (start {}, frame {}): {}",
                f.signature(),
                f.start_scene,
                f.frame,
                f.detail
            )
        })
        .collect();
    assert!(r.hangs.is_empty(), "hung ticks: {:?}", r.hangs);
    assert!(
        fatal.is_empty(),
        "soak smoke hit panics:\n{}",
        fatal.join("\n")
    );
}

#[test]
fn soak_long() {
    if std::env::var_os("LEGAIA_SOAK_SEEDS").is_none()
        && std::env::var_os("LEGAIA_SOAK_FRAMES").is_none()
    {
        eprintln!("[skip] soak_long: set LEGAIA_SOAK_SEEDS / LEGAIA_SOAK_FRAMES to run");
        return;
    }
    let Some(src) = source() else { return };
    let probe = open_session(&src);
    let all = scene_set(&probe);
    drop(probe);
    let known: BTreeSet<String> = all.iter().cloned().collect();
    let shops = if std::env::var_os("LEGAIA_SOAK_SCENES").is_some() {
        Vec::new()
    } else {
        shop_scenes(&src, &all)
    };
    let scenes = filter_scenes(&all, &shops);
    let cfg = SoakConfig {
        tag: std::env::var("LEGAIA_SOAK_TAG").unwrap_or_else(|_| "long".into()),
        scenes,
        seeds: seed_list(1),
        frames: env_u64("LEGAIA_SOAK_FRAMES").unwrap_or(3600),
        jobs: default_jobs(),
        confirm: !env_flag("LEGAIA_SOAK_NO_CONFIRM"),
    };
    eprintln!(
        "[soak-long] {} scenes x {} seeds x {} frames, {} jobs, source {:?}",
        cfg.scenes.len(),
        cfg.seeds.len(),
        cfg.frames,
        cfg.jobs,
        src
    );
    let r = soak(&src, &known, &cfg);
    if env_flag("LEGAIA_SOAK_STRICT") {
        let fatal = r
            .outcomes
            .iter()
            .flat_map(|o| o.findings.iter())
            .filter(|f| f.rank() == 0)
            .count();
        assert_eq!(fatal, 0, "strict soak: {fatal} rank-0 findings");
    }
}

#[test]
fn soak_replay() {
    let Some(path) = std::env::var_os("LEGAIA_SOAK_REPLAY") else {
        eprintln!("[skip] soak_replay: set LEGAIA_SOAK_REPLAY=<file or dir>");
        return;
    };
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for file in replay_files(Path::new(&path)) {
        let lr = load_replay(&file);
        eprintln!(
            "[soak-replay] {} ({} frames, scene {})",
            file.display(),
            lr.spec.frames,
            lr.spec.scene
        );
        let t = lr.tunables();
        let (spec, pads) = (lr.spec, lr.pads);
        let (src, known) = (src.clone(), known.clone());
        let h = std::thread::Builder::new()
            .name("soak-replay".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let r = run_one(&src, &known, &spec, Some(&pads), &t, None);
                (r.findings, r.stats)
            })
            .unwrap();
        let (findings, stats) = h.join().expect("replay thread");
        eprintln!("[soak-replay] stats: {stats:?}");
        for f in &findings {
            eprintln!(
                "[soak-replay] finding {} at frame {}: {}",
                f.signature(),
                f.frame,
                f.detail
            );
        }
        if let Some(sig) = lr.signature {
            let hit = findings.iter().any(|f| f.signature() == sig);
            eprintln!("[soak-replay] recorded signature {sig:?} reproduced: {hit}");
        }
    }
}

/// A replay path, or every `*.replay.toml` directly inside a directory.
fn replay_files(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.to_string_lossy().ends_with(".replay.toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn soak_fixtures() {
    let dir = repo_root().join("scripts/replays/soak");
    let files = if dir.is_dir() {
        replay_files(&dir)
    } else {
        Vec::new()
    };
    // Disc-free half: every committed fixture parses and carries a scene.
    for f in &files {
        let lr = load_replay(f);
        assert!(
            !lr.spec.scene.is_empty(),
            "{} has no soak-scene",
            f.display()
        );
        assert_eq!(lr.pads.len() as u64, lr.spec.frames, "{}", f.display());
    }
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for f in files {
        let lr = load_replay(&f);
        let t = lr.tunables();
        let (src, known) = (src.clone(), known.clone());
        let (spec, pads) = (lr.spec, lr.pads);
        let findings = std::thread::Builder::new()
            .name("soak-fixture".into())
            .stack_size(64 << 20)
            .spawn(move || run_one(&src, &known, &spec, Some(&pads), &t, None).findings)
            .unwrap()
            .join()
            .expect("fixture thread");
        let hit = lr
            .signature
            .as_ref()
            .is_some_and(|sig| findings.iter().any(|g| &g.signature() == sig));
        eprintln!(
            "[soak-fixture] {} -> {}",
            f.file_name().unwrap().to_string_lossy(),
            if hit {
                "still reproduces"
            } else {
                "NO LONGER REPRODUCES (fixed? delete the fixture)"
            }
        );
    }
}

/// Upper bound on the battles a fixed-finding replay may open
/// (`# soak-expect-max-battles = N`): a loop that stops *signing* as a
/// `battle_loop` could still fight twice.
const MAX_BATTLES_TAG: &str = "# soak-expect-max-battles = ";

/// The regression half of the fixture set: `scripts/replays/soak/fixed/`
/// holds replays of findings a fix closed. Each must replay WITHOUT its
/// recorded signature (and within its battle bound) - a hard failure, unlike
/// [`soak_fixtures`], which only reports.
#[test]
fn soak_fixed_fixtures() {
    let dir = repo_root().join("scripts/replays/soak/fixed");
    let files = if dir.is_dir() {
        replay_files(&dir)
    } else {
        Vec::new()
    };
    for f in &files {
        let lr = load_replay(f);
        assert!(
            lr.signature.is_some(),
            "{} has no soak-signature",
            f.display()
        );
        assert_eq!(lr.pads.len() as u64, lr.spec.frames, "{}", f.display());
    }
    let Some(src) = source() else { return };
    install_panic_hook();
    let probe = open_session(&src);
    let known: BTreeSet<String> = scene_set(&probe).into_iter().collect();
    drop(probe);
    for f in files {
        let text = std::fs::read_to_string(&f).expect("read replay");
        let max_battles: Option<u32> =
            parse_tag(&text, MAX_BATTLES_TAG).and_then(|s| s.parse().ok());
        let lr = load_replay(&f);
        let t = lr.tunables();
        let (src, known) = (src.clone(), known.clone());
        let (spec, pads) = (lr.spec, lr.pads);
        let out = std::thread::Builder::new()
            .name("soak-fixed".into())
            .stack_size(64 << 20)
            .spawn(move || run_one(&src, &known, &spec, Some(&pads), &t, None))
            .unwrap()
            .join()
            .expect("fixed-fixture thread");
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let sig = lr.signature.expect("checked above");
        eprintln!(
            "[soak-fixed] {name}: {} frames, {} battles, findings {:?}",
            out.stats.frames_run,
            out.stats.battles,
            out.findings
                .iter()
                .map(|g| g.signature())
                .collect::<Vec<_>>()
        );
        assert!(
            !out.findings.iter().any(|g| g.signature() == sig),
            "{name}: the fixed finding `{sig}` reproduces again"
        );
        if let Some(max) = max_battles {
            assert!(
                out.stats.battles <= max,
                "{name}: {} battles opened, at most {max} expected",
                out.stats.battles
            );
        }
    }
}

// Disc-free unit tests.

#[test]
fn replay_header_round_trips() {
    let spec = RunSpec {
        scene: "town01".into(),
        seed: 7,
        frames: 5,
    };
    let pads = [0u16, 0x4000, 0x4000, 0, 0x0010];
    let f = Finding {
        detector: "softlock",
        start_scene: "town01".into(),
        scene: "town01".into(),
        mode: "Field".into(),
        frame: 4,
        location: "dialogue".into(),
        detail: "x".into(),
    };
    let text = replay_text(&spec, &pads, Some(&f), Some(1));
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.replay.toml");
    std::fs::write(&p, text).unwrap();
    let lr = load_replay(&p);
    assert_eq!(lr.spec.scene, "town01");
    assert_eq!(lr.spec.seed, 7);
    assert_eq!(lr.pads, pads);
    assert_eq!(lr.signature.as_deref(), Some("softlock|town01|dialogue"));
    assert_eq!(lr.min_distinct, Some(1));
}

#[test]
fn run_seed_is_order_independent_and_distinct() {
    assert_eq!(run_seed("town01", 1), run_seed("town01", 1));
    assert_ne!(run_seed("town01", 1), run_seed("town01", 2));
    assert_ne!(run_seed("town01", 1), run_seed("town02", 1));
    let mut a = Rng::new(3);
    let mut b = Rng::new(3);
    for _ in 0..64 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}

#[test]
fn variant_strips_payload() {
    assert_eq!(variant("Menu { cursor: 2 }"), "Menu");
    assert_eq!(variant("Some(3)"), "Some");
    assert_eq!(variant("Idle"), "Idle");
}
