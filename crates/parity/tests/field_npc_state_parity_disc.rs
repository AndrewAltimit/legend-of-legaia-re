//! Disc + save-library gated BREADTH oracle: every catalogued **field-mode**
//! retail capture (mednafen `.mcr` + PCSX-Redux `.sstate` library backups,
//! enumerated from `scripts/scenarios.toml`) versus a cold engine entry into
//! the same scene with the capture's story-flag bank seeded byte-for-byte.
//!
//! Goes beyond the town01 entry-position oracle
//! (`field_npc_entry_positions_disc.rs`): it sweeps EVERY library state whose
//! RAM says `game_mode == 0x03` on a non-worldmap scene - mid-beat states,
//! post-battle states, interiors, later chapters - and compares, per retail
//! partition-1 actor node (`_DAT_8007C354` list, `+0x50 = N0 + placement`):
//!
//! - **visibility**: retail parked at the `(0x7F,0x7F)` off-map sentinel
//!   <-> engine parked (the spawn-prologue despawn arrangement);
//! - **position**: both placed -> within the patrol-locality bound
//!   (`NPC_ROUTE_LOCALITY`; retail walkers roam their authored local route
//!   between capture and comparison, the seat is what the pre-run pins);
//! - **heading**: diagnostic-only (`+0x26`, retail `0` = Z-, engine `0` = Z+):
//!   a captured walker faces whichever way it last moved, so heading drift is
//!   expected dynamics, not a seat bug - reported, never gated;
//! - **collision class**: both placed -> the script's pass-through bit
//!   (`+0x10 & 1`, which `FUN_801CF754` reads with the cull bit `2`) must
//!   match the engine channel's word;
//! - **story flags**: after the engine's scene entry (which runs the real MAN
//!   entry script + spawn prologues against the seeded bank), the engine's
//!   `DAT_80085758` mirror must still equal the capture's bank bit-for-bit.
//!   Retail captured mid-scene has already run the same entry script, so any
//!   engine-side flip is a divergence.
//!
//! Known, classified divergences are pinned in [`KNOWN_DIVERGENCES`] so the
//! gate stays green while the open items remain visible in the log.
//!
//! Skips (passes) when `LEGAIA_DISC_BIN`, `extracted/`, the scenario
//! manifest, or the library backups are missing - CI runs without disc data.

use std::path::{Path, PathBuf};

use legaia_engine_scene::scene::{DefaultMapIdResolver, is_world_map_scene};
use legaia_engine_scene_host::scene_host::SceneHost;
use psx_ram_image::ScenarioManifest;
use psx_ram_image::scenarios::library_backup_for;

const RAM_MASK: u32 = 0x001F_FFFF;
/// Field-actor list sentinel head (the `FUN_8003BC08` tick class).
const ACTOR_LIST_HEAD_VA: u32 = 0x8007_C354;
/// Active-scene CDNAME label (8 bytes).
const SCENE_NAME_VA: u32 = 0x8007_050C;
/// Game-mode register (`0x03` = field free-roam).
const GAME_MODE_VA: u32 = 0x8007_B83C;
/// System story-flag bank (`DAT_80085758`, MSB-first bit order).
const FLAG_BANK_VA: u32 = 0x8008_5758;
const FLAG_BANK_LEN: usize = 0x1100;

/// Divergences surveyed, classified, and deliberately tolerated. Each entry
/// keeps the gate green while documenting exactly what still diverges and
/// why. Key = `(scenario label, divergence key)`; the key is stable
/// (`vis:<pi>` / `pos:<pi>` / `flag:<idx>` / `enter`), and a key ending in
/// `*` prefix-matches (for whole-cluster capture-context classifications).
struct KnownDivergence {
    label: &'static str,
    key: &'static str,
    /// `a-reported` = engine bug with retail evidence, owned by the
    /// field-VM/stepping lane; `b` = expected dynamics / capture context a
    /// cold scene entry cannot know; `c` = open RE thread - see the note.
    class: &'static str,
    note: &'static str,
}

/// The comparison model behind class `b`: a catalogued capture is a MID-VISIT
/// snapshot. The engine side reproduces the retail FRESH-ENTRY arrangement
/// (the `FUN_8003A1E4` spawn-prologue pre-run against the capture's flag
/// bank); anything a mid-visit beat re-arranged after retail's own entry -
/// walk-on choreography seats/parks, opening/ending timeline hides, post-
/// battle staging - legitimately differs. Where possible each note cites a
/// sibling capture proving the fresh-entry arrangement (e.g. the rikuroa
/// `pre_caruban` state pins retail's own entry seats byte-equal to the
/// engine's).
/// The conc3 inn-stay arrival record, shared by its four `vis` entries.
const CONC3_INN_ARRIVAL: &str = "post-arrival-record capture: the conc innkeeper sets 0x53C and \
     changes scene to CONC3; conc3's P1[0] spawns P2[9] (44 59 at +0x90) only while 0x53C is \
     set and 0x5F7 clear (75 3C / 75 F7 at +0x85 / +0x8C). P2[9] is the wake-up cutscene: \
     after its fade it parks P1[8] / P1[7] / P1[23] / P1[9] (A3 2A/29/39/2B 7F 7F at \
     +0xAF..+0xBB), sets 0x3F9, and sets 0x5F7 before idling. The capture holds 0x53C, 0x3F9 \
     and 0x5F7 all set, so a cold entry against its bank skips the spawn and seats the four - \
     as retail's own entry with that bank does. The engine's run of the arrival record parks \
     them (engine-scene-host conc3_inn_arrival_disc.rs)";

