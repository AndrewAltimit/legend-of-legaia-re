//! `LegaiaSfx` - disc-decoded one-shot sound cues for the site pages.
//!
//! The retail sound path is three pieces, all already reverse-engineered:
//!
//! 1. **The cue id.** Game code fires a sound by writing a cue id into the
//!    4-entry ring at `_DAT_8007B6D8` (directly, or through the dispatcher
//!    `FUN_8004FCC8`). The Baka Fighter overlay writes the ring **directly** -
//!    `FUN_801D3B18` (`_DAT_8007b6d8 = 9`, the exchange hit), the menu SM
//!    (`FUN_801CF388` family: `0x20` / `0x21` / `0x37`) and the score tally
//!    `FUN_801D239C` (`0x21`). Those four are the *only* cue ids the whole
//!    overlay writes.
//! 2. **The descriptor.** The ring drainer `FUN_80016B6C` indexes
//!    `&DAT_8006F198 + ring_value * 8` - so the ring value **is** the static
//!    SFX-descriptor index ([`legaia_asset::sfx_table`],
//!    [`docs/formats/sfx-table.md`]). Each descriptor gives program + tone +
//!    note + voice count.
//! 3. **The bank.** The descriptor's program/tone index a loaded VAB. The
//!    **class-2 sound bank at extraction PROT [`SFX_BANK_PROT_INDEX`]** (raw
//!    loader index `0x367`) is loaded by *both* the battle scene loader
//!    (`FUN_800520F0`, `a1 = 2`) and the Baka Fighter init (`FUN_801CF00C`:
//!    `FUN_8001FC00(0x367, 2, ...)`), so it is the bank behind both the duel's
//!    cues and the battle-side strike cue the arts page wants.
//!
//! This module walks that chain in the browser off the visitor's own disc:
//! SCUS -> descriptor table, PROT 869 -> VAB, then renders each cue through
//! the clean-room SPU ([`legaia_engine_audio`]) to a PCM buffer the page plays
//! with WebAudio. No Sony bytes ship with the site - everything decodes at
//! runtime from the loaded image.

use super::*;

use legaia_asset::sfx_table::SfxTable;

/// Class-2 sound bank (raw loader index `0x367`): the SFX VAB both the battle
/// scene loader `FUN_800520F0` and the Baka Fighter init `FUN_801CF00C` load.
pub const SFX_BANK_PROT_INDEX: u32 = 869;
/// The alternate class-2 bank `FUN_800520F0` swaps in on `DAT_8007BD11 == 4`
/// (raw `0x36D`). Used as a fallback when [`SFX_BANK_PROT_INDEX`] doesn't
/// parse on a given image.
pub const SFX_BANK_ALT_PROT_INDEX: u32 = 875;

/// Exchange hit - disc-sourced: `FUN_801D3B18` writes `_DAT_8007b6d8 = 9`
/// when it applies an exchange's damage.
pub const CUE_HIT: u8 = 0x09;
/// Menu confirm - disc-sourced (Baka menu SM, `FUN_801CF388` family).
pub const CUE_CONFIRM: u8 = 0x20;
/// Menu cursor move / score-tally tick - disc-sourced (`FUN_801CF388`
/// family + the tally drain `FUN_801D239C`).
pub const CUE_CURSOR: u8 = 0x21;
/// Menu cancel - disc-sourced (Baka menu SM).
pub const CUE_CANCEL: u8 = 0x37;
/// The generic "play sound" strike cue: the art record's documented Hit
/// Effect Cue kind (`0x1A`, see `docs/formats/art-data.md`), which is also the
/// canonical cue [`legaia_engine_audio::SfxBank::vanilla`] maps.
pub const CUE_ART_STRIKE: u8 = 0x1A;

/// How a cue id was chosen. Surfaced per event so the pages can be honest
/// about which sounds are retail's and which are the site's pick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Traced to a retail ring write / documented record field.
    Disc,
    /// Retail plays nothing here; the site reuses the closest existing cue.
    Site,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Disc => "disc",
            Source::Site => "site",
        }
    }
}

