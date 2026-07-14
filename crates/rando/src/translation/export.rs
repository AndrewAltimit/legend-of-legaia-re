//! Export: walk a user-supplied disc and build the source language pack.
//!
//! Coverage (see `docs/tooling/translation.md` for the map):
//!
//! - `SCUS_942.54` name pools: item names + shared item-type strings
//!   (`legaia_asset::item_names`), spell names (`legaia_asset::spell_names`),
//!   Tactical Arts names (`legaia_art::arts_table`), accessory passive
//!   names + descriptions (`legaia_asset::accessory_passive`), and the
//!   new-game party name fields (`legaia_asset::new_game`). Strings are
//!   pointer-addressed and deduplicated by target VA; interior pointers
//!   (another table slot pointing into a string's span) clamp the budget.
//! - Scene dialog: `0x1F`-lead segments in every scene-bundle MAN
//!   (LZS-decompressed; keys carry the decompressed-domain offset).
//! - Inline text: the same segments in raw PROT carriers (v12 event-script
//!   prescripts, streaming-MAN dungeon scenes), scanned across every PROT
//!   entry with the quality gate in [`super::segments`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};

use legaia_asset::scene_asset_table;
use legaia_asset::{accessory_passive, item_names, new_game, spell_names};

use crate::disc::DiscPatcher;

use super::markup;
use super::pack::{Entry, LanguagePack};
use super::segments;

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// Longest SCUS string we will follow before deciding a pointer is bogus.
const MAX_SCUS_STRLEN: usize = 512;

/// A located scene-bundle MAN, shared by export and import: the compressed
/// stream's placement inside the PROT entry plus the decompressed bytes.
pub struct SceneManText {
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Length of the compressed stream on disc (what the repack overwrites).
    pub compressed_len: usize,
    /// Bytes the recompressed MAN must fit within (descriptor-boundary
    /// budget - see [`crate::man_compressed_budget`]).
    pub compressed_budget: usize,
    /// Decompressed MAN (dialog edits mutate this in place, same-size).
    pub decoded: Vec<u8>,
}

impl SceneManText {
    /// Locate a scene bundle's MAN, or `None` if the entry isn't a scene
    /// bundle / has no MAN / the stream doesn't decode.
    pub fn locate(entry: &[u8]) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()?;
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        Some(Self {
            man_offset,
            compressed_len: consumed,
            compressed_budget: crate::man_compressed_budget(&table, man_offset, entry.len()),
            decoded,
        })
    }

    /// Byte range the compressed stream occupies in the PROT entry. Text found
    /// *inside* it by the raw scanner is a coincidence of the LZS bytes, not a
    /// string: writing there would corrupt the stream, and a MAN repack moves
    /// those bytes anyway. Export excludes the range.
    pub fn compressed_span(&self) -> std::ops::Range<usize> {
        self.man_offset..self.man_offset + self.compressed_len
    }

    /// Recompress the (mutated) MAN; `None` if it would overflow the
    /// original compressed footprint.
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// Collects `(string VA, contexts)` per section, then materializes entries
/// with interior-pointer-aware budgets. Deduplication is **global**: several
/// tables point into one string pool (e.g. Tactical Arts are also named in
/// the spell-id space), so a VA is owned by the first section that claims it
/// and later claims only merge their context. One VA = one entry = one write.
struct ScusCollector<'a> {
    scus: &'a [u8],
    /// VA -> (owning section, joined context).
    strings: BTreeMap<u32, (&'static str, String)>,
}

impl<'a> ScusCollector<'a> {
    fn new(scus: &'a [u8]) -> Self {
        Self {
            scus,
            strings: BTreeMap::new(),
        }
    }

    fn add(&mut self, section: &'static str, va: u32, context: String) {
        let (_, slot) = self.strings.entry(va).or_insert((section, String::new()));
        if slot.is_empty() {
            *slot = context;
        } else if !slot.contains(&context) && slot.len() < 120 {
            slot.push_str(", ");
            slot.push_str(&context);
        }
    }

    /// Raw string bytes at `va` (up to, not including, the NUL). `None` for
    /// unmappable pointers, missing terminators, or empty strings.
    fn read_str(&self, va: u32) -> Option<Vec<u8>> {
        let off = item_names::file_offset_for_va(self.scus, va)?;
        let tail = self.scus.get(off..)?;
        let len = tail
            .iter()
            .take(MAX_SCUS_STRLEN)
            .position(|&b| b == 0)
            .filter(|&l| l > 0)?;
        Some(tail[..len].to_vec())
    }

