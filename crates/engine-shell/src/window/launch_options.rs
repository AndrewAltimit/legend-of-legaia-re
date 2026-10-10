//! What a `play-window` run is launched with: the screenshot / key-script
//! harness config, the debug seeds, the cheat switches, the capture PNG writer
//! and the key-name tables the scripts and bindings are spelled in.

use super::*;

/// Deterministic screenshot harness for `play-window`: render one frame
/// offscreen at [`Self::capture_tick`] and write it to [`Self::path`], then
/// exit - and/or a periodic sweep ([`Self::sweep`]) that captures every N
/// ticks into a directory. [`Self::pad_script`] maps a world-tick to a
/// pad mask (one-tick edges, or held ranges via `FIRST-LAST:BUTTON`) so a
/// capture run can auto-open + navigate a menu - or hold a direction to
/// walk somewhere - without `xdotool`.
pub struct ScreenshotConfig {
    /// Single-shot output (`--screenshot`). `None` when only the periodic
    /// sweep is requested.
    pub path: Option<std::path::PathBuf>,
    pub capture_tick: u64,
    /// Periodic capture sweep (`--screenshot-every` + `--screenshot-dir`).
    pub sweep: Option<ScreenshotSweep>,
    /// tick -> pad button mask pressed for exactly that tick.
    pub pad_script: std::collections::HashMap<u64, u16>,
    /// tick -> window **keys** pressed (and released again) on exactly that
    /// tick, in the listed order ([`Self::key_script`], `--key-script`).
    ///
    /// This is a different channel from [`Self::pad_script`], not a superset
    /// of it, and the split is structural rather than stylistic. `pad_script`
    /// writes the pad word directly and the keyboard handler never sees it;
    /// several of the window's surfaces are reachable *only* from the
    /// keyboard handler - every minigame entry (`K`/`U`/`L`/`O`/`M`/`B`), the
    /// fishing prize exchange (`P` + arrows + Enter) and the inline-dialogue
    /// option picker - so a pad-only script cannot open any of them. A
    /// key-script entry is delivered as a press followed immediately by a
    /// release, so a key that also maps to a pad button leaves no bit latched
    /// and the engine tick never sees an edge from it: drive pad input with
    /// `--pad-script`, dev/entry keys with this.
    pub key_script: std::collections::HashMap<u64, Vec<KeyCode>>,
    /// `LEGAIA_HUD_COUNTDOWN=<n>`: the field party HUD's idle countdown
    /// (retail's `_DAT_801F348C`) the capture must land on, so a stationary
    /// seat shows the readout exactly when the retail frame does
    /// ([`legaia_engine_core::world_map_panel_host::hud_phase_hold`]). Set by
    /// the retail-compare image channel from the state's own RAM.
    pub hud_countdown: Option<i16>,
    /// `LEGAIA_CAPTURE_TALK=<slot>@<tick>`: at that world tick, talk to the
    /// scene's placement `slot` through `World::trigger_field_interact` - the
    /// call the walk-up interaction probe makes - so a capture can open a
    /// merchant's or innkeeper's conversation without walking to it. The
    /// browser twin is `LegaiaRuntime::debug_talk_to_placement`.
    pub talk_at: Option<(u64, u8)>,
    /// `LEGAIA_CAPTURE_SHOP=<idx>@<tick>`: stage scene shop `idx` at `tick`
    /// (`World::debug_arm_scene_shop`), the browser page's
    /// `debug_arm_scene_shop` twin.
    pub shop_at: Option<(u64, usize)>,
    /// `LEGAIA_CAPTURE_GATE=state[,white|black,age]`: capture the first
    /// frame the battle's action SM holds the retail capture's phase
    /// ([`legaia_parity::retail_compare_battle::PhaseGate`]) instead of
    /// at a fixed tick; `capture_tick` becomes the deadline, past which the
    /// run exits without a PNG. Set by the retail-compare image channel.
    pub phase_gate: Option<legaia_parity::retail_compare_battle::PhaseGate>,
    /// `LEGAIA_BATTLE_ORBIT_YAW=<yaw>`: the battle idle orbit's azimuth the
    /// capture must show, held while the orbit owns yaw
    /// ([`legaia_engine_vm::battle_cam_script::BattleCamera::align_orbit_yaw`]).
    /// The orbit is a clock, so this is its phase alignment - the camera twin
    /// of [`Self::hud_countdown`]. Set by the retail-compare image channel
    /// from the battle state's own rotation global.
    pub battle_orbit_yaw: Option<f32>,
    /// `LEGAIA_BATTLE_BACKDROP_YAW=<angle>`: the backdrop draw's slot-1 Y
    /// angle the capture must show (`World::seed_battle_backdrop_slot_1_yaw`),
    /// held on every battle frame. The angle is time spent in keeping fights
    /// since boot, so this is its phase alignment - the backdrop twin of
    /// [`Self::battle_orbit_yaw`]. Set by the retail-compare image channel
    /// from the battle state's `0x800891D2`.
    pub battle_backdrop_yaw: Option<u16>,
    /// `LEGAIA_BATTLE_CAM_ALIGN=<16 numbers>`: retail's live battle camera
    /// and its tween endpoints (pitch, yaw, eye trio, focus trio each), for
    /// starting the captured phase's glide where retail's stood
    /// ([`legaia_engine_vm::battle_cam_script::BattleCamera::align_glide_origin`]).
    /// Set by the retail-compare image channel.
    pub battle_cam_align: Option<(
        legaia_engine_vm::battle_cam_script::BattleCamPose,
        legaia_engine_vm::battle_cam_script::BattleCamPose,
    )>,
    /// `LEGAIA_SEAT_HUD_GLIDES_LANDED=1`: the retail capture's HUD widget
    /// glides had all landed, so the captured frame lands the engine's too
    /// (`World::land_battle_hud_glides`). Set by the retail-compare image
    /// channel.
    pub seat_hud_glides_landed: bool,
    /// `LEGAIA_SEAT_HUD_GLIDES=x:y:elapsed:total,...`: the retail capture's
    /// HUD widget glides still in flight, each at the elapsed its displayed
    /// frame shows; the captured frame seats the engine's on them
    /// (`World::seat_battle_hud_glide`, the combo cluster's age). Set by the
    /// retail-compare image channel.
    pub seat_hud_glides: Vec<legaia_parity::retail_compare_battle::HudGlideSeat>,
    /// `LEGAIA_SCRIPT_GATE=<flat>:<head hex>:<pc>:<wait>`: the field twin of
    /// [`Self::phase_gate`] - capture the first frame the engine's context
    /// for a retail capture's running record holds its PC
    /// ([`legaia_parity::retail_compare_script::ScriptGate`]), resuming
    /// the record from its start when nothing runs it and paging its dialog
    /// boxes on the way. `capture_tick` is the deadline.
    pub script_gate: Option<legaia_parity::retail_compare_script::ScriptGate>,
    /// `LEGAIA_SEAT_LATCHES=59c,134` (hex): the system flags a captured
    /// record latched after the scene entry ran. The seed lands the save
    /// without them so the entry does not read them, and they are raised at
    /// the settle tick, where retail's state holds them
    /// (`RetailObs::seed_save`).
    pub seat_latches: Vec<u16>,
    /// `LEGAIA_SEAT_OBJECT_MODELS=<record>:<model>,..`: a retail state's
    /// drawn actors' live model ids, written over the placed objects' stream
    /// swaps (`World::object_live_models`) on the capture frame.
    pub seat_object_models: Vec<(usize, i16)>,
    /// `LEGAIA_SEAT_OBJECT_CLIPS=<record>:<clip>:<cursor>:<flags>:<rate>;..`:
    /// a retail state's drawn actors' clip words
    /// (`legaia_parity::retail_compare::retail_object_clips`), written over
    /// the matching placed props' clips on the capture frame
    /// (`World::seed_object_prop_clip`).
    pub seat_object_clips: Vec<legaia_parity::retail_compare::ObjectClipSeed>,
    /// `LEGAIA_SEAT_MORPHS=<fields>`: a retail state's live VDF morph
    /// envelopes (`legaia_parity::retail_compare::retail_morphs`), written
    /// over the engine's on the capture frame (`World::seed_field_morph`).
    pub seat_morphs: Vec<legaia_parity::retail_compare::MorphSeed>,
    /// `LEGAIA_SEAT_WALKERS=<fields>`: a retail state's ambient walkers'
    /// live seats (`legaia_parity::retail_compare::retail_walkers`), stood
    /// on the capture frame (`World::seed_ambient_walker`).
    pub seat_walkers: Vec<legaia_parity::retail_compare::WalkerSeed>,
    /// `LEGAIA_SEAT_PANEL=<fields>`: a retail state's live image-panel
    /// widget (`legaia_parity::retail_compare::retail_panel`), installed on
    /// the capture frame - the vignette record that spawned it ran before
    /// the record the seed resumes.
    pub seat_panel: Option<legaia_engine_core::screen_fx::PanelWidget>,
    /// `LEGAIA_SEAT_CLEAR=r,g,b`: a retail state's frame clear colour (the
    /// draw environment's `r0 / g0 / b0`), written over the engine's on the
    /// capture frame - the system script's op-`4C 13` history.
    pub seat_clear: Option<[u8; 3]>,
    /// `LEGAIA_SEAT_PAGE_MARK=frame`: which frame of the dialogue page
    /// mark's two-frame strip a retail state's displayed frame shows
    /// (`legaia_parity::retail_compare::retail_page_mark`), seated on the
    /// capture frame.
    pub seat_page_mark: Option<u8>,
    /// `LEGAIA_SEAT_FOG`: a retail state's live fog-pool records, installed
    /// over the pool on the frame the capture is taken
    /// (`FogPool::install_snapshot`); taken once.
    pub seat_fog: std::cell::Cell<Option<Vec<legaia_engine_core::fog_particles::FogParticle>>>,
    /// `LEGAIA_SEAT_SPRITE_ARMS`: a retail state's live sprite-arm sheets,
    /// installed over the ambient pool's on the frame the capture is taken
    /// (`World::install_sprite_arm_snapshot`); taken once.
    pub seat_sprite_arms:
        std::cell::Cell<Option<Vec<legaia_engine_core::world::ambient::SpriteArmSeed>>>,
    /// `LEGAIA_BATTLE_DRIVE=menu,<flow>,<seat>` /
    /// `action,<seat>,<state>,<category>,<queued>`: walk the fight through
    /// its pad path to the retail capture's phase and capture the first
    /// frame that holds it
    /// ([`legaia_parity::retail_compare_battle::BattleDrive`]), the
    /// same driver the headless seed runs. `capture_tick` becomes the
    /// deadline. Set by the retail-compare image channel.
    pub battle_drive: Option<legaia_parity::retail_compare_battle::BattleDrive>,
    /// Whether the drive's one-shot world seed has been armed (it is armed
    /// on the first battle tick).
    pub battle_drive_primed: std::cell::Cell<bool>,
    /// Ticks the drive's phase has held so far; the capture waits for
    /// [`legaia_parity::retail_compare_battle::BattleDrive::hold_ticks`]
    /// of them, as the headless seed does.
    pub battle_drive_held: std::cell::Cell<u32>,
    /// `LEGAIA_BATTLE_BARS=slot:hp:mp,...`: the capture's mid-fight HP / MP,
    /// seeded on the first battle tick as the headless seed seeds them
    /// ([`legaia_parity::retail_compare_battle::apply_bar_seeds`]).
    pub battle_bars: Vec<legaia_parity::retail_compare_battle::BarSeed>,
    /// Whether [`Self::battle_bars`] have been seeded.
    pub battle_bars_seeded: std::cell::Cell<bool>,
}

