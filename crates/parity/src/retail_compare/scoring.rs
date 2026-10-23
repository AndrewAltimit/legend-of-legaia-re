//! Per-channel scoring of an engine observation against the retail one:
//! party, flags, inventory, facing and camera scores, and the state report
//! they fold into.

use super::*;

/// Linear falloff: `1` within `full`, `0` at or beyond `zero`.
pub(super) fn falloff(delta: f64, full: f64, zero: f64) -> f64 {
    let d = delta.abs();
    if d <= full {
        1.0
    } else if d >= zero {
        0.0
    } else {
        1.0 - (d - full) / (zero - full)
    }
}

/// Wrapped difference of two 12-bit angles.
pub(super) fn angle_delta(a: i16, b: i16) -> f64 {
    let d = (i32::from(a) - i32::from(b)).rem_euclid(4096);
    f64::from(d.min(4096 - d))
}

/// The channel names, in report order.
pub const CHANNELS: &[&str] = &[
    "scene",
    "mode",
    "position",
    "footing",
    "camera",
    "facing",
    "npcs",
    "bgm",
    "fog_gate",
    "view_window",
    "party",
    "flags",
    "inventory",
    "enemies",
    "enemy_hp",
    "battle_party",
    "phase",
    "menu",
    "image",
];

/// One state's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateReport {
    pub label: String,
    pub emulator: String,
    pub fingerprint: String,
    pub scene: String,
    pub game_mode: u8,
    pub class: StateClass,
    /// Empty when seeded; otherwise why not.
    pub unseeded: String,
    /// Channel -> score in `[0, 1]`. Absent = not measured.
    pub channels: BTreeMap<String, f64>,
    /// Channel -> one-line human detail (both sides' values).
    pub detail: BTreeMap<String, String>,
    /// Mean of the measured channels.
    pub score: Option<f64>,
    /// Pixel metric detail when the image channel ran.
    pub image: Option<ImageScore>,
    /// The scenario's `resident_patch`: set when the state was made on a
    /// patched disc, so it replays that build's executable, not retail's.
    #[serde(default)]
    pub resident_patch: Option<String>,
    /// Set when the scenario names its patched image (`resident_disc`) but
    /// the run could not use it, so the engine ran the retail disc: why.
    /// The committed baseline was cut on the state's own image, so such a
    /// run is neither compared against it nor written into it
    /// ([`Baseline::fallback_skips`]).
    #[serde(default)]
    pub resident_disc_missing: Option<String>,
}