    /// Bytes of **alignment padding** usable past this string's terminator.
    ///
    /// The name pools are 4-byte aligned, so a string of length `n` occupies
    /// `align4(n + 1)` bytes and the 0..3 bytes after its NUL are zero filler
    /// (verified per string, not assumed: the run must actually be zeros).
    /// Those bytes are dead - nothing reads past a terminator - so a
    /// translation may spill into them as long as it re-terminates, which buys
    /// the tight name tables an extra ~1.5 bytes on average. The window never
    /// crosses another pointed-to string: any collected VA inside it clamps
    /// the run, and `all_vas` includes the VAs of empty strings (a pointer to
    /// a bare NUL), which is the only thing that could legally live in there.
    fn padding_slack(&self, va: u32, strlen: usize, all_vas: &BTreeSet<u32>) -> usize {
        let Some(off) = item_names::file_offset_for_va(self.scus, va) else {
            return 0;
        };
        let end = off + strlen; // the NUL
        let aligned = (end + 4) & !3; // first offset past the terminator, 4-aligned
        let mut u = end + 1;
        while u < aligned
            && self.scus.get(u) == Some(&0)
            && !all_vas.contains(&(va + (u - off) as u32))
        {
            u += 1;
        }
        u - 1 - end
    }

    /// Materialize one section's entries (the VAs it owns), clamping each
    /// budget at the nearest interior pointer (any collected VA that lands
    /// inside the string's span - overwriting past it would corrupt the
    /// other slot's string) and widening it across the zero alignment padding
    /// that follows the terminator (see [`Self::padding_slack`]).
    fn entries_for(&self, section: &str, all_vas: &BTreeSet<u32>) -> Vec<Entry> {
        let mut out = Vec::new();
        for (&va, (owner, context)) in &self.strings {
            if *owner != section {
                continue;
            }
            let Some(bytes) = self.read_str(va) else {
                continue;
            };
            let strlen = bytes.len();
            let interior = all_vas
                .range(va + 1..va + strlen as u32 + 1)
                .next()
                .map(|&v| (v - va - 1) as usize);
            // An interior pointer means another slot's string starts inside
            // this one's span: the budget stops there, and there is no padding
            // to reclaim (the pool continues).
            let budget = match interior {
                Some(clamped) => clamped,
                None => strlen + self.padding_slack(va, strlen, all_vas),
            };
            if budget == 0 {
                continue;
            }
            out.push(Entry {
                key: format!("scus:str:0x{va:08x}"),
                context: context.clone(),
                source: markup::decode(&bytes),
                translation: String::new(),
                budget,
            });
        }
        out
    }

