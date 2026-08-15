//! Delilas party swap: play as Gi / Lu / Che while the story's ravine
//! duels (and the Muscle Dome Master legs) field Vahn / Noa / Gala.
//!
//! A pure model-and-name identity swap over `legaia_asset::party_swap`:
//! each playable character's battle files rebuild around the mapped
//! sibling's model (their own animations, arts, stats and story are
//! untouched), each sibling's monster block rebuilds around the mapped
//! character's battle model (the Delilas movesets drive it - the duels
//! play exactly as before with the fighters exchanged), the monster
//! blocks are renamed to the characters they now depict, and the
//! new-game template names the party after the siblings. The mapping is
//! caller-chosen: any permutation of the three siblings over the three
//! characters.
//!
//! Field-map visuals swap too: PROT 0874's party field meshes + atlas
//! rebuild from the same monster models (`party_swap::fieldize`), so
//! walking around towns shows the same siblings the battles do.

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive;
use legaia_asset::new_game;
use legaia_asset::party_swap::{self, PlayerRig, fieldize, playerize, winpose};

/// PROT entry of `readef.DAT` (the battle side-band streaming slots).
pub const READEF_ENTRY: usize = 894;

use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// One Delilas sibling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sibling {
    Gi,
    Che,
    Lu,
}

impl Sibling {
    /// The sibling's monster-archive id.
    pub fn monster_id(self) -> u16 {
        match self {
            Sibling::Gi => 162,
            Sibling::Che => 163,
            Sibling::Lu => 164,
        }
    }

    /// The retail monster-block display name.
    pub fn retail_block_name(self) -> &'static str {
        match self {
            Sibling::Gi => "Gi Delilas",
            Sibling::Che => "Che Delilas",
            Sibling::Lu => "Lu Delilas",
        }
    }

    /// The party display name the sibling fights under.
    pub fn display_name(self) -> &'static str {
        match self {
            Sibling::Gi => "Gi",
            Sibling::Che => "Che",
            Sibling::Lu => "Lu",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gi" => Some(Sibling::Gi),
            "che" => Some(Sibling::Che),
            "lu" => Some(Sibling::Lu),
            _ => None,
        }
    }
}

/// Which sibling replaces each playable character. Always a permutation
/// of all three ([`PartyMapping::parse`] enforces it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartyMapping {
    pub vahn: Sibling,
    pub noa: Sibling,
    pub gala: Sibling,
}

impl Default for PartyMapping {
    /// Archetype-aligned: Gi (the leader) for Vahn, Lu for Noa, Che (the
    /// bruiser) for Gala.
    fn default() -> Self {
        PartyMapping {
            vahn: Sibling::Gi,
            noa: Sibling::Lu,
            gala: Sibling::Che,
        }
    }
}

impl PartyMapping {
    /// Parse `"gi,lu,che"`-style mappings: three comma-separated sibling
    /// names in Vahn, Noa, Gala order, each used exactly once.
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 3 {
            bail!("expected three comma-separated siblings (e.g. gi,lu,che)");
        }
        let mut siblings = Vec::with_capacity(3);
        for p in &parts {
            let sib = Sibling::parse(p)
                .ok_or_else(|| anyhow::anyhow!("unknown sibling {p:?} (gi / che / lu)"))?;
            if siblings.contains(&sib) {
                bail!("sibling {p:?} used twice - the mapping must be a permutation");
            }
            siblings.push(sib);
        }
        Ok(PartyMapping {
            vahn: siblings[0],
            noa: siblings[1],
            gala: siblings[2],
        })
    }

    /// `(player entry, rig, template slot, character name, sibling)` per
    /// playable character.
    pub fn pairs(&self) -> [(usize, &'static PlayerRig, usize, &'static str, Sibling); 3] {
        [
            (863, &party_swap::RIG_VAHN_GALA, 0, "Vahn", self.vahn),
            (864, &party_swap::RIG_NOA, 1, "Noa", self.noa),
            (865, &party_swap::RIG_VAHN_GALA, 2, "Gala", self.gala),
        ]
    }
}

