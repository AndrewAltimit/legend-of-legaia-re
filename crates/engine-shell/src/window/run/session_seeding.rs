//! Debug seeding of a freshly booted play-window session: the cheat file,
//! the `--party` spec, the `--set-flag` story flags, the `--battle` fight,
//! and the environment seeds (the capture seat and palettes, debug spells,
//! the RNG seed, the start tile, the sparring tutorial arm, the demo party).

use super::*;

/// Parse a GameShark `.gs.txt` or Mednafen `.cht` cheat file and
/// apply every entry to `world` through the
/// [`legaia_engine_cheats::cheat_applier`] registry. Logs per-entry
/// status to stderr.
pub(super) fn apply_cheat_file(
    world: &mut legaia_engine_core::world::World,
    path: &Path,
    strict: bool,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // The extension decides; a file with neither says which by its text -
    // the same sniff the play page's pasted code list goes through.
    use legaia_engine_cheats::cheat_applier::{CheatTextFormat, apply_text};
    let format = match path.extension().and_then(|s| s.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("cht") => CheatTextFormat::MednafenCht,
        _ => CheatTextFormat::sniff(&text),
    };
    let report = apply_text(world, &text, format, strict)?;
    eprintln!(
        "Cheat report ({} entries, {} writes; {} applied, {} unmapped, {} unknown):",
        report.per_entry.len(),
        report.total_writes,
        report.applied,
        report.unmapped,
        report.unknown_addresses
    );
    for entry in &report.per_entry {
        let total = entry.applied + entry.skipped;
        let tag = if entry.applied == total {
            "ok  "
        } else if entry.applied == 0 {
            "skip"
        } else {
            "part"
        };
        eprintln!(
            "  {tag}  {:.<60} {}/{} writes",
            entry.description, entry.applied, total
        );
    }
    Ok(())
}

/// Parse a `--party` composition spec: comma-separated character names
/// (case-insensitive `vahn`/`noa`/`gala`/`terra`) or 0-based roster
/// indices, in battle order (`legaia_engine_session::party_spec`, the parser
/// the browser page shares).
pub(super) fn parse_party_spec(spec: &str) -> Result<Vec<u8>> {
    legaia_engine_session::party_spec::parse_party_spec(spec).map_err(|e| anyhow::anyhow!(e))
}

/// Raise every `--set-flag` index in the world's shared system-flag bank.
///
/// The bank is retail's `DAT_80085758`, the same one the field VM's op `0x07`
/// writes and `FUN_8003CE64` reads, so a raised bit is indistinguishable from
/// one a script raised. Called twice - once before scene entry and once after
/// the `--seed-party` reset - and idempotent, because setting a bit twice is
/// setting a bit.
pub(super) fn seed_debug_story_flags(
    session: &mut NativeSession,
    seeds: &super::super::DebugSeeds,
) {
    if seeds.story_flags.is_empty() {
        return;
    }
    for &flag in &seeds.story_flags {
        session.host.world.system_flag_set(flag);
    }
    log::info!(
        "play-window: --set-flag raised {} story flag(s): {:?}",
        seeds.story_flags.len(),
        seeds.story_flags
    );
}

/// A `--battle` operand: a scene MAN formation-row index, "the first row
/// the scene registered that carries monsters", or a formation cell no row
/// carries (`cell:[s:]id,...`, hex ids, `s:` = scripted), registered as a
/// formation of its own the way the retail comparison's headless seed does
/// (`legaia_engine_capture::formation_cell`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::window) enum BattleEntry {
    Row(u16),
    First,
    Cell(legaia_engine_capture::formation_cell::FormationCell),
}

impl BattleEntry {
    pub(super) fn parse(spec: &str) -> Result<Self> {
        let t = spec.trim();
        if t.eq_ignore_ascii_case("first") {
            return Ok(Self::First);
        }
        if let Some(cell) = legaia_engine_capture::formation_cell::FormationCell::parse_spec(t) {
            return Ok(Self::Cell(cell));
        }
        t.parse::<u16>().map(Self::Row).map_err(|_| {
            anyhow::anyhow!(
                "--battle wants a formation row index, `first` or `cell:[s:]id,...`, got {spec:?}"
            )
        })
    }

    /// Resolve against the world's registered formations; a cell is
    /// registered first, its monsters read out of the archive when the
    /// catalog lacks them.
    pub(super) fn resolve(&self, session: &mut NativeSession) -> Option<u16> {
        let world = &mut session.host.world;
        match self {
            Self::Row(row) => Some(*row),
            Self::First => world.first_rollable_formation_id(),
            Self::Cell(cell) => {
                let scene = world.active_scene_label.clone();
                let index = &session.host.index;
                cell.register(world, &scene, || index.entry_bytes_extended(867).ok())
            }
        }
    }
}