/// Debug state `play-window` seeds before the first world tick, so a capture
/// run can reach a surface an ordinary boot would have to be played into.
///
/// Both members are **debug affordances**, not player surfaces: they write
/// game state a save would otherwise have to carry. They exist because the
/// two screens the parity sweeps most need - a Seru cast and a Muscle Dome
/// ring with a chip crossed out - are each gated on state no CLI flag could
/// reach. A cast needs the caster to have *learned* the spell (the Magic arm
/// parks on an unlearned action id), and the dome's magic ban is seeded at
/// arena entry from story flags `0x536` / `0x537` / `0x538`
/// (`legaia_engine_core::muscle_dome::COURSE_UNLOCK_FLAGS`), the last one set
/// winning with `0x321` - course 2 plus
/// `SPECIAL_MAGIC_FORBIDDEN`.
#[derive(Debug, Default, Clone)]
pub struct DebugSeeds {
    /// `--learn-spell <ID>`, repeatable: spell ids prepended (level 1) to the
    /// lead party member's learned list, the same path the pre-existing
    /// `LEGAIA_LEARN_SPELLS` env var takes.
    pub learn_spells: Vec<u8>,
    /// `--set-flag <N>`, repeatable: system/story-flag indices raised in the
    /// shared bank `DAT_80085758`.
    pub story_flags: Vec<u16>,
    /// `--resume-save <PATH>`: an LGSF file to land the way a card Load
    /// does ([`crate::boot::BootSession::resume_save`]) in
    /// place of the `--scene` door entry.
    pub resume_save: Option<std::path::PathBuf>,
    /// `--cheat-*`: the player cheats applied once the scene is entered.
    pub cheats: PlayCheats,
}