/// garmel `P2[12]` (Zeto's appearance) stages the party on the player. Every
/// capture below is inside that record: its bank holds `0x198` (the record's
/// `51 98` at +0x12) and `0x363` (`53 63` at +0xBB) and not `0x199` (its tail
/// `51 99` at +0x9C0). The record parks `0x1B` / `0x1C` (P1[3] / P1[4]) with
/// `CC 1B 51 7F 7F 80 ..` / `CC 1C 51 7F 7F 80 ..` (+0xCE / +0xD5), then seats
/// them on the player with `CC 1B 37` / `CC 1C 37` (+0xDC / +0xDF) and walks
/// them out with `A2` ExecMove. Retail's nodes keep the same player-relative
/// offsets in all three states ((+64, +192) and (-64, +64)); a cold entry
/// holds their `(127,127)` header parks, as retail's own entry does
/// (garmel_field_card_boot and chapter2_garmel_pre_songi park both).
const GARMEL_P2_12_PARTY: &str = "mid-beat capture inside garmel P2[12]: 0x198 / 0x363 set, \
     0x199 clear; the record seats 0x1B / 0x1C on the player (CC 1B 37 / CC 1C 37 at \
     +0xDC / +0xDF) and walks them out, so retail holds them at fixed player-relative \
     offsets while a cold entry holds the (127,127) header parks the fresh-entry \
     garmel captures show. The 4C 37 player-coord copy is 0x801E0FB0; the staging is \
     pinned by engine-scene man_midvisit_rearrangement_disc.rs";

/// Zeto (`0x1D`, P1[5]) is seated by the same record.
const GARMEL_P2_12_ZETO: &str = "mid-beat capture inside garmel P2[12]: the record seats Zeto \
     (0x1D) with CC 1D 51 17 38 00 37 at +0x3E7, tile (23, 56) = (3008, 7232), exactly \
     retail's node; the capture is parked at +0x544, before the record's +0x9A2 re-park. \
     A cold entry holds the header park";

const VOZZ_P2_17_SEATS: &str = "mid-beat capture inside vozz P2[17] (Ozma's hatching, gated \
     by the poked 0x2C3 the record clears at +0x12): the record seats 0x06 / 0x07 / 0x09 / \
     0x0B (P1[2] / P1[3] / P1[5] / P1[7]) with its own 4C 51 seats - +0x2AD (233, 100) = \
     (13568, 12864) for 0x06, +0x541 / +0x548 / +0x54F = (15680, 12800) / (15808, 12864) / \
     (16000, 13184) for 0x09 / 0x07 / 0x0B, each retail's node exactly in the crawl state, \
     and the hover state's nodes within one step of the +0x32 / +0x40 seats after their \
     ExecMoves. A cold entry holds the prologue arrangement";

const VOZZ_P2_17_FLAG_1F: &str = "vozz P1[0]'s install slice sets 0x1F (50 1F at +0x49) on \
     every load and P2[17] clears it (60 1F at +0x4F1); the crawl capture is parked at \
     +0x7CE, past the clear, and the hover capture (+0x39C) still holds it set";

const BYLON_P2_6_PARTY: &str = "mid-beat capture inside bylon P2[6] (the Mist arrives, 0x2A2 \
     set by its +0x14): the record seats 0x48 / 0x35 (P1[37] / P1[18]) on the player with \
     CC 48 37 / CC 35 37 at +0x96 / +0x99 and walks them out with A2 ExecMove, so retail \
     holds them a step or two from the player; a cold entry holds their header parks";

const NILBOA_P2_7_SEATS: &str = "mid-beat capture inside nilboa P2[7] (the Delilas \
     introduction, parked at +0xC21 / +0xC6A): the record parks 0x30 (P1[22]) with \
     A3 30 7F 7F at +0x53, seats 0x31 / 0x32 (P1[23] / P1[24]) with CC 31 51 DD 76 .. / \
     CC 32 51 E6 7A .. at +0x9F / +0xA6 ((11968, 15168) before the landing arc, \
     (13184, 15680) exactly) and 0x2E (P1[20]) with A3 2E 6D 76 at +0x700 ((14016, 15168), \
     exactly retail's node). A cold entry holds the prologue seats";

const NILBOA_RAVINE_BLOCKS: &str = "visit history, inferred: the two push blocks 0x27 / 0x28 \
     (P1[13] / P1[14]) are already parked in the parent capture \
     nivora_cutscene_delilas_intro, before P2[7] runs. The only parks of them on disc are \
     the ravine-region arms P2[17] +0xCC (the 0x11 arm, run only while 0x0D is set) and \
     P2[26] +0x0E; the family captures hold 0x0D clear (P2[7]'s 60 0D at +0x51), so \
     neither runs from a cold entry with this bank, and the blocks stand on the pushed \
     seat their prologue picks from 0x457 (A3 27 B1 41 / A3 28 32 3F)";

