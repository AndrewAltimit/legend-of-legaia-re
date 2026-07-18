//! Disc-gated: the site/arts.html WASM surface (`LegaiaArts`) must assemble
//! every character's battle mesh and resolve every art to a decodable
//! keyframe clip off a real disc image.
//!
//! Two layers of coverage:
//!
//! 1. **Bank-level** - for each of the four player files (863..866), every
//!    art-bank record's `"ME"` stream decodes to a clip with a plausible
//!    frame count and the pose buffers match the assembled rig width.
//! 2. **Arts-table-level** - every art in the SCUS arts-name table
//!    (`legaia_art::arts_table`, the same data the site's `arts.json` mirrors)
//!    resolves to a *named* bank record by name or by combo, and that
//!    record's clip decoded - or the art is explicitly listed in
//!    [`KNOWN_UNRESOLVED`] so coverage stays visible.
//!
//! No Sony bytes are asserted - only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_art::arts_voice::{self, ArtsVoiceTable};
use legaia_art::{Character, arts_table};
use legaia_web_viewer::arts_view::LegaiaArts;

/// Arts-name-table entries with no matching art-bank record on the retail
/// disc (asserted exact - a new resolution must be removed from here).
const KNOWN_UNRESOLVED: &[(&str, &str)] = &[];

fn loaded() -> Option<(LegaiaArts, Vec<u8>)> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut arts = LegaiaArts::new();
    arts.load_disc(bytes.clone()).ok()?;
    Some((arts, bytes))
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

