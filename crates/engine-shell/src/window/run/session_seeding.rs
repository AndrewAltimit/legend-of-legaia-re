//! Debug seeding of a freshly booted play-window session: the cheat file,
//! the `--party` spec, the `--set-flag` story flags and the `--battle` fight.

use super::*;

/// Parse a GameShark `.gs.txt` or Mednafen `.cht` cheat file and
/// apply every entry to `world` through the
/// [`legaia_engine_core::cheat_applier`] registry. Logs per-entry
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
    use legaia_engine_core::cheat_applier::{CheatTextFormat, apply_text};
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
/// indices, in battle order.
pub(super) fn parse_party_spec(spec: &str) -> Result<Vec<u8>> {
    spec.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "vahn" => Ok(0u8),
            "noa" => Ok(1),
            "gala" => Ok(2),
            "terra" => Ok(3),
            other => other.parse::<u8>().map_err(|_| {
                anyhow::anyhow!(
                    "unknown party member '{t}' (use vahn/noa/gala/terra or a roster index)"
                )
            }),
        })
        .collect()
}

/// Raise every `--set-flag` index in the world's shared system-flag bank.
///
/// The bank is retail's `DAT_80085758`, the same one the field VM's op `0x07`
/// writes and `FUN_8003CE64` reads, so a raised bit is indistinguishable from
/// one a script raised. Called twice - once before scene entry and once after
/// the `--seed-party` reset - and idempotent, because setting a bit twice is
/// setting a bit.
pub(super) fn seed_debug_story_flags(session: &mut BootSession, seeds: &super::super::DebugSeeds) {
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

/// A `--battle` operand: a scene MAN formation-row index, or "the first row
/// the scene registered that carries monsters".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum BattleEntry {
    Row(u16),
    First,
}

impl BattleEntry {
    pub(super) fn parse(spec: &str) -> Result<Self> {
        let t = spec.trim();
        if t.eq_ignore_ascii_case("first") {
            return Ok(Self::First);
        }
        t.parse::<u16>().map(Self::Row).map_err(|_| {
            anyhow::anyhow!("--battle wants a formation row index or `first`, got {spec:?}")
        })
    }

    /// Resolve against the world's registered formations.
    pub(super) fn resolve(self, world: &legaia_engine_core::world::World) -> Option<u16> {
        match self {
            Self::Row(row) => Some(row),
            Self::First => world.first_rollable_formation_id(),
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
pub(super) fn arm_requested_battle(session: &mut BootSession, spec: &str) {
    let entry = match BattleEntry::parse(spec) {
        Ok(e) => e,
        Err(err) => {
            log::error!("play-window: {err:#}");
            return;
        }
    };
    let world = &mut session.host.world;
    let Some(row) = entry.resolve(world) else {
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

    // `LEGAIA_BATTLE_INFLIGHT=caster,spell,target[;x:z,...]`: dispatch that
    // cast the moment the first command prompt opens - the retail comparison
    // corpus's replay of a capture taken mid-cast (`InflightCastSeed`), with
    // each battle slot's ground (`-` keeps the seat). A debug seam.
    if let Some(seed) = std::env::var("LEGAIA_BATTLE_INFLIGHT").ok().and_then(|s| {
        let (head, ground_s) = s.split_once(';').unwrap_or((s.as_str(), ""));
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
        (v.len() == 3).then(|| legaia_engine_core::world::InflightCastSeed {
            caster: v[0],
            spell_id: v[1],
            target: v[2],
            ground,
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