/// Event -> cue map for the Baka Fighter page. The first five rows are the
/// complete set of ring writes in the duel overlay; the last two are site
/// picks (retail fires **no** cue for a round-start banner or a match loss -
/// its banners are silent, so the page reuses the confirm / cancel blips).
const BAKA_EVENTS: &[(&str, u8, Source, &str)] = &[
    (
        "hit",
        CUE_HIT,
        Source::Disc,
        "FUN_801D3B18 damage application",
    ),
    ("confirm", CUE_CONFIRM, Source::Disc, "Baka menu SM"),
    ("cursor", CUE_CURSOR, Source::Disc, "Baka menu SM"),
    ("cancel", CUE_CANCEL, Source::Disc, "Baka menu SM"),
    (
        "tally",
        CUE_CURSOR,
        Source::Disc,
        "FUN_801D239C score-tally drain",
    ),
    (
        "round_start",
        CUE_CONFIRM,
        Source::Site,
        "retail's READY/FIGHT banner fires no cue",
    ),
    (
        "match_lose",
        CUE_CANCEL,
        Source::Site,
        "retail's LOSE banner fires no cue",
    ),
];

/// Event -> cue map for the arts page. The move-power table's `+0x0d` sound
/// cue covers **enemy special attacks only** (a party art's move id is
/// unmapped - see `docs/formats/move-power.md`), so an art has no per-art cue
/// id to read off the disc. The page fires the art record's documented generic
/// sound kind instead.
const ART_EVENTS: &[(&str, u8, Source, &str)] = &[(
    "strike",
    CUE_ART_STRIKE,
    Source::Disc,
    "art-record Hit Effect Cue kind 0x1A (per-art cue ids are not on disc)",
)];

/// One rendered cue: its id plus interleaved-stereo PCM at the SPU rate.
struct RenderedCue {
    id: u8,
    /// Interleaved stereo i16 at [`legaia_engine_audio::SPU_INTERNAL_RATE`].
    pcm: Vec<i16>,
    /// Peak absolute sample - the page uses it for clip-safe gain staging.
    peak: i16,
}

/// Longest one-shot the renderer will keep (SPU samples). Retail SFX are far
/// shorter; this only bounds a pathological sustained descriptor.
const MAX_CUE_SAMPLES: usize = legaia_engine_audio::SPU_INTERNAL_RATE as usize * 2;

/// Render one cue through a freshly-uploaded bank so no other cue's voice tail
/// bleeds into it.
///
/// Fires the descriptor the way the retail drainer `FUN_80016B6C` does: the
/// program is `+0`, the **region index** is `+1` (not a key-range lookup - the
/// SFX path names the tone directly), the note-level attribute is `+2`, and a
/// multi-voice cue (`+3 & 0x1F`) keys consecutive regions `tone + i` on
/// consecutive voices. Returns `None` when nothing sounded (the program / tone
/// / sample doesn't resolve in this bank).
fn render_cue(
    bank_buf: &[u8],
    report: &legaia_vab::VabReport,
    desc: &legaia_asset::sfx_table::SfxDescriptor,
    id: u8,
) -> Option<RenderedCue> {
    use legaia_engine_audio::{Spu, VabBank, spu::ram::SpuAllocator};

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let vab = VabBank::upload(&mut spu, &mut alloc, report, bank_buf);

    let voices = desc.voice_count().max(1) as usize;
    let mut sounded = false;
    for i in 0..voices {
        sounded |= vab.play_tone(
            &mut spu,
            i,
            desc.program as usize,
            desc.tone as usize + i,
            desc.note,
            100,
        );
    }
    if !sounded {
        return None;
    }

    let mut pcm: Vec<i16> = Vec::new();
    let mut peak: i16 = 0;
    // Render until every voice has finished, bounded by MAX_CUE_SAMPLES.
    let mut silent_run = 0usize;
    for _ in 0..MAX_CUE_SAMPLES {
        let (l, r) = spu.tick();
        peak = peak.max(l.saturating_abs()).max(r.saturating_abs());
        pcm.push(l);
        pcm.push(r);
        if l == 0 && r == 0 {
            silent_run += 1;
            // ~10 ms of continuous silence after the cue has sounded ends it.
            if peak > 0 && silent_run > 441 {
                break;
            }
        } else {
            silent_run = 0;
        }
    }
    if peak == 0 {
        return None;
    }
    // Drop the trailing silence the loop rolled through.
    let keep = pcm.len().saturating_sub(silent_run * 2);
    pcm.truncate(keep.max(2));
    Some(RenderedCue { id, pcm, peak })
}