#[test]
fn every_character_assembles_with_idle_and_decodable_art_bank() {
    let Some((mut arts, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for cslot in 0..4u32 {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true, "char {cslot} assembles: {st}");
        let name = st["character"].as_str().unwrap();
        let parts = st["part_count"].as_u64().unwrap() as usize;
        assert!(parts > 0, "{name}: rig width");
        // Mesh buffers are parallel + framed.
        let n = arts.mesh_positions().len() / 3;
        assert!(n > 0, "{name}: mesh has vertices");
        assert_eq!(arts.mesh_uvs().len(), n * 2, "{name}: uvs parallel");
        assert_eq!(arts.mesh_cba_tsb().len(), n * 2, "{name}: cba parallel");
        assert_eq!(arts.mesh_object_ids().len(), n, "{name}: obj ids parallel");
        let idx = arts.mesh_indices();
        assert!(
            !idx.is_empty() && idx.len().is_multiple_of(3),
            "{name}: tris"
        );
        assert!(idx.iter().all(|&i| (i as usize) < n), "{name}: idx bound");
        let b = arts.mesh_bounds();
        assert_eq!(b.len(), 4);
        assert!(b[3] > 0.0, "{name}: bounds radius");
        assert_eq!(arts.vram_bytes().len(), 1024 * 512 * 2, "{name}: VRAM");
        // Idle loop: present, plausible, pose buffer matches the rig.
        let idle_frames = st["idle"]["frames"].as_u64().unwrap_or(0) as usize;
        assert!(idle_frames > 0, "{name}: idle stream: {st}");
        assert_eq!(
            arts.idle_pose_frames().len(),
            idle_frames * parts * 6,
            "{name}: idle pose layout"
        );
        // Every bank record decodes (the retail files carry no dead records).
        let bank = st["arts"].as_array().unwrap();
        assert!(!bank.is_empty(), "{name}: art bank non-empty");
        for a in bank {
            let i = a["index"].as_u64().unwrap() as u32;
            let rec_name = a["name"].as_str().unwrap();
            assert_eq!(
                a["ok"], true,
                "{name} art record {i} ({rec_name:?}): {}",
                a["why"]
            );
            let frames = a["frames"].as_u64().unwrap() as usize;
            assert!(frames > 0, "{name} art record {i} ({rec_name:?}): frames");
            assert_eq!(
                arts.art_pose_frames(i).len(),
                frames * parts * 6,
                "{name} art record {i}: pose layout"
            );
        }
        eprintln!(
            "[arts] {name}: {} bank records, {} named, rig {parts}, idle {idle_frames}f",
            bank.len(),
            bank.iter()
                .filter(|a| !a["name"].as_str().unwrap().is_empty())
                .count(),
        );
    }
}

#[test]
fn every_arts_table_art_resolves_to_a_decoded_clip() {
    let Some((mut arts, disc_bytes)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let scus =
        legaia_web_viewer::disc::extract_scus(&disc_bytes).expect("SCUS_942.54 in disc image");
    let table = arts_table::parse_from_scus(&scus).expect("arts-name table");
    let mut unresolved: Vec<(String, String)> = Vec::new();
    for (cslot, character) in [
        (0u32, Character::Vahn),
        (1, Character::Noa),
        (2, Character::Gala),
    ] {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true, "{character:?} assembles");
        let bank = st["arts"].as_array().unwrap();
        for entry in table.iter().filter(|e| e.character == character) {
            let want_name = norm(&entry.name);
            let want_combo: Vec<u64> = entry.commands.iter().map(|&c| c as u64).collect();
            let hit = bank.iter().find(|a| {
                let rec_name = a["name"].as_str().unwrap();
                if !rec_name.is_empty() && norm(rec_name) == want_name {
                    return true;
                }
                let combo: Vec<u64> = a["combo"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect();
                // Combo matches need >= 2 directions: the base records and
                // the Super-Art tail records carry a 1-byte placeholder
                // combo. Gala's Miracle record is un-named on disc, so
                // un-named records still count here.
                want_combo.len() >= 2 && combo == want_combo
            });
            match hit {
                Some(a) => {
                    assert_eq!(
                        a["ok"], true,
                        "{character:?} '{}' matched record {} but its clip failed: {}",
                        entry.name, a["index"], a["why"]
                    );
                    assert!(
                        a["frames"].as_u64().unwrap() > 0,
                        "{character:?} '{}': zero-frame clip",
                        entry.name
                    );
                }
                None => unresolved.push((format!("{character:?}"), entry.name.clone())),
            }
        }
    }
    let unresolved_pairs: Vec<(&str, &str)> = unresolved
        .iter()
        .map(|(c, n)| (c.as_str(), n.as_str()))
        .collect();
    assert_eq!(
        unresolved_pairs, KNOWN_UNRESOLVED,
        "arts-table entries without a decoded bank clip changed; update KNOWN_UNRESOLVED"
    );
}

/// The resolution ladder `site/js/arts-viewer.js` uses to map one curated
/// art card onto a bank record:
///
/// 0. **action constant** - the curated `action_constant` IS the staged
///    anim id `FUN_8004AD80` materializes (bank record `ac - 0x10`); this
///    is exact on retail (it lands Vahn's *Hyper Elbow* on its dev-named
///    "Miyawaki Chop" record despite the documented walkthrough combo
///    divergence, and Gala's un-named Miracle record);
/// 1. **exact** - a named record whose name AND combo both match;
/// 2. **combo** - any record with the exact combo (needs >= 2 directions;
///    named records preferred) - how the hypers land on their dev-named
///    records ("Tornado Flame" -> "Beatfire");
/// 3. **name** - a named record with a matching name (records with a
///    placeholder <= 1-byte combo preferred: the Super-Art tail records).
///
/// Returns the record index in `bank`.
fn resolve_art(
    bank: &[serde_json::Value],
    action_constant: Option<u64>,
    name: &str,
    directions: &[u64],
) -> Option<usize> {
    // 0. action constant (staged anim id space starts at 0x10)
    if let Some(ac) = action_constant
        && ac >= 0x10
        && let Some(i) = bank.iter().position(|a| a["anim_id"].as_u64() == Some(ac))
    {
        return Some(i);
    }
    let want = norm(name);
    let combo_of = |a: &serde_json::Value| -> Vec<u64> {
        a["combo"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect()
    };
    fn name_of(a: &serde_json::Value) -> &str {
        a["name"].as_str().unwrap()
    }
    // 1. exact
    if let Some(i) = bank.iter().position(|a| {
        let n = name_of(a);
        !n.is_empty() && norm(n) == want && combo_of(a) == directions
    }) {
        return Some(i);
    }
    // 2. combo (named preferred)
    if directions.len() >= 2 {
        let hits: Vec<usize> = bank
            .iter()
            .enumerate()
            .filter(|(_, a)| combo_of(a) == directions)
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = hits
            .iter()
            .find(|&&i| !name_of(&bank[i]).is_empty())
            .or(hits.first())
        {
            return Some(i);
        }
    }
    // 3. name (placeholder-combo records preferred)
    let hits: Vec<usize> = bank
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            let n = name_of(a);
            !n.is_empty() && norm(n) == want
        })
        .map(|(i, _)| i)
        .collect();
    hits.iter()
        .find(|&&i| combo_of(&bank[i]).len() <= 1)
        .or(hits.first())
        .copied()
}

/// The chain a resolved art plays: the record plus every immediately
/// following record that shares its non-empty name OR its full (>= 2
/// direction) combo. The multi-segment arts ship their strikes as
/// consecutive bank records: "Hurricane Kick" = 0x1C..0x1E (one combo,
/// three clips), "Rolling Combo" / "Jurassic Blow" = same-named pairs.
fn chain_of(bank: &[serde_json::Value], first: usize) -> Vec<usize> {
    let mut chain = vec![first];
    let name = bank[first]["name"].as_str().unwrap();
    let combo = bank[first]["combo"].as_array().unwrap();
    let mut i = first + 1;
    while i < bank.len() {
        let n = bank[i]["name"].as_str().unwrap();
        let c = bank[i]["combo"].as_array().unwrap();
        let same_name = !name.is_empty() && n == name;
        let same_combo = combo.len() >= 2 && c == combo;
        if !(same_name || same_combo) {
            break;
        }
        chain.push(i);
        i += 1;
    }
    chain
}

/// Curated arts (`data/gamedata/arts.toml` - the rows the site's `arts.json`
/// mirrors 1:1) with no resolvable bank record. Asserted exact.
const KNOWN_UNRESOLVED_CURATED: &[(&str, &str)] = &[];

#[test]
fn every_curated_art_card_resolves_to_a_decoded_clip_chain() {
    let Some((mut arts, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let toml_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gamedata/arts.toml");
    let curated: toml::Value = std::fs::read_to_string(toml_path)
        .expect("arts.toml")
        .parse()
        .expect("arts.toml parses");
    let rows = curated["arts"].as_array().expect("arts array");
    let mut unresolved: Vec<(String, String)> = Vec::new();
    for (cslot, character) in [(0u32, "Vahn"), (1, "Noa"), (2, "Gala")] {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true, "{character} assembles");
        let bank = st["arts"].as_array().unwrap();
        let parts = st["part_count"].as_u64().unwrap() as usize;
        for row in rows
            .iter()
            .filter(|r| r["character"].as_str() == Some(character))
        {
            let name = row["name"].as_str().unwrap();
            let directions: Vec<u64> = row["directions"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_integer().unwrap() as u64).collect())
                .unwrap_or_default();
            let ac = row["action_constant"].as_integer().map(|v| v as u64);
            let Some(first) = resolve_art(bank, ac, name, &directions) else {
                unresolved.push((character.to_string(), name.to_string()));
                continue;
            };
            for i in chain_of(bank, first) {
                let a = &bank[i];
                assert_eq!(
                    a["ok"], true,
                    "{character} '{name}' chain record {i}: {}",
                    a["why"]
                );
                let frames = a["frames"].as_u64().unwrap() as usize;
                assert!(frames > 0, "{character} '{name}' chain record {i}: frames");
                assert_eq!(
                    arts.art_pose_frames(i as u32).len(),
                    frames * parts * 6,
                    "{character} '{name}' chain record {i}: pose layout"
                );
            }
        }
    }
    let unresolved_pairs: Vec<(&str, &str)> = unresolved
        .iter()
        .map(|(c, n)| (c.as_str(), n.as_str()))
        .collect();
    assert_eq!(
        unresolved_pairs, KNOWN_UNRESOLVED_CURATED,
        "curated arts without a decoded bank clip changed; update KNOWN_UNRESOLVED_CURATED"
    );
}

#[test]
#[ignore]
fn probe_dump_bank_names() {
    let Some((mut arts, disc_bytes)) = loaded() else {
        return;
    };
    let scus = legaia_web_viewer::disc::extract_scus(&disc_bytes).unwrap();
    let table = arts_table::parse_from_scus(&scus).unwrap();
    for (cslot, character) in [
        (0u32, Character::Vahn),
        (1, Character::Noa),
        (2, Character::Gala),
    ] {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        eprintln!("== {character:?} bank:");
        for a in st["arts"].as_array().unwrap() {
            eprintln!(
                "  [{:2}] id {:#04x} name {:?} combo {:?} rate {} base {} frames {}",
                a["index"],
                a["anim_id"].as_u64().unwrap(),
                a["name"],
                a["combo"],
                a["rate"],
                a["base"],
                a["frames"]
            );
        }
        eprintln!("== {character:?} arts table:");
        for e in table.iter().filter(|e| e.character == character) {
            let combo: Vec<u8> = e.commands.iter().map(|&c| c as u8).collect();
            eprintln!(
                "  idx {:2} {:?} ap {} combo {:?} miracle {}",
                e.index, e.name, e.ap, combo, e.is_miracle
            );
        }
    }
}

/// The Tactical-Arts VOICE bank: each hero's own arts-shout file (Vahn=`XA2`,
/// Noa=`XA4`, Gala=`XA6`, the RE'd `FUN_8004C140` cue; `XA30.XA` is the
/// normal-move grunt, `XA3`/`XA5` the stereo fanfares). Assert the WASM surface
/// demuxes a bank of real, non-silent mono clips per hero keyed by their own XA
/// channel, resolves each voiced art to a decodable channel, and gives Terra none.
#[test]
fn arts_voice_bank_demuxes_per_character() {
    let Some((mut arts, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Expected voice file per character; Terra = none.
    let expect: [Option<&str>; 4] = [Some("XA2.XA"), Some("XA4.XA"), Some("XA6.XA"), None];
    for (cslot, want) in expect.iter().enumerate() {
        let st: serde_json::Value =
            serde_json::from_str(&arts.set_character(cslot as u32)).unwrap();
        assert_eq!(st["ok"], true, "char {cslot} assembles");
        let name = st["character"].as_str().unwrap().to_string();
        match want {
            Some(file) => {
                let v = &st["voice"];
                assert!(v.is_object(), "{name}: voice metadata present");
                assert_eq!(v["file"], *file, "{name}: voice file");
                let count = v["count"].as_u64().unwrap() as usize;
                assert!(count > 0, "{name}: voice bank non-empty");
                let channels = v["channels"].as_array().unwrap();
                assert_eq!(channels.len(), count, "{name}: channel metadata count");
                // Every channel: a real shout (not silence), mono 37.8 kHz,
                // trimmed of its silent tail, addressable by its own ch_no.
                for ch in channels {
                    let ch_no = ch["channel"].as_u64().unwrap();
                    assert_eq!(ch["rate"].as_u64(), Some(37_800), "{name} ch{ch_no}: rate");
                    assert_eq!(ch["stereo"], false, "{name} ch{ch_no}: mono");
                    let pcm = arts.voice_channel_pcm_i16(ch_no as u32);
                    assert_eq!(
                        pcm.len() as u64,
                        ch["samples"].as_u64().unwrap(),
                        "{name} ch{ch_no}: sample count matches metadata"
                    );
                    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                    assert!(peak > 3200, "{name} ch{ch_no}: voice peak {peak}");
                    let secs = pcm.len() as f64 / 37_800.0;
                    assert!((0.2..2.8).contains(&secs), "{name} ch{ch_no}: {secs}s");
                }
                // Per-art mapping: a voiced art returns exactly its channel's clip.
                let bank = st["arts"].as_array().unwrap();
                let mut checked = 0;
                for a in bank {
                    let Some(vc) = a["voice_channel"].as_u64() else {
                        continue;
                    };
                    let i = a["index"].as_u64().unwrap() as u32;
                    assert_eq!(
                        arts.art_voice_pcm_i16(i),
                        arts.voice_channel_pcm_i16(vc as u32),
                        "{name}: art {i} -> channel {vc}"
                    );
                    checked += 1;
                }
                assert!(checked > 0, "{name}: at least one voiced art");
            }
            None => {
                assert!(st["voice"].is_null(), "{name}: no voice bank");
                assert!(arts.art_voice_pcm_i16(0).is_empty(), "{name}: empty PCM");
                assert!(arts.voice_channel_pcm_i16(0).is_empty(), "{name}: empty ch");
            }
        }
    }
}

/// The arts-voice cue (`FUN_8004C140`) parsed from the disc's `SCUS_942.54`:
/// the per-character clip file must be XA2 / XA4 / XA6, and the per-art channel
/// each character's bank record plays must be a real member of that art's
/// candidate pool - not a modulo guess. Anchored to the live PCSX-Redux capture
/// (Vahn's Somersault, action constant 0x27, fired channels 0 and 6).
#[test]
fn arts_voice_is_the_retail_cue_not_a_guess() {
    let Some((mut arts, disc_bytes)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let scus =
        legaia_web_viewer::disc::extract_scus(&disc_bytes).expect("SCUS_942.54 in disc image");
    let table = ArtsVoiceTable::parse_from_scus(&scus).expect("arts-voice table parses");

    // Clip file per character = XA<(char*2+1)+1>.XA.
    assert_eq!(arts_voice::clip_file(0), Some("XA2.XA"), "Vahn = XA2");
    assert_eq!(arts_voice::clip_file(1), Some("XA4.XA"), "Noa = XA4");
    assert_eq!(arts_voice::clip_file(2), Some("XA6.XA"), "Gala = XA6");
    assert_eq!(arts_voice::clip_slot(0), Some(1));
    assert_eq!(arts_voice::clip_slot(2), Some(5));

    // Capture anchor: Vahn's Somersault (action constant 0x27) fired channels
    // 0 and 6 through FUN_8004C140 - both must be in its candidate pool.
    let vahn_27 = table.channels(0, 0x27).expect("Vahn 0x27 has a voice pool");
    assert!(
        vahn_27.contains(&0) && vahn_27.contains(&6),
        "Vahn 0x27 pool {vahn_27:?} must contain the captured channels 0 and 6"
    );
    // The dur formula is verified against the capture (chan 0 -> 0x2D).
    assert_eq!(table.duration(0, 0), Some(0x2D), "Vahn ch0 dur");
    assert_eq!(table.duration(0, 6), Some(0x3D), "Vahn ch6 dur");

    // Through the site surface: every art with a voice_channel plays a real
    // pool member of its own action constant, and that channel is a non-silent
    // decoded clip of the right file.
    for cslot in 0..3u32 {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true);
        let voice = &st["voice"];
        assert!(
            !voice.is_null(),
            "char {cslot} has a voice bank off a full disc"
        );
        assert_eq!(
            voice["file"].as_str(),
            arts_voice::clip_file(cslot as usize),
            "char {cslot} voice file"
        );
        let mut voiced = 0usize;
        for a in st["arts"].as_array().unwrap() {
            let idx = a["index"].as_u64().unwrap() as u32;
            let anim_id = a["anim_id"].as_u64().unwrap() as u8;
            let Some(ch) = a["voice_channel"].as_u64() else {
                continue;
            };
            let ch = ch as u8;
            let pool = table.channels(cslot as usize, anim_id).unwrap_or_else(|| {
                panic!("char {cslot} art 0x{anim_id:02X}: voice_channel set but no pool")
            });
            assert!(
                pool.contains(&ch),
                "char {cslot} art 0x{anim_id:02X}: channel {ch} not in real pool {pool:?}"
            );
            let pcm = arts.art_voice_pcm_i16(idx);
            assert!(
                !pcm.is_empty() && pcm.iter().any(|&s| s.unsigned_abs() > 256),
                "char {cslot} art 0x{anim_id:02X} ch{ch}: voice clip decoded non-silent"
            );
            voiced += 1;
        }
        assert!(voiced > 0, "char {cslot} has at least one voiced art");
    }
}

#[test]
fn export_character_glb_carries_the_whole_animation_bank() {
    let Some((mut arts, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for cslot in 0..4u32 {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true, "char {cslot} assembles");
        let name = st["character"].as_str().unwrap().to_string();
        let parts = st["part_count"].as_u64().unwrap() as usize;
        let bank = st["arts"].as_array().unwrap();
        let decoded = bank.iter().filter(|a| a["ok"] == true).count();
        let has_idle = !st["idle"].is_null();

        let glb = arts.export_character_glb();
        assert!(
            glb.len() > 100_000,
            "{name}: .glb non-trivial ({} bytes)",
            glb.len()
        );
        // GLB container: magic, version, declared length.
        assert_eq!(&glb[0..4], b"glTF", "{name}: magic");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len(), "{name}: declared length");
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let root: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(root["asset"]["version"], "2.0");

        // One node per assembled TMD object + the reorienting root.
        let nodes = root["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), parts + 1, "{name}: node count");
        assert_eq!(nodes[parts]["name"].as_str(), Some(name.as_str()));

        // Every decoded bank record + the idle became a named animation.
        let anims = root["animations"].as_array().unwrap();
        assert_eq!(
            anims.len(),
            decoded + usize::from(has_idle),
            "{name}: animation count"
        );
        assert!(anims.len() > 1, "{name}: more than one animation");
        if has_idle {
            assert_eq!(anims[0]["name"].as_str(), Some("battle idle"));
        }
        // Names are unique; durations are plausible (retail clips run
        // fractions of a second up to a few seconds at 7.5 * rate fps).
        let mut names: Vec<&str> = anims.iter().map(|a| a["name"].as_str().unwrap()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "{name}: animation names unique");
        let accessors = root["accessors"].as_array().unwrap();
        for a in anims {
            let chans = a["channels"].as_array().unwrap();
            assert!(!chans.is_empty(), "{name}: animation has channels");
            let in_acc = a["samplers"][0]["input"].as_u64().unwrap() as usize;
            let dur = accessors[in_acc]["max"][0].as_f64().unwrap();
            assert!(
                (0.0..30.0).contains(&dur),
                "{name} '{}': implausible duration {dur}",
                a["name"]
            );
        }
        eprintln!(
            "[arts-glb] {name}: {} bytes, {} animations, {} nodes",
            glb.len(),
            anims.len(),
            nodes.len()
        );
    }
}