/// Arm the `--battle` fight on a world that has just entered its scene.
///
/// The battle is driven through `World::force_encounter`, which hands the
/// named row to the encounter session's transition state machine exactly as a
/// region roll does - so the intro, the BGM swap and the battle-load path are
/// the ordinary ones, and what the window shows is what an organic encounter
/// shows. Nothing here runs when `--battle` is absent.
pub(super) fn arm_requested_battle(session: &mut NativeSession, spec: &str) {
    let entry = match BattleEntry::parse(spec) {
        Ok(e) => e,
        Err(err) => {
            log::error!("play-window: {err:#}");
            return;
        }
    };
    let resolved = entry.resolve(session);
    let world = &mut session.host.world;
    let Some(row) = resolved else {
        log::error!(
            "play-window: --battle {spec} found no registered formation in '{}' (rows: {:?})",
            world.active_scene_label,
            world.registered_formation_ids()
        );
        return;
    };
    // The transition is drained by the live field tick, so the loop has to be
    // on for the armed fight to open at all. `--battle` is an explicit request
    // for a fight; honour it over `--no-live-loop`.
    if !world.toggles.live_gameplay_loop {
        log::info!("play-window: --battle turns the live loop on (it drains the transition)");
        world.toggles.live_gameplay_loop = true;
    }
    // A scripted carrier's fight is entered by its own record, and that
    // record can raise a one-shot system-flag arm on the way in - the Rim
    // Elm sparring record's `50 19` two ops before its `3E FF` battle-entry
    // op, which is retail's whole condition for the sparring tutorial.
    // `--battle` on that carrier's row replays the entry without running the
    // record, so the arm is replayed with it; every other row, and a scene
    // whose script never raises the flag, is untouched.
    if world.replay_scripted_battle_arm(row) {
        log::info!(
            "play-window: --battle {row} is a scripted carrier's fight - its record's \
             system-flag arm is replayed with the entry"
        );
    }
    // The same record's op-0x35 words pick the fight's music - a scripted
    // boss's event starts its theme and selects the battle sound set just
    // before the entry op - so `--battle` replays them as well.
    if world.replay_scripted_battle_score(row) {
        log::info!(
            "play-window: --battle {row} replays its record's BGM words (track start + sound set)"
        );
    }
    // `LEGAIA_BATTLE_STAGE=N[,K]` stamps the battle-stage variant the fight
    // is staged in (`_DAT_8007BD60 & 0x1F`) and, with `K`, battle init's
    // keep-object-1 byte (`_DAT_8007B64B`), for a fight entered without
    // standing on the region tile that names them - the retail comparison
    // corpus reads both off the capture.
    if let Ok(s) = std::env::var("LEGAIA_BATTLE_STAGE") {
        let mut it = s.trim().split(',');
        if let Some(v) = it.next().and_then(|v| v.trim().parse::<u8>().ok()) {
            world.seed_battle_stage_variant(v);
        }
        if let Some(k) = it.next().and_then(|k| k.trim().parse::<u8>().ok()) {
            world.seed_battle_backdrop_keep_object_1(k != 0);
        }
    }
    // `LEGAIA_BATTLE_SHINY_SERU=1` (the retail-compare image channel, for a
    // state made on a shiny-Seru patched disc): the shiny roll's battle-entry
    // draw is that build's, so it runs even under `--retail`'s pin.
    if std::env::var("LEGAIA_BATTLE_SHINY_SERU").is_ok_and(|v| v.trim() == "1") {
        world.toggles.shiny_seru = true;
    }
    // `LEGAIA_BATTLE_FRAME_STEP=step,state` (the retail-compare image
    // channel): run the battle on frames of `step` vsyncs from the first tick
    // the action SM reaches `state` (`World::seed_battle_frame_step`).
    if let Some((step, state)) = std::env::var("LEGAIA_BATTLE_FRAME_STEP")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.trim().parse::<u8>().ok()?, b.trim().parse::<u8>().ok()?))
        })
    {
        world.seed_battle_frame_step(step, state);
    }

    // `LEGAIA_BATTLE_INFLIGHT=caster,spell,target[;x:z,...[;cursor,...]]`:
    // dispatch that cast the moment the first command prompt opens - the
    // retail comparison corpus's replay of a capture taken mid-cast
    // (`InflightCastSeed`), with each battle slot's ground (`-` keeps the
    // seat) and idle-loop cursor (`-` keeps the replay's). A debug seam.
    if let Some(seed) = std::env::var("LEGAIA_BATTLE_INFLIGHT").ok().and_then(|s| {
        let (head, rest) = s.split_once(';').unwrap_or((s.as_str(), ""));
        let (ground_s, idle_s) = rest.split_once(';').unwrap_or((rest, ""));
        let v: Vec<u8> = head
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        let mut ground = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        for (g, tok) in ground.iter_mut().zip(ground_s.split(',')) {
            *g = tok.split_once(':').and_then(|(x, z)| {
                Some([x.trim().parse::<i16>().ok()?, z.trim().parse::<i16>().ok()?])
            });
        }
        let mut idle = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        for (c, tok) in idle.iter_mut().zip(idle_s.split(',')) {
            *c = tok.trim().parse::<i32>().ok();
        }
        (v.len() == 3).then(|| legaia_engine_core::world::InflightCastSeed {
            caster: v[0],
            spell_id: v[1],
            target: v[2],
            ground,
            idle,
        })
    }) {
        log::info!("play-window: LEGAIA_BATTLE_INFLIGHT seeds {seed:?} at the first prompt");
        world.battle.inflight_seed = Some(seed);
    }
    // `LEGAIA_BATTLE_MONSTER_CAST=seat,spell` (decimal or `0x..`): the
    // monster in engine battle `seat` casts `spell` on its next turn instead
    // of the AI's pick (`BattleState::forced_monster_cast`, the seam the
    // retail comparison corpus replays a mid-cast capture through). Pairs
    // with `--battle <ROW>` to watch one enemy special from a cold boot. A
    // debug seam.
    if let Some((seat, spell)) = std::env::var("LEGAIA_BATTLE_MONSTER_CAST")
        .ok()
        .and_then(|s| {
            let parse = |t: &str| {
                let t = t.trim();
                t.strip_prefix("0x")
                    .map_or_else(|| t.parse::<u8>().ok(), |h| u8::from_str_radix(h, 16).ok())
            };
            let (a, b) = s.split_once(',')?;
            Some((parse(a)?, parse(b)?))
        })
    {
        log::info!(
            "play-window: LEGAIA_BATTLE_MONSTER_CAST seeds seat {seat} -> spell {spell:#04x}"
        );
        world.battle.forced_monster_cast = Some((seat, spell));
    }
    // `LEGAIA_BATTLE_RNG_SEED=<u32>`: the world stream's state at the entry,
    // so the fight does not inherit however many field draws the boot took.
    // The retail comparison corpus pins it on both its sides
    // (`retail_compare_battle::BATTLE_RNG_SEEDS`).
    if let Some(seed) = std::env::var("LEGAIA_BATTLE_RNG_SEED")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        world.rng_state = seed;
        world.encounters.rng_hold = Some(seed);
    }
    if world.force_encounter(row) {
        log::info!(
            "play-window: --battle armed formation row {row} in '{}' - the fight opens through \
             the normal encounter transition",
            world.active_scene_label
        );
    } else {
        world.encounters.rng_hold = None;
    }
}