/// The `--cheat-*` operands - the native twin of the browser play page's
/// Cheats panel (its "No encounters" switch is `--no-live-loop` here, and
/// `F7` at runtime). Both hosts build the shared
/// `legaia_engine_core::cheats::PlayerCheat` list and apply it through
/// `World::apply_cheat`.
#[derive(Debug, Default, Clone)]
pub struct PlayCheats {
    pub level: Option<u8>,
    pub gold: Option<u32>,
    pub coins: Option<u32>,
    /// `ITEM[:QTY]` specs, resolved against the disc's item names.
    pub items: Vec<String>,
    /// `--cheat-restore`: HP / MP refilled and the fallen revived at scene
    /// entry.
    pub restore: bool,
    pub max_ap: bool,
    pub seru_level: Option<u8>,
    pub arts: bool,
    pub max_items: bool,
}

impl PlayCheats {
    /// The requested cheats as the shared list
    /// ([`legaia_engine_core::cheats::PlayerCheat`]) - the values the play
    /// page's Cheats panel builds, in panel order.
    pub fn list(&self) -> Vec<legaia_engine_core::cheats::PlayerCheat> {
        use legaia_engine_core::cheats::PlayerCheat;
        let mut out = Vec::new();
        out.extend(self.level.map(PlayerCheat::PartyLevel));
        if self.restore {
            out.push(PlayerCheat::Restore);
        }
        if self.max_ap {
            out.push(PlayerCheat::MaxAp);
        }
        out.extend(self.seru_level.map(PlayerCheat::GrantSeru));
        if self.arts {
            out.push(PlayerCheat::LearnAllArts);
        }
        if self.max_items {
            out.push(PlayerCheat::MaxItems);
        }
        out.extend(self.gold.map(|g| PlayerCheat::Gold(i64::from(g))));
        out.extend(self.coins.map(|c| PlayerCheat::Coins(u64::from(c))));
        out.extend(self.items.iter().map(|s| PlayerCheat::give_item_spec(s)));
        out
    }