    fn all_vas(&self) -> BTreeSet<u32> {
        self.strings.keys().copied().collect()
    }
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

/// Pointer word at `va` inside the loaded data segment.
fn ptr_at(scus: &[u8], va: u32) -> Option<u32> {
    read_u32(scus, item_names::file_offset_for_va(scus, va)?)
}

fn collect_scus_sections(scus: &[u8], pack: &mut LanguagePack) -> Result<()> {
    let mut col = ScusCollector::new(scus);

    // Item names + the shared type strings (record = [name_ptr][type_ptr][meta]).
    let items = item_names::ItemNameTable::from_scus(scus);
    for id in 0..=255u8 {
        if let Some((_, ptr)) = item_names::name_ptr_slot(scus, id)
            && ptr != 0
        {
            col.add("items", ptr, format!("item 0x{id:02x}"));
        }
        if let Some(tp) = ptr_at(scus, item_names::TABLE_VA + id as u32 * 12 + 4)
            && tp != 0
        {
            let name = items
                .as_ref()
                .and_then(|t| t.name(id))
                .unwrap_or("?")
                .to_string();
            col.add("item_types", tp, format!("type of 0x{id:02x} {name}"));
        }
    }

    // Spell names (stats record +8 = display-name pointer).
    let spells = spell_names::SpellNameTable::from_scus(scus);
    for id in 0..=255u16 {
        let va = spell_names::STATS_VA + id as u32 * spell_names::RECORD_STRIDE as u32 + 8;
        if let Some(ptr) = ptr_at(scus, va)
            && ptr != 0
        {
            let name = spells
                .as_ref()
                .and_then(|t| t.name(id as u8))
                .unwrap_or("?")
                .to_string();
            col.add("spells", ptr, format!("spell 0x{id:02x} {name}"));
        }
    }

    // Tactical Arts names (record +0xC = name pointer).
    if let Some(arts) = legaia_art::arts_table::parse_from_scus(scus) {
        for (i, art) in arts.iter().enumerate() {
            let va = legaia_art::arts_table::TABLE_VA
                + (i * legaia_art::arts_table::RECORD_STRIDE) as u32
                + 0xC;
            if let Some(ptr) = ptr_at(scus, va)
                && ptr != 0
            {
                col.add("arts", ptr, format!("art '{}'", art.name));
            }
        }
    }

    // Accessory passive names (+4) and descriptions (+8).
    for idx in 0..accessory_passive::PASSIVE_COUNT as u32 {
        let rec =
            accessory_passive::PASSIVE_TABLE_VA + idx * accessory_passive::PASSIVE_STRIDE as u32;
        if let Some(ptr) = ptr_at(scus, rec + 4)
            && ptr != 0
        {
            col.add(
                "accessory_passives",
                ptr,
                format!("passive 0x{idx:02x} name"),
            );
        }
        if let Some(ptr) = ptr_at(scus, rec + 8)
            && ptr != 0
        {
            col.add(
                "accessory_passives",
                ptr,
                format!("passive 0x{idx:02x} description ('|' = line break)"),
            );
        }
    }

    let all = col.all_vas();
    pack.sections.items = col.entries_for("items", &all);
    pack.sections.item_types = col.entries_for("item_types", &all);
    pack.sections.spells = col.entries_for("spells", &all);
    pack.sections.arts = col.entries_for("arts", &all);
    pack.sections.accessory_passives = col.entries_for("accessory_passives", &all);

    // New-game party names: fixed 10-byte NUL-padded fields, not pointers.
    for n in 0..new_game::PARTY_RECORDS {
        let va = new_game::PARTY_TEMPLATE_VA + (n * new_game::RECORD_STRIDE) as u32 + 16;
        let Some(off) = item_names::file_offset_for_va(scus, va) else {
            continue;
        };
        let Some(field) = scus.get(off..off + new_game::NAME_LEN) else {
            continue;
        };
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        if len == 0 {
            continue;
        }
        pack.sections.party_names.push(Entry {
            key: format!("scus:party:{n}"),
            context: format!("new-game roster slot {n} (10-byte field)"),
            source: markup::decode(&field[..len]),
            translation: String::new(),
            budget: new_game::NAME_LEN - 1,
        });
    }
    Ok(())
}

/// Per-PROT-entry scan length: the entry's footprint clamped to the next
/// entry's start, so overlapping extended footprints don't export the same
/// disc bytes twice under two keys.
fn clamped_entry_lens(patcher: &DiscPatcher) -> Vec<usize> {
    let n = patcher.entry_count();
    let lbas: BTreeSet<u32> = (0..n).filter_map(|i| patcher.entry_disc_lba(i)).collect();
    (0..n)
        .map(|i| {
            let (Some(lba), Some(size)) = (patcher.entry_disc_lba(i), patcher.entry_footprint(i))
            else {
                return 0;
            };
            let cap = lbas
                .range(lba + 1..)
                .next()
                .map(|&next| (next - lba) as usize * 2048)
                .unwrap_or(usize::MAX);
            (size as usize).min(cap)
        })
        .collect()
}

/// Export the full language pack from an opened disc.
pub fn export_pack(patcher: &DiscPatcher) -> Result<LanguagePack> {
    let mut pack = LanguagePack::new("en");
    pack.notes = "Source export - fill `translation:` fields (markup: printable ASCII, \
                  {xx}/{xx:yy} byte escapes, '|' = newline glyph) and run \
                  `legaia-rando translate import`."
        .to_string();

    let scus = patcher
        .read_named_file("SCUS_942.54")
        .context("SCUS_942.54 not found in disc image")?;
    collect_scus_sections(&scus, &mut pack)?;

    let cdname = patcher.cdname();
    let scene_of = |idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for_extraction_index(m, idx as u32))
            .unwrap_or("?")
            .to_string()
    };

    let mut seen_lba = BTreeSet::new();
    for (idx, &clamped_len) in clamped_entry_lens(patcher).iter().enumerate() {
        let Some(lba) = patcher.entry_disc_lba(idx) else {
            continue;
        };
        if !seen_lba.insert(lba) {
            continue; // duplicate TOC entry over the same disc bytes
        }
        let Ok(mut entry) = patcher.read_entry(idx) else {
            continue;
        };
        entry.truncate(clamped_len);
        let scene = scene_of(idx);

        // Scene-bundle MAN (LZS domain).
        let man = SceneManText::locate(&entry);
        if let Some(man) = &man {
            for seg in segments::scan(&man.decoded) {
                let text = &man.decoded[seg.text_off..seg.text_off + seg.len];
                pack.sections.scene_dialog.push(Entry {
                    key: format!("man:{idx}:0x{off:x}", off = seg.text_off),
                    context: scene.clone(),
                    source: markup::decode(text),
                    translation: String::new(),
                    budget: seg.len,
                });
            }
        }

        // Raw carriers (v12 prescripts, streaming MANs, ...), skipping anything
        // the scanner found inside this entry's compressed MAN stream - those
        // "strings" are LZS bytes, not text.
        let compressed = man.as_ref().map(|m| m.compressed_span());
        for seg in segments::scan(&entry) {
            if compressed
                .as_ref()
                .is_some_and(|c| c.contains(&seg.text_off))
            {
                continue;
            }
            let text = &entry[seg.text_off..seg.text_off + seg.len];
            pack.sections.inline_text.push(Entry {
                key: format!("raw:{idx}:0x{off:x}", off = seg.text_off),
                context: scene.clone(),
                source: markup::decode(text),
                translation: String::new(),
                budget: seg.len,
            });
        }
    }
    Ok(pack)
}