const KNOWN_DIVERGENCES: &[KnownDivergence] = &[
    KnownDivergence {
        label: "son_field_door_hop",
        key: "pos:4",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "son_field_door_hop",
        key: "pos:6",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "son_field_door_hop",
        key: "pos:24",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "son_field_door_hop",
        key: "vis:2",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "son_field_door_hop",
        key: "vis:3",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "son_field_door_hop",
        key: "vis:23",
        class: "b",
        note: "a door-hop capture standing in a cutscene: the system script seats these records shot by shot (`CC 05 51 ..`, the `4C 51` arms at `0x801E1880`), clustered round (2752, 2752) in retail's RAM, where a cold entry holds their authored seats. Inferred from the seats and the script; the state's frame reads .996.",
    },
    KnownDivergence {
        label: "concnow_field_door_hop",
        key: "vis:2",
        class: "b",
        note: "mid-arrival-record capture: P2[13] seats the pair with `CC 32 51 1E 1C` / `CC 33 51 1E 1C` and walks them to tiles (31, 29) / (29, 29); a cold entry parks them, as retail's own entry does",
    },
    KnownDivergence {
        label: "concnow_field_door_hop",
        key: "vis:3",
        class: "b",
        note: "mid-arrival-record capture: P2[13] seats the pair with `CC 32 51 1E 1C` / `CC 33 51 1E 1C` and walks them to tiles (31, 29) / (29, 29); a cold entry parks them, as retail's own entry does",
    },
    KnownDivergence {
        label: "concnow_field_door_hop_settled",
        key: "vis:2",
        class: "b",
        note: "mid-arrival-record capture: P2[13] seats the pair with `CC 32 51 1E 1C` / `CC 33 51 1E 1C` and walks them to tiles (31, 29) / (29, 29); a cold entry parks them, as retail's own entry does",
    },
    KnownDivergence {
        label: "concnow_field_door_hop_settled",
        key: "vis:3",
        class: "b",
        note: "mid-arrival-record capture: P2[13] seats the pair with `CC 32 51 1E 1C` / `CC 33 51 1E 1C` and walks them to tiles (31, 29) / (29, 29); a cold entry parks them, as retail's own entry does",
    },
    KnownDivergence {
        label: "concend_field_door_hop",
        key: "vis:1",
        class: "b",
        note: "mid-arrival-record capture: concend's record seats the pair with `CC 07 51 2E 27` / `CC 08 51 2E 27` and walks them to tiles (47, 40) / (45, 40); a cold entry parks them",
    },
    KnownDivergence {
        label: "concend_field_door_hop",
        key: "vis:2",
        class: "b",
        note: "mid-arrival-record capture: concend's record seats the pair with `CC 07 51 2E 27` / `CC 08 51 2E 27` and walks them to tiles (47, 40) / (45, 40); a cold entry parks them",
    },
    KnownDivergence {
        label: "rim_elm_zoom_intro",
        key: "vis:*",
        class: "b",
        note: "mid-opening-timeline capture: the fly-in choreography still hides these \
               villagers; the engine's free-roam entry seats them exactly where the s3/s4 \
               free-roam captures show retail seats them",
    },
    KnownDivergence {
        label: "vahn_walks_out",
        key: "vis:*",
        class: "b",
        note: "same mid-opening-timeline park cohort as rim_elm_zoom_intro",
    },
    KnownDivergence {
        label: "ending_vignette_fullscreen",
        key: "vis:*",
        class: "b",
        note: "mid-ending-choreography capture (three of the walkers are mid-step); the \
               vignette timeline seats these actors, a cold entry parks them",
    },
    KnownDivergence {
        label: "ending_panel_corner",
        key: "vis:*",
        class: "b",
        note: "same ending-vignette choreography seats, later beat",
    },
    KnownDivergence {
        label: "ending_vignette_biron",
        key: "vis:*",
        class: "b",
        note: "Biron-monastery ending vignette choreography seats",
    },
    KnownDivergence {
        label: "s1_newgame_field",
        key: "pos:7",
        class: "b",
        note: "opdeene opening-timeline actor captured mid-beat away from its entry seat",
    },
    KnownDivergence {
        label: "s1_newgame_field",
        key: "flag:*",
        class: "b",
        note: "opening-latch timing skew, reported to the timeline lane: the engine's \
               opdeene entry latches 414/1320 and clears 418 during scene entry, retail \
               performs the same writes a few beats into the opening timeline (the s3 \
               free-roam capture holds the same end-state: 414 set, 418 clear)",
    },
    KnownDivergence {
        label: "rikuroa_post_caruban",
        key: "vis:*",
        class: "b",
        note: "post-Caruban staging keeps the rescue-party actors placed for the rest of \
               the visit; with 0x142 latched both retail's and the engine's spawn prologue \
               stop at the 0x142 arm's 0x21 NOP with the actor at its (127,127) header \
               park, so a fresh re-entry parks them in retail too",
    },
    KnownDivergence {
        label: "rikuroa_post_genesis_tree",
        key: "vis:4",
        class: "b",
        note: "a mid-visit beat parked this cutscene-only actor; its authored header tile \
               is (0,0) and retail's own fresh-entry arrangement stands it at (64,64) - \
               the pre_caruban capture shows the retail node exactly there",
    },
    KnownDivergence {
        label: "dolk2_market_noa",
        key: "vis:*",
        class: "b",
        note: "market-crowd beat swap: a mid-visit choreography record parks the day \
               cohort (their prologues seat them at market tiles, then the beat hides \
               them) and seats the crowd cohort 53..60 (bare idle-loop prologues, \
               (127,127) header parks). The capture's bank still has the P1[2] spawn \
               latch 0x2FE clear, so the swap ran through another path; a fresh-entry \
               dolk2 capture would close the remaining question",
    },
    KnownDivergence {
        label: "dolk2_market_noa",
        key: "pos:2",
        class: "b",
        note: "walker captured mid-route 896 units off its prologue seat (both placed)",
    },
    KnownDivergence {
        label: "conc_field_card_boot",
        key: "flag:1758",
        class: "b",
        note: "0x6DE is a live player-POSITION latch, so its value is a property of \
               where the capture stands, not of what the entry ran. A flag-write watch \
               across retail's own cold conc entry shows the clear running twice \
               (P1[1]'s spawn prologue +0x10, the P1[0] entry script +0x18) and then \
               P1[0]'s park loop re-arming it from +0x10B on every pass the player is \
               outside tiles 10..=51 x 14..=72 - the capture's player stands at tile \
               (17, 97), outside. A cold entry at entry point 0 stands at tile (29, 53), \
               inside, where retail writes nothing either. Pinned both ways by \
               conc_flag_6de_position_latch_disc.rs",
    },
    KnownDivergence {
        label: "chapter2_garmel_pre_zeto",
        key: "vis:3",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "chapter2_garmel_pre_zeto",
        key: "vis:4",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_rising",
        key: "vis:3",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_rising",
        key: "vis:4",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_rising",
        key: "vis:5",
        class: "b",
        note: GARMEL_P2_12_ZETO,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_taunt",
        key: "vis:3",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_taunt",
        key: "vis:4",
        class: "b",
        note: GARMEL_P2_12_PARTY,
    },
    KnownDivergence {
        label: "garmel_zeto_appears_taunt",
        key: "vis:5",
        class: "b",
        note: GARMEL_P2_12_ZETO,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_hover",
        key: "vis:2",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_hover",
        key: "vis:3",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_hover",
        key: "vis:5",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_crawl",
        key: "vis:2",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_crawl",
        key: "vis:3",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_crawl",
        key: "vis:5",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_crawl",
        key: "pos:7",
        class: "b",
        note: VOZZ_P2_17_SEATS,
    },
    KnownDivergence {
        label: "vozz_ozma_hatching_crawl",
        key: "flag:31",
        class: "b",
        note: VOZZ_P2_17_FLAG_1F,
    },
    KnownDivergence {
        label: "bylon_mist_gala_goes_ahead",
        key: "vis:18",
        class: "b",
        note: BYLON_P2_6_PARTY,
    },
    KnownDivergence {
        label: "bylon_mist_gala_goes_ahead",
        key: "vis:37",
        class: "b",
        note: BYLON_P2_6_PARTY,
    },
    KnownDivergence {
        label: "bylon_mist_noa_vahn",
        key: "vis:37",
        class: "b",
        note: BYLON_P2_6_PARTY,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "vis:13",
        class: "b",
        note: NILBOA_RAVINE_BLOCKS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "vis:14",
        class: "b",
        note: NILBOA_RAVINE_BLOCKS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "vis:20",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "vis:22",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "pos:23",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_pose",
        key: "pos:24",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "vis:13",
        class: "b",
        note: NILBOA_RAVINE_BLOCKS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "vis:14",
        class: "b",
        note: NILBOA_RAVINE_BLOCKS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "vis:20",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "vis:22",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "pos:23",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "nilboa_delilas_family_after_pose",
        key: "pos:24",
        class: "b",
        note: NILBOA_P2_7_SEATS,
    },
    KnownDivergence {
        label: "cort_evolved_approach_cutscene",
        key: "vis:1",
        class: "b",
        note: "mid-cutscene capture by its own catalog entry (the approach cinematic to \
               the final fight): the cutscene choreography seats this actor at the \
               tableau mark, a cold jouind entry parks it",
    },
    KnownDivergence {
        label: "cort_evolved_approach_cutscene",
        key: "vis:2",
        class: "b",
        note: "sibling of vis:1 - both retail nodes sit on the same tableau mark \
               (13184,2048), the same cutscene seat",
    },
    KnownDivergence {
        label: "cort_evolved_pre_battle",
        key: "vis:2",
        class: "c",
        note: "pre-battle staging places this cohort around the player half a second \
               before the evolved-Cort load (the capture's own catalog framing); cold \
               entry parks them. Same open question as garmel pre-Zeto: needs the \
               jouine staging record traced (or a fresh-entry jouine capture) to decide \
               whether a beat or the prologue seats them",
    },
    KnownDivergence {
        label: "cort_evolved_pre_battle",
        key: "vis:3",
        class: "c",
        note: "sibling of vis:2 - same pre-battle staging cohort",
    },
    KnownDivergence {
        label: "cort_evolved_pre_battle",
        key: "vis:4",
        class: "c",
        note: "sibling of vis:2 - same pre-battle staging cohort",
    },
    KnownDivergence {
        label: "nivora_cutscene_delilas_intro",
        key: "vis:*",
        class: "b",
        note: "mid-cutscene capture (the Delilas walk-in at the Nivora ravine): the \
               intro choreography has already hidden these nilboa actors when the \
               siblings take the stage; the engine's cold entry seats them per the \
               spawn prologue. A fresh-entry nilboa capture would pin the entry seats",
    },
    KnownDivergence {
        label: "nivora_cutscene_delilas_intro",
        key: "pos:*",
        class: "b",
        note: "same walk-in choreography: actors 22/23 are captured on their cutscene \
               marks, away from the entry seats the engine reproduces",
    },
    KnownDivergence {
        label: "doman_arrival_from_korb2",
        key: "flag:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 vis:*: the MAN \
               loader's install slice (FUN_8003AB2C) has not run, so its flag writes are \
               missing from retail's bank",
    },
    KnownDivergence {
        label: "son_arrival_from_doman",
        key: "flag:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 flag:*",
    },
    KnownDivergence {
        label: "nivora_cutscene_delilas_intro",
        key: "flag:1140",
        class: "b",
        note: "nilboa P1[0]'s install slice clears 0x474 (64 74 at +0xCD) on every load; \
               the capture is mid-visit, and retail's bank holds it raised since",
    },
    KnownDivergence {
        label: "rikuroa_post_genesis_tree",
        key: "flag:31",
        class: "b",
        note: "rikuroa P1[0]'s install slice sets 0x1F (50 1F at +0x49) on every load; \
               the capture is mid-visit (and on a patched executable), retail's bank holds \
               it cleared since",
    },
    KnownDivergence {
        label: "doman_arrival_from_korb2",
        key: "vis:*",
        class: "b",
        note: "pre-load capture: the scene label already names the destination while the \
               actor pool is still map03's (retail's seats run along z = 3520 at a 512-unit \
               pitch - the world-map portal row), so there is no destination arrangement \
               yet to compare; the engine's fresh entry is the arrangement retail builds \
               next",
    },
    KnownDivergence {
        label: "doman_arrival_from_korb2",
        key: "pos:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 vis:*",
    },
    KnownDivergence {
        label: "son_arrival_from_doman",
        key: "vis:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 vis:*",
    },
    KnownDivergence {
        label: "son_arrival_from_doman",
        key: "pos:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 vis:*",
    },
    KnownDivergence {
        label: "son_arrival_from_doman",
        key: "solid:*",
        class: "b",
        note: "same pre-load capture context as doman_arrival_from_korb2 vis:*: the node \
               still carries the previous scene's actor word",
    },
    KnownDivergence {
        label: "kor5_post_436_organic",
        key: "vis:*",
        class: "b",
        note: "mid-visit capture taken after the whole P2[3] -> P2[8] chain ran (a \
               0x464 battle, a scene reload, P2[5]'s 0x436 write): the chain's walk-on \
               beats re-seat and re-show actors after retail's own entry, which a cold \
               entry against the captured bank does not replay",
    },
    KnownDivergence {
        label: "kor5_post_436_organic",
        key: "pos:*",
        class: "b",
        note: "same mid-chain capture context as kor5_post_436_organic vis:*",
    },
    KnownDivergence {
        label: "kor5_post_436_organic",
        key: "flag:1561",
        class: "b",
        note: "capture context, not an engine defect: 0x619's only unconditional SET is \
               P1[2]+0x1A, which sits in that record's SPAWN section (0x25 at pc0, the \
               SET, a CamCfg, the raw 0x21 terminator at +0x1F - the interaction resumes \
               at +0x20). FUN_8003A1E4 runs that section at every MAN-loading entry \
               (first-opcode gate 0x24/0x25 at 0x8003A480 passes), so retail's fresh entry \
               sets it too - both kor5 states captured before the chain clears it \
               (kor5_field_card_boot, kor5_post_43a_checkpoint) hold it set. This state \
               was captured after P2[4]'s CLEAR (+0x16) and a post-battle same-scene \
               reload, and that reload re-spawns no partition-1 record: FUN_801D6704 \
               passes a0 = loader mask & 4 (MAN loaded) to FUN_8003AEB0, which skips the \
               spawn loop at 0x8003B8A0 when it is zero - the capture's zero SETs over two \
               reloads. A cold entry against the post-CLEAR bank re-runs the spawn \
               section, exactly as retail's own fresh entry would",
    },
    KnownDivergence {
        label: "doman_arrival_from_korb2",
        key: "flag:2",
        class: "b",
        note: "pre-load capture by its own catalog entry (about 60 vsyncs before doman \
               loads, the player still on map03's portal tile): map03's script has raised \
               system flag 2, the place-name banner's one-shot, and the MAN loader's tail \
               (FUN_8003AEB0, andi 0xdf at 0x8003BBD4) consumes it on every load - so the \
               engine's entry clears a flag retail has not reached the clear for yet",
    },
    KnownDivergence {
        label: "town01_tetsu_topic_prompt",
        key: "vis:33",
        class: "b",
        note: "mid-conversation capture (inference, not traced): the state is replayed \
               from s4_rimelm_door_transition along a recorded human route and taken with \
               Tetsu's talk record engaged and his topic prompt open, so beats that ran \
               after the scene's entry have parked slots 33 / 34 at the (16320, 16320) \
               park point. A cold entry seats both, as retail's own entry does in every \
               other town01 state in the library",
    },
    KnownDivergence {
        label: "town01_tetsu_topic_prompt",
        key: "vis:34",
        class: "b",
        note: "same mid-conversation capture context as town01_tetsu_topic_prompt vis:33",
    },
    KnownDivergence {
        label: "tunnelc_field_route_walk",
        key: "flag:864",
        class: "b",
        note: "set-then-clear inside retail's own entry, captured after the clear: Xain's \
               P1[4] spawn section (FUN_8003A1E4) sets 0x360 and writes 80 to script \
               counter 1 (53 60, 4C CA 01 50 00); P1[0]'s per-frame body, past its install \
               pass's 0x21 at +0x8E, tests 0x360 (+0x165), takes the 79 < counter-1 arm \
               and clears it (63 60 at +0x1B0), then zeroes the counter (4C CA 01 00 00). \
               The capture holds that end state (0x360 clear, counter 1 = 0, Xain at his \
               spawn seat (7872,1856) = 23 3D 0E). The oracle compares after the install \
               slice and before any per-frame pass; the engine clears it on its fifth tick. Pinned by \
               engine-scene-host entry_install_slice_disc.rs",
    },
    KnownDivergence {
        label: "conc3_field_inn_night",
        key: "vis:7",
        class: "b",
        note: CONC3_INN_ARRIVAL,
    },
    KnownDivergence {
        label: "conc3_field_inn_night",
        key: "vis:8",
        class: "b",
        note: CONC3_INN_ARRIVAL,
    },
    KnownDivergence {
        label: "conc3_field_inn_night",
        key: "vis:9",
        class: "b",
        note: CONC3_INN_ARRIVAL,
    },
    KnownDivergence {
        label: "conc3_field_inn_night",
        key: "vis:23",
        class: "b",
        note: CONC3_INN_ARRIVAL,
    },
];