    /// Apply every requested cheat to `world` through
    /// `World::apply_cheat`, logging each outcome line. `templates` is the
    /// executable's New Game party template, which a level below the current
    /// one is rebuilt from.
    pub(crate) fn apply(
        &self,
        world: &mut legaia_engine_core::world::World,
        templates: Option<&legaia_asset::new_game::StartingParty>,
    ) {
        for cheat in self.list() {
            let line = world.apply_cheat(&cheat, templates);
            log::info!("play-window: --cheat-{}: {line}", cheat.key());
        }
    }
}

impl DebugSeeds {
    /// Attach the `--cheat-*` operands.
    pub fn with_cheats(mut self, cheats: PlayCheats) -> Self {
        self.cheats = cheats;
        self
    }

    /// Parse the two repeatable operands. Each accepts decimal or `0x` hex.
    pub fn from_args(
        learn_spell: &[String],
        set_flag: &[String],
        resume_save: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        fn num(s: &str) -> Option<u32> {
            let s = s.trim();
            s.strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .map_or_else(
                    || s.parse::<u32>().ok(),
                    |h| u32::from_str_radix(h, 16).ok(),
                )
        }
        let mut seeds = Self {
            resume_save,
            ..Self::default()
        };
        for s in learn_spell {
            let v = num(s)
                .filter(|v| *v <= u32::from(u8::MAX))
                .with_context(|| format!("--learn-spell '{s}' is not a spell id (0..=0xFF)"))?;
            seeds.learn_spells.push(v as u8);
        }
        for s in set_flag {
            let v = num(s)
                .filter(|v| *v <= u32::from(u16::MAX))
                .with_context(|| format!("--set-flag '{s}' is not a flag index (0..=0xFFFF)"))?;
            seeds.story_flags.push(v as u16);
        }
        Ok(seeds)
    }
}

/// Periodic capture sweep for `play-window`: one PNG (`tick_%05d.png`) into
/// [`Self::dir`] every [`Self::every`] ticks; the run exits after the
/// capture at/past [`Self::last_tick`] (or when the window closes).
pub struct ScreenshotSweep {
    pub dir: std::path::PathBuf,
    pub every: u64,
    pub last_tick: Option<u64>,
}