/// `--seed-party`: seed the New Game starting party (Vahn from the SCUS
/// template) so the pause menu's Status / party screens render real content
/// rather than an empty roster. Runs after field entry; `begin_new_game`
/// only resets story/money/inventory + sets mode=Field, leaving the loaded
/// scene intact.
///
/// A `--world-map` entry keeps its mode: `begin_new_game`'s `mode = Field`
/// is the New Game boot's field launch, and letting it stand turned the
/// overworld into a field scene - the field fog pool (the kingdom MAN's
/// fog bit) then drew its sheets across the terrain through the field
/// camera, and the terrain took the field draw path, neither of which the
/// world map runs.
pub(super) fn seed_new_game_party(session: &mut NativeSession) {
    let entered = session.host.world.mode;
    session.begin_new_game();
    if entered == legaia_engine_mode::mode::SceneMode::WorldMap {
        session.host.world.mode = entered;
    }
    let seeded = session.host.world.party.roster.members.len();
    log::info!("play-window: --seed-party seeded {seeded} roster member(s)");
}

/// Debug seat: `LEGAIA_SEAT=X,Z` puts the player on raw world `(X, Z)`
/// (floor-sampled Y) and re-arms the zone camera's arrival snap - the
/// frame-pairing aid for comparing this host against a retail save state
/// at that state's own player position. The play page's twin is
/// `play_debug_seat`.
pub(super) fn apply_debug_seat_env(session: &mut NativeSession) {
    // `LEGAIA_SEAT_FRAME_STEP=<n>` (the retail-compare image channel): the
    // vsyncs a game frame retail had measured at the capture instant.
    if let Some(step) = std::env::var("LEGAIA_SEAT_FRAME_STEP")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
    {
        session.host.world.seed_frame_step(step);
    }
    if let Ok(seat) = std::env::var("LEGAIA_SEAT") {
        let xz: Vec<i16> = seat
            .split(',')
            .filter_map(|v| v.trim().parse::<i16>().ok())
            .collect();
        if let [x, z] = xz[..]
            && session.host.debug_seat_standing(x, z)
        {
            // `LEGAIA_SEAT_CAMERA_BLOCK` (the retail-compare image channel):
            // the snap composes from the retail state's own camera
            // parameter block instead of a tile re-query.
            match std::env::var("LEGAIA_SEAT_CAMERA_BLOCK")
                .ok()
                .and_then(|b| legaia_engine_capture::field_seeds::camera_block_from_env(&b))
            {
                Some(block) => session.camera.zone.arm_arrival_over(block),
                None => session.camera.zone.arm_arrival(),
            }
            // `LEGAIA_SEAT_FOCUS=FX,FZ` (stored form, X / Z negated): a
            // retail state's focus pair when it is not on the player - the
            // snap lands it instead of pinning the player, and the follow
            // ease leaves it there until the player moves, as retail's does.
            if let Some([fx, fz]) = std::env::var("LEGAIA_SEAT_FOCUS").ok().and_then(|f| {
                let v: Vec<i32> = f
                    .split(',')
                    .filter_map(|v| v.trim().parse::<i32>().ok())
                    .collect();
                <[i32; 2]>::try_from(v).ok()
            }) {
                session.camera.zone.seat_focus_after_snap([fx, fz]);
            }
            // `LEGAIA_SEAT_EYE=X,Y,Z`: a retail state's eye translation,
            // which the follow ease leaves short of the composed pose while
            // the player stands or is locked - the snap lands it instead.
            if let Some(eye) = std::env::var("LEGAIA_SEAT_EYE").ok().and_then(|f| {
                let v: Vec<i32> = f
                    .split(',')
                    .filter_map(|v| v.trim().parse::<i32>().ok())
                    .collect();
                <[i32; 3]>::try_from(v).ok()
            }) {
                session.camera.zone.seat_eye_after_snap(eye);
            }
            // `LEGAIA_SEAT_ANGLES=P,Y,H`: the state's pitch / yaw / `H`,
            // eased on the same moved-frame legs as the eye.
            if let Some(angles) = std::env::var("LEGAIA_SEAT_ANGLES").ok().and_then(|f| {
                let v: Vec<i32> = f
                    .split(',')
                    .filter_map(|v| v.trim().parse::<i32>().ok())
                    .collect();
                <[i32; 3]>::try_from(v).ok()
            }) {
                session.camera.zone.seat_angles_after_snap(angles);
            }
            // `LEGAIA_SEAT_HEADING=H` (retail space, `0` = -Z): stand the
            // seated player in the state's own heading, which is walk
            // history like the position - as the arrival facing too, so an
            // entry script's `4C 3A` hands over the same heading.
            if let Some(h) = std::env::var("LEGAIA_SEAT_HEADING")
                .ok()
                .and_then(|h| h.trim().parse::<i16>().ok())
            {
                session.host.world.locomotion.arrival_facing = h;
                session.host.world.apply_arrival_facing();
            }
            log::info!("play-window: LEGAIA_SEAT seated the player at ({x}, {z})");
        } else {
            log::warn!("play-window: LEGAIA_SEAT='{seat}' not applied (want X,Z and a player)");
        }
    }
}

