//! The ladder's inputs: where the extracted tree and save library are, the
//! spine file's milestones, and each milestone's retail anchor read down to
//! its SC save window.

use super::*;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

pub(super) fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    [
        repo_root().join("extracted"),
        PathBuf::from("extracted"),
        PathBuf::from("../extracted"),
        PathBuf::from("../../extracted"),
    ]
    .into_iter()
    .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

pub(super) fn library_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_SAVES_LIBRARY").map(PathBuf::from)
        && d.is_dir()
    {
        return Some(d);
    }
    [
        repo_root().join("saves/library"),
        PathBuf::from("saves/library"),
        PathBuf::from("../saves/library"),
        PathBuf::from("../../saves/library"),
    ]
    .into_iter()
    .find(|d| d.is_dir())
}

/// Everything a disc-gated run needs, or `None` (with the skip printed).
pub(super) struct Inputs {
    pub(super) extracted: PathBuf,
    pub(super) library: PathBuf,
}

pub(super) fn inputs() -> Option<Inputs> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return None;
    };
    let Some(library) = library_dir() else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return None;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: set once, before any save-state read, from the test thread.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    Some(Inputs { extracted, library })
}

// ---------------------------------------------------------------------------
// The spine file
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct SpineFile {
    pub(super) milestone: Vec<Milestone>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Milestone {
    pub(super) id: String,
    pub(super) scene: String,
    #[serde(default)]
    pub(super) seed: Option<String>,
    pub(super) reach: String,
    #[serde(default)]
    pub(super) reach_flags: Vec<u16>,
    /// Story waypoints between the previous milestone and this one: scenes
    /// the retail run passed through to play a beat the route's shortest
    /// path skips. Visited in order, each with its beats pass played.
    #[serde(default)]
    pub(super) via: Vec<String>,
    /// The waypoints the continuous run (part C) plays instead of `via`:
    /// story a seeded segment starts past, or plays in an order its seat
    /// does not reach, but a run of its own has to play on the way.
    #[serde(default)]
    pub(super) continuous_via: Option<Vec<String>>,
    #[serde(default)]
    pub(super) anchor: Option<AnchorRef>,
    /// `false` for an anchor from outside the main playthrough (a debug
    /// credits run): it anchors the scene but not the order.
    #[serde(default = "yes")]
    pub(super) order_check: bool,
    /// `false` for an anchor that names the scene but is not a save of the
    /// playthrough at that point (a door-tile poke from an earlier state):
    /// the segment after it seeds from the nearest earlier milestone whose
    /// anchor does seed, so it starts from the story state the run had.
    #[serde(default = "yes")]
    pub(super) seeds_next: bool,
    /// The milestone's scene commits a fight as the party arrives (its entry
    /// record runs `3E FF` before the pad comes back): route knowledge the
    /// continuous hand walks in ready for, as a player who has read ahead
    /// does (`continuous::FIGHT_ON_ARRIVAL`).
    #[serde(default)]
    pub(super) arrival_fight: bool,
}

pub(super) fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum AnchorRef {
    Card { card: String, save: String },
    State { label: String, fingerprint: String },
}

impl AnchorRef {
    pub(super) fn describe(&self) -> String {
        match self {
            Self::Card { card, save } => format!("card {card} {save}"),
            Self::State { label, .. } => format!("state {label}"),
        }
    }
}

pub(super) fn load_spine() -> Vec<Milestone> {
    let path = repo_root().join("scripts/replays/full_game_spine.toml");
    let text = std::fs::read_to_string(&path).expect("read full_game_spine.toml");
    let file: SpineFile = toml::from_str(&text).expect("parse full_game_spine.toml");
    file.milestone
}

// ---------------------------------------------------------------------------
// Anchors: the SC save window of a retail save
// ---------------------------------------------------------------------------

/// RAM base of the SC save window (`block_offset = ram - 0x80084140`).
pub(super) const SC_RAM_BASE: u32 = 0x8008_4140;
pub(super) const SC_LEN: usize = 0x2000;
/// SC offset of the play-time counter (`0x80084570`).
pub(super) const SC_PLAY_TIME: usize = 0x430;
/// SC offset of the pause menu's Field Move word (`0x800846CC`, Walk `0` /
/// Run non-zero). The run selector XORs it with the held run button
/// (`docs/subsystems/field-locomotion.md`), and it is inside the window a
/// card load restores, so the anchor's player walks or runs by it.
pub(super) const SC_FIELD_MOVE: usize = 0x58C;
/// SC offset of the system-flag bank (`0x80085758`) and its full extent up
/// to the item window at `0x80085958`.
pub(super) const SC_SYSTEM_FLAGS: usize = 0x1618;
pub(super) const SC_SYSTEM_FLAGS_LEN: usize = 0x200;
/// How much of that bank `legaia_save`'s story window reaches - the whole
/// bank; part A fails on an anchor flag past it.
pub(super) const SAVE_WINDOW_SYSTEM_BYTES: usize = legaia_save::card::RETAIL_STORY_FLAGS_OFFSET
    + legaia_save::card::RETAIL_STORY_FLAGS_SIZE
    - SC_SYSTEM_FLAGS;

pub(super) struct Anchor {
    pub(super) sc: Vec<u8>,
    pub(super) scene: String,
    /// Where to seat the player when the save's own position snapshot is not
    /// it: the live position of a field-run state. `None` lets the resume
    /// path seat the party from the snapshot, as a card load does.
    pub(super) seat: Option<(i16, i16)>,
    pub(super) play_time: u32,
    pub(super) flags: BTreeSet<u16>,
}

pub(super) fn flags_of(sc: &[u8]) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    for (i, b) in sc[SC_SYSTEM_FLAGS..SC_SYSTEM_FLAGS + SC_SYSTEM_FLAGS_LEN]
        .iter()
        .enumerate()
    {
        for k in 0..8 {
            if b & (0x80 >> k) != 0 {
                out.insert((i * 8 + k) as u16);
            }
        }
    }
    out
}