impl ScreenshotConfig {
    /// Parse the CLI flags into a config. `None` when none of `--screenshot`,
    /// `--screenshot-every` and `--key-script` is present. Errors on an
    /// unparseable `--pad-script` / `--key-script` entry or a half-specified
    /// sweep.
    ///
    /// `--key-script` alone is enough to build a config: it is the scripted
    /// half of a capture run and dropping it silently when no PNG was asked
    /// for would make a scripted run look like it simply did nothing.
    pub fn from_args(
        path: Option<std::path::PathBuf>,
        capture_tick: u64,
        every: Option<u64>,
        dir: Option<std::path::PathBuf>,
        last_tick: Option<u64>,
        pad_script: Option<&str>,
        key_script: Option<&str>,
    ) -> Result<Option<Self>> {
        let sweep = match (every, dir) {
            (Some(every), Some(dir)) => {
                anyhow::ensure!(every > 0, "--screenshot-every must be > 0");
                Some(ScreenshotSweep {
                    dir,
                    every,
                    last_tick,
                })
            }
            (None, None) => None,
            _ => anyhow::bail!("--screenshot-every and --screenshot-dir must be set together"),
        };
        if path.is_none() && sweep.is_none() && key_script.is_none() {
            return Ok(None);
        }
        let mut script = std::collections::HashMap::new();
        if let Some(spec) = pad_script {
            for entry in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (tick, btn) = entry
                    .split_once(':')
                    .with_context(|| format!("pad-script entry '{entry}' is not TICK:BUTTON"))?;
                let button = legaia_engine_core::input::PadButton::from_name(btn.trim())
                    .with_context(|| format!("pad-script button '{btn}' is not a pad button"))?;
                // `TICK` (one-tick edge) or `FIRST-LAST` (held across the
                // inclusive range - walking somewhere needs a held direction).
                let tick = tick.trim();
                let (first, last) =
                    match tick.split_once('-') {
                        Some((a, b)) => {
                            let a: u64 = a.trim().parse().with_context(|| {
                                format!("pad-script tick '{a}' is not a number")
                            })?;
                            let b: u64 = b.trim().parse().with_context(|| {
                                format!("pad-script tick '{b}' is not a number")
                            })?;
                            anyhow::ensure!(a <= b, "pad-script range '{tick}' is reversed");
                            (a, b)
                        }
                        None => {
                            let t: u64 = tick.parse().with_context(|| {
                                format!("pad-script tick '{tick}' is not a number")
                            })?;
                            (t, t)
                        }
                    };
                for t in first..=last {
                    *script.entry(t).or_insert(0) |= button.mask();
                }
            }
        }
        let mut keys: std::collections::HashMap<u64, Vec<KeyCode>> =
            std::collections::HashMap::new();
        if let Some(spec) = key_script {
            for entry in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (tick, key) = entry
                    .split_once(':')
                    .with_context(|| format!("key-script entry '{entry}' is not TICK:KEY"))?;
                let code = key_from_name(key.trim())
                    .with_context(|| format!("key-script key '{key}' is not a window key name"))?;
                let t: u64 = tick
                    .trim()
                    .parse()
                    .with_context(|| format!("key-script tick '{tick}' is not a number"))?;
                keys.entry(t).or_default().push(code);
            }
        }
        Ok(Some(Self {
            path,
            capture_tick,
            sweep,
            pad_script: script,
            key_script: keys,
            talk_at: std::env::var("LEGAIA_CAPTURE_TALK").ok().and_then(|v| {
                let (slot, tick) = v.trim().split_once('@')?;
                Some((tick.trim().parse().ok()?, slot.trim().parse().ok()?))
            }),
            shop_at: std::env::var("LEGAIA_CAPTURE_SHOP").ok().and_then(|v| {
                let (idx, tick) = v.trim().split_once('@')?;
                Some((tick.trim().parse().ok()?, idx.trim().parse().ok()?))
            }),
            hud_countdown: std::env::var("LEGAIA_HUD_COUNTDOWN")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            phase_gate: std::env::var("LEGAIA_CAPTURE_GATE")
                .ok()
                .and_then(|v| legaia_parity::retail_compare_battle::PhaseGate::from_env(&v)),
            battle_orbit_yaw: std::env::var("LEGAIA_BATTLE_ORBIT_YAW")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_backdrop_yaw: std::env::var("LEGAIA_BATTLE_BACKDROP_YAW")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_cam_align: std::env::var("LEGAIA_BATTLE_CAM_ALIGN")
                .ok()
                .and_then(|v| legaia_parity::retail_compare_battle::cam_align_from_env(&v)),
            seat_hud_glides_landed: std::env::var_os("LEGAIA_SEAT_HUD_GLIDES_LANDED").is_some(),
            seat_hud_glides: std::env::var("LEGAIA_SEAT_HUD_GLIDES")
                .map(|v| legaia_parity::retail_compare_battle::HudGlideSeat::list_from_env(&v))
                .unwrap_or_default(),
            script_gate: std::env::var("LEGAIA_SCRIPT_GATE")
                .ok()
                .and_then(|v| legaia_parity::retail_compare_script::ScriptGate::from_env(&v)),
            seat_fog: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_FOG")
                    .ok()
                    .map(|v| legaia_parity::retail_compare::fog_from_env(&v)),
            ),
            seat_sprite_arms: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_SPRITE_ARMS")
                    .ok()
                    .map(|v| legaia_parity::retail_compare::sprite_arms_from_env(&v)),
            ),
            seat_clear: std::env::var("LEGAIA_SEAT_CLEAR").ok().and_then(|v| {
                let c: Vec<u8> = v.split(',').filter_map(|e| e.trim().parse().ok()).collect();
                <[u8; 3]>::try_from(c).ok()
            }),
            seat_page_mark: std::env::var("LEGAIA_SEAT_PAGE_MARK")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            seat_panel: std::env::var("LEGAIA_SEAT_PANEL")
                .ok()
                .and_then(|v| legaia_parity::retail_compare::panel_from_env(&v)),
            seat_walkers: std::env::var("LEGAIA_SEAT_WALKERS")
                .map(|v| legaia_parity::retail_compare::walkers_from_env(&v))
                .unwrap_or_default(),
            seat_morphs: std::env::var("LEGAIA_SEAT_MORPHS")
                .map(|v| legaia_parity::retail_compare::morphs_from_env(&v))
                .unwrap_or_default(),
            seat_object_clips: std::env::var("LEGAIA_SEAT_OBJECT_CLIPS")
                .map(|v| legaia_parity::retail_compare::object_clips_from_env(&v))
                .unwrap_or_default(),
            seat_object_models: std::env::var("LEGAIA_SEAT_OBJECT_MODELS")
                .map(|v| {
                    v.split(',')
                        .filter_map(|e| {
                            let (r, m) = e.split_once(':')?;
                            Some((r.trim().parse().ok()?, m.trim().parse().ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            seat_latches: std::env::var("LEGAIA_SEAT_LATCHES")
                .map(|v| {
                    v.split(',')
                        .filter_map(|i| u16::from_str_radix(i.trim(), 16).ok())
                        .collect()
                })
                .unwrap_or_default(),
            battle_drive: std::env::var("LEGAIA_BATTLE_DRIVE")
                .ok()
                .and_then(|v| legaia_parity::retail_compare_battle::BattleDrive::from_env(&v)),
            battle_drive_primed: std::cell::Cell::new(false),
            battle_drive_held: std::cell::Cell::new(0),
            battle_bars: std::env::var("LEGAIA_BATTLE_BARS")
                .map(|v| legaia_parity::retail_compare_battle::bar_seeds_from_env(&v))
                .unwrap_or_default(),
            battle_bars_seeded: std::cell::Cell::new(false),
        }))
    }
}

/// Window key name -> winit `KeyCode`, the inverse of [`keycode_to_name`] over
/// the whole physical-key surface `play-window` listens on (not just the
/// pad-bindable subset that function covers).
///
/// Names are case-insensitive. Every dev / entry key the window's keyboard
/// handler tests for is a plain letter, so the letter rows are what make
/// `--key-script` able to open a minigame at all.
pub(super) fn key_from_name(name: &str) -> Option<KeyCode> {
    Some(match name.trim().to_ascii_uppercase().as_str() {
        "UP" => KeyCode::ArrowUp,
        "DOWN" => KeyCode::ArrowDown,
        "LEFT" => KeyCode::ArrowLeft,
        "RIGHT" => KeyCode::ArrowRight,
        "ENTER" | "RETURN" => KeyCode::Enter,
        "SPACE" => KeyCode::Space,
        "RSHIFT" => KeyCode::ShiftRight,
        "A" => KeyCode::KeyA,
        "B" => KeyCode::KeyB,
        "C" => KeyCode::KeyC,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "M" => KeyCode::KeyM,
        "N" => KeyCode::KeyN,
        "O" => KeyCode::KeyO,
        "P" => KeyCode::KeyP,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "S" => KeyCode::KeyS,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        // Window-toggle band. F-keys can never collide with a pad binding,
        // which is why the three arms that used to sit on `E` / `V` / `C`
        // live here (see `keycode_to_name`).
        // Every F-key the keyboard handler has an arm for, so the
        // enhancement toggles (`F4` occlusion fade, `F8` time of day, `F9`
        // ground fog) and the cheats (`F6` / `F7`) are scriptable like the
        // rest - a capture comparing a toggle against the play page's
        // checkbox needs the native half reachable from a script.
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "W" => KeyCode::KeyW,
        "X" => KeyCode::KeyX,
        "Y" => KeyCode::KeyY,
        "Z" => KeyCode::KeyZ,
        "0" => KeyCode::Digit0,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        "5" => KeyCode::Digit5,
        "6" => KeyCode::Digit6,
        "7" => KeyCode::Digit7,
        "8" => KeyCode::Digit8,
        "9" => KeyCode::Digit9,
        _ => return None,
    })
}

/// Write a [`CaptureImage`] (RGBA8, row-major) to a PNG file. Used by the
/// `--screenshot` harness in the redraw path.
pub fn write_capture_png(path: &Path, img: &CaptureImage) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating screenshot dir {}", dir.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("creating screenshot {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .context("png header")?
        .write_image_data(&img.rgba)
        .context("png data")?;
    Ok(())
}

/// Map a winit `KeyCode` to the user-friendly key name used in
/// [`legaia_engine_core::input::Mapping`]. Returns `""` for keys outside
/// the default set.
/// Physical key -> the key **name** the engine's binding table is keyed by
/// ([`legaia_engine_core::input::Mapping`]).
///
/// This function is a filter, and it was silently narrower than the table it
/// feeds: `C`, `V`, `D` and `E` are bound (Triangle, Square, Right, R1) and
/// none of them was listed, so `pad_button_for_key` got `""` and the native
/// window could not press **Triangle or Square at all**. Nothing surfaced
/// that - a key that resolves to no button is indistinguishable from a key
/// the player did not press - and it takes two of the four face buttons off
/// the keyboard, which is most of the dance minigame's input and the
/// three-actor talk's leader switch.
///
/// Every name here must appear in `Mapping::web_default`, and every key that
/// table binds must appear here. Two of the four repaired rows are still
/// shadowed *upstream*, by a window key arm in `event_handler/keyboard.rs`
/// that returns before the pad lookup: `C` (camera vantage) and `D`
/// (occlusion fade, named in the user guide). Moving those two arms onto
/// non-pad keys is what finishes the set.
pub(super) fn keycode_to_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyC => "C",
        KeyCode::KeyV => "V",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyD => "D",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyW => "W",
        KeyCode::KeyE => "E",
        KeyCode::Enter => "Enter",
        KeyCode::Space => "Space",
        KeyCode::ShiftRight => "RShift",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        _ => "",
    }
}