fn known(label: &str, key: &str) -> Option<&'static KnownDivergence> {
    KNOWN_DIVERGENCES.iter().find(|k| {
        k.label == label
            && match k.key.strip_suffix('*') {
                Some(prefix) => key.starts_with(prefix),
                None => k.key == key,
            }
    })
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn first_existing(cands: &[&str]) -> Option<PathBuf> {
    cands.iter().map(PathBuf::from).find(|p| p.exists())
}

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes(ram[o..o + 4].try_into().unwrap())
}

fn u16_at(ram: &[u8], va: u32) -> u16 {
    let o = (va & RAM_MASK) as usize;
    u16::from_le_bytes(ram[o..o + 2].try_into().unwrap())
}

fn i16_at(ram: &[u8], va: u32) -> i16 {
    u16_at(ram, va) as i16
}

fn u8_at(ram: &[u8], va: u32) -> u8 {
    ram[(va & RAM_MASK) as usize]
}

/// Both coordinates in the far-corner park region (`tile 0x7F` = world
/// `16256..=16320`): the actor is off-field, not a visible placement.
fn is_parked(x: i16, z: i16) -> bool {
    x >= 0x3F00 && z >= 0x3F00
}

/// One retail field-actor node lifted from a capture's RAM.
#[derive(Debug, Clone, Copy)]
struct RetailActor {
    /// `actor[+0x50]`: flat MAN record index (`N0 + placement` for
    /// partition-1 NPCs).
    id50: u8,
    x: i16,
    z: i16,
    /// `actor[+0x26]` render heading, 12-bit, retail `0` = Z-.
    heading: u16,
    /// `actor[+0x10]` flags word (`0x100` script-engaged, `0x400`
    /// motion-VM walking, `0x80000` move-disabled).
    flags: u32,
}