/// The retail-compare image channel's palette seeds: a retail state's live
/// CLUT-cell cycler snapshots (`LEGAIA_SEAT_CLUT_FX`) and its scroller rects
/// with their captured texels (`LEGAIA_SEAT_VRAM_RECTS=<file>`).
pub(super) fn apply_capture_palette_seeds(session: &mut NativeSession) {
    // `LEGAIA_SEAT_CLUT_FX` (the retail-compare image channel): a retail
    // state's live CLUT-cell cycler snapshots, written over the matching
    // cycled cells so the frame shows the captured palette phase
    // (`AmbientFxState::cell_fx_seed`).
    if let Ok(fx) = std::env::var("LEGAIA_SEAT_CLUT_FX") {
        session.host.world.ambient.cell_fx_seed =
            legaia_engine_capture::field_seeds::cell_fx_from_env(&fx);
    }

    // `LEGAIA_SEAT_VRAM_RECTS=<file>` (the retail-compare image channel): a
    // retail state's scroller rects with their captured texels, written over
    // the engine's after every field VRAM pass
    // (`AmbientFxState::vram_rect_seed`).
    if let Some(bytes) =
        std::env::var_os("LEGAIA_SEAT_VRAM_RECTS").and_then(|p| std::fs::read(p).ok())
    {
        session.host.world.ambient.vram_rect_seed =
            legaia_engine_capture::field_seeds::vram_rects_from_file(&bytes);
    }
}