pub(super) fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

pub(super) fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub(super) fn load_anchor(lib: &Path, a: &AnchorRef) -> Result<Anchor, String> {
    match a {
        AnchorRef::Card { card, save } => {
            let path = lib.join("cards").join(card);
            let mounted =
                legaia_save::emu::MountedCard::open(&path).map_err(|e| format!("{e:#}"))?;
            for block in 1..=15u8 {
                let Some(frame) = mounted.dir_frame(block) else {
                    continue;
                };
                let name = ascii(&frame[0x0A..0x0A + 20]);
                if !name.ends_with(save.as_str()) {
                    continue;
                }
                let Some(sc) = mounted.sc_block(block) else {
                    continue;
                };
                let sc = sc.to_vec();
                return Ok(Anchor {
                    scene: ascii(&sc[0x408..0x410]),
                    seat: None,
                    play_time: i32_at(&sc, SC_PLAY_TIME) as u32,
                    flags: flags_of(&sc),
                    sc,
                });
            }
            Err(format!("{card}: no save named {save}"))
        }
        AnchorRef::State { label, fingerprint } => {
            let med = lib.join("mednafen").join(format!("{fingerprint}.mcr"));
            let pcsx = lib.join("pcsx-redux").join(format!("{fingerprint}.sstate"));
            let ram: Vec<u8> = if med.exists() {
                let st =
                    legaia_mednafen::SaveState::from_path(&med).map_err(|e| format!("{e:#}"))?;
                st.main_ram().map_err(|e| format!("{e:#}"))?.to_vec()
            } else if pcsx.exists() {
                let st =
                    legaia_pcsx_redux::SaveState::from_path(&pcsx).map_err(|e| format!("{e:#}"))?;
                st.main_ram().to_vec()
            } else {
                return Err(format!(
                    "{label}: fingerprint {fingerprint} not in the library"
                ));
            };
            let base = (SC_RAM_BASE & 0x1F_FFFF) as usize;
            let sc = ram[base..base + SC_LEN].to_vec();
            let scene = legaia_ram_image::game_anchors::scene_name(&ram);
            let mode = legaia_ram_image::game_anchors::game_mode(&ram);
            let seat = if mode == 0x03 {
                legaia_ram_image::game_anchors::player_pos(&ram)
            } else {
                None
            };
            Ok(Anchor {
                scene,
                seat,
                play_time: i32_at(&sc, SC_PLAY_TIME) as u32,
                flags: flags_of(&sc),
                sc,
            })
        }
    }
}