#[cfg(test)]
mod cheat_list_host_tests {
    /// Every row of the shared cheat list has a control on both hosts: a
    /// `--cheat-<key>` flag here and a `cheat(rt, '<key>'` call on the play
    /// page. "No encounters" is the one switch each host already had under
    /// another name (`--no-live-loop` / `F7`, the page's checkbox).
    #[test]
    fn every_listed_cheat_has_a_control_on_both_hosts() {
        let cli = include_str!("../bin/legaia-engine/cli.rs");
        let page = include_str!("../../../../site/_content/play.html");
        for (key, _) in legaia_engine_core::cheats::PLAYER_CHEAT_LIST {
            if key == "encounters" {
                assert!(cli.contains("no_live_loop"));
                assert!(page.contains("play-cheat-no-encounters"));
                continue;
            }
            let flag = format!("cheat_{}:", key.replace('-', "_"));
            assert!(cli.contains(&flag), "native flag for cheat '{key}'");
            let call = format!("cheat(rt, '{key}'");
            assert!(page.contains(&call), "play page control for cheat '{key}'");
        }
        assert!(page.contains("cheat_apply_codes"), "page codes box");
        assert!(cli.contains("cheat_file:"), "native --cheat-file");
    }
}

#[cfg(test)]
mod key_script_tests {
    use super::{ScreenshotConfig, key_from_name};
    use winit::keyboard::KeyCode;

