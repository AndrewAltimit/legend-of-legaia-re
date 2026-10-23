//! The scene set and its run labels: every playable scene off the disc, the
//! minigame / shop / round-trip / save label suffixes, the scene filter and
//! the seed list.

use super::*;

// ---------------------------------------------------------------------------
// Scene set
// ---------------------------------------------------------------------------

/// Every CDNAME label that resolves to a playable scene (a kingdom overworld,
/// or a scene whose field MAN resolves), unioned with the decoded `0x3F`
/// destinations of those scenes that also resolve.
pub(super) fn scene_set(session: &NativeSession) -> Vec<String> {
    use legaia_engine_field::man_field_scripts::scene_destinations;
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
        let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
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

pub(super) fn default_jobs() -> usize {
    env_u64("LEGAIA_SOAK_JOBS")
        .map(|n| n as usize)
        .unwrap_or(4)
        .max(1)
}

/// Minigame pseudo-scenes soaked alongside the scene set: each venue plus the
/// door-warp `sub_id` it requests (fishing 0, slot 3, Baka Fighter 4, Muscle
/// Dome 5, dance 6 - `legaia_engine_mode::minigame_entry::MinigameSubId`).
pub(super) const MINIGAME_RUNS: [&str; 5] = [
    "balden+mg0",
    "koin1+mg3",
    "koin3+mg4",
    "koin3+mg5",
    "koin1+mg6",
];

/// `"town01+rt"` -> `("town01", true)`: the run round-trips a save every
/// [`ROUND_TRIP_EVERY`] frames. A plain label passes through.
pub(super) fn split_round_trip(label: &str) -> (&str, bool) {
    match label.strip_suffix("+rt") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// `"jou+rage"` -> `("jou", true)`: every party member carries the Evil
/// Medallion's Rage passive (character record `+0xF8` bit `0x2000`, ability
/// word 1) for the whole run, so every battle is fought with each member
/// delegated (`+0x16E |= 0x380`, the anim tick's Rage arm). Outermost of the
/// run suffixes: `jou+talk+rage`.
pub(super) fn split_rage(label: &str) -> (&str, bool) {
    match label.strip_suffix("+rage") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// The Rage passive's bit in ability word 1 (record `+0xF8`).
const RAGE_BIT: u32 = 0x2000;

/// Raise [`RAGE_BIT`] on every roster member. Re-applied every frame: an
/// equipment change rebuilds the ability bits from what is worn.
pub(super) fn give_party_rage(w: &mut legaia_engine_core::world::World) {
    for m in &mut w.party.roster.members {
        let mut bits = m.ability_bits();
        let word = u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]) | RAGE_BIT;
        bits[4..8].copy_from_slice(&word.to_le_bytes());
        m.set_ability_bits(bits);
    }
}

/// `"town01+shop"` -> `("town01", true)`: the run opens one of the scene's
/// shops every [`SHOP_VISIT_EVERY`] free field frames.
pub(super) fn split_shop(label: &str) -> (&str, bool) {
    match label.strip_suffix("+shop") {
        Some(scene) => (scene, true),
        None => (label, false),
    }
}

/// Frames between two shop visits in a `+shop` run.
pub(super) const SHOP_VISIT_EVERY: u64 = 900;

/// The scenes whose field MAN carries at least one priced gold shop (the
/// list scene entry decodes into `ShopState::scene_shops`), as `+shop`
/// pseudo-scenes. Entered once each in a probe session.
pub(super) fn shop_scenes(src: &Source, all: &[String]) -> Vec<String> {
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

/// `"koin1+mg3"` -> `("koin1", Some(3))`; a plain label passes through.
pub(super) fn split_minigame(label: &str) -> (&str, Option<u8>) {
    match label.split_once("+mg") {
        Some((scene, id)) => (scene, id.parse().ok()),
        None => (label, None),
    }
}

pub(super) fn filter_scenes(all: &[String], extra: &[String]) -> Vec<String> {
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
            // Every field scene again as a talk-visit run.
            .chain(
                all.iter()
                    .filter(|s| !is_world_map_scene(s))
                    .map(|s| format!("{s}+talk")),
            )
            .chain(extra.iter().cloned())
            // The boot front end's two ways into the game.
            .chain(FRONT_END_RUNS.iter().map(|s| s.to_string()))
            .collect(),
    };
    // `LEGAIA_SOAK_SAVE=<save>` plays every run from that card save. A
    // `LEGAIA_SOAK_CARD` other than the default rides in the label
    // (`@<card>:<save>`), so a replay written from the run names its card.
    if let Ok(save) = std::env::var("LEGAIA_SOAK_SAVE")
        && !save.trim().is_empty()
    {
        let save = match std::env::var("LEGAIA_SOAK_CARD") {
            Ok(card) if !card.trim().is_empty() && card.trim() != SOAK_CARD => {
                format!("{}:{}", card.trim(), save.trim())
            }
            _ => save.trim().to_string(),
        };
        for s in &mut scenes {
            if !s.contains('@') {
                *s = format!("{s}@{save}");
            }
        }
    }
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

pub(super) fn seed_list(default_count: u64) -> Vec<u64> {
    let base = env_u64("LEGAIA_SOAK_SEED_BASE").unwrap_or(1);
    let n = env_u64("LEGAIA_SOAK_SEEDS").unwrap_or(default_count);
    (base..base + n).collect()
}