/// Everything the oracle needs from one capture.
struct RetailCapture {
    label: String,
    scene: String,
    game_mode: u8,
    actors: Vec<RetailActor>,
    flag_bank: Vec<u8>,
    player_pos: Option<(i16, i16)>,
}

fn scene_name_from_ram(ram: &[u8]) -> String {
    let mut s = String::new();
    for i in 0..8 {
        let b = u8_at(ram, SCENE_NAME_VA + i);
        if !(0x20..0x7f).contains(&b) {
            break;
        }
        s.push(b as char);
    }
    s
}

fn capture_from_ram(label: &str, ram: &[u8]) -> RetailCapture {
    let head = u32_at(ram, ACTOR_LIST_HEAD_VA);
    let mut actors = Vec::new();
    if (head & 0xFFE0_0000) == 0x8000_0000 {
        let mut node = u32_at(ram, head);
        let mut hops = 0;
        while node != 0 && node != head && (node & 0xFFE0_0000) == 0x8000_0000 && hops < 192 {
            actors.push(RetailActor {
                id50: u8_at(ram, node + 0x50),
                x: i16_at(ram, node + 0x14),
                z: i16_at(ram, node + 0x18),
                heading: u16_at(ram, node + 0x26) & 0xFFF,
                flags: u32_at(ram, node + 0x10),
            });
            node = u32_at(ram, node);
            hops += 1;
        }
    }
    let bank_off = (FLAG_BANK_VA & RAM_MASK) as usize;
    let player = {
        let p = u32_at(ram, 0x8007_C364);
        ((p & 0xFFE0_0000) == 0x8000_0000).then(|| (i16_at(ram, p + 0x14), i16_at(ram, p + 0x18)))
    };
    RetailCapture {
        label: label.to_string(),
        scene: scene_name_from_ram(ram),
        game_mode: u8_at(ram, GAME_MODE_VA),
        actors,
        flag_bank: ram[bank_off..bank_off + FLAG_BANK_LEN].to_vec(),
        player_pos: player,
    }
}