pub(crate) fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub(super) fn party_score(
    retail: &legaia_save::Party,
    engine: &legaia_save::Party,
) -> (f64, String, Vec<String>) {
    let mut total = 0usize;
    let mut equal = 0usize;
    let mut diffs = Vec::new();
    for (i, r) in retail.members.iter().enumerate() {
        let e = engine.members.get(i);
        let rv = r.hp_mp_sp();
        let fields: [(&str, i64, Option<i64>); 6] = [
            (
                "hp",
                i64::from(rv.hp_cur),
                e.map(|e| i64::from(e.hp_mp_sp().hp_cur)),
            ),
            (
                "hp_max",
                i64::from(rv.hp_max),
                e.map(|e| i64::from(e.hp_mp_sp().hp_max)),
            ),
            (
                "mp",
                i64::from(rv.mp_cur),
                e.map(|e| i64::from(e.hp_mp_sp().mp_cur)),
            ),
            (
                "mp_max",
                i64::from(rv.mp_max),
                e.map(|e| i64::from(e.hp_mp_sp().mp_max)),
            ),
            (
                "level",
                i64::from(r.level()),
                e.map(|e| i64::from(e.level())),
            ),
            (
                "equip",
                i64::from_le_bytes(pad8(&r.equipment().slots)),
                e.map(|e| i64::from_le_bytes(pad8(&e.equipment().slots))),
            ),
        ];
        for (name, want, got) in fields {
            total += 1;
            if got == Some(want) {
                equal += 1;
            } else {
                diffs.push(format!("m{i}.{name} retail={want} engine={got:?}"));
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        equal as f64 / total as f64
    };
    let detail = format!(
        "{equal}/{total} fields; retail {} member(s), engine {}",
        retail.members.len(),
        engine.members.len()
    );
    (score, detail, diffs)
}

pub(super) fn pad8(s: &[u8]) -> [u8; 8] {
    let mut o = [0u8; 8];
    for (d, v) in o.iter_mut().zip(s) {
        *d = *v;
    }
    o
}

pub(crate) fn flags_score(retail: &[u8], engine: &[u8]) -> (f64, String) {
    let n = retail.len().max(engine.len());
    let mut differ = 0u32;
    let mut union = 0u32;
    // The first few differing bits, named: a system flag by its id (the
    // bank at `+0x158`, MSB-first), anything below it by byte and mask;
    // `+` is set on the engine side only, `-` on retail's only.
    let mut named = Vec::new();
    for i in 0..n {
        let r = retail.get(i).copied().unwrap_or(0);
        let e = engine.get(i).copied().unwrap_or(0);
        differ += (r ^ e).count_ones();
        union += (r | e).count_ones();
        for bit in 0..8u8 {
            let m = 0x80u8 >> bit;
            if (r ^ e) & m != 0 && named.len() < 4 {
                let sign = if e & m != 0 { '+' } else { '-' };
                named.push(match i.checked_sub(SYSTEM_FLAG_WINDOW) {
                    Some(k) => format!("{sign}sys 0x{:03X}", k * 8 + usize::from(bit)),
                    None => format!("{sign}[0x{i:03X}]&0x{m:02X}"),
                });
            }
        }
    }
    let score = if union == 0 {
        1.0
    } else {
        1.0 - f64::from(differ) / f64::from(union)
    };
    let names = if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    };
    (
        score,
        format!("{differ} differing bit(s) of {union} set{names}"),
    )
}

pub(crate) fn inventory_score(
    retail: &legaia_save::SaveFile,
    engine: &legaia_save::SaveFile,
) -> (f64, String) {
    let rs = &retail.ext.item_slots;
    let es = &engine.ext.item_slots;
    let n = rs.len().max(es.len());
    let mut total = 0usize;
    let mut equal = 0usize;
    for i in 0..n {
        let r = rs.get(i).copied().unwrap_or((0, 0));
        let e = es.get(i).copied().unwrap_or((0, 0));
        if r == (0, 0) && e == (0, 0) {
            continue;
        }
        total += 1;
        equal += usize::from(r == e);
    }
    total += 1;
    let gold_ok = retail.ext.money == engine.ext.money;
    equal += usize::from(gold_ok);
    (
        equal as f64 / total as f64,
        format!(
            "{equal}/{total} (slots + gold); gold retail={} engine={}",
            retail.ext.money, engine.ext.money
        ),
    )
}

/// A placement standing farther than this from its retail position is a
/// position miss, not a facing one: its heading is not scored.
pub(super) const FACING_SEAT_RADIUS: f64 = 96.0;

/// The facing channel: the player's heading and every placement the engine
/// holds near its retail position, each wrapped-angle delta on its own
/// falloff (an eighth turn and more scores zero). Mismatches beyond a
/// sixteenth turn are listed by flat index with both headings.
pub(crate) fn facing_score(retail: &RetailObs, engine: &EngineObs) -> Option<(f64, String)> {
    let part = |r: i16, e: i16| falloff(angle_delta(r, e), 32.0, 512.0);
    let mut parts = Vec::new();
    let mut miss = Vec::new();
    if let (Some(r), Some(e)) = (retail.player_facing, engine.player_facing) {
        parts.push(part(r, e));
        if angle_delta(r, e) > 256.0 {
            miss.push(format!("player r={:#05x} e={:#05x}", r & 0xFFF, e & 0xFFF));
        }
    }
    let mut skipped = 0;
    let mut ambient = 0;
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    let dump = std::env::var_os("LEGAIA_RC_FACING_DUMP").is_some();
    for a in &retail.actor_facings {
        let Some(&(e, ex, ez)) = engine.npc_facings.get(&a.flat) else {
            continue;
        };
        if dump {
            eprintln!(
                "facing {}: flat {}{} flags {:#010x} model {} r=({}, {}) {:#05x} e=({}, {}) {:#05x}",
                retail.scene,
                a.flat,
                if a.ambient { " (ambient)" } else { "" },
                a.flags,
                a.model,
                a.x,
                a.z,
                a.facing & 0xFFF,
                ex,
                ez,
                e & 0xFFF
            );
        }
        // A parked actor (the off-map seat) is not drawn: its heading is
        // not on screen.
        if (a.x, a.z) == (hide, hide) {
            continue;
        }
        let dx = f64::from(i32::from(ex) - i32::from(a.x));
        let dz = f64::from(i32::from(ez) - i32::from(a.z));
        if (dx * dx + dz * dz).sqrt() > FACING_SEAT_RADIUS {
            skipped += 1;
            continue;
        }
        // An ambient motion stream's heading is time and `rand()` history
        // since the entry - the actor's walk history, as the player's is
        // once the pad has turned it.
        if a.ambient {
            ambient += 1;
            continue;
        }
        parts.push(part(a.facing, e));
        if angle_delta(a.facing, e) > 256.0 {
            miss.push(format!(
                "flat {}{} at ({}, {}) model {} r={:#05x} e={:#05x}",
                a.flat,
                if a.flags & 0x0100_0000 != 0 {
                    " (party)"
                } else {
                    ""
                },
                a.x,
                a.z,
                a.model,
                a.facing & 0xFFF,
                e & 0xFFF
            ));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let n = parts.len();
    let score = parts.iter().sum::<f64>() / n as f64;
    Some((
        score,
        format!(
            "{n} actors scored, {skipped} off their retail seat, {ambient} ambient; misses: [{}]",
            miss.join("; ")
        ),
    ))
}

/// The `npcs` channel: where each placement stands. Every retail node the
/// engine holds a placement for (matched by flat index, as the facing
/// channel does) and that no ambient motion stream is walking scores:
///
/// * `0` when one side has it on the off-map park seat and the other draws
///   it - an NPC retail hid that the engine shows, or the reverse;
/// * when both park it, `1` on the exact retail coordinates and `0`
///   anywhere else: a park seat is a script literal (`4C 51 7F 7F`), and a
///   spawn section's `41` compass step after it lands the node one step
///   short on an axis (`town01` `P1[36]` at `(16320, 16256)`), so a
///   one-step miss is a real placement difference, not jitter;
/// * otherwise its `(X, Z)` distance, 1 within 8 units, 0 at 128.
///
/// The facing channel only scores a placement within 96 units of its seat,
/// so without this channel a placement in the wrong place, or one the
/// engine draws while retail has parked it, scored nowhere.
pub(crate) fn npc_seat_score(retail: &RetailObs, engine: &EngineObs) -> Option<(f64, String)> {
    let mut parts = Vec::new();
    let mut miss = Vec::new();
    let mut both_parked = 0usize;
    for a in &retail.actor_facings {
        if a.ambient {
            continue;
        }
        let Some(&(_, ex, ez)) = engine.npc_facings.get(&a.flat) else {
            continue;
        };
        let (rp, ep) = (npc_parked(a.x, a.z), npc_parked(ex, ez));
        both_parked += usize::from(rp && ep);
        let score = npc_pair_score((a.x, a.z), (ex, ez));
        if score < 1.0 {
            miss.push(format!(
                "flat {} model {} r=({}, {}){} e=({}, {}){}",
                a.flat,
                a.model,
                a.x,
                a.z,
                if rp { " parked" } else { "" },
                ex,
                ez,
                if ep { " parked" } else { "" }
            ));
        }
        parts.push(score);
    }
    if parts.is_empty() {
        return None;
    }
    let n = parts.len();
    Some((
        parts.iter().sum::<f64>() / n as f64,
        format!(
            "{n} placements scored ({both_parked} parked on both sides); misses: [{}]",
            miss.join("; ")
        ),
    ))
}

/// Is `(x, z)` on the off-map park seat? Retail parks at the seat itself or
/// one `41` compass step short of it on an axis (a spawn section's step after
/// its park seat). This window only classifies a node; a pair both sides
/// park still scores its exact coordinates ([`npc_pair_score`]).
pub(crate) fn npc_parked(x: i16, z: i16) -> bool {
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    x >= hide - 64 && z >= hide - 64
}

/// One placement's `npcs` score from its retail and engine `(X, Z)`
/// ([`npc_seat_score`]): `0` when exactly one side parks it, the exact
/// coordinates when both do, else the distance falloff (1 within 8, 0 at 128).
pub(crate) fn npc_pair_score(retail: (i16, i16), engine: (i16, i16)) -> f64 {
    match (
        npc_parked(retail.0, retail.1),
        npc_parked(engine.0, engine.1),
    ) {
        (true, true) => f64::from(u8::from(retail == engine)),
        (true, false) | (false, true) => 0.0,
        (false, false) => {
            let dx = f64::from(i32::from(engine.0) - i32::from(retail.0));
            let dz = f64::from(i32::from(engine.1) - i32::from(retail.1));
            falloff((dx * dx + dz * dz).sqrt(), 8.0, 128.0)
        }
    }
}

/// The camera channel: mean of pitch, yaw (wrapped), `H` and the three eye
/// words, each on its own falloff.
pub(crate) fn camera_score(r: &CameraObs, e: &CameraObs) -> (f64, String) {
    let parts = [
        falloff(angle_delta(r.pitch, e.pitch), 16.0, 256.0),
        falloff(angle_delta(r.yaw, e.yaw), 16.0, 256.0),
        falloff(f64::from(i32::from(r.h) - i32::from(e.h)), 4.0, 128.0),
        falloff(f64::from(r.eye[0] - e.eye[0]), 16.0, 1024.0),
        falloff(f64::from(r.eye[1] - e.eye[1]), 16.0, 1024.0),
        falloff(f64::from(r.eye[2] - e.eye[2]), 16.0, 1024.0),
        falloff(f64::from(r.focus[0] - e.focus[0]), 16.0, 1024.0),
        falloff(f64::from(r.focus[1] - e.focus[1]), 16.0, 1024.0),
    ];
    // Focus prints as world X / Z (the stored words are negated).
    let world = |f: [i32; 2]| [-f[0], -f[1]];
    (
        parts.iter().sum::<f64>() / parts.len() as f64,
        format!(
            "retail pitch/yaw/H={}/{}/{} eye={:?} focus={:?}; engine {}/{}/{} eye={:?} focus={:?}",
            r.pitch,
            r.yaw,
            r.h,
            r.eye,
            world(r.focus),
            e.pitch,
            e.yaw,
            e.h,
            e.eye,
            world(e.focus)
        ),
    )
}

/// Score one seeded state. `image` is the frame comparison when it ran.
pub fn compare(
    retail: &RetailObs,
    engine: &EngineObs,
    image: Option<ImageScore>,
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut footing_note = None;
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };

    if retail.class == StateClass::Field
        && let Some((score, detail)) = facing_score(retail, engine)
    {
        put("facing", score, detail);
    }
    if retail.class == StateClass::Field
        && let Some((score, detail)) = npc_seat_score(retail, engine)
    {
        put("npcs", score, detail);
    }
    let scene_ok = engine.scene.as_deref() == Some(retail.scene.as_str());
    put(
        "scene",
        f64::from(u8::from(scene_ok)),
        format!("retail={} engine={:?}", retail.scene, engine.scene),
    );
    let want_mode = match retail.class {
        StateClass::WorldMap => SceneMode::WorldMap,
        StateClass::Menu => SceneMode::Menu,
        _ => SceneMode::Field,
    };
    if let Some(Ok(menu)) = &retail.menu {
        put(
            "menu",
            f64::from(u8::from(engine.menu_subscreen == Some(menu.subscreen))),
            format!(
                "retail sub-screen=0x{:02X} ({:?}{}) engine={:?}",
                menu.subscreen,
                menu.row,
                match (menu.row, menu.equip_depth) {
                    (legaia_engine_menus::field_menu::FieldMenuRow::Equip, 0) => {
                        ", character picker"
                    }
                    (_, 1) => ", slot browse",
                    (_, 2) => ", candidate list",
                    _ => "",
                },
                engine.menu_subscreen.map(|s| format!("0x{s:02X}"))
            ),
        );
    }
    put(
        "mode",
        f64::from(u8::from(engine.mode == want_mode)),
        format!(
            "retail=0x{:02X} ({:?}) engine={:?}",
            retail.game_mode, retail.class, engine.mode
        ),
    );
    if let (Some(r), Some(e)) = (retail.player, engine.player) {
        let dx = f64::from(i32::from(e[0]) - i32::from(r[0]));
        let dz = f64::from(i32::from(e[2]) - i32::from(r[2]));
        let dist = (dx * dx + dz * dz).sqrt();
        put(
            "position",
            falloff(dist, 4.0, 256.0),
            format!(
                "retail=({}, {}) engine=({}, {}) dist={dist:.1}",
                r[0], r[2], e[0], e[2]
            ),
        );
    }
    if matches!(retail.class, StateClass::Field | StateClass::WorldMap)
        && let (Some(r), Some(floor)) = (retail.player, engine.floor_at_retail)
    {
        match retail.script_height {
            // A script holds the player's height: retail's `Y` is that, not
            // the floor, so there is no floor reading to score against.
            Some(h) => {
                footing_note = Some(format!(
                    "not scored: retail Y={} is script-held (+0x8E={h}); engine floor={floor}",
                    r[1]
                ));
            }
            None => {
                let d = f64::from(floor - i32::from(r[1]));
                put(
                    "footing",
                    falloff(d, 2.0, 128.0),
                    format!("retail footing={} engine floor={floor}", r[1]),
                );
            }
        }
    }
    let (s, d) = camera_score(&retail.camera, &engine.camera);
    put("camera", s, d);
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!(
            "retail={}{} engine={:?}{}",
            retail.bgm_id,
            if retail.bgm_sounding { "" } else { " (held)" },
            engine.bgm_id,
            if engine.bgm_held { " (held)" } else { "" },
        ),
    );
    put(
        "fog_gate",
        f64::from(u8::from(engine.fog_gate == retail.fog_gate)),
        format!("retail={} engine={}", retail.fog_gate, engine.fog_gate),
    );
    if let Some(rw) = retail.view_window {
        // A state is saved on an emulator frame boundary, which on a game
        // frame of two or three vsyncs can fall inside the render pass: the
        // decoration pass clips the four bytes against the region box and
        // puts them back in its epilogue, so such a capture holds the
        // clipped window. Either form is the engine's.
        let same = |w: [i8; 4]| rw.iter().zip(w).filter(|(a, b)| **a == *b).count();
        let stored = same(engine.view_window);
        let in_pass = engine.view_window_in_pass.map_or(0, same);
        put(
            "view_window",
            stored.max(in_pass) as f64 / 4.0,
            format!(
                "retail={rw:?} engine={:?} (in the decoration pass {:?}{})",
                engine.view_window,
                engine.view_window_in_pass,
                if in_pass > stored { ", matched" } else { "" },
            ),
        );
    }
    if let Some(rs) = &retail.save {
        let (s, d, diffs) = party_score(&rs.party, &engine.save.party);
        let d = if diffs.is_empty() {
            d
        } else {
            format!(
                "{d}; {}",
                diffs.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
            )
        };
        put("party", s, d);
        let (s, d) = flags_score(&rs.ext.story_flag_bits, &engine.save.ext.story_flag_bits);
        put("flags", s, d);
        let (s, d) = inventory_score(rs, &engine.save);
        put("inventory", s, d);
    }
    if let Some(img) = image {
        put(
            "image",
            img.within,
            format!("mae={:.1} within={:.3} ({})", img.mae, img.within, img.note),
        );
    }
    if let Some(p) = engine.script {
        det.insert(
            "script".into(),
            match p.met_at {
                Some(t) => format!(
                    "retail parked at pc {} wait {}; engine reached it at tick {t}{}",
                    p.pc,
                    p.wait,
                    if p.resumed {
                        " (record resumed from its start)"
                    } else {
                        ""
                    }
                ),
                None => format!(
                    "retail parked at pc {} wait {}; engine did not reach it in {} ticks (sampled at the settle window)",
                    p.pc,
                    p.wait,
                    legaia_engine_capture::script_gate::SCRIPT_GATE_DEADLINE
                ),
            },
        );
    }
    if let Some(note) = footing_note {
        det.insert("footing".to_string(), note);
    }
    (ch, det)
}

/// Mean of the measured channels.
pub fn state_score(channels: &BTreeMap<String, f64>) -> Option<f64> {
    (!channels.is_empty()).then(|| round3(channels.values().sum::<f64>() / channels.len() as f64))
}