/// Debug learn path: `--learn-spell 0x81` (repeatable) or the older
/// `LEGAIA_LEARN_SPELLS=0x81,0x9e` prepends those spell ids (level 1) onto
/// the lead character's record, so a seeded New Game party can reach the
/// Magic arm and cast - the cast-presentation screenshot harness. A
/// development aid for the parity sweeps, not a player surface.
pub(super) fn teach_debug_spells(
    session: &mut NativeSession,
    debug_seeds: &super::super::DebugSeeds,
) {
    let parse = |s: &str| {
        let s = s.trim();
        s.strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .map_or_else(|| s.parse::<u8>().ok(), |h| u8::from_str_radix(h, 16).ok())
    };
    let from_env: Vec<u8> = std::env::var("LEGAIA_LEARN_SPELLS")
        .ok()
        .map(|list| list.split(',').filter_map(parse).collect())
        .unwrap_or_default();
    let mut learned = 0usize;
    for id in debug_seeds.learn_spells.iter().copied().chain(from_env) {
        if let Some(lead) = session.host.world.party.roster.members.first_mut() {
            legaia_engine_battle::magic_xp::learn_spell_prepend(lead, id);
            learned += 1;
        }
    }
    if learned > 0 {
        log::info!("play-window: taught the lead {learned} debug spell(s)");
    } else if !debug_seeds.learn_spells.is_empty() {
        log::warn!(
            "play-window: --learn-spell had no lead party member to teach \
                 (pass --seed-party, or --boot-ui and start a game, first)"
        );
    }
}

/// Debug RNG seed override: `LEGAIA_RNG_SEED=<u32>` (decimal or `0x` hex)
/// replaces the world's boot seed, so a sweep can be re-rolled onto a
/// different deterministic stream - the way to reach an AI branch (a
/// monster cast) the default seed never picks. A development aid for the
/// parity sweeps, not a player surface.
pub(super) fn apply_rng_seed_env(session: &mut NativeSession) {
    if let Ok(spec) = std::env::var("LEGAIA_RNG_SEED") {
        let spec = spec.trim();
        let seed = spec
            .strip_prefix("0x")
            .or_else(|| spec.strip_prefix("0X"))
            .map_or_else(
                || spec.parse::<u32>().ok(),
                |h| u32::from_str_radix(h, 16).ok(),
            );
        if let Some(seed) = seed {
            session.host.world.rng_state = seed;
            log::info!("play-window: LEGAIA_RNG_SEED reseeded the world RNG to {seed:#010x}");
        }
    }
}

/// Debug start-position override: `LEGAIA_START_TILE=X,Z` seats the player
/// at that tile's centre after boot (tile*128+0x40, the op-0x3F entry-tile
/// mapping). Useful for parking on the overworld continent - a direct
/// `--world-map` boot has no door warp to seat the player, and the ocean
/// is unwalkable, so the (0,0) default strands the marker at sea. E.g.
/// Rim Elm's retail map01 arrival is `LEGAIA_START_TILE=96,25`.
pub(super) fn apply_start_tile_env(session: &mut NativeSession) {
    if let Ok(spec) = std::env::var("LEGAIA_START_TILE") {
        let parts: Vec<i32> = spec
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        if let [tx, tz] = parts[..] {
            let (cx, cz) = (tx.clamp(0, 255) as u8, tz.clamp(0, 255) as u8);
            session.host.world.seat_player_at_tile_rescued(cx, cz);
            log::info!("play-window: LEGAIA_START_TILE seated player at tile ({cx},{cz})");
        } else {
            log::warn!("play-window: LEGAIA_START_TILE ignored (want \"X,Z\"): {spec:?}");
        }
    }
}