/// Rename a decoded monster block's display name in place. The new name
/// (plus the retail `0x01` colour-escape prefix when present) must fit
/// the old string's byte span; the tail NUL-pads.
pub fn rename_block(block: &mut [u8], new_name: &str) -> Result<()> {
    let name_off = u32::from_le_bytes(
        block
            .get(0..4)
            .ok_or_else(|| anyhow::anyhow!("block too short"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let region = block
        .get_mut(name_off..)
        .ok_or_else(|| anyhow::anyhow!("name offset {name_off:#x} out of range"))?;
    let prefix = usize::from(region.first() == Some(&0x01));
    let old_len = region[prefix..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| anyhow::anyhow!("unterminated name string"))?;
    if new_name.len() > old_len {
        bail!(
            "name {new_name:?} ({} bytes) does not fit the {} -byte slot",
            new_name.len(),
            old_len
        );
    }
    region[prefix..prefix + old_len].fill(0);
    region[prefix..prefix + new_name.len()].copy_from_slice(new_name.as_bytes());
    Ok(())
}

/// Report of one [`apply_delilas_party`] run.
#[derive(Debug, Default)]
pub struct DelilasPartyReport {
    /// `false` when every pairing was already applied.
    pub changed: bool,
    /// Human-readable per-pair notes (scales, texture downscales).
    pub notes: Vec<String>,
}

/// Apply the party swap onto the disc: monster blocks re-skinned +
/// renamed, player battle files rebuilt, new-game template renamed.
/// Idempotent - a block already carrying its mapped character's name is
/// skipped whole; an unrecognized name (neither retail nor applied)
/// aborts before any write.
pub fn apply_delilas_party(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
) -> Result<DelilasPartyReport> {
    let mut report = DelilasPartyReport::default();
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read monster archive")?;
    // Retail player files for all three heroes, captured before the
    // model loop patches them: the signature-art anim retarget must run
    // against the same retail rest/mesh statistics the win-pose
    // conversion uses.
    let mut retail_players: Vec<Vec<u8>> = Vec::with_capacity(3);
    for (entry, _, _, who, _) in mapping.pairs() {
        retail_players.push(
            patcher
                .read_entry_footprint(entry)
                .with_context(|| format!("read retail {who} player file"))?,
        );
    }

    // Baseline pass before any write: every target block's name must be
    // its retail sibling name (fresh) or its mapped character's (already
    // applied).
    let mut fresh = Vec::new();
    for (entry, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let name = monster_archive::record(&archive, id)?
            .map(|r| r.name)
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?;
        if name == who {
            continue; // this pairing is already applied
        }
        if name != sibling.retail_block_name() {
            bail!(
                "monster id {id} is named {name:?} - neither retail \
                 ({:?}) nor swapped ({who:?}); refusing to touch an \
                 unrecognized build",
                sibling.retail_block_name()
            );
        }
        fresh.push(entry);
    }

    for (entry, rig, template_slot, who, sibling) in mapping.pairs() {
        if !fresh.contains(&entry) {
            continue;
        }
        let id = sibling.monster_id();
        let player_file = patcher
            .read_entry_footprint(entry)
            .with_context(|| format!("read player file PROT {entry}"))?;

        // Enemy side: the sibling's block wears the character's model
        // and name.
        let swapped = party_swap::swap_into_block(&player_file, rig, &archive, id)
            .with_context(|| format!("{who} -> monster {id}"))?;
        let mut block = swapped.block;
        rename_block(&mut block, who).with_context(|| format!("rename monster {id} to {who:?}"))?;
        let slot = monster_archive::encode_slot(&block)
            .with_context(|| format!("re-encode monster {id}"))?;
        patcher.patch_monster_slot(id, &slot)?;

        // Player side: the character wears the sibling's model.
        let entry_len = patcher
            .read_entry(entry)
            .with_context(|| format!("read PROT entry {entry}"))?
            .len();
        let playerized =
            playerize::playerize_player_file(&player_file, entry_len, rig, &archive, id)
                .with_context(|| format!("{who} <- monster {id}"))?;
        patcher.patch_prot_entry(entry, 0, &playerized.file)?;

        // Win poses: the character's eight base "ME" victory streams
        // (readef.DAT slot 3*char+2) rebuild from the sibling's own
        // victory clip, retargeted onto the player rig - the swapped
        // character celebrates like the Delilas they depict. Non-fatal:
        // a failed rebuild leaves the retail pose (with a note).
        match winpose::victory_clip(&archive, id).and_then(|clip| {
            let readef = patcher
                .read_entry_footprint(READEF_ENTRY)
                .context("read readef.DAT")?;
            let slot_idx = winpose::base_slot_index(template_slot);
            let off = slot_idx * winpose::READEF_SLOT;
            let slot = readef
                .get(off..off + winpose::READEF_SLOT)
                .ok_or_else(|| anyhow::anyhow!("readef slot {slot_idx} out of range"))?;
            let rebuilt = winpose::rebuild_base_slot(slot, &clip, rig, &player_file, &archive, id)?;
            patcher.patch_prot_entry(READEF_ENTRY, off as u64, &rebuilt)?;
            Ok(clip.action_id)
        }) {
            Ok(_) => report.notes.push(format!(
                "{who}: victory poses <- {}'s own victory clip",
                sibling.display_name()
            )),
            Err(e) => report
                .notes
                .push(format!("{who}: victory poses stay retail ({e:#})")),
        }

        // New-game template name (fixed 10-byte NUL-padded field; only
        // affects new games - existing saves keep their stored names).
        let scus = patcher
            .read_named_file(crate::steal::SCUS_NAME)
            .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
        let tmpl_off = new_game::party_template_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("starting-party template not found in SCUS"))?
            as u64;
        let name_off = tmpl_off + (template_slot * new_game::RECORD_STRIDE) as u64 + 16;
        let mut field = vec![0u8; new_game::NAME_LEN];
        field[..sibling.display_name().len()].copy_from_slice(sibling.display_name().as_bytes());
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, name_off, &field)
            .with_context(|| format!("write template name for slot {template_slot}"))?;

        report.changed = true;
        for w in swapped.warnings.iter().chain(playerized.warnings.iter()) {
            report
                .notes
                .push(format!("{who} <-> {}: {w}", sibling.display_name()));
        }
    }

    // Field forms: rebuild PROT 0874 so the party walks around as the
    // mapped siblings too (built from the same monster models as the
    // battle side, so both forms match). Runs only alongside a fresh
    // apply - an already-swapped 0874 must not re-convert.
    if report.changed {
        let field_entry = fieldize::PROT_ENTRY_INDEX;
        let prot_0874 = patcher
            .read_entry_footprint(field_entry)
            .context("read PROT 0874")?;
        let entry_len = patcher
            .read_entry(field_entry)
            .context("PROT 0874 length")?
            .len();
        let field_mapping = [
            mapping.vahn.monster_id(),
            mapping.noa.monster_id(),
            mapping.gala.monster_id(),
        ];
        // Preferred source: the siblings' own field NPC meshes (nilboa
        // duel scene) - retail-authored chibi geometry that fits the §0
        // budget at full detail. The battle-model conversion is the
        // fallback (it survives only via heavy decimation).
        let npc_pack = patcher.read_entry_footprint(fieldize::NPC_PACK_ENTRY)?;
        let npc_bundle = patcher.read_entry_footprint(fieldize::NPC_BUNDLE_ENTRY)?;
        let fieldized = fieldize::fieldize_pack_npc(
            &prot_0874,
            entry_len,
            &npc_pack,
            &npc_bundle,
            field_mapping,
        )
        .or_else(|npc_err| {
            report.notes.push(format!(
                "field: NPC-mesh source unavailable ({npc_err:#}); using battle-model conversion"
            ));
            fieldize::fieldize_pack(&prot_0874, entry_len, &archive, field_mapping)
        })
        .context("rebuild field forms (PROT 0874)")?;
        patcher.patch_prot_entry(field_entry, 0, &fieldized.entry)?;
        for w in &fieldized.warnings {
            report.notes.push(format!("field: {w}"));
        }

        // Battle-voice passes, in dependency order: every XA mute first,
        // then the XA + victory-clip fills (which SOURCE the siblings'
        // grunts from monster.snd), and the duel-bank splice LAST -
        // the splice overwrites the sibling banks with the heroes'
        // samples, so a fill that runs after it reads Vahn's voice back
        // out of Lu's bank and hands the "sibling" slots to the wrong
        // speaker.

        // Sibling XA victory lines - captured off the still-retail
        // reels BEFORE any mute below wipes them (XA21 mutes whole).
        let victory_lines = crate::delilas_xa_voice::capture_victory_lines(patcher, mapping);

        // Retail arts shouts, same read-before-mute law: the `adjusted`
        // arts-voice mode re-voices this audio toward the siblings.
        let hero_shouts = if arts_voice == crate::delilas_voice_fx::ArtsVoiceMode::Adjusted {
            crate::delilas_xa_voice::capture_hero_shouts(patcher)
        } else {
            crate::delilas_xa_voice::HeroShoutCapture {
                banks: Default::default(),
                fanfare: Default::default(),
                staged2: Default::default(),
            }
        };

        // The arts XA shout banks (XA2/XA4/XA6 - the character's VOICE
        // on arts, item use and other callouts) have no sibling
        // counterpart to splice (the Delilas only grunt), so hearing
        // Vahn shout out of Gi's body is worse than silence: mute the
        // swapped characters' banks. The cue still fires (routing
        // untouched); the sectors decode to silence, and the spliced
        // SPU grunts remain the audible voice.
        // `original` arts-voice mode keeps the retail shouts: skip the
        // mute entirely (the fill below leaves the banks untouched too).
        if arts_voice != crate::delilas_voice_fx::ArtsVoiceMode::Original {
            for (slot, file) in ["XA/XA2.XA", "XA/XA4.XA", "XA/XA6.XA"].iter().enumerate() {
                let who = ["Vahn", "Noa", "Gala"][slot];
                let n = patcher
                    .silence_xa_file(file)
                    .with_context(|| format!("mute {who} XA shout bank"))?;
                report
                    .notes
                    .push(format!("{who}: XA shout bank muted ({n} sectors)"));
            }
        }

        // The SECOND voice cue: `XA30.XA` carries the party's normal-move
        // grunt, one channel per character (Vahn 0, Noa 4, Gala 6 - see
        // docs/subsystems/battle-action.md "Battle voice cues"). The
        // battle-action input handler fires it on every ordinary swing -
        // and a tactical art IS a chain of swings, so with only XA2/4/6
        // muted the loudest Vahn line in an art still played. Mute the
        // three hero channels; every other channel in the bank survives.
        let n = patcher
            .silence_xa_channels("XA/XA30.XA", &[0, 4, 6])
            .context("mute party XA30 grunt channels")?;
        report.notes.push(format!(
            "party: XA30 grunt channels 0/4/6 muted ({n} sectors)"
        ));

        // The victory barks: the battle-event bark jukebox (the sound
        // command byte `gp+0x9F4` dispatch in `FUN_8004E568`) resolves
        // its char-keyed victory ids into `XA21.XA` - Vahn picks
        // randomly between ids 0x1A2/0x1A3 (channels 2/3), with 0x1A4 /
        // 0x1A6 / 0x1A7 (channels 4/6/7) as the sibling arms. The whole
        // file is short bark reels (7-22 s per channel); it mutes whole.
        // (`XA12.XA` is NOT touched: its only captured battle fire went
        // through the NON-voice jingle path - id 0x0B, whole-channel dur
        // - i.e. results music, not a hero line.)
        let n = patcher
            .silence_xa_file("XA/XA21.XA")
            .context("mute battle bark bank XA21")?;
        report
            .notes
            .push(format!("party: XA21 victory-bark bank muted ({n} sectors)"));

        // The jukebox's two outlying arms point INTO the music files:
        // id 0x19F = XA20 channel 7, id 0x1AF = XA22 channel 7 - the
        // close-call ("barely won") victory barks. Both channels are
        // short bark reels (12-17 s) interleaved beside 27-274 s music
        // channels; only channel 7 mutes, the music is untouched.
        for file in ["XA/XA20.XA", "XA/XA22.XA"] {
            let n = patcher
                .silence_xa_channels(file, &[7])
                .with_context(|| format!("mute {file} bark channel 7"))?;
            report
                .notes
                .push(format!("party: {file} bark channel 7 muted ({n} sectors)"));
        }

        // The FOURTH voice tier: the staged-event id space (id >= 0x100
        // through `FUN_8004FCC8`; the anim materialiser `FUN_8004AD80`
        // picks the id from an inline char-keyed table - Vahn 0x101,
        // Noa 0x111, Gala 0x121). Two 8-channel banks per hero:
        // Vahn = XA1 + XA27, Noa = XA3 + XA28, Gala = XA5 + XA29.
        //
        // These are NOT bare voice lines: XA1/3/5 are the Hyper / Super
        // / Miracle **fanfare** banks and XA27/28/29 the Seru-magic
        // fanfare streams - stereo cue beds carrying the hero's voice
        // over a jingle. They follow `arts_voice` for the same reason
        // the shout banks do, and `Original` leaves them alone entirely:
        // a Hyper Art fires no shout from the XA2/4/6 pool, so muting
        // its fanfare is the whole difference between a cue and silence.
        if arts_voice != crate::delilas_voice_fx::ArtsVoiceMode::Original {
            for (who, file) in [
                ("Vahn", "XA/XA1.XA"),
                ("Vahn", "XA/XA27.XA"),
                ("Noa", "XA/XA3.XA"),
                ("Noa", "XA/XA28.XA"),
                ("Gala", "XA/XA5.XA"),
                ("Gala", "XA/XA29.XA"),
            ] {
                let n = patcher
                    .silence_xa_file(file)
                    .with_context(|| format!("mute {who} staged-event bank {file}"))?;
                report
                    .notes
                    .push(format!("{who}: {file} fanfare bank muted ({n} sectors)"));
            }
        }

        // Then give the silenced slots the siblings' REAL voices: their
        // monster.snd grunts, XA-encoded over the muted channels. Must
        // run after EVERY mute above (a later whole-file mute would
        // erase the fill) and before the duel-bank splice below (which
        // replaces the sibling banks' samples with the heroes').
        let notes = crate::delilas_xa_voice::fill_hero_xa_voices(
            patcher,
            mapping,
            &victory_lines,
            arts_voice,
            &hero_shouts,
        )
        .context("fill hero XA voice slots with sibling grunts")?;
        report.notes.extend(notes);

        // The FIFTH voice tier, and the one every XA sweep is blind to:
        // the ordinary victory pose's voice is an SPU sample streamed
        // from `monster.snd`'s own sector TOC (`FUN_8003e104`; pose
        // action -> clip byte via the SCUS tables at 0x800788A0 /
        // 0x80078867). Replace the heroes' clip bands with the mapped
        // siblings' own victory lines, re-pitched to their recorded
        // rates - verbatim SPU-ADPCM, same file.
        let notes =
            crate::delilas_xa_voice::fill_hero_victory_clips(patcher, mapping, &victory_lines)
                .context("fill hero victory-voice clips in monster.snd")?;
        report.notes.extend(notes);

        // Battle voices: the party grunts like the mapped siblings.
        // LAST of the voice passes - this swaps the heroes' samples
        // INTO the sibling banks, so any pass sourcing "the sibling's
        // voice" from monster.snd after this point reads the wrong
        // speaker.
        let notes = crate::delilas_voice::splice_party_voices(patcher, mapping)
            .context("splice party battle voices")?;
        report.notes.extend(notes);

        // The signature-special reskin, once per hero slot (name +
        // combo + the sibling's own clip as the staged animation + the
        // fanfare duration to cover the soundtrack the fills above
        // wrote).
        for (_, rig, slot, who, sibling) in mapping.pairs() {
            let notes = reskin_signature_art(
                patcher,
                slot,
                sibling,
                rig,
                &retail_players[slot],
                &archive,
                &victory_lines,
            )
            .with_context(|| format!("reskin the {who}-slot signature art"))?;
            report.notes.extend(notes);
        }
    }
    Ok(report)
}

/// The sibling's signature special, as the disc spells it. All three
/// are 13 characters, which is what lets the rename be an in-place
/// same-length write over a host art of the same width.
fn signature_name(sibling: Sibling) -> &'static [u8] {
    match sibling {
        Sibling::Gi => b"Blazing Slash",
        Sibling::Che => b"Megaton Press",
        Sibling::Lu => b"Plasma Strike",
    }
}