/// The site's shared sound-cue surface: renders every cue the minigame + arts
/// pages fire, once, off the loaded disc.
#[wasm_bindgen]
pub struct LegaiaSfx {
    cues: Vec<RenderedCue>,
    /// PROT entry the cues were rendered from.
    bank_index: u32,
}

impl Default for LegaiaSfx {
    fn default() -> Self {
        Self::new()
    }
}

/// Every cue id the site can fire, de-duplicated.
fn site_cue_ids() -> Vec<u8> {
    let mut ids: Vec<u8> = BAKA_EVENTS
        .iter()
        .map(|(_, id, _, _)| *id)
        .chain(ART_EVENTS.iter().map(|(_, id, _, _)| *id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Emit the `{ event: { cue, source, why } }` JSON body for one event table.
fn events_json(events: &[(&str, u8, Source, &str)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (event, id, source, why) in events {
        map.insert(
            (*event).to_string(),
            serde_json::json!({
                "cue": id,
                "source": source.as_str(),
                "why": why,
            }),
        );
    }
    serde_json::Value::Object(map)
}

#[wasm_bindgen]
impl LegaiaSfx {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            cues: Vec::new(),
            bank_index: 0,
        }
    }

    /// Decode + render every site cue from a full Mode2/2352 disc image.
    ///
    /// Walks the retail chain: `SCUS_942.54` -> the static SFX descriptor
    /// table, `PROT.DAT` -> the class-2 sound bank ([`SFX_BANK_PROT_INDEX`]),
    /// then each cue's descriptor -> a one-shot through the clean-room SPU.
    /// Holds only the rendered PCM afterwards (the disc bytes are dropped), so
    /// a page can call this alongside its own decoder without a second copy of
    /// the image.
    ///
    /// Returns JSON:
    /// ```json
    /// { "ok": true, "bank": 869, "rate": 44100,
    ///   "cues": [ { "id": 9, "samples": 5400, "peak": 8123 }, ... ] }
    /// ```
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        let scus = disc::extract_scus(&bytes)
            .ok_or_else(|| JsValue::from_str("sfx: SCUS_942.54 not found (needs a full .bin)"))?;
        let table = SfxTable::from_scus(&scus)
            .ok_or_else(|| JsValue::from_str("sfx: SFX descriptor table did not resolve"))?;
        let prot = disc::extract_prot_dat(&bytes)
            .ok_or_else(|| JsValue::from_str("sfx: PROT.DAT not found in this disc image"))?;
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("sfx: PROT.DAT TOC parse failed"))?;

        let mut rendered = Vec::new();
        let mut bank_index = 0u32;
        for cand in [SFX_BANK_PROT_INDEX, SFX_BANK_ALT_PROT_INDEX] {
            let Some(meta) = entries.iter().find(|e| e.index == cand) else {
                continue;
            };
            let off = meta.byte_offset as usize;
            let end = (meta.byte_offset + meta.size_bytes) as usize;
            let Some(buf) = prot.get(off..end.min(prot.len())) else {
                continue;
            };
            // The bank is a scene-VAB-prefixed stream: 4-byte chunk0 header,
            // then the VAB body (the same `+4` slice the BGM path takes).
            let Some((report, vab_off)) = [4usize, 0]
                .into_iter()
                .find_map(|o| legaia_vab::parse(buf, o).ok().map(|r| (r, o)))
            else {
                continue;
            };
            let body = &buf[vab_off..];
            rendered = site_cue_ids()
                .into_iter()
                .filter_map(|id| {
                    let desc = table.get(id)?;
                    render_cue(body, &report, desc, id)
                })
                .collect();
            if !rendered.is_empty() {
                bank_index = cand;
                break;
            }
        }
        if rendered.is_empty() {
            return Err(JsValue::from_str(
                "sfx: no cue resolved in the class-2 sound bank",
            ));
        }

        let list: Vec<serde_json::Value> = rendered
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "samples": c.pcm.len() / 2,
                    "peak": c.peak,
                })
            })
            .collect();
        let json = serde_json::json!({
            "ok": true,
            "bank": bank_index,
            "rate": legaia_engine_audio::SPU_INTERNAL_RATE,
            "cues": list,
        })
        .to_string();
        self.cues = rendered;
        self.bank_index = bank_index;
        Ok(json)
    }

    /// Sample rate of every buffer [`Self::cue_pcm_i16`] returns.
    pub fn sample_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }

    /// PROT entry the cues were rendered from (0 until [`Self::load_disc`]).
    pub fn bank_prot_index(&self) -> u32 {
        self.bank_index
    }

    /// Cue ids that rendered, in ascending order.
    pub fn cue_ids(&self) -> Vec<u32> {
        self.cues.iter().map(|c| c.id as u32).collect()
    }

    /// One cue's interleaved-stereo i16 PCM at [`Self::sample_rate`]. Empty
    /// when the id didn't render on this disc.
    pub fn cue_pcm_i16(&self, id: u32) -> Vec<i16> {
        self.cues
            .iter()
            .find(|c| c.id as u32 == id)
            .map(|c| c.pcm.clone())
            .unwrap_or_default()
    }

    /// Peak absolute sample of one cue (0 when absent). The page stages gain
    /// off this so a quiet cue is audible without the loud ones clipping.
    pub fn cue_peak(&self, id: u32) -> u32 {
        self.cues
            .iter()
            .find(|c| c.id as u32 == id)
            .map(|c| c.peak as u32)
            .unwrap_or(0)
    }

    /// Baka Fighter event -> cue map, with per-event provenance. The page
    /// names events (`"hit"`, `"confirm"`, ...) and never hard-codes a cue id.
    pub fn baka_cues_json(&self) -> String {
        events_json(BAKA_EVENTS).to_string()
    }

    /// Tactical-arts event -> cue map (see [`ART_EVENTS`]).
    pub fn art_cues_json(&self) -> String {
        events_json(ART_EVENTS).to_string()
    }

    /// Resolve one event name to its cue id (`255` when the event is unknown -
    /// no real descriptor uses `0xFF`).
    pub fn cue_for_event(&self, table: &str, event: &str) -> u32 {
        let rows = match table {
            "arts" => ART_EVENTS,
            _ => BAKA_EVENTS,
        };
        rows.iter()
            .find(|(name, _, _, _)| *name == event)
            .map(|(_, id, _, _)| *id as u32)
            .unwrap_or(0xFF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baka_event_map_is_the_traced_ring_writes() {
        // The four ids the duel overlay writes into `_DAT_8007B6D8`.
        let disc_cues: Vec<u8> = BAKA_EVENTS
            .iter()
            .filter(|(_, _, s, _)| *s == Source::Disc)
            .map(|(_, id, _, _)| *id)
            .collect();
        for id in [CUE_HIT, CUE_CONFIRM, CUE_CURSOR, CUE_CANCEL] {
            assert!(disc_cues.contains(&id), "cue 0x{id:02X} is disc-sourced");
        }
        // Nothing outside that set may be labelled disc-sourced.
        for id in disc_cues {
            assert!(matches!(
                id,
                CUE_HIT | CUE_CONFIRM | CUE_CURSOR | CUE_CANCEL
            ));
        }
    }

    #[test]
    fn cue_lookup_by_event() {
        let s = LegaiaSfx::new();
        assert_eq!(s.cue_for_event("baka", "hit"), CUE_HIT as u32);
        assert_eq!(s.cue_for_event("baka", "tally"), CUE_CURSOR as u32);
        assert_eq!(s.cue_for_event("arts", "strike"), CUE_ART_STRIKE as u32);
        assert_eq!(s.cue_for_event("baka", "nope"), 0xFF);
    }

    #[test]
    fn every_site_cue_id_is_in_the_static_descriptor_range() {
        // The static table is ids 0x00..=0x63; a site cue outside it would
        // need the runtime `.dpk` bank instead.
        for id in site_cue_ids() {
            assert!(id <= 0x63, "cue 0x{id:02X} is a static descriptor");
        }
    }
}