/// Log the battle chip / banner labels the boot read off the disc.
pub(super) fn log_battle_ui_labels(session: &NativeSession) {
    // Battle chip / banner labels off the user's own disc: the `Ambushed!` and
    // `surprised the enemy` lines, `Spirit`, `Escape` and the per-character
    // Ra-Seru magic-command name, both halves installed at boot (`session.rs`,
    // the builder the browser page shares). Empty on a partial extraction, in
    // which case the port's own wording draws instead of nothing.
    //
    // The party cast trigger's per-spell anim-pair lists are installed by
    // `NativeSession::open` for every session (`native.rs`), auto-battles
    // included.
    {
        let n = session.host.world.battle.ui_strings.len();
        log::info!("play-window: battle UI labels read off the disc ({n} string(s))");
    }
}

/// `LEGAIA_BATTLE_TUTORIAL`: a one-shot poke at the sparring tutorial's arm
/// flag, for a player-driven session.
pub(super) fn apply_battle_tutorial_env(session: &mut NativeSession) {
    // Sparring tutorial: nothing to gate here.
    //
    // The prompt corpus (stage overlay 967) is installed by scene entry for
    // every host, and the arming is the disc's own one-shot system-flag test
    // consumed in `World::enter_battle` - the port of the entity SM's
    // battle-entry tail (`FUN_801DA51C`, `0x801DA698..0x801DA6B0`). The flag
    // is raised by town01's Tetsu record executing `50 19` two ops before its
    // battle-entry op, so the tutorial fires in the fight retail fires it in
    // and in no other, with no host cooperation at all.
    //
    // What used to be here - a `--player-battle` flag AND `scene == "town01"`
    // AND an env var - was a development shim standing in for that condition,
    // and it was one-host: nothing equivalent existed on the browser page, so
    // the page drew tutorial boxes it could never be given.
    //
    // `LEGAIA_BATTLE_TUTORIAL` survives as a pure debug affordance, and each
    // arm is a one-shot poke at the same flag rather than a mode:
    //   `=0`     drop an arm the boot scene's script already raised,
    //   `=1`     raise it, so the next battle in any scene is the tutorial,
    //   `=now`   raise it AND drop straight into a fight.
    // `=0` cannot stop a *later* script SET - suppressing that would mean
    // adding a knob retail has no equivalent of.
    match std::env::var("LEGAIA_BATTLE_TUTORIAL").ok().as_deref() {
        Some("0") => {
            // Drop the arm the field script may already have raised.
            session
                .host
                .world
                .system_flag_clear(legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG);
            log::info!("play-window: LEGAIA_BATTLE_TUTORIAL=0 suppressed the sparring arm");
        }
        Some(forced) => {
            session
                .host
                .world
                .system_flag_set(legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG);
            let n = session.host.world.battle.tutorial_script.len();
            log::info!(
                "play-window: LEGAIA_BATTLE_TUTORIAL forced the sparring arm for the next \
                     battle ({n} prompt(s) off the disc)"
            );
            // `=now` drops straight into the sparring fight instead of
            // waiting for a step-driven encounter, so the prompt boxes are
            // reachable in one run. Retail reaches this fight through the
            // town01 story script.
            if forced == "now" {
                let world = &mut session.host.world;
                let pc = world.party.party_count.clamp(1, 3);
                world.enter_battle(pc, 1);
                // Seed only the combatant slots - the field scene's other
                // actors keep whatever the scene gave them.
                for a in world.actors.iter_mut().take(pc as usize + 1) {
                    if a.battle.max_hp == 0 {
                        a.battle.max_hp = 100;
                        a.battle.hp = 100;
                    }
                }
                // Tag the monster seat with an id. `enter_battle` only seats
                // actors; the id is what a step-driven encounter would have
                // written from its resolved formation, and the whole battle
                // RENDER path keys on it - `battle_monster_slots()` is empty
                // without it, so `enter_battle_render` returns before it
                // builds the stage backdrop or the ground grid. Without this
                // the forced fight showed the field scene behind its HUD.
                // `LEGAIA_BATTLE_TUTORIAL_MONSTER` picks the id.
                let mid: u16 = std::env::var("LEGAIA_BATTLE_TUTORIAL_MONSTER")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                if let Some(a) = world.actors.get_mut(pc as usize) {
                    a.battle_monster_id = Some(mid);
                }
                log::info!(
                    "play-window: LEGAIA_BATTLE_TUTORIAL=now entered the sparring fight \
                         (monster id {mid})"
                );
            }
        }
        None => {}
    }
}