/// The host Hyper art a hero slot gives up to carry the sibling's
/// signature special.
struct HostArt {
    /// Retail name - must be the same byte length as [`signature_name`],
    /// and the rename hits every occurrence in SCUS.
    retail_name: &'static [u8],
    /// Index among the character's non-Miracle arts.
    index: u8,
    /// The art's action constant - the key its fanfare cue is selected
    /// by ([`legaia_art::hyper_fanfare::HYPER_FANFARES`]).
    action_constant: u8,
    /// The replacement 5-input combo, checked unique against the
    /// character's other arts before anything is written.
    combo: [legaia_art::queue::Command; 5],
}

/// Which art each hero slot (0 Vahn / 1 Noa / 2 Gala) gives up.
fn host_art(slot: usize) -> Option<HostArt> {
    use legaia_art::queue::Command::{Down, Left, Right};
    match slot {
        0 => Some(HostArt {
            retail_name: b"Burning Flare",
            index: 1,
            action_constant: 0x1C,
            combo: [Left, Right, Left, Right, Down],
        }),
        _ => None,
    }
}

/// The fanfare row the slot's host art fires through. The cue is a coin
/// flip between a PAIR of channels of the character's own fanfare bank
/// (`XA1`/`XA3`/`XA5`), and which pair is per-art - so the sibling's
/// special soundtrack has to be written to that art's pair, not to a
/// fixed one.
fn signature_fanfare(slot: usize) -> Option<legaia_art::hyper_fanfare::HyperFanfare> {
    let art = host_art(slot)?;
    legaia_art::hyper_fanfare::HYPER_FANFARES
        .iter()
        .find(|f| f.cslot as usize == slot && f.action_constant == art.action_constant)
        .copied()
}