    /// Every key the window's own handler tests for has to be nameable, or
    /// `--key-script` silently cannot reach that arm.
    ///
    /// The minigame entries are the reason this exists: they are the only way
    /// into the dance / fishing / casino / dome / duel surfaces, `--pad-script`
    /// cannot open any of them, and a missing name here fails as "the script
    /// parsed but nothing happened" rather than as an error.
    #[test]
    fn every_minigame_entry_key_is_nameable() {
        for (name, want) in [
            ("K", KeyCode::KeyK),
            ("U", KeyCode::KeyU),
            ("L", KeyCode::KeyL),
            ("P", KeyCode::KeyP),
            ("O", KeyCode::KeyO),
            ("M", KeyCode::KeyM),
            ("B", KeyCode::KeyB),
            ("Down", KeyCode::ArrowDown),
            ("Enter", KeyCode::Enter),
        ] {
            assert_eq!(key_from_name(name), Some(want), "key name '{name}'");
            assert_eq!(
                key_from_name(&name.to_lowercase()),
                Some(want),
                "key names are case-insensitive"
            );
        }
        assert_eq!(key_from_name("NotAKey"), None);
    }

    /// Every `KeyCode` the keyboard handler has an arm for is reachable from
    /// `--key-script`, read off the handler's own source so a new arm is
    /// covered the day it lands.
    ///
    /// The F-key band was the hole: `F4` / `F8` / `F9` - the occlusion fade,
    /// the time of day and the ground fog, the three enhancement toggles a
    /// side-by-side against the play page's checkboxes has to flip - had arms
    /// and no names, so a script could not drive the native half of the
    /// comparison at all.
    #[test]
    fn every_handler_key_is_nameable() {
        let src = include_str!("event_handler/keyboard.rs");
        let mut names: Vec<String> = ["Up", "Down", "Left", "Right", "Enter", "Space", "RShift"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        names.extend((b'A'..=b'Z').map(|c| (c as char).to_string()));
        names.extend((0..=9).map(|d| d.to_string()));
        names.extend((1..=12).map(|f| format!("F{f}")));
        let reachable: std::collections::BTreeSet<String> = names
            .iter()
            .filter_map(|n| key_from_name(n))
            .map(|k| format!("{k:?}"))
            .collect();
        let mut handled = std::collections::BTreeSet::new();
        for (i, _) in src.match_indices("KeyCode::") {
            let tail = &src[i + "KeyCode::".len()..];
            let end = tail
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(tail.len());
            handled.insert(tail[..end].to_string());
        }
        // `Escape` quits through the event loop (`handle_keyboard`), which a
        // script deliberately never reaches; `--screenshot-last-tick` ends a
        // scripted run instead.
        handled.remove("Escape");
        assert!(
            handled.len() >= 20,
            "the handler reads its keys as `KeyCode::*`"
        );
        let missing: Vec<&String> = handled.iter().filter(|k| !reachable.contains(*k)).collect();
        assert!(
            missing.is_empty(),
            "handler keys `--key-script` cannot name: {missing:?}"
        );
    }

    /// Every key the engine's default pad table binds must survive
    /// `keycode_to_name`, or the native window cannot press that button.
    ///
    /// The filter was narrower than the table and nothing said so: a key that
    /// resolves to no button looks exactly like a key nobody pressed, so
    /// **Triangle and Square were unreachable from the keyboard** while every
    /// gate stayed green. The assertion runs in the direction that catches
    /// that - from the table to the filter, not the reverse - so adding a
    /// binding without a name fails here rather than in a player's hands.
    #[test]
    fn every_bound_key_survives_the_window_name_filter() {
        use legaia_engine_core::input::Mapping;
        let mapping = Mapping::web_default();
        let mut checked = 0;
        let mut names: Vec<&String> = mapping.bindings.keys().collect();
        names.sort();
        for name in names {
            let Some(code) = super::key_from_name(name) else {
                panic!("bound key '{name}' has no KeyCode - `--key-script` cannot reach it");
            };
            assert_eq!(
                super::keycode_to_name(code),
                name.as_str(),
                "bound key '{name}' does not survive keycode_to_name - the native window \
                 resolves it to no pad button"
            );
            checked += 1;
        }
        assert!(checked >= 16, "the default table binds at least 16 keys");
    }

    /// `--key-script` alone builds a config. Returning `None` unless a PNG was
    /// also asked for would drop the whole script, and a scripted run that
    /// dropped its script looks exactly like one that had nothing to do.
    #[test]
    fn a_key_script_alone_is_enough_to_build_a_config() {
        let cfg = ScreenshotConfig::from_args(None, 0, None, None, None, None, Some("40:L,40:P"))
            .expect("parses")
            .expect("a key script alone builds a config");
        assert_eq!(
            cfg.key_script.get(&40).map(Vec::as_slice),
            Some([KeyCode::KeyL, KeyCode::KeyP].as_slice()),
            "same-tick keys keep their listed order"
        );
        assert!(
            ScreenshotConfig::from_args(None, 0, None, None, None, None, None)
                .expect("parses")
                .is_none(),
            "no capture and no script is still no config"
        );
    }

    /// An unparseable entry is an error, not a silently empty script.
    #[test]
    fn a_bad_key_script_entry_errors() {
        for bad in ["40", "40:NotAKey", "x:L"] {
            assert!(
                ScreenshotConfig::from_args(None, 0, None, None, None, None, Some(bad)).is_err(),
                "'{bad}' must not parse"
            );
        }
    }
}

#[cfg(test)]
mod key_binding_tests {
    use super::keycode_to_name;
    use legaia_engine_core::input::{Mapping, PadButton};
    use winit::keyboard::KeyCode;

    /// Start answers to Enter and Space on the desktop build too.
    ///
    /// The native path has two halves that can disagree silently: this file
    /// turns a winit `KeyCode` into a key *name*, and `Mapping` turns that name
    /// into a button. A name missing from `keycode_to_name` resolves to `""`,
    /// which binds to nothing - so the binding can be correct in the shared
    /// table and still dead in the window, with neither file looking wrong.
    #[test]
    fn start_answers_to_both_enter_and_space() {
        let m = Mapping::default();
        for code in [KeyCode::Enter, KeyCode::Space] {
            let name = keycode_to_name(code);
            assert!(!name.is_empty(), "{code:?} must resolve to a key name");
            assert_eq!(
                m.pad_button_for_key(name),
                Some(PadButton::Start),
                "{code:?} -> {name} must be Start"
            );
        }
    }
}