/// Play-window demo seeding (NOT part of the shared field-live core): two
/// demo items, two saved chains per character and a one-command demo art
/// written over every character's `Art1B` record, so the Item / Arts
/// submenus are exercisable by hand on an empty boot save. Opt-in
/// (`LEGAIA_DEMO_BATTLE_SEED=1`): it used to ride the default-on
/// player-battle flag, so every native session that booted without a save
/// played with free items and a fabricated art the browser page never
/// had - the same engine, two different parties.
pub(super) fn seed_demo_battle(session: &mut NativeSession, player_battle: bool) {
    let demo_seed = std::env::var("LEGAIA_DEMO_BATTLE_SEED").is_ok_and(|v| v == "1");
    if player_battle && demo_seed {
        log::info!("play-window: LEGAIA_DEMO_BATTLE_SEED=1 - seeding demo items / chains / art");
        let world = &mut session.host.world;
        if world.party.inventory.is_empty() {
            world.party.inventory.insert(0x01, 5); // Healing Leaf
            world.party.inventory.insert(0x13, 3); // Bomb (offensive)
        }
        if world.party.saved_chains.is_empty() {
            use legaia_save::SavedChainRecord;
            for slot in 0u8..3 {
                world.party.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Quick".into(),
                    sequence: vec![1, 2],
                });
                world.party.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Combo".into(),
                    sequence: vec![1, 2, 3, 4],
                });
            }
            // Stage a demo art record per character so the "Combo" chain
            // (it ends in Up) resolves through the real art-power path -
            // two damage strikes that burn the target. "Quick" has no
            // matching record and falls back to the synthetic profile.
            use legaia_art::power::PowerByte;
            use legaia_art::queue::{ActionConstant, Command};
            use legaia_art::record::EnemyEffect;
            for character in legaia_art::Character::all() {
                world.set_art_record(
                    character,
                    ActionConstant::Art1B,
                    legaia_art::ArtRecord {
                        action: ActionConstant::Art1B,
                        commands: vec![Command::Up],
                        anim_index: 0,
                        anim_extra: vec![],
                        name: None,
                        power: vec![PowerByte::from_byte(0x18), PowerByte::from_byte(0x1D)],
                        dmg_timing: vec![],
                        effect_cues: Default::default(),
                        hit_cues: vec![],
                        identifier: 0,
                        anim_speed: 0,
                        enemy_effect: EnemyEffect::Toxic,
                        repeat_frames: Default::default(),
                        background: 0,
                        runtime_address: None,
                    },
                );
            }
        }
    }
}

/// Install the requested present-party composition (after the roster +
/// cheats have settled, so the actor reseed reads final records). The
/// flag overrides whatever composition the boot save carried.
pub(super) fn install_party_spec(session: &mut NativeSession, spec: &str) -> Result<()> {
    let slots = parse_party_spec(spec)?;
    // Seed the roster slots the spec names before installing the
    // composition. `--seed-party` seeds retail's New Game roster, which is
    // Vahn alone (correct for the early game), so a `--party vahn,noa,gala`
    // asked three battle ordinals to resolve to roster slots 1 and 2 that
    // did not exist: both HUD panels read `P2 0/0` / `P3 0/0` and neither
    // actor got a live HP / SPD mirror. Fill them from the same SCUS
    // template rows retail seeds those characters from when they join.
    // Records already carrying stats (a loaded save) are left alone.
    if let Some(starting) = session.starting_party.clone() {
        let seeded = session
            .host
            .world
            .seed_party_members(&starting, &slots.clone());
        if seeded > 0 {
            log::info!(
                "play-window: --party seeded {seeded} roster slot(s) from the SCUS \
                     new-game template"
            );
        }
    } else {
        log::warn!(
            "play-window: --party cannot seed absent roster slots - the SCUS \
                 starting-party template was not readable from this boot source"
        );
    }
    let world = &mut session.host.world;
    // The composition, the battle stat fold and the MP ceiling the battle
    // HUD draws: the install the browser page shares.
    legaia_engine_session::party_spec::install_party_composition(world, slots.clone());
    if world.party.active_party.len() != slots.len() {
        log::warn!(
            "play-window: --party {spec}: kept the first {} of {slots:?} \
                 (3 on-screen positions)",
            world.party.active_party.len()
        );
    }
    log::info!(
        "play-window: present party = {:?} (roster slots, battle order)",
        world.party.active_party
    );
    Ok(())
}