/// The two fanfare-bank channels the slot's signature art plays through.
pub(crate) fn signature_fanfare_channels(slot: usize) -> Option<(u8, u8)> {
    signature_fanfare(slot).map(|f| f.channel_pair())
}

/// The [`legaia_art::queue::Character`] a hero slot names.
fn slot_character(slot: usize) -> legaia_art::queue::Character {
    use legaia_art::queue::Character;
    match slot {
        0 => Character::Vahn,
        1 => Character::Noa,
        _ => Character::Gala,
    }
}

/// Reskin one hero slot's Hyper art as the sibling mapped onto it.
///
/// Four coordinated edits: a same-length name swap in the SCUS
/// arts-name table (menu + battle banner), a fresh 5-input combo
/// written to both copies retail keeps in sync (the SCUS display glyphs
/// and the player-file record0 matcher), the sibling's own monster clip
/// retargeted onto the player rig into the host art's "ME" stream (host
/// rate byte halved so a clip resampled into the host's shorter stream
/// keeps its authored pace), and the fanfare duration table extended so
/// the sibling's soundtrack - where one was captured - plays to
/// completion.
///
/// Must run while record0 still holds the VANILLA combo bytes (the
/// playerize rebuild keeps record0 verbatim, so ordering after it is
/// fine).
fn reskin_signature_art(
    patcher: &mut DiscPatcher,
    slot: usize,
    sibling: Sibling,
    rig: &party_swap::PlayerRig,
    retail_player: &[u8],
    archive: &[u8],
    lines: &crate::delilas_xa_voice::VictoryLines,
) -> Result<Vec<String>> {
    use legaia_art::arts_table;
    use legaia_art::queue::Command;
    let who = ["Vahn", "Noa", "Gala"][slot];
    let character = slot_character(slot);
    let Some(art) = host_art(slot) else {
        return Ok(vec![format!(
            "{}'s signature art: no {who}-slot host art wired yet (skipped)",
            sibling.display_name()
        )]);
    };
    let mut notes = Vec::new();

    // 1. Name: same-length swap everywhere the string appears.
    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .context("read SCUS for the art rename")?;
    let old = art.retail_name;
    let new = signature_name(sibling);
    if old.len() != new.len() {
        bail!(
            "{who}-slot rename is not same-length: {} -> {}",
            String::from_utf8_lossy(old),
            String::from_utf8_lossy(new)
        );
    }
    let mut hits = Vec::new();
    let mut at = 0usize;
    while let Some(pos) = scus[at..].windows(old.len()).position(|w| w == old) {
        hits.push(at + pos);
        at += pos + 1;
    }
    if hits.is_empty() {
        bail!(
            "SCUS carries no '{}' string to rename",
            String::from_utf8_lossy(old)
        );
    }
    for &off in &hits {
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, off as u64, new)
            .context("write art name")?;
    }
    notes.push(format!(
        "art renamed: {} -> {} ({}x)",
        String::from_utf8_lossy(old),
        String::from_utf8_lossy(new),
        hits.len()
    ));

    // 2. Combo: fresh 5-input sequence, checked unique among the
    // character's own arts.
    let edits =
        crate::arts::ArtsEdits::locate(patcher.image()).context("locate arts-name table")?;
    let target = edits
        .records()
        .iter()
        .find(|r| r.character == character && r.index == art.index && !r.is_miracle)
        .cloned()
        .with_context(|| {
            format!(
                "{who} art index {} ({}) not found",
                art.index,
                String::from_utf8_lossy(old)
            )
        })?;
    let new_combo: Vec<Command> = art.combo.to_vec();
    for r in edits.records() {
        if r.character == character && r.cmd_ptr != target.cmd_ptr && r.commands == new_combo {
            bail!(
                "the {who}-slot combo collides with {who} art index {}",
                r.index
            );
        }
    }
    let layout = arts_table::combo_string_layout(&scus, target.cmd_ptr)
        .with_context(|| format!("decode {} combo layout", String::from_utf8_lossy(old)))?;
    let plan = vec![crate::arts::ComboEdit {
        cmd_ptr: target.cmd_ptr,
        direction_slots: layout.direction_slots.clone(),
        old_directions: layout.directions.clone(),
        new_directions: new_combo.clone(),
    }];
    // Matcher first (player record0), then the display glyphs. The
    // host bank record is found by its VANILLA combo bytes; its rate
    // byte (entry +0x78) halves 2 -> 1 in the SAME record0 rewrite, so
    // the shorter host stream carrying the sibling's longer clip plays
    // at her authored pace instead of double speed.
    use legaia_asset::battle_char_assembly;
    let char_edits = edits.player_edits(&plan, character);
    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;
    let host = char_edits
        .first()
        .and_then(|(vanilla, _)| {
            bank.iter()
                .find(|r| !r.uses_base_archive() && r.combo == *vanilla)
        })
        .cloned();
    let mut offset_edits = Vec::new();
    if let Some(h) = &host {
        offset_edits.push((h.entry_offset + 0x78, 1u8));
    }
    if (!char_edits.is_empty() || !offset_edits.is_empty())
        && let Some((lzs_off, recompressed)) =
            crate::arts::patch_player_record0_full(&entry, &char_edits, &offset_edits)
    {
        patcher
            .patch_prot_entry(index, lzs_off as u64, &recompressed)
            .context("write player record0 combo matcher + anim rate")?;
    }
    for (off, bytes) in edits.glyph_patches(&plan) {
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, off, &bytes)
            .context("write combo display glyph")?;
    }
    notes.push(format!(
        "{} combo: {} (was {})",
        String::from_utf8_lossy(new),
        combo_str(&new_combo),
        combo_str(&target.commands)
    ));

    // 3. The sibling's own animation: the highest-energy command-band
    // clip of their monster archive, retargeted onto the player rig
    // into the host art's "ME" stream at its retail (parts, frames)
    // shape - the art record's timing/effect/cue fields stay valid.
    // Non-fatal: a failed rebuild leaves the host animation (with a
    // note).
    let source_id = sibling.monster_id();
    match host
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("host art bank record not found by vanilla combo"))
        .and_then(|h| {
            let clip = monster_archive::animations(archive, source_id)?
                .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no animations"))?
                .into_iter()
                .filter(|a| {
                    (0x0C..=0x1F).contains(&a.action_id)
                        && a.part_count == party_swap::CANONICAL_PARTS
                })
                .max_by_key(|a| a.frame_count)
                .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no attack clip"))?;
            let readef = patcher
                .read_entry_footprint(READEF_ENTRY)
                .context("read readef.DAT")?;
            let slot_idx = battle_char_assembly::art_me_slot(slot, false);
            let off = slot_idx * winpose::READEF_SLOT;
            let slot = readef
                .get(off..off + winpose::READEF_SLOT)
                .ok_or_else(|| anyhow::anyhow!("readef art slot {slot_idx} out of range"))?;
            let rebuilt = winpose::rebuild_art_slot_entry(
                slot,
                h.stream_source as usize,
                &clip,
                rig,
                retail_player,
                archive,
                source_id,
            )?;
            patcher.patch_prot_entry(READEF_ENTRY, off as u64, &rebuilt)?;
            Ok(clip.action_id)
        }) {
        Ok(tag) => notes.push(format!(
            "{} animation: {}'s own clip (tag 0x{tag:02X}) on the player rig",
            String::from_utf8_lossy(new),
            sibling.display_name()
        )),
        Err(e) => notes.push(format!(
            "{} animation stays the host's ({e:#})",
            String::from_utf8_lossy(new)
        )),
    }

    // 4. Fanfare duration: the cue's read span must cover the excerpt
    // the fills wrote into the host art's own channel pair or the audio
    // cuts early. The table is indexed by jingle id - 0x100, so the two
    // rows to widen are the art's `base_id` pair, NOT a fixed {4, 7}
    // (that pair is Burning Flare's; every art has its own). Measured
    // against retail (every id's table entry vs its channel's own
    // length, across 24 ids): the entry is CENTISECONDS of that
    // channel's audio - `entry ~= secs * 100`, so `dur = entry * 0.6`
    // is a 60 Hz tick budget, not the 75-sectors/s physical span an
    // earlier reading assumed (which over-ran every write by 25%).
    // Skipped for a sibling with no captured soundtrack.
    if let Some(secs) = lines.special_secs(slot)
        && let Some(fanfare) = signature_fanfare(slot)
    {
        let toff = legaia_art::hyper_fanfare::dur_table_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("fanfare duration table not found in SCUS"))?;
        let entry_val = ((secs * 100.0).ceil() as u16).to_le_bytes();
        let base = (fanfare.base_id - 0x100) as usize;
        for n in [base, base + 3] {
            patcher
                .patch_named_file(crate::arts::SCUS_NAME, (toff + n * 2) as u64, &entry_val)
                .context("write fanfare duration")?;
        }
        notes.push(format!(
            "{} fanfare duration: {secs:.1} s",
            String::from_utf8_lossy(new)
        ));
    }
    Ok(notes)
}

/// A combo as the game prints it: `L R L R D`.
fn combo_str(cmds: &[legaia_art::queue::Command]) -> String {
    use legaia_art::queue::Command;
    cmds.iter()
        .map(|c| match c {
            Command::Up => "U",
            Command::Down => "D",
            Command::Left => "L",
            Command::Right => "R",
        })
        .collect::<Vec<_>>()
        .join(" ")
}