/// Load a library backup's main RAM, whichever emulator captured it.
fn load_backup_ram(path: &Path) -> Option<Vec<u8>> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sstate") => {
            let st = psx_pcsx_redux::SaveState::from_path(path).ok()?;
            Some(st.main_ram().to_vec())
        }
        _ => {
            let st = psx_mednafen::SaveState::from_path(path).ok()?;
            let ram = st.main_ram().ok()?;
            Some(ram.to_vec())
        }
    }
}

/// A single comparison finding.
struct Divergence {
    label: String,
    scene: String,
    key: String,
    detail: String,
}

#[allow(clippy::too_many_lines)]
#[test]
fn catalogued_field_states_match_retail_npc_and_flag_state() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(manifest_path) = first_existing(&[
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]) else {
        eprintln!("[skip] scenarios manifest missing");
        return;
    };
    let Some(library) =
        first_existing(&["saves/library", "../saves/library", "../../saves/library"])
    else {
        eprintln!("[skip] saves library missing");
        return;
    };
    // The PCSX-Redux RAM locator needs the SCUS anchor bytes.
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let verbose = std::env::var_os("LEGAIA_NPC_PARITY_VERBOSE").is_some();

    // ---- Enumerate: every field-phase scenario with a resolvable backup. ----
    let mut captures: Vec<RetailCapture> = Vec::new();
    for scn in &manifest.scenarios {
        if scn.phase.as_deref() != Some("field") {
            continue;
        }
        let Some(fp) = scn.backup_fingerprint.as_deref() else {
            continue;
        };
        let Some(path) = library_backup_for("mednafen", &library, fp)
            .or_else(|| library_backup_for("pcsx-redux", &library, fp))
        else {
            continue;
        };
        let Some(ram) = load_backup_ram(&path) else {
            eprintln!("[warn] {}: backup {} unreadable", scn.label, path.display());
            continue;
        };
        let cap = capture_from_ram(&scn.label, &ram);
        // Field free-roam states only: other modes (scene-load gaps, ending
        // vignette machinery) run a different actor arrangement.
        if cap.game_mode != 0x03 {
            if verbose {
                eprintln!(
                    "[note] {}: game_mode {:#x} != 0x03 field - skipped",
                    cap.label, cap.game_mode
                );
            }
            continue;
        }
        if cap.scene.is_empty() {
            eprintln!("[warn] {}: no readable scene name - skipped", cap.label);
            continue;
        }
        // The kingdom overworlds run the world-map entity SM
        // (`FUN_801DA51C`), not the field NPC placement model.
        if is_world_map_scene(&cap.scene) {
            if verbose {
                eprintln!(
                    "[note] {}: worldmap scene {} - skipped",
                    cap.label, cap.scene
                );
            }
            continue;
        }
        if let Some(exp) = scn.expected_active_scene.as_deref()
            && exp != cap.scene
        {
            eprintln!(
                "[warn] {}: manifest expects scene {exp}, RAM says {} - using RAM",
                cap.label, cap.scene
            );
        }
        captures.push(cap);
    }
    if captures.is_empty() {
        eprintln!("[skip] no field-mode library captures resolvable");
        return;
    }

    let locality = legaia_engine_field::man_field_scripts::NPC_ROUTE_LOCALITY;
    let mut divergences: Vec<Divergence> = Vec::new();
    let mut tolerated = 0usize;
    let mut states_compared = 0usize;
    let mut total_slots = 0usize;
    let mut total_parked_both = 0usize;
    let mut heading_checked = 0usize;
    let mut heading_drift = 0usize;
    let mut solid_checked = 0usize;

    for cap in &captures {
        // ---- Engine side: seed the capture's flag bank, cold-enter. ----
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        host.world.flags.system_flags = cap.flag_bank.clone();
        // Enter where the capture stands, as a card load's MAN loader does
        // (`FUN_8003AEB0` seats the save's position before its install slice,
        // `FUN_8003AB2C`): the slice's region selector and `CD F8` box tests
        // then read retail's position, not entry point 0's.
        if let Some((x, z)) = cap.player_pos {
            host.set_entry_seat(x, z);
        }
        if let Err(err) = host.enter_field_scene(&cap.scene, 0) {
            divergences.push(Divergence {
                label: cap.label.clone(),
                scene: cap.scene.clone(),
                key: "enter".into(),
                detail: format!("enter_field_scene failed: {err:#}"),
            });
            continue;
        }
        // The MAN loader's last step, which the engine runs at the head of
        // its first tick (`World::run_entry_script_install_slice`).
        host.world.run_entry_script_install_slice();
        states_compared += 1;

        let scene = host.scene.as_ref().expect("scene loaded");
        let placements = match scene.field_actor_placements(&host.index) {
            Ok(Some(p)) => p,
            _ => {
                if verbose {
                    eprintln!("[note] {} ({}): no MAN placements", cap.label, cap.scene);
                }
                Vec::new()
            }
        };
        let n0 = scene
            .field_man_payload(&host.index)
            .ok()
            .flatten()
            .and_then(|man| {
                legaia_scene_formats::man_section::parse(&man)
                    .ok()
                    .map(|mf| mf.header.partition_counts[0].max(0) as u8)
            })
            .unwrap_or(0);

        let mut slots = 0usize;
        let mut parked_both = 0usize;
        for ra in &cap.actors {
            if ra.id50 < n0 {
                continue; // partition-0 object actors / specials
            }
            let pi = ra.id50 - n0;
            let Some(p) = placements.iter().find(|p| p.index == pi as usize) else {
                continue;
            };
            let (ex, ez) = host
                .world
                .npcs
                .positions
                .get(&pi)
                .copied()
                .unwrap_or((p.world_x, p.world_z));
            slots += 1;
            let (rp, ep) = (is_parked(ra.x, ra.z), is_parked(ex, ez));
            if rp && ep {
                parked_both += 1;
                continue;
            }
            if rp != ep {
                let key = format!("vis:{pi}");
                let detail = format!(
                    "retail {} at ({},{}), engine {} at ({ex},{ez}) [rflags {:#x}]",
                    if rp { "PARKED" } else { "placed" },
                    ra.x,
                    ra.z,
                    if ep { "PARKED" } else { "placed" },
                    ra.flags
                );
                match known(&cap.label, &key) {
                    Some(k) => {
                        tolerated += 1;
                        eprintln!(
                            "[known-{}] {} ({}) {key}: {detail} - {}",
                            k.class, cap.label, cap.scene, k.note
                        );
                    }
                    None => divergences.push(Divergence {
                        label: cap.label.clone(),
                        scene: cap.scene.clone(),
                        key,
                        detail,
                    }),
                }
                continue;
            }
            let (dx, dz) = (
                (ex as i32 - ra.x as i32).abs(),
                (ez as i32 - ra.z as i32).abs(),
            );
            if dx.max(dz) > locality {
                let key = format!("pos:{pi}");
                let detail = format!(
                    "engine seat ({ex},{ez}) is {dx}/{dz} units from retail ({},{})",
                    ra.x, ra.z
                );
                match known(&cap.label, &key) {
                    Some(k) => {
                        tolerated += 1;
                        eprintln!(
                            "[known-{}] {} ({}) {key}: {detail} - {}",
                            k.class, cap.label, cap.scene, k.note
                        );
                    }
                    None => divergences.push(Divergence {
                        label: cap.label.clone(),
                        scene: cap.scene.clone(),
                        key,
                        detail,
                    }),
                }
                continue;
            }
            // Collision class: `FUN_801CF754` admits a placement while its
            // `+0x10 & 3` is clear. Bit 1 is the visibility cull, rewritten
            // every tick from the camera window, so only the script's bit 0
            // is a property of the seat; the engine carries it on the
            // placement channel's own word.
            if let Some(ef) = host
                .world
                .field_vm
                .channels
                .iter()
                .find(|c| !c.object_bind && c.placement_index == pi as usize)
                .map(|c| c.ctx.flags)
            {
                solid_checked += 1;
                if (ef ^ ra.flags) & 1 != 0 {
                    let key = format!("solid:{pi}");
                    let detail = format!(
                        "pass-through bit: retail +0x10 {:#010x}, engine {ef:#010x}",
                        ra.flags
                    );
                    match known(&cap.label, &key) {
                        Some(k) => {
                            tolerated += 1;
                            eprintln!(
                                "[known-{}] {} ({}) {key}: {detail} - {}",
                                k.class, cap.label, cap.scene, k.note
                            );
                        }
                        None => divergences.push(Divergence {
                            label: cap.label.clone(),
                            scene: cap.scene.clone(),
                            key,
                            detail,
                        }),
                    }
                }
            }
            // Heading: diagnostic only. Engine convention 0 = Z+, retail
            // 0 = Z- (engine = retail + 0x800 mod 0x1000); a walker keeps its
            // last travel direction, so drift is expected dynamics.
            if let Some(&eh) = host.world.npcs.headings.get(&pi) {
                heading_checked += 1;
                let expected = ((ra.heading + 0x800) & 0xFFF) as i16;
                if (eh & 0xFFF) != expected {
                    heading_drift += 1;
                    if verbose {
                        eprintln!(
                            "[hdg] {} ({}) pi {pi}: engine {:#x}, retail {:#x} (engine-frame {:#x}) [rflags {:#x}]",
                            cap.label,
                            cap.scene,
                            eh & 0xFFF,
                            ra.heading,
                            expected,
                            ra.flags
                        );
                    }
                }
            }
        }
        total_slots += slots;
        total_parked_both += parked_both;

        // ---- Story-flag bank: engine entry must be a no-op vs the capture. ----
        // Except the region-selector bands: the install slice the MAN loader
        // runs (`FUN_8003AB2C`) clears each one, and a later pass of the body
        // re-selects the bit from the region-type latch the player tick
        // writes (`_DAT_8007B8F4`). Every capture holds a pass that ran after
        // a player tick; the cold entry holds the install's clear.
        let bands = legaia_parity::retail_compare::selector_bands(&host.world.field_bytecode);
        let in_band = |idx: usize| bands.iter().any(|b| b.contains(&(idx as u16)));
        let bank = &host.world.flags.system_flags;
        for (byte, (&e, &r)) in bank.iter().zip(cap.flag_bank.iter()).enumerate() {
            if e == r {
                continue;
            }
            for bit in 0..8 {
                let mask = 0x80u8 >> bit;
                if (e ^ r) & mask != 0 {
                    let idx = byte * 8 + bit;
                    if in_band(idx) && e & mask == 0 {
                        tolerated += 1;
                        continue;
                    }
                    let key = format!("flag:{idx}");
                    let detail = format!(
                        "engine {} flag {idx} ({:#x}) during entry; retail bank holds {}",
                        if e & mask != 0 { "SET" } else { "CLEARED" },
                        idx,
                        (r & mask != 0) as u8
                    );
                    match known(&cap.label, &key) {
                        Some(k) => {
                            tolerated += 1;
                            eprintln!(
                                "[known-{}] {} ({}) {key}: {detail} - {}",
                                k.class, cap.label, cap.scene, k.note
                            );
                        }
                        None => divergences.push(Divergence {
                            label: cap.label.clone(),
                            scene: cap.scene.clone(),
                            key,
                            detail,
                        }),
                    }
                }
            }
        }

        let walking = cap.actors.iter().filter(|a| a.flags & 0x400 != 0).count();
        let engaged = cap.actors.iter().filter(|a| a.flags & 0x100 != 0).count();
        eprintln!(
            "[state] {} ({}): {} retail nodes, {slots} placement slots ({parked_both} parked in \
             both), {walking} walking, {engaged} engaged, player {:?}",
            cap.label,
            cap.scene,
            cap.actors.len(),
            cap.player_pos
        );
    }

    eprintln!(
        "[sweep] {states_compared} field states compared, {total_slots} placement slots \
         ({total_parked_both} parked in both), heading checked {heading_checked} \
         (drift {heading_drift}), collision class checked {solid_checked}, {tolerated} known divergences tolerated, {} open",
        divergences.len()
    );
    for d in &divergences {
        eprintln!(
            "  DIVERGENCE {} ({}) {}: {}",
            d.label, d.scene, d.key, d.detail
        );
    }

    // Non-vacuity: the sweep really covered a broad corpus.
    assert!(
        states_compared >= 5,
        "expected >=5 comparable field states, got {states_compared}"
    );
    assert!(
        total_slots >= 100,
        "expected >=100 compared placement slots, got {total_slots}"
    );
    assert!(
        divergences.is_empty(),
        "{} unclassified retail-vs-engine divergence(s) - see the log",
        divergences.len()
    );
}