/// `--cheat-*`: after the roster and the present party have settled, so
/// the level cheat grows the members that will actually fight.
pub(super) fn apply_debug_cheats(
    session: &mut NativeSession,
    debug_seeds: &super::super::DebugSeeds,
) {
    let templates = session
        .host
        .new_game_defaults
        .as_ref()
        .map(|d| d.party.clone());
    debug_seeds
        .cheats
        .apply(&mut session.host.world, templates.as_ref());
}

/// `--battle <ROW|first>`: arm a deterministic fight. Last of the world
/// setup, so the party / cheats / New Game reset have all settled and the
/// combatants entering the battle are the ones the run configured.
pub(super) fn arm_launch_battle(
    session: &mut NativeSession,
    spec: &str,
    boot_ui: bool,
    party: Option<&str>,
) {
    if boot_ui {
        // The title / save-select flow enters its own scene afterwards and
        // would reset the session under the armed fight.
        log::warn!("play-window: --battle ignored under --boot-ui (the boot flow re-enters)");
    } else if session.host.world.mode == legaia_engine_mode::mode::SceneMode::Battle {
        log::warn!("play-window: --battle skipped - a battle is already open");
    } else {
        // `LEGAIA_BATTLE_SETTLE=N`: tick the landed field N frames with
        // no input before the fight is armed - the retail comparison
        // corpus's headless seed enters every battle from a field that
        // settled `retail_compare::SETTLE_TICKS` first, and seeds the
        // world stream after it. Armed at boot instead, the encounter
        // transition owned the very first tick, so the scene's entry
        // scripts (the overworld's ambient-particle gate among them)
        // never ran and the two sides entered the same fight on
        // different `rand()` streams. The ticks run here, before the
        // window loop, with the session draining its own queues and
        // running the fog pool's render step as the headless seed does.
        if let Some(n) = std::env::var("LEGAIA_BATTLE_SETTLE")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            session.set_host_drains_queues(false);
            for _ in 0..n {
                if let Err(e) = session.tick() {
                    log::error!("play-window: LEGAIA_BATTLE_SETTLE tick: {e:#}");
                    break;
                }
                session.field_vram_tick();
                session.fog_render_tick();
            }
            session.set_host_drains_queues(true);
            // `LEGAIA_BATTLE_LATCHES=23c,19` (hex): the system flags the
            // fight's own record raised on its way into the entry op.
            // The corpus lands the save without them and raises them
            // here, after the settle, where retail's record did
            // (`retail_compare_battle::entry_latches`).
            if let Ok(v) = std::env::var("LEGAIA_BATTLE_LATCHES") {
                for idx in v
                    .split(',')
                    .filter_map(|f| u16::from_str_radix(f.trim(), 16).ok())
                {
                    session.host.world.system_flag_set(idx);
                }
            }
            // The settle can run a script that re-seats the present
            // party (a scripted duel's entry); the headless seed installs
            // the fight's roster after its settle, so the child does too.
            if let Some(slots) = party.and_then(|p| parse_party_spec(p).ok())
                && slots != session.host.world.party.active_party
            {
                session.host.world.set_active_party(slots);
            }
        }
        // `LEGAIA_BATTLE_ARM_TICK=N`: hold the arm until world tick `N`, so
        // the encounter opens out of a field the window has already drawn
        // (the intro's capture is then of a frame the player saw, which a
        // boot-time arm - capturing the very first frame - cannot show).
        if let Some(tick) = std::env::var("LEGAIA_BATTLE_ARM_TICK")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&t| t > 0)
        {
            log::info!("play-window: --battle {spec} held until tick {tick}");
            if let Ok(mut slot) = DEFERRED_BATTLE.lock() {
                *slot = Some((tick, spec.to_string()));
            }
            return;
        }
        arm_requested_battle(session, spec);
    }
}

/// The `--battle` arm `LEGAIA_BATTLE_ARM_TICK` is holding: `(tick, spec)`.
static DEFERRED_BATTLE: std::sync::Mutex<Option<(u64, String)>> = std::sync::Mutex::new(None);

/// Arm a held `--battle` once the world reaches its tick
/// (`LEGAIA_BATTLE_ARM_TICK`). A no-op on every other tick and in every run
/// that holds none.
pub(in crate::window) fn fire_deferred_battle(session: &mut NativeSession, tick_no: u64) {
    let due = DEFERRED_BATTLE
        .lock()
        .ok()
        .and_then(|mut slot| match slot.as_ref() {
            Some((t, _)) if tick_no >= *t => slot.take(),
            _ => None,
        });
    if let Some((_, spec)) = due {
        arm_requested_battle(session, &spec);
    }
}
