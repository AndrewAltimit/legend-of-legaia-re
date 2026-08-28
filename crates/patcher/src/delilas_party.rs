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

/// PROT entry of the raw battle-action overlay (base VA `0x801CE818`) -
/// where the per-character attack-camera jump tables and the
/// per-character element table live.
const BATTLE_OVERLAY_ENTRY: usize = 898;

use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// How much of the swapped hero's Tactical-Arts kit becomes the
/// sibling's.
///
/// See [`apply_delilas_moveset`] for what `Delilas` rebuilds and
/// [`retained_bank_rows`] for why the host arts it keeps cannot be
/// dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelilasMoveMode {
    /// Every art keeps the host hero's animation; only the one
    /// reskinned Hyper plays the sibling's signature special.
    #[default]
    Hybrid,
    /// The hero's whole art-stream archive is rebuilt from the
    /// sibling's own motions, the arts that survive are renamed after
    /// the clip each plays, and every art the Supers and the Miracle do
    /// not need is blanked out of the matcher.
    Delilas,
}

impl std::str::FromStr for DelilasMoveMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hybrid" => Ok(Self::Hybrid),
            "delilas" => Ok(Self::Delilas),
            other => Err(format!(
                "unknown Delilas move mode {other:?} (expected hybrid or delilas)"
            )),
        }
    }
}

impl std::fmt::Display for DelilasMoveMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Hybrid => "hybrid",
            Self::Delilas => "delilas",
        })
    }
}

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
/// Whether the signature cast route may claim the SCUS injection arena.
///
/// The arena is shared with `--shiny-seru`, `--show-super-arts` and
/// `--arts-ap-grant`/`--arts-ap-cost`; when any of those is enabled the
/// FRONTEND passes [`CastRoutePolicy::ArenaTaken`] and the route downgrades
/// to the art-side signature up front - order-independently, leaving the
/// player files and cast module untouched - instead of racing the other
/// feature for the bytes (one apply order used to hard-error the whole
/// patch, the other silently shipped a half-installed route).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastRoutePolicy {
    /// No arena-claiming feature is enabled: install the cast route.
    Install,
    /// An arena feature is enabled: keep the art-side signature, say so.
    ArenaTaken,
}

/// Per-run visual options of the party swap, off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DelilasPartyOptions {
    /// Keep Che's welded hammer-fist on the swapped mesh (his authored
    /// giant hammer) instead of mirroring the other fist in; the host's
    /// own weapon is then NOT fused into his hand. Clips that assume a
    /// hand-sized part swing the hammer wide - the comparison trade the
    /// flag opts into. Che only: Gi's welded blade-fist stays replaced
    /// (its reach is what caused the catalogued Spirit-charge streak).
    pub keep_che_hammer: bool,
}

pub fn apply_delilas_party(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
    move_mode: DelilasMoveMode,
    cast_route: CastRoutePolicy,
) -> Result<DelilasPartyReport> {
    apply_delilas_party_with(
        patcher,
        mapping,
        arts_voice,
        move_mode,
        cast_route,
        DelilasPartyOptions::default(),
    )
}

/// [`apply_delilas_party`] with [`DelilasPartyOptions`] explicit.
pub fn apply_delilas_party_with(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
    move_mode: DelilasMoveMode,
    cast_route: CastRoutePolicy,
    options: DelilasPartyOptions,
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
    // readef.DAT before any pass rewrites its ME slots: the enemy-side
    // anim mirror retargets the heroes' own clips (incl. the base-ME
    // victory flourish) into the swapped monster blocks.
    let retail_readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read retail readef.DAT")?;

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
        let keep_hammer = options.keep_che_hammer && id == Sibling::Che.monster_id();
        let playerized = playerize::playerize_player_file_with(
            &player_file,
            entry_len,
            rig,
            &archive,
            id,
            template_slot,
            Some(&patcher.read_entry_footprint(READEF_ENTRY)?),
            keep_hammer,
        )
        .with_context(|| format!("{who} <- monster {id}"))?;
        if keep_hammer {
            report
                .notes
                .push(format!("{who}: Che's welded hammer kept on the mesh"));
        }
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
            let rebuilt = winpose::rebuild_base_slot(
                slot,
                &clip,
                rig,
                &player_file,
                &archive,
                id,
                party_swap::playerize::kept_welded_hand(id, keep_hammer),
            )?;
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
        // The element each slot now fights in. First of the post-model
        // passes because it is pure identity - it decides what every
        // attack of that slot deals and takes, not just the signature
        // art's, and nothing below reads it.
        report
            .notes
            .extend(retarget_character_elements(patcher, mapping, &archive)?);

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

        // The nilboa duel scene's own Delilas NPC meshes become the
        // mapped heroes (PROT 0639 members 106/107/108 + the 0638 head
        // TIMs), so the ravine no longer shows two Delilas sets. Uses
        // the pre-fieldize PROT 0874 capture above - load-bearing: the
        // rewritten 0874 carries siblings, not heroes.
        let nivora = crate::nivora_field::apply_nivora_field(patcher, mapping, &prot_0874)
            .context("nilboa field mirror")?;
        report.notes.extend(nivora.notes);

        // The remaining Delilas event appearances (map stone, floating
        // castle, past Conkram) mirror the same way - their sibling
        // meshes live inside each scene's bundle instead of a separate
        // pack. Same pre-fieldize PROT 0874 dependency.
        let events = crate::nivora_field::apply_event_field(patcher, mapping, &prot_0874)
            .context("event-scene field mirrors")?;
        report.notes.extend(events.notes);

        // Battle-voice passes, in dependency order: every XA mute first,
        // then the XA + victory-clip fills (which SOURCE the siblings'
        // grunts from monster.snd), and the duel-bank splice LAST -
        // the splice overwrites the sibling banks with the heroes'
        // samples, so a fill that runs after it reads Vahn's voice back
        // out of Lu's bank and hands the "sibling" slots to the wrong
        // speaker.

        // The Plasma Strike bed's intro remaster runs FIRST: it edits
        // the same XA20 channel the special-cue capture below excerpts,
        // so ordering the boost ahead of the capture makes the spliced
        // fanfare open audibly too (the pass-order law).
        match crate::delilas_xa_voice::boost_cast_bed_intro(patcher) {
            Ok(true) => report
                .notes
                .push("cast bed: Plasma Strike music advanced to open with the walk".into()),
            Ok(false) => {}
            Err(e) => report
                .notes
                .push(format!("cast bed: intro remaster skipped ({e:#})")),
        }

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
        // The transplanted-burst cave holds exactly ONE record (88 bytes
        // between prototype ids 37 and 39 - the battle overlay is packed
        // to the byte), so the first hero slot claims it and the other
        // two keep the borrowed cast projectile.
        let mut cave_taken = false;
        for (_, rig, slot, who, sibling) in mapping.pairs() {
            let ctx = SignatureCtx {
                slot,
                sibling,
                rig,
                retail_player: &retail_players[slot],
                archive: &archive,
                natural_wrist_hand: party_swap::playerize::kept_welded_hand(
                    sibling.monster_id(),
                    options.keep_che_hammer && sibling == Sibling::Che,
                ),
            };
            let notes = reskin_signature_art(patcher, &ctx, &mut cave_taken)
                .with_context(|| format!("reskin the {who}-slot signature art"))?;
            report.notes.extend(notes);

            // The rest of the kit, when the caller asked for it. Runs
            // last per slot: it carries the signature stream the reskin
            // just authored into the archive it re-authors, so it has
            // to see that pass's output.
            if move_mode == DelilasMoveMode::Delilas {
                match apply_delilas_moveset(patcher, &ctx) {
                    Ok(notes) => report.notes.extend(notes),
                    Err(e) => report
                        .notes
                        .push(format!("{who} moves: stay the host's ({e:#})")),
                }
            }

            // Always last per slot: the charge-loop aliasing guard reads
            // the archive as it will ship, whichever move mode built it.
            match clamp_charge_loop_windows(patcher, slot, who) {
                Ok(notes) => report.notes.extend(notes),
                Err(e) => report
                    .notes
                    .push(format!("{who} charge-loop guard skipped ({e:#})")),
            }
        }

        // The cast route: the mapped sibling's signature plays the RETAIL
        // enemy cast module (camera track, effect barrage, multi-hit
        // build-up) instead of the art-side approximation - Blazing
        // Slash (spell 0x79, PROT 958), Megaton Press (0x7A, 959) and
        // Plasma Strike (0x7B, 960), each module carrying its own
        // damage-retarget + wipe-skip + staged-walk fold edit set in
        // `crate::delilas_cast`. Claims the SCUS injection gap, so it
        // composes with neither --shiny-seru nor --show-super-arts - on
        // a conflict the note says so and the art-side signature stays.
        let routes: Vec<crate::delilas_cast::CastRoute> = mapping
            .pairs()
            .iter()
            .filter_map(|&(_, _, slot, _, sibling)| {
                host_art(slot).map(|art| crate::delilas_cast::CastRoute {
                    char_index: slot as u8,
                    art_constant: art.action_constant,
                    spell_id: signature_spell_id(sibling),
                })
            })
            .collect();
        if !routes.is_empty() && cast_route == CastRoutePolicy::ArenaTaken {
            report.notes.push(
                "cast route: art-side signature kept (shiny-seru / show-super-arts / \
                 arts-ap own the SCUS injection arena this run; no cast edits applied)"
                    .to_string(),
            );
        } else if !routes.is_empty() {
            // The CASTER's own body animation: author real staged rows
            // (the sibling's wind-up + payoff on player rows 0x0A/0x0B,
            // Block re-homed to row 0x06 across every player file) so
            // each module's folded stage walk delivers real clips. When
            // the rewrite cannot land, fall back to the probe-proven
            // Che-only shape: pin 959's stages to the empty row 0x0A
            // (caster holds a pose; the enemy-side Megaton also loses
            // its smash stage) and keep Gi/Lu on the art-side reskin.
            let module_name = |spell: u8| match spell {
                0x79 => "Blazing Slash (958)",
                0x7A => "Megaton Press (959)",
                _ => "Plasma Strike (960)",
            };
            match author_staged_cast_rows(patcher, mapping, &retail_players, &archive, &options) {
                Ok(authored) => {
                    report.notes.extend(authored.notes);
                    let gi = authored.gi_unfold.as_deref();
                    let lu = authored.lu_unfold.as_deref();
                    let installed = crate::delilas_cast::patch_module_959(patcher, false)
                        .and_then(|_| crate::delilas_cast::patch_module_958(patcher, gi))
                        .and_then(|_| crate::delilas_cast::patch_module_960(patcher, lu))
                        .and_then(|_| crate::delilas_cast::install_cast_hook(patcher, &routes))
                        .and_then(|_| crate::delilas_cast::install_stage_caves(patcher, gi, lu))
                        .and_then(|_| crate::delilas_cast::install_delilas_arena(patcher))
                        .and_then(|_| crate::delilas_cast::install_strike_morph(patcher))
                        .and_then(|_| crate::delilas_cast::install_cast_label_gate(patcher))
                        .and_then(|_| crate::delilas_cast::install_chain_admission_tier(patcher));
                    match installed {
                        Ok(_) => {
                            report.notes.push(
                                "cast label: the special's spell name replaces the arts banner \
                                 (state-0x28 label runs for every Magic cast)"
                                    .into(),
                            );
                            report.notes.push(
                                "chain admission: Hyper tier 10 -> 5 per arrow, so an \
                                 art-then-special chain clears the matcher's AP gate \
                                 from ~40 AP (the special itself admits at 25)"
                                    .into(),
                            );
                            for r in &routes {
                                let walk = match r.spell_id {
                                    0x79 if gi.is_some() => " (un-folded retail walk)",
                                    0x7B if lu.is_some() => " (un-folded retail walk)",
                                    0x7A => "",
                                    _ => " (folded walk)",
                                };
                                report.notes.push(format!(
                                    "cast route: slot {} signature runs the retail {} module{}",
                                    r.char_index,
                                    module_name(r.spell_id),
                                    walk
                                ));
                            }
                        }
                        Err(e) => report
                            .notes
                            .push(format!("cast route: art-side signature kept ({e:#})")),
                    }
                }
                Err(e) => {
                    report.notes.push(format!(
                        "cast route: caster rows stay pinned to the held pose ({e:#})"
                    ));
                    let che_routes: Vec<crate::delilas_cast::CastRoute> = routes
                        .iter()
                        .filter(|r| r.spell_id == 0x7A)
                        .copied()
                        .collect();
                    let installed = crate::delilas_cast::patch_module_959(patcher, true)
                        .and_then(|_| crate::delilas_cast::install_cast_hook(patcher, &che_routes))
                        .and_then(|_| crate::delilas_cast::install_delilas_arena(patcher))
                        .and_then(|_| crate::delilas_cast::install_strike_morph(patcher))
                        .and_then(|_| crate::delilas_cast::install_cast_label_gate(patcher))
                        .and_then(|_| crate::delilas_cast::install_chain_admission_tier(patcher));
                    match installed {
                        Ok(_) => {
                            report.notes.push(
                                "cast label: the special's spell name replaces the arts banner \
                                 (state-0x28 label runs for every Magic cast)"
                                    .into(),
                            );
                            report.notes.push(
                                "chain admission: Hyper tier 10 -> 5 per arrow, so an \
                                 art-then-special chain clears the matcher's AP gate \
                                 from ~40 AP (the special itself admits at 25)"
                                    .into(),
                            );
                            for r in &che_routes {
                                report.notes.push(format!(
                                    "cast route: slot {} signature runs the retail {} module \
                                     (pinned pose); other slots keep the art-side signature",
                                    r.char_index,
                                    module_name(r.spell_id)
                                ));
                            }
                        }
                        Err(e) => report
                            .notes
                            .push(format!("cast route: art-side signature kept ({e:#})")),
                    }
                }
            }
        }

        // Enemy-side anim mirror, LAST: the swapped duel blocks fight
        // with the mapped hero's own clips (idle / walk / reactions /
        // swings, plus the hero's 50-AP Hyper across the cast module's
        // staged entries). Runs after every pass that touches the
        // monster slots, the player files or readef; all its inputs are
        // the pre-patch captures above.
        let retail = crate::enemy_anim_mirror::RetailSources {
            archive: &archive,
            players: [&retail_players[0], &retail_players[1], &retail_players[2]],
            readef: &retail_readef,
        };
        report
            .notes
            .extend(crate::enemy_anim_mirror::apply_enemy_anim_mirror(
                patcher, mapping, &retail,
            )?);
    }
    Ok(report)
}

/// Give each hero slot the mapped sibling's own **element**.
///
/// The battle overlay's per-character element table
/// (`0x801F5480`, one byte per 1-based character id;
/// `legaia_asset::element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET`) is the
/// only per-character element on the disc, and retail seeds it Vahn = fire,
/// Noa = wind, Gala = thunder. Two routines index it, both with
/// `DAT_8007BD10[actor] - 1` (the slot's active member id), and both use the
/// result as a row/column of the affinity matrix `0x801F53E8`:
/// `FUN_801DD864` (`0x801dd8ac` / `0x801dd900`) and the hit kernel
/// `FUN_801EC3E4` (`0x801ecf38` attacker, `0x801ecf94` defender). So the
/// table decides what element every one of that slot's attacks *deals* and
/// what it *takes* - and until this runs, a swapped party fights in the
/// hero's element: Lu's Plasma Strike lands as fire out of Vahn's slot and
/// Che's Megaton Press as thunder out of Gala's.
///
/// The replacement is not a choice - each sibling's monster record already
/// carries their own element at `+0x1D` (the same byte `FUN_801EC3E4` reads
/// for an enemy attacker at `0x801ecf68`), and the three read Gi = fire,
/// Che = earth, Lu = thunder. Taken from the archive image captured
/// **before** the model loop, so a re-skinned block cannot feed it back.
///
/// This is the whole character, not just the signature art: retail has no
/// per-art element, so a Ra-Seru cast and a basic swing scale through the
/// same byte.
fn retarget_character_elements(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    archive: &[u8],
) -> Result<Vec<String>> {
    use legaia_asset::element_affinity as ea;
    let mut notes = Vec::new();
    for (_, _, slot, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let element = monster_archive::record(archive, id)
            .with_context(|| format!("read monster {id} for its element"))?
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?
            .element;
        let name = ea::Element::from_id(element)
            .map(|e| e.name())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} carries element id {element}, outside the {}-element space",
                    sibling.display_name(),
                    ea::ELEMENT_COUNT
                )
            })?;
        // The table is 1-based on character id and the party slots are
        // characters 1..=3, so the slot index IS the table index.
        let off = ea::CHARACTER_ELEMENTS_FILE_OFFSET + slot;
        patcher
            .patch_prot_entry(BATTLE_OVERLAY_ENTRY, off as u64, &[element])
            .with_context(|| format!("write the {who}-slot element"))?;
        notes.push(format!(
            "{who} element: {} ({}'s own)",
            name,
            sibling.display_name()
        ));
    }
    Ok(notes)
}

// ---------------------------------------------------------------------------
// Delilas move mode: the whole art kit re-animated from the sibling's
// own clips.
// ---------------------------------------------------------------------------

/// Bank row the arts matcher starts each scan at, and the Miracle Art's
/// own record.
///
/// `FUN_801EED1C` seeds its row cursor `li s3, 0xb` (`0x801EF2EC`) and
/// abandons the whole scan when the bank's record count is `<= 0x0B`
/// (`0x801EF2F4`-`0x801EF2FC`), so rows below 11 are never matched. Row
/// 11 is additionally the Miracle Art: the substitution path branches to
/// the wholesale queue overwrite from `0x801F64F4` only while the
/// rows-visited counter is still zero (`0x801EF4D8`-`0x801EF4E0`), i.e.
/// only on this first row - and reading the disc confirms it, row 11
/// carrying `RDLULURDL` / `LURDULUDR` / `RRDUDUDLL`, the three Miracle
/// combos the SCUS arts table flags.
const MIRACLE_BANK_ROW: usize = 0x0B;

/// Queue action constant of bank row `row`: the matcher writes
/// `s3 + 0x10` (`0x801EF63C` single-record arts, `0x801EF610` the
/// multi-record form), so the two spaces differ by a constant.
const ART_CONSTANT_BASE: usize = 0x10;

/// VA of the per-character **innate art cap** the learn-on-use gate
/// reads (`FUN_801EFBFC`, `0x801EFD0C`-`0x801EFD18`): an art id is only
/// self-taught when `cap < id`. Reads `[3, 5, 3]` on the USA disc, which
/// is exactly each character's Hyper-Art block - those are granted by a
/// script instead (the `+0x74E` insert at `0x80041FB4` in SCUS
/// `FUN_800402F4`), so blanking their combos would list an art that can
/// never fire.
const INNATE_ART_CAP_VA: u32 = 0x801F_686C;

/// Record-image span of an art record's zero-terminated combo. `+0x0A`
/// is the stream index and `+0x0B..+0x0D` size the matcher's row stride,
/// so blanking must stop short of them.
const COMBO_FIELD: std::ops::Range<usize> = 0..9;

/// Record-image span of the inline art name (`+0x10`, NUL-padded, ends
/// where the embedded action entry begins).
const NAME_FIELD: std::ops::Range<usize> = 0x10..0x24;

/// Bank rows whose art constant appears in one of the character's Super
/// Art `find` patterns.
///
/// A Super is not entered as a combo - `FUN_801EF9E4` walks the
/// **finished** action queue at `actor[+0x1DF]` and tail-matches it
/// against the resident trigger table, so a Super can only fire if every
/// regular art of its `find` string reached that queue, and the only
/// writer that puts an art constant there is the combo matcher. Blanking
/// one of these rows would silently cost the Super.
fn super_critical_rows(
    character: legaia_art::queue::Character,
) -> std::collections::BTreeSet<usize> {
    legaia_art::super_art::SUPER_ARTS
        .iter()
        .filter(|s| s.character == character)
        .flat_map(|s| s.art_sequence())
        .filter_map(|c| (c as usize).checked_sub(ART_CONSTANT_BASE))
        .collect()
}

/// The innate cap byte for a party slot, read off the battle overlay.
fn innate_art_cap(overlay: &[u8], slot: usize) -> Result<u8> {
    let off = (INNATE_ART_CAP_VA - BATTLE_OVERLAY_BASE) as usize + slot;
    overlay
        .get(off)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("innate art cap for slot {slot} is past the overlay"))
}

/// Which bank rows keep a working combo under [`DelilasMoveMode::Delilas`].
///
/// Four reasons a row survives, and only the first is about taste:
///
/// - the **signature host** row, which now carries the sibling's special;
/// - row 11, the **Miracle Art** - its combo is the only thing that
///   reaches the wholesale queue overwrite;
/// - every row a **Super Art** trigger names ([`super_critical_rows`]);
/// - every row whose art id is at or below the **innate cap**, because
///   those are script-granted and blanking them would leave a listed art
///   that can never be performed.
///
/// Everything else is blanked, and blanking hides it for free: an art is
/// only listed once `FUN_801EFBFC` has inserted it at char record
/// `+0x185` on a successful performance, and a blanked combo can never
/// be performed. The load-bearing reason is the `combo_len == 1` guard at
/// `0x801EF424`, which abandons a one-input match outright - a blanked
/// combo is zero-terminated at byte 0, so it can only ever complete at
/// length 1. That guard is retail's own mechanism for the same job: the
/// Super and Miracle **finisher** rows all carry a single-`D` combo and
/// are unreachable for exactly this reason. The weaker argument - that
/// the `token - 0x0B` compare at `0x801EF3EC` needs a `0x0B` queue token
/// and `0x0B` is `BlockAnim`, not an input - is a second line of defence
/// and was not proved exhaustively over every queue writer.
fn retained_bank_rows(
    character: legaia_art::queue::Character,
    cap: u8,
    host_row: usize,
    bank_len: usize,
) -> std::collections::BTreeSet<usize> {
    let mut keep = super_critical_rows(character);
    keep.insert(MIRACLE_BANK_ROW);
    keep.insert(host_row);
    for row in MIRACLE_BANK_ROW..bank_len {
        if (row - MIRACLE_BANK_ROW) <= cap as usize {
            keep.insert(row);
        }
    }
    keep.retain(|&r| r < bank_len);
    keep
}

/// Menu labels for the sibling's swing clips, in the archive order
/// [`legaia_asset::party_swap::moveset::swing_entries`] returns.
///
/// [`LABEL_MAX`] bytes at most, for every sibling. The SCUS arts-name
/// field is rewritten in place over the retail string plus its measured
/// NUL padding, the tightest field any retained art carries is Vahn's
/// "Cyclone", and the mapping is a free permutation - so a label sized
/// against the slot its sibling usually lands in would silently keep the
/// retail name under a rearranged party.
/// Longest menu label that fits every retained art's name field on the
/// USA disc. Vahn's "Cyclone" is the binding one: seven string bytes and
/// one byte of NUL padding, and a replacement needs one of those for its
/// own terminator.
const LABEL_MAX: usize = 7;

fn swing_labels(sibling: Sibling) -> &'static [&'static str] {
    match sibling {
        Sibling::Gi => &["Gi Cut", "Gi Chop", "Gi Ram", "Gi Rush"],
        Sibling::Che => &["Che Ram", "Che Jab", "Che Hit", "Che Arm"],
        Sibling::Lu => &["Lu Bolt", "Lu Zap", "Lu Jolt", "Lu Volt"],
    }
}

/// Clamp the base-archive loop windows to the stream the loop actually
/// addresses at runtime - the "Spirit streak" guard.
///
/// The Spirit charge loop is the base-archive record `0x11`: loop count
/// `+0x84 = 0xFF` over window `[+0x85, +0x86)`, stream source `0`. At
/// commit the runtime materializes the stream by decoding entry
/// `stream_source` out of whichever readef archive is RESIDENT in the
/// side-band streaming buffer (`FUN_8002b28c(_DAT_8007BD74, ..)`), and it
/// routinely commits with the MAIN archive resident - measured live on
/// retail and on a patched disc alike. The loop window then addresses rows
/// of the MAIN archive's entry `stream_source` up to `+0x86 - 1`,
/// regardless of that stream's real frame count. A row past the decoded
/// body reads virgin materialize scratch - all zeros - and an all-zero
/// pose row collapses every part onto the model origin, which the charge
/// close-up camera sits on: the GTE near-projection smears the hand /
/// fused-weapon prims across the screen. Retail dodges it only when its
/// scratch happens to hold sane stale rows there.
///
/// The guard clamps `+0x86` to the aliased main entry's frame count (and
/// `+0x85` under it), so no phantom row is ever addressed; the decoder's
/// own `frame == +0x86 - 1` arm then routes the interpolation partner to
/// `+0x85`, so the last in-window frame never reads one-past-the-end
/// either. The frames clamped away are duplicates of the held charge
/// pose, so the charge looks identical when the correct base-archive
/// stream is resident.
fn clamp_charge_loop_windows(
    patcher: &mut DiscPatcher,
    slot: usize,
    who: &str,
) -> Result<Vec<String>> {
    use legaia_asset::battle_char_assembly;
    use legaia_asset::party_swap::moveset;

    let character = slot_character(slot);
    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_off = battle_char_assembly::art_me_slot(slot, false) * winpose::READEF_SLOT;
    let me = readef
        .get(me_off..me_off + winpose::READEF_SLOT)
        .ok_or_else(|| anyhow::anyhow!("readef art slot for {who} out of range"))?;
    let main_frames = moveset::entry_frames(me).context("read the main stream frame counts")?;

    let mut edits: Vec<(usize, u8)> = Vec::new();
    let mut notes = Vec::new();
    for rec in &bank {
        // The Spirit charge loop only. The other base records share the
        // aliasing exposure in principle, but none has been observed to
        // materialize mis-resident, and clamping them would degrade their
        // real loop holds; the charge's clamped-away frames are duplicates
        // of the held pose, so it alone is free to guard.
        if !rec.uses_base_archive() || rec.anim_id != 0x11 {
            continue;
        }
        let Some(&aliased) = main_frames.get(rec.stream_source as usize) else {
            continue;
        };
        let cap = aliased.min(u8::MAX as usize) as u8;
        let e = rec.entry_offset;
        let (Some(&lo), Some(&hi)) = (rec0.get(e + 0x85), rec0.get(e + 0x86)) else {
            continue;
        };
        if rec.rate_alt == 0 || hi == 0 || cap < 2 || hi <= cap {
            continue;
        }
        let new_lo = lo.min(cap - 1);
        edits.push((e + 0x85, new_lo));
        edits.push((e + 0x86, cap));
        notes.push(format!(
            "{who} anim {:#04x}: charge-loop window [{lo}, {hi}) clamped to \
             [{new_lo}, {cap}) - the aliased main stream {} carries {aliased} rows",
            rec.anim_id, rec.stream_source
        ));
    }
    if edits.is_empty() {
        return Ok(notes);
    }
    let (lzs_off, recompressed) = crate::arts::patch_player_record0_full(&entry, &[], &edits)
        .ok_or_else(|| {
            anyhow::anyhow!("{who}'s record0 will not fit with the charge-loop guard applied")
        })?;
    patcher
        .patch_prot_entry(index, lzs_off as u64, &recompressed)
        .context("write the charge-loop guarded art bank")?;
    Ok(notes)
}

/// Rebuild one hero slot's whole art kit around the mapped sibling.
///
/// Runs after [`reskin_signature_art`], and depends on it: the signature
/// stream it built is carried into the new archive byte-identical, so
/// every frame-indexed field that pass tuned stays valid.
///
/// Four coordinated edits, all same-size:
///
/// 1. the main `"ME"` slot is re-authored from the sibling's motions
///    ([`legaia_asset::party_swap::moveset`]) - the retail streams are
///    dropped, which is the only way Noa's slot (2446 free bytes) has
///    room for anything new;
/// 2. every art record that reads that archive is repointed at one of
///    the new streams and re-timed to its rate, with the record's
///    frame-indexed hit list and effect-script gates rescaled from the
///    stream it used to read;
/// 3. the host's impact-effect class and mid-clip loop hold are cleared
///    (both are keyed to choreography that no longer exists), and every
///    inline record name becomes the label of the clip it now plays -
///    which is also what makes the block fit, since a handful of
///    repeated strings compress far better than 22 distinct ones;
/// 4. the arts outside [`retained_bank_rows`] have their combos blanked.
fn apply_delilas_moveset(patcher: &mut DiscPatcher, ctx: &SignatureCtx<'_>) -> Result<Vec<String>> {
    use legaia_asset::battle_char_assembly;
    use legaia_asset::party_swap::moveset;

    let &SignatureCtx {
        slot,
        sibling,
        rig,
        retail_player,
        archive,
        natural_wrist_hand,
        ..
    } = ctx;
    let who = ["Vahn", "Noa", "Gala"][slot];
    let character = slot_character(slot);
    let mut notes = Vec::new();

    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;

    // The signature row, found by the combo the reskin just wrote.
    let host_combo: Vec<u8> = host_art(slot)
        .ok_or_else(|| anyhow::anyhow!("no {who}-slot host art"))?
        .combo
        .iter()
        .map(|c| c.as_byte())
        .collect();
    let host = bank
        .iter()
        .find(|r| !r.uses_base_archive() && r.combo == host_combo)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{who}'s signature combo is not in the art bank - the reskin did not land"
            )
        })?;
    let host_row = host.index;
    let signature_stream = host.stream_source as usize;

    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay")?;
    let cap = innate_art_cap(&overlay, slot)?;
    let keep = retained_bank_rows(character, cap, host_row, bank.len());

    // Re-author the stream archive from the sibling's own clips.
    let source_id = sibling.monster_id();
    let anims = moveset::sibling_animations(archive, source_id)?;
    let chain = signature_clip_chain(sibling);
    let swings = moveset::swing_entries(&anims, chain);
    let approach = moveset::approach_entry(&anims);
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_off = battle_char_assembly::art_me_slot(slot, false) * winpose::READEF_SLOT;
    let me = readef
        .get(me_off..me_off + winpose::READEF_SLOT)
        .ok_or_else(|| anyhow::anyhow!("readef art slot for {who} out of range"))?;
    let old_frames = moveset::entry_frames(me).context("read the retail stream frame counts")?;
    let rebuilt = moveset::rebuild_moveset_archive(
        me,
        signature_stream,
        &anims,
        approach,
        &swings,
        rig,
        retail_player,
        archive,
        source_id,
        natural_wrist_hand,
    )
    .with_context(|| {
        format!(
            "rebuild {who}'s art streams from {}",
            sibling.display_name()
        )
    })?;

    // Repoint / re-time / rename every record that reads that archive.
    // Nothing is written until BOTH halves are known to fit: a rebuilt
    // archive whose records still hold their retail stream indices would
    // send most of them past the end of it.
    let labels = swing_labels(sibling);
    let mut offset_edits: Vec<(usize, u8)> = Vec::new();
    let mut assignments: Vec<(usize, usize, &'static str)> = Vec::new();
    let mut blanked = Vec::new();
    let mut nth = 0usize;
    for rec in &bank {
        if rec.uses_base_archive() {
            continue;
        }
        let record_off = rec.entry_offset - battle_char_assembly::ART_ENTRY_OFFSET;
        if rec.index == host_row {
            // The signature keeps the stream the reskin authored; only
            // its index moved.
            offset_edits.push((record_off + 0x0A, rebuilt.signature as u8));
            continue;
        }
        if rec.index < MIRACLE_BANK_ROW {
            // The combo starters: the sibling's locomotion clip, which is
            // what a step-in before a chain wants.
            let stream = &rebuilt.streams[rebuilt.approach];
            offset_edits.push((record_off + 0x0A, rebuilt.approach as u8));
            offset_edits.push((rec.entry_offset + 0x78, stream.rate));
            continue;
        }
        let swing = rebuilt.swing_for(nth);
        let label = labels[(nth % rebuilt.swings.len()).min(labels.len() - 1)];
        nth += 1;
        let stream = &rebuilt.streams[swing];
        let from = old_frames
            .get(rec.stream_source as usize)
            .copied()
            .unwrap_or(0);
        offset_edits.push((record_off + 0x0A, swing as u8));
        offset_edits.push((rec.entry_offset + 0x78, stream.rate));
        // The host's element spark / afterimage tint, and the mid-clip
        // loop hold - both keyed to choreography that is gone.
        offset_edits.push((rec.entry_offset + IMPACT_CLASS_OFFSET, 0));
        for k in [0x84usize, 0x85, 0x86] {
            offset_edits.push((rec.entry_offset + k, 0));
        }
        // Frame-indexed fields, rescaled from the stream the record used
        // to read onto the one it reads now.
        for i in 0..4 {
            let f = rec.effect_script.get(0x10 + i).copied().unwrap_or(0);
            if f != 0 {
                offset_edits.push((
                    rec.entry_offset + 0x10 + i,
                    rescale_frame(f, from, stream.frames),
                ));
            }
        }
        for i in 0..FX_RECORDS {
            let at = FX_BASE + i * FX_RECORD;
            let gate = rec.effect_script.get(at).copied().unwrap_or(0);
            if gate != 0 {
                offset_edits.push((
                    rec.entry_offset + at,
                    rescale_frame(gate, from, stream.frames),
                ));
            }
        }
        // The inline name: the clip's label, NUL-padded over the retail
        // string. Repetition here is what buys the LZS margin.
        let mut field = label.as_bytes().to_vec();
        field.resize(NAME_FIELD.len(), 0);
        offset_edits.extend(
            field
                .iter()
                .enumerate()
                .map(|(i, &b)| (record_off + NAME_FIELD.start + i, b)),
        );
        // A retail row whose combo is a single direction is already
        // unmatchable (`0x801EF424` rejects a one-input match outright)
        // - those are the Super and Miracle finisher rows. Blanking them
        // too is free and compresses, but only a real multi-input art
        // counts as one this mode hid.
        let is_art = rec.combo.len() >= 2;
        if !keep.contains(&rec.index) {
            offset_edits.extend(COMBO_FIELD.map(|i| (record_off + i, 0u8)));
            if is_art {
                blanked.push(rec.index);
            }
        } else if is_art {
            assignments.push((rec.index, swing, label));
        }
    }

    let (lzs_off, recompressed) =
        crate::arts::patch_player_record0_full(&entry, &[], &offset_edits).ok_or_else(|| {
            anyhow::anyhow!(
                "{who}'s record0 will not fit its LZS footprint with the Delilas \
                 moveset applied"
            )
        })?;
    let region = crate::arts::record0_lzs_region(&entry)
        .ok_or_else(|| anyhow::anyhow!("{who} record0 LZS region"))?;

    // Both halves fit - commit them together.
    patcher
        .patch_prot_entry(READEF_ENTRY, me_off as u64, &rebuilt.bytes)
        .context("write the rebuilt art-stream archive")?;
    patcher
        .patch_prot_entry(index, lzs_off as u64, &recompressed)
        .context("write the repointed art bank")?;
    notes.push(format!(
        "{who} moves: {} stream(s) rebuilt from {}'s own clips - the signature, \
         the approach and {} swing(s) - {} B of {} used",
        rebuilt.streams.len(),
        sibling.display_name(),
        rebuilt.swings.len(),
        rebuilt.used,
        winpose::READEF_SLOT
    ));
    notes.push(format!(
        "{who} record0: {} B of {} used ({} B spare)",
        recompressed.len(),
        region.avail,
        region.avail - recompressed.len()
    ));

    // The menu side: each surviving art is named after the clip it plays.
    match rename_retained_arts(patcher, character, &assignments) {
        Ok((n, 0)) => notes.push(format!("{who} art names: {n} renamed after their clip")),
        Ok((n, skipped)) => notes.push(format!(
            "{who} art names: {n} renamed after their clip, {skipped} too tight \
             for a {LABEL_MAX}-byte label and left retail"
        )),
        Err(e) => notes.push(format!("{who} art names: left retail ({e:#})")),
    }
    notes.push(format!(
        "{who} arts: {} performable (the signature, the Miracle, {} Super component(s) \
         and the innate block below cap {cap}), {} blanked out of the matcher",
        assignments.len() + 1,
        super_critical_rows(character).len(),
        blanked.len()
    ));
    let mut per_clip: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, _, label) in &assignments {
        *per_clip.entry(label).or_default() += 1;
    }
    notes.push(format!(
        "{who} clips: {}",
        per_clip
            .iter()
            .map(|(l, n)| format!("{l} x{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(notes)
}

/// Rewrite the SCUS arts-name string of each retained art to the label of
/// the sibling clip it plays. Same-size, in place, through each record's
/// own name pointer; a label that will not fit its field is skipped.
fn rename_retained_arts(
    patcher: &mut DiscPatcher,
    character: legaia_art::queue::Character,
    assignments: &[(usize, usize, &'static str)],
) -> Result<(usize, usize)> {
    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
    let records = legaia_art::arts_table::raw_records_from_scus(&scus)
        .ok_or_else(|| anyhow::anyhow!("arts-name table not parseable"))?;
    let mut written = std::collections::BTreeSet::new();
    let mut n = 0usize;
    let mut skipped = 0usize;
    for &(row, _, label) in assignments {
        let id = row - MIRACLE_BANK_ROW;
        let Some(rec) = records
            .iter()
            .find(|r| r.character == character && !r.is_miracle && r.index as usize == id)
        else {
            continue;
        };
        let Some(field) = legaia_art::arts_table::name_field(&scus, rec.record_file_offset) else {
            continue;
        };
        // The field is the string plus its measured NUL padding, and a
        // replacement needs one byte of that for its own terminator - so
        // a label may be longer than the retail name it covers. A name
        // string can also be shared between records; the first
        // assignment in bank order wins, so the write stays deterministic.
        if !written.insert(field.file_offset) {
            continue;
        }
        if label.len() + 1 > field.budget {
            skipped += 1;
            continue;
        }
        let mut bytes = label.as_bytes().to_vec();
        bytes.resize(field.budget, 0);
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, field.file_offset as u64, &bytes)
            .context("write a retained art's name")?;
        n += 1;
    }
    Ok((n, skipped))
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
    /// A better-choreographed camera arm to take, when the host art's
    /// own has nothing to re-time.
    ///
    /// Taken as a **swap**, not a retarget: every arm in the dispatcher
    /// is already live in some character's table, so pointing this art
    /// at another one would alias an arm a second art still uses, and
    /// [`retime_camera_arm`] would then follow the alias and mistune
    /// that art too. The slots that held the borrowed arm therefore take
    /// this art's own in exchange - no art loses its camera, both
    /// cameras are still retail, and the borrowed arm ends up reachable
    /// from exactly one slot, which is what makes re-timing it safe.
    ///
    /// `None` where the host art's own arm is already cursor-gated and
    /// so has choreography worth re-timing - see [`host_art`].
    camera_swap: Option<u32>,
}

/// Which art each hero slot (0 Vahn / 1 Noa / 2 Gala) gives up: each
/// character's 50-AP Hyper.
///
/// They are the only three that clear every gate at once. The combo has
/// to be the same LENGTH as the one it replaces (`player_edits` drops a
/// mismatched edit and `glyph_patches` zips slot-for-slot), which by
/// itself rules out every 3- and 4-input Hyper. Of what is left, Noa's
/// Hurricane Kick is disqualified twice over - three bank records carry
/// its combo, and its ME stream is shared with a Super Art - and the
/// remaining candidates share a combo STRING across characters
/// (`0x80014198` is both Vahn's Tornado Flame and Gala's Thunder Punch,
/// so rewriting one rewrites the other's menu glyphs).
///
/// The combo must be free of every other art of that character **as a
/// substring at any offset**, which is a much stronger condition than
/// not being equal to one. The retail matcher (`FUN_801EED1C`, the
/// arrow-to-art normalisation) walks the scan START index DOWNWARD from
/// 15 and takes the first art that matches anywhere, so **a match
/// starting later in the input beats a longer match starting earlier**,
/// regardless of art length or bank order. An ordinary art also does not
/// consume its run - only its last direction becomes the action
/// constant, the leading N-1 stay in the queue and remain matchable.
///
/// `L R L R D` was the first choice and it fails on Gala: his Battering
/// Ram is `L R D`, which sits at offset 2, so it matches three passes
/// before the host art gets a look, does not consume, and leaves `L R`
/// for Back Punch - two arts fire and the signature never does. Vahn
/// survived the same mistake only by luck: his collision (`L R L`, Hyper
/// Elbow) is at offset 0, and the host being a Hyper consumes all five
/// inputs before the ordinary row is reached.
///
/// `L R U U D` occurs at no offset in any bank record of any of the
/// three (405 of the 1024 five-input combos are substring-free on all
/// three; the alternates `L R R R D`, `U R U R D`, `L D U R D` are
/// equally clean).
fn host_art(slot: usize) -> Option<HostArt> {
    use legaia_art::queue::Command::{Down, Left, Right, Up};
    let combo = [Left, Right, Up, Up, Down];
    match slot {
        0 => Some(HostArt {
            retail_name: b"Burning Flare",
            index: 1,
            action_constant: 0x1C,
            combo,
            // Burning Flare's own arm (`0x801D7650`) reads no cursor at
            // all: one static framing for the entire swing. That is why
            // this slot reads flat, and it is also why there is nothing
            // here to re-time. It is not unique in that - three of Noa's
            // arms are flat too - but it is the only flat one any of the
            // three host arts dispatches to. Take Tornado Flame's arm instead (two cursor
            // bands, gate at keyframe 14, three ramp folds) and hand
            // Tornado Flame - and the Miracle finisher that shares it -
            // the static one in exchange.
            camera_swap: Some(0x801D_74A8),
        }),
        1 => Some(HostArt {
            retail_name: b"Vulture Blade",
            index: 4,
            action_constant: 0x1F,
            combo,
            // Vulture Blade's arm is already the second-richest in
            // Noa's table (two bands, gate at keyframe 14, five ramp
            // folds, no side effects) and is hers alone, so it is
            // re-timed in place.
            camera_swap: None,
        }),
        2 => Some(HostArt {
            retail_name: b"Explosive Fist",
            index: 1,
            action_constant: 0x1C,
            combo,
            // Explosive Fist's arm is the most band-rich in the entire
            // dispatcher: four framings across the swing at keyframes
            // 4/7/10, no side effects, and uniquely it reads no table
            // row and never touches `ctx+0x26D`, so it is immune to the
            // per-turn column coin-flip. Nothing to upgrade to, and
            // hers alone, so it is re-timed in place.
            camera_swap: None,
        }),
        _ => None,
    }
}

/// The monster-archive animation **entry index** carrying each sibling's
/// signature choreography.
///
/// Entry index, not action tag, and not a heuristic. The tag space does
/// not separate a special from an ordinary castable - Gi's and Che's
/// signature clips are both tagged `0x23`, and the old
/// `max_by_key(frame_count)` over the `0x0C..=0x1F` band therefore could
/// not reach either of them (it returned a generic castable for Gi, and
/// only found Che's by landing on a byte-identical duplicate).
///
/// A signature move is a **chain**, not a clip. The enemy-side modules
/// stage several archive entries in sequence - `delilas_dome` records
/// Lu's action `0x7B` as `14 -> 12 -> 13` and Che's `0x7A` as
/// `10 -> 11` - and shipping only the last stage is why the reskinned
/// arts showed the payoff swing with no wind-up: Megaton Press skipped
/// the lift, Blazing Slash skipped two thirds of itself.
///
/// Gi's chain is the one no static evidence pinned. `10 -> 11 -> 12` was
/// inferred from clip shape (an 11-frame crouch, a 30-frame leap with
/// the largest torso rise in his archive, a 23-frame slash), and a
/// player watching Blazing Slash independently reported "3 different
/// mini animations", which is the count that inference predicts.
fn signature_clip_chain(sibling: Sibling) -> &'static [usize] {
    match sibling {
        Sibling::Gi => &[10, 11, 12],
        Sibling::Che => &[10, 11],
        Sibling::Lu => &[14, 12, 13],
    }
}

/// One sibling's staged-caster chain: which archive clips are authored
/// into the player file, in module walk order, and how the head table
/// binds them at rest.
struct StagedChain {
    /// Archive entry index of each clip, module walk order (opener first).
    clips: &'static [usize],
    /// Per-clip hard keyframe floor (`cast_stage::stage_ladder`).
    floors: &'static [usize],
    /// Per-clip: carry the SOURCE entry's authored loop window across
    /// (`cast_stage::build_entry` rescales it).
    windows: &'static [bool],
    /// Per-clip: host at the SOURCE's exact frames + rate instead of
    /// the duration-true rate-1 re-timing
    /// (`cast_stage::stage_ladder`). Required where a module cursor
    /// gate rides the stage - the anim cursor climbs `2 * rate`
    /// sixteenths a tick, so re-timing a gated clip to rate 1 halves
    /// the climb and slides every absolute-cursor test off retail's
    /// tick schedule.
    identity: &'static [bool],
    /// Per-clip id/tag byte = the table row the clip is reached through.
    row_ids: &'static [u8],
    /// At-rest head-table bindings `(row, chain index)`.
    binding: &'static [(usize, usize)],
}

/// The FULL retail chain per sibling - the un-folded module walk, where
/// the module-side stage caves ([`crate::delilas_cast`]) repoint row
/// `0x0A` (and, for 958, row `0x0B`) at each mid-stage:
///
/// * Gi (module 958, walk `10,11,12,10,11,13`): crouch wind-up, leap,
///   mid slash, second crouch+leap pass, finale. Rows at rest:
///   `0x0A` -> crouch (10), `0x0B` -> leap (11); the caves swap `0x0A`
///   to 12/back-to-10 and `0x0B` to 13 for the finale (reset by the s2
///   cave next cast).
/// * Lu (module 960, walk `10,14,12,13,15`): raise, charge, channel,
///   strike, closing flourish. Rows at rest: `0x0A` -> raise (10),
///   `0x0B` -> flourish (15, the burst stage); the caves walk `0x0A`
///   through 14/12/13. The strike (13) hosts IDENTITY: module 0960's
///   mp5 confirm (cursor `0x90`) and damage tick (cursor `0x160`, 28
///   ticks after the module releases the authored `[15, 15]` park at
///   file `+0x1638`) both ride the strike stage as absolute-cursor
///   tests, so the hosted clip must keep the source's own 39-frame
///   rate-2 schedule and its park window.
/// * Che (module 959): the retail walk IS two stages - chain == fold.
fn staged_chain_full(sibling: Sibling) -> StagedChain {
    use legaia_asset::party_swap::enemy_anim::{PAYOFF_FLOOR_FRAMES, RETAIL_STAGED_FLOOR as RF};
    match sibling {
        Sibling::Gi => StagedChain {
            clips: &[10, 11, 12, 13],
            floors: &[RF, RF, RF, RF],
            // Retail windows: crouch holds [9, 10], leap holds [8, 9],
            // the slash authors none (the retail flurry replays), the
            // finale parks on its last frame.
            windows: &[true, true, true, true],
            identity: &[false, false, false, false],
            row_ids: &[0x0A, 0x0B, 0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Che => StagedChain {
            clips: &[10, 11],
            floors: &[RF, RF],
            // Che's lift/smash author no windows - carrying them is a
            // no-op, kept uniform.
            windows: &[true, true],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Lu => StagedChain {
            clips: &[10, 14, 12, 13, 15],
            floors: &[RF, RF, RF, PAYOFF_FLOOR_FRAMES, RF],
            // The strike (13) hosts IDENTITY (39f rate 2) with its
            // authored [15, 15] park intact: module 0960 confirms mp5
            // at cursor 0x90 (strike frame 9), RELEASES the park
            // itself (file +0x1638 clears the caster's +0x176/+0x21B
            // hold budget) and fires the damage a fixed 28 ticks
            // later at cursor 0x160 (frame 22) - the burst lands ON
            // the thrust only when the hosted clip reproduces the
            // source's cursor schedule tick for tick. The rate-1
            // re-timing (a windowless 23f host) halved the cursor
            // climb: mp5 confirmed 36 ticks late, the park never
            // held, and the burst decoupled from the release - the
            // audible desync against the module-fired cast bed.
            windows: &[true, true, true, true, true],
            identity: &[false, false, false, true, false],
            row_ids: &[0x0A, 0x0A, 0x0A, 0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 4)],
        },
    }
}

/// The FOLDED two-clip fallback (the pre-cave shape): the module's
/// staged walk stays folded onto rows `0x0A`/`0x0B`
/// (`MODULE_95x_STAGE_REMAP_EDITS`), so a sibling with a longer retail
/// chain contributes its first stage and its payoff - Gi the crouch
/// wind-up + leap, Lu the charge + strike (the fold's damage build-up
/// rides the restaged wind-up row, hence Lu's
/// [`enemy_anim::PAYOFF_FLOOR_FRAMES`] floor on the charge).
fn staged_chain_folded(sibling: Sibling) -> StagedChain {
    use legaia_asset::party_swap::enemy_anim::{PAYOFF_FLOOR_FRAMES, RETAIL_STAGED_FLOOR as RF};
    match sibling {
        Sibling::Gi | Sibling::Che => StagedChain {
            clips: &[10, 11],
            floors: &[RF, RF],
            windows: &[true, true],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Lu => StagedChain {
            clips: &[14, 13],
            // The fold's damage build-up rides the restaged charge (row
            // 0x0A carries the PAYOFF floor), so BOTH slots drop their
            // windows here: a hold would park the cursor short of the
            // keyframe-22 damage gate.
            floors: &[PAYOFF_FLOOR_FRAMES, RF],
            windows: &[false, false],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
    }
}

/// PROT entry of Terra's player battle file (`data\battle\PLAYER4`).
const TERRA_PLAYER_ENTRY: usize = 866;

/// What [`author_staged_cast_rows`] landed: the notes, plus the authored
/// entries' decoded-image offsets for each module that hosts a chain
/// LONGER than the folded pair (what the module-side stage caves need).
/// `None` = that sibling shipped the folded two-row shape.
struct AuthoredCastRows {
    notes: Vec<String>,
    /// Gi / module 958: `[crouch, leap, slash, finale]` offsets.
    gi_unfold: Option<Vec<usize>>,
    /// Lu / module 960: `[raise, charge, channel, strike, flourish]`.
    lu_unfold: Option<Vec<usize>>,
}

/// Author the signature caster rows for every routed slot, and re-home
/// the Block reaction across every player file.
///
/// What lands, all-or-nothing (every record[0] splice is computed before
/// the first byte is written, so a failure leaves the disc untouched):
///
/// 1. Each mapped slot's record[0] gets real staged rows: the sibling's
///    FULL retail chain when the LZS budget takes it, the folded
///    wind-up + payoff pair otherwise ([`staged_chain_full`] /
///    [`staged_chain_folded`]), hosted below the loader's sub-record
///    scratch ([`party_swap::cast_stage::build_staged_cast_rows`]), with
///    the retail Block entry re-homed byte-unmoved onto placeholder row
///    `0x06`.
/// 2. Files hosting no rows (Terra always) get the same one-word
///    row-`0x06` -> Block re-home.
/// 3. The party-init Block-reaction literal flips `0x0B` -> `0x06`
///    ([`crate::delilas_cast::relocate_block_reaction`]) so every slot's
///    guard keeps its retail clip while the cast modules own row `0x0B`.
fn author_staged_cast_rows(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    retail_players: &[Vec<u8>],
    archive: &[u8],
    options: &DelilasPartyOptions,
) -> Result<AuthoredCastRows> {
    use legaia_asset::battle_char_assembly;
    use party_swap::cast_stage;

    // Expand region writes into the offset-edit form
    // `patch_player_record0_full` consumes, dropping bytes that already
    // hold the target value (so an already-applied file reads as "no
    // change" instead of mistaking the encoder's `None` for an
    // overflow). The three outcomes are kept apart: an overflow is a
    // pose-ladder signal, not an error.
    enum Plan {
        NoChange,
        Fit(usize, Vec<u8>),
        Overflow,
    }
    let plan_splice = |entry: &[u8], writes: &[(usize, Vec<u8>)], label: &str| -> Result<Plan> {
        let decoded = battle_char_assembly::decode_record0(entry)
            .with_context(|| format!("decode {label} record0"))?;
        let edits: Vec<(usize, u8)> = writes
            .iter()
            .flat_map(|(off, bytes)| bytes.iter().enumerate().map(move |(i, &b)| (off + i, b)))
            .filter(|&(off, b)| decoded.get(off).copied() != Some(b))
            .collect();
        if edits.is_empty() {
            return Ok(Plan::NoChange);
        }
        Ok(
            match crate::arts::patch_player_record0_full(entry, &[], &edits) {
                Some((off, bytes)) => Plan::Fit(off, bytes),
                None => Plan::Overflow,
            },
        )
    };

    // Every write is planned before the first byte lands, so a failure
    // leaves the disc untouched. `(PROT entry, file offset, bytes)`.
    let mut commits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut row_hosts: Vec<usize> = Vec::new();

    let mut unfold: std::collections::HashMap<u8, Vec<usize>> = std::collections::HashMap::new();
    for (host_entry, rig, slot, who, sibling) in mapping.pairs() {
        let source_id = sibling.monster_id();
        let clips = monster_archive::animations(archive, source_id)
            .with_context(|| format!("read monster {source_id} animations"))?
            .unwrap_or_default();
        let full = staged_chain_full(sibling);
        let folded = staged_chain_folded(sibling);
        let pick = |spec: &StagedChain| -> Result<Vec<&monster_archive::MonsterAnimation>> {
            let picked: Vec<&monster_archive::MonsterAnimation> =
                spec.clips.iter().filter_map(|&i| clips.get(i)).collect();
            if picked.len() != spec.clips.len() {
                bail!(
                    "monster {source_id} carries {} of the {} staged clips",
                    picked.len(),
                    spec.clips.len()
                );
            }
            Ok(picked)
        };

        // Reclaim the descriptor-table slack up front (free compressed-
        // stream footprint, transparent to the loader - see
        // `cast_stage::push_up_desc_table`), then walk the pose ladder
        // against the real LZS budget.
        let mut live = patcher
            .read_entry(host_entry)
            .with_context(|| format!("read {who} player file"))?;
        if let Some(writes) = cast_stage::push_up_desc_table(&live)
            .with_context(|| format!("re-lay {who}'s descriptor table"))?
        {
            for (off, bytes) in &writes {
                live[*off..*off + bytes.len()].copy_from_slice(bytes);
                commits.push((host_entry, *off, bytes.clone()));
            }
        }
        // The staged rows GROW the decoded record[0] (inserted below
        // `clut_a_off` - everything from that offset on is the loader's
        // sub-record decode scratch, so rows parked any higher are
        // destroyed during battle load). The commit is therefore a full
        // stream replacement plus the three shifted header words, not a
        // same-size byte splice.
        let decoded_live = battle_char_assembly::decode_record0(&live)
            .with_context(|| format!("decode {who} record0"))?;
        let (clut_a_live, _) = cast_stage::record0_clut_offsets(&live)
            .with_context(|| format!("read {who} record0 header"))?;
        let region = crate::arts::record0_lzs_region(&live)
            .ok_or_else(|| anyhow::anyhow!("{who}'s record0 LZS region not found"))?;
        match cast_stage::staged_state(&decoded_live, clut_a_live)? {
            cast_stage::StagedState::Applied => {
                // Recover the authored layout so the module-side caves can
                // re-derive their offsets on an idempotent re-run. A file
                // authored with neither the full nor the folded chain is
                // an older build's layout - only a clean image re-patches.
                match cast_stage::recover_entry_offsets(
                    &decoded_live,
                    clut_a_live,
                    full.clips.len(),
                ) {
                    Ok(offs) => {
                        unfold.insert(sibling.monster_id() as u8, offs);
                        notes.push(format!(
                            "cast route: {who} caster rows already present (full chain)"
                        ));
                    }
                    Err(_) => {
                        cast_stage::recover_entry_offsets(
                            &decoded_live,
                            clut_a_live,
                            folded.clips.len(),
                        )
                        .with_context(|| {
                            format!(
                                "{who}'s staged rows match neither the full nor the folded \
                                 chain; patch a clean retail image instead"
                            )
                        })?;
                        notes.push(format!(
                            "cast route: {who} caster rows already present (folded chain)"
                        ));
                    }
                }
            }
            cast_stage::StagedState::Stale => bail!(
                "{who}'s player file carries the superseded payload-reuse staged-row \
                 layout (its rows are destroyed at battle load); patch a clean retail \
                 image instead"
            ),
            cast_stage::StagedState::Absent => {
                // The source clips' authored loop windows, index-aligned
                // with `animations()` (same walk, same skip rules).
                let src_windows = monster_archive::animation_loop_windows(archive, source_id)
                    .with_context(|| format!("read monster {source_id} loop windows"))?
                    .unwrap_or_default();
                // Source sound-cue tracks, index-aligned like the windows:
                // the punch-volley impacts the retail cast fires through
                // `FUN_800508DC` (zeroing them is what made a player-cast
                // flurry silent).
                let src_cues = monster_archive::animation_cue_tracks(archive, source_id)
                    .with_context(|| format!("read monster {source_id} cue tracks"))?
                    .unwrap_or_default();
                let build = |spec: &StagedChain| -> Result<(cast_stage::StagedCastRows, Vec<u8>)> {
                    let chain = pick(spec)?;
                    let windows: Vec<Option<monster_archive::ActionLoopWindow>> = spec
                        .clips
                        .iter()
                        .zip(spec.windows)
                        .map(|(&i, &keep)| {
                            if keep {
                                src_windows.get(i).copied().flatten()
                            } else {
                                None
                            }
                        })
                        .collect();
                    let cue_tracks: Vec<monster_archive::ActionCueTrack> = spec
                        .clips
                        .iter()
                        .map(|&i| src_cues.get(i).cloned().unwrap_or_default())
                        .collect();
                    let mut packed: Option<Vec<u8>> = None;
                    let built = cast_stage::build_staged_cast_rows(
                        &live,
                        &retail_players[slot],
                        rig,
                        archive,
                        source_id,
                        &chain,
                        spec.floors,
                        &windows,
                        &cue_tracks,
                        spec.identity,
                        spec.row_ids,
                        spec.binding,
                        party_swap::playerize::kept_welded_hand(
                            source_id,
                            options.keep_che_hammer && sibling == Sibling::Che,
                        ),
                        |decoded_new| {
                            let mut c = legaia_lzs::compress(decoded_new);
                            if c.len() > region.avail {
                                c = legaia_lzs::compress_optimal(decoded_new);
                            }
                            if c.len() > region.avail {
                                return Ok(false);
                            }
                            packed = Some(c);
                            Ok(true)
                        },
                    )?;
                    let packed =
                        packed.ok_or_else(|| anyhow::anyhow!("fits oracle accepted no stream"))?;
                    Ok((built, packed))
                };
                // The full retail chain first; the folded two-clip shape
                // is the budget fallback (and identical for Che).
                let (built, packed, is_full) = match build(&full) {
                    Ok((b, p)) => (b, p, true),
                    Err(full_err) => {
                        if full.clips.len() == folded.clips.len() {
                            return Err(
                                full_err.context(format!("author {who}'s staged cast rows"))
                            );
                        }
                        notes.push(format!(
                            "cast route: {who} full chain does not fit ({full_err:#}); \
                             folded two-clip chain kept"
                        ));
                        let (b, p) = build(&folded)
                            .with_context(|| format!("author {who}'s staged cast rows"))?;
                        (b, p, false)
                    }
                };
                if is_full && full.clips.len() > folded.clips.len() {
                    unfold.insert(sibling.monster_id() as u8, built.entry_offsets.clone());
                }
                let (ca, cb, bud) = built.header;
                let mut hdr = Vec::with_capacity(12);
                hdr.extend_from_slice(&ca.to_le_bytes());
                hdr.extend_from_slice(&cb.to_le_bytes());
                hdr.extend_from_slice(&bud.to_le_bytes());
                // Header words +0x04/+0x08/+0x0C sit right before the LZS
                // stream at header +0x10.
                commits.push((host_entry, region.lzs_off - 0x10 + 4, hdr));
                commits.push((host_entry, region.lzs_off, packed));
                let stages: Vec<String> = built
                    .frames
                    .iter()
                    .zip(&built.source_frames)
                    .zip(built.rates.iter().zip(&built.holds))
                    .map(|((f, sf), (r, h))| {
                        let hold = if *h > 1 {
                            format!(" hold x{h}")
                        } else {
                            String::new()
                        };
                        format!("{f}f (of {sf}) rate {r}{hold}")
                    })
                    .collect();
                notes.push(format!(
                    "cast route: {who} caster rows inserted (+{:#x} decoded bytes, {} stage \
                     clips) - {}",
                    built.delta,
                    built.frames.len(),
                    stages.join(", "),
                ));
            }
        }
        row_hosts.push(host_entry);
    }

    // Row-6 Block re-home in every file that hosts no staged rows
    // (Terra always: the party-init literal is shared by all four
    // slots). The one-word edit is compression-neutral in practice, but
    // a file already at its ceiling gets the same descriptor-table
    // reclaim as a fallback.
    for (other_entry, other_who) in mapping
        .pairs()
        .into_iter()
        .map(|(e, _, _, w, _)| (e, w))
        .chain([(TERRA_PLAYER_ENTRY, "Terra")])
        .filter(|&(e, _)| !row_hosts.contains(&e))
    {
        let mut f = patcher
            .read_entry(other_entry)
            .with_context(|| format!("read {other_who} player file"))?;
        let write = cast_stage::relocate_block_row(&f)
            .with_context(|| format!("re-home {other_who}'s Block row"))?;
        let mut plan = plan_splice(&f, std::slice::from_ref(&write), other_who)?;
        if matches!(plan, Plan::Overflow)
            && let Some(writes) = cast_stage::push_up_desc_table(&f)
                .with_context(|| format!("re-lay {other_who}'s descriptor table"))?
        {
            for (off, bytes) in &writes {
                f[*off..*off + bytes.len()].copy_from_slice(bytes);
                commits.push((other_entry, *off, bytes.clone()));
            }
            plan = plan_splice(&f, std::slice::from_ref(&write), other_who)?;
        }
        match plan {
            Plan::NoChange => {}
            Plan::Fit(off, bytes) => commits.push((other_entry, off, bytes)),
            Plan::Overflow => bail!(
                "{other_who}'s record0 will not fit its LZS footprint with the Block \
                 row re-homed"
            ),
        }
    }

    // Everything fits - commit, then the SCUS literal.
    for (entry, off, bytes) in &commits {
        patcher
            .patch_prot_entry(*entry, *off as u64, bytes)
            .with_context(|| format!("write staged-row bytes into PROT {entry}"))?;
    }
    crate::delilas_cast::relocate_block_reaction(patcher)
        .context("re-home the party Block reaction")?;

    notes.push("cast route: Block re-homed to row 0x06 on all four files".to_string());
    Ok(AuthoredCastRows {
        notes,
        gi_unfold: unfold.remove(&(Sibling::Gi.monster_id() as u8)),
        lu_unfold: unfold.remove(&(Sibling::Lu.monster_id() as u8)),
    })
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
pub fn signature_fanfare_channels(slot: usize) -> Option<(u8, u8)> {
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

/// Everything one hero slot's signature-art reskin reads.
#[derive(Clone, Copy)]
struct SignatureCtx<'a> {
    /// 0 Vahn / 1 Noa / 2 Gala.
    slot: usize,
    /// The sibling mapped onto that slot.
    sibling: Sibling,
    rig: &'a party_swap::PlayerRig,
    /// The hero's RETAIL player file, captured before the model loop.
    retail_player: &'a [u8],
    archive: &'a [u8],
    /// Canonical hand whose kept welded weapon plays the sibling's own
    /// wrist relation ([`party_swap::kept_welded_hand`]).
    natural_wrist_hand: Option<usize>,
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
    ctx: &SignatureCtx<'_>,
    cave_taken: &mut bool,
) -> Result<Vec<String>> {
    use legaia_art::arts_table;
    use legaia_art::queue::Command;
    let &SignatureCtx {
        slot,
        sibling,
        rig,
        retail_player,
        archive,
        natural_wrist_hand,
    } = ctx;
    let who = ["Vahn", "Noa", "Gala"][slot];
    let character = slot_character(slot);
    let Some(art) = host_art(slot) else {
        return Ok(vec![format!(
            "{}'s signature art: no {who}-slot host art wired yet (skipped)",
            sibling.display_name()
        )]);
    };
    let mut notes = Vec::new();

    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .context("read SCUS for the art rename")?;
    let old = art.retail_name;
    let new = signature_name(sibling);
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

    // 1. Name: written through the record's own `+0xC` pointer, never by
    // searching the image for the old text. The strings nest - searching
    // for "Hurricane" finds the "Hurricane Kick" that contains it - so a
    // text-driven renamer is one table row away from corrupting a
    // neighbour. The field is NUL-padded, so a shorter name is written
    // with the tail cleared to the old length.
    let field = legaia_art::arts_table::name_field(&scus, target.record_file_offset)
        .with_context(|| format!("locate the {who} art's name field"))?;
    let current = scus
        .get(field.file_offset..field.file_offset + field.len)
        .unwrap_or_default();
    if current != old {
        bail!(
            "{who} art index {} reads {:?}, expected {:?} - the host-art table is stale",
            art.index,
            String::from_utf8_lossy(current),
            String::from_utf8_lossy(old)
        );
    }
    if new.len() > old.len() || old.len() >= field.budget {
        bail!(
            "{:?} ({} B) does not fit the {who} art's {}-byte name field",
            String::from_utf8_lossy(new),
            new.len(),
            field.budget
        );
    }
    let mut name_bytes = new.to_vec();
    name_bytes.resize(old.len(), 0);
    patcher
        .patch_named_file(
            crate::arts::SCUS_NAME,
            field.file_offset as u64,
            &name_bytes,
        )
        .context("write art name")?;
    notes.push(format!(
        "art renamed: {} -> {}",
        String::from_utf8_lossy(old),
        String::from_utf8_lossy(new)
    ));

    // 2. Combo: fresh 5-input sequence, checked unique among the
    // character's own arts.
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
    // host bank record is found by its VANILLA combo bytes, and its rate
    // byte (entry +0x78) is re-timed in the SAME record0 rewrite so the
    // host's shorter stream, now carrying the sibling's longer clip,
    // still runs for the clip's authored duration.
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

    // `patch_player_record0_full` reports success when ANY of its edits
    // changed, so a combo needle that matches nothing is dropped in
    // silence while the offset edits still write - the art would then
    // display the new combo in the menu and still answer to the old one
    // in battle. Prove the needle exists first.
    for (vanilla, _) in &char_edits {
        let hits = rec0
            .windows(vanilla.len() + 1)
            .filter(|w| &w[..vanilla.len()] == vanilla.as_slice() && w[vanilla.len()] == 0)
            .count();
        if hits == 0 {
            bail!(
                "{who} record0 carries no {} combo to rewrite - the matcher \
                 would keep answering to the retail input",
                combo_str(&target.commands)
            );
        }
    }

    // The sibling's signature clip, and the readef shape it has to fit -
    // both needed before the record0 write, because the re-timed rate is
    // a function of the two.
    let source_id = sibling.monster_id();
    let chain_entries = signature_clip_chain(sibling);
    let sibling_clips = monster_archive::animations(archive, source_id)
        .with_context(|| format!("read monster {source_id} animations"))?
        .unwrap_or_default();
    let chain: Vec<&monster_archive::MonsterAnimation> = chain_entries
        .iter()
        .filter_map(|&i| sibling_clips.get(i))
        .collect();
    // The payoff stage - what the art record's hit timing has to line up
    // with, and the shape check's subject.
    let clip = chain.last().map(|c| (*c).clone());
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_slot_idx = battle_char_assembly::art_me_slot(slot, false);
    let me_off = me_slot_idx * winpose::READEF_SLOT;
    // 3. The sibling's own choreography, retargeted onto the player rig
    // into the host art's "ME" stream. Rebuilt BEFORE the record0 write,
    // because the frame count it lands on is what every frame-indexed
    // field of the art record has to be rescaled against. Non-fatal: a
    // failed rebuild leaves the host animation (with a note).
    let rebuilt = (|| -> Result<winpose::RebuiltArtSlot> {
        let h = host
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("host art bank record not found by vanilla combo"))?;
        if chain.len() != chain_entries.len() {
            bail!("monster {source_id} is missing a stage of {chain_entries:?}");
        }
        for (i, c) in chain_entries.iter().zip(&chain) {
            if c.part_count != party_swap::CANONICAL_PARTS {
                bail!(
                    "monster {source_id} entry {i} has {} parts, expected {}",
                    c.part_count,
                    party_swap::CANONICAL_PARTS
                );
            }
        }
        let me = readef
            .get(me_off..me_off + winpose::READEF_SLOT)
            .ok_or_else(|| anyhow::anyhow!("readef art slot {me_slot_idx} out of range"))?;
        winpose::rebuild_art_slot_entry(
            me,
            h.stream_source as usize,
            &chain,
            rig,
            retail_player,
            archive,
            source_id,
            natural_wrist_hand,
        )
    })();
    let rebuilt = match rebuilt {
        Ok(r) => {
            patcher
                .patch_prot_entry(READEF_ENTRY, me_off as u64, &r.bytes)
                .context("write the retargeted art stream")?;
            let dropped = chain_entries.len() - r.stages;
            notes.push(format!(
                "{} animation: {}'s own {}-stage chain {:?}{}, {} frames \
                 (host stream carried {})",
                String::from_utf8_lossy(new),
                sibling.display_name(),
                r.stages,
                &chain_entries[dropped..],
                if dropped > 0 {
                    format!(" ({dropped} wind-up stage(s) dropped - slot too tight)")
                } else {
                    String::new()
                },
                r.frames,
                r.retail_frames
            ));
            Some(r)
        }
        Err(e) => {
            notes.push(format!(
                "{} animation stays the host's ({e:#})",
                String::from_utf8_lossy(new)
            ));
            None
        }
    };

    // Everything the art record says about WHEN things happen is an index
    // into the stream that just changed under it, so each frame-indexed
    // field is rescaled by the same ratio.
    let mut offset_edits = Vec::new();
    if let (Some(h), Some(c), Some(r)) = (&host, &clip, &rebuilt) {
        // The chain is written at the rate its stages were stretched to,
        // except on the retail-shape fallback, where it has to be
        // re-timed the old way.
        let rate = if r.frames == r.retail_frames && r.stages == 1 {
            winpose::retimed_rate(r.frames, c.frame_count, c.rate)
        } else {
            r.rate
        };
        offset_edits.push((h.entry_offset + 0x78, rate));
        notes.push(format!(
            "{} pace: {} frames at rate {}",
            String::from_utf8_lossy(new),
            r.frames,
            rate
        ));
        // One clock for the whole move: the frames the sibling's chain
        // actually connects on drive both the damage and the burst.
        let contacts = chain_contacts(&chain, r);
        let (hits, why) = retimed_hit_frames(h, r, &contacts);
        offset_edits.extend(hits);
        notes.push(format!("{} hits: {why}", String::from_utf8_lossy(new)));

        // The real enemy-side burst, when the cave is still free: one of
        // the signature cast module's own effect records, transplanted
        // into a spare prototype slot so the art's one-byte effect id can
        // name it. Non-fatal - without it the borrowed cast projectile
        // stands.
        let burst = match crate::delilas_effects::plan(patcher, sibling, *cave_taken) {
            Ok(Some(p)) => {
                let note = crate::delilas_effects::apply(patcher, &p)
                    .context("install the transplanted burst record")?;
                *cave_taken = true;
                notes.push(format!("{} burst: {note}", String::from_utf8_lossy(new)));
                Some(p.effect_id)
            }
            Ok(None) => None,
            Err(e) => {
                notes.push(format!(
                    "{} burst: not transplanted ({e:#})",
                    String::from_utf8_lossy(new)
                ));
                None
            }
        };

        let (fx, why) = effect_script_edits(h, c, &sibling_clips, r, burst, &contacts);
        offset_edits.extend(fx);
        notes.push(format!("{} effects: {why}", String::from_utf8_lossy(new)));

        // Drop the mid-clip loop hold. Entry +0x84 seeds a hold counter
        // and +0x85/+0x86 bound the window it replays: Vahn's Burning
        // Flare holds frames 9-10 five times, which is its multi-hit
        // flurry and is nonsense over someone else's choreography (and
        // meaningless anyway once the frame count moves).
        //
        // Safe because +0x84 is a hold, not the rate the sibling doc
        // comment on `ArtAnimRecord::rate_alt` reads it as. Census over
        // all three player files: it is 0 on every playable art record
        // but five, including records whose clips run at rate 3, 4 and
        // 7 - a rate field of 0 would freeze them. The rate is +0x78,
        // and each of the five holds bounds a window strictly inside
        // its own clip. 0xFF stays the base-archive marker; the hosts
        // are 5 / 0 / 0, so none of them is one.
        for k in [0x84usize, 0x85, 0x86] {
            offset_edits.push((h.entry_offset + k, 0));
        }

        // Drop the host's impact-effect class. Entry `+0x7A` is a 1..5
        // selector that `FUN_801EC3E4` copies into `actor[+0x21F]` (and
        // whose config row it copies into `actor[+0x04]`), and it drives
        // TWO renderers that both sit OUTSIDE the art's 8-record effect
        // script - which is why rewriting that script does not silence
        // them:
        //
        //   - `FUN_8004998C` streams an element spark along the swing
        //     path at random cadence, `efect.dat` sprite 0x0B for
        //     selector 1 and 0x10 for selector 2;
        //   - `FUN_80049348` draws afterimage copies of the mesh tinted
        //     from a per-CHARACTER table (`0x80076908 + (char-1)*4`),
        //     fading per copy.
        //
        // Vahn's Burning Flare is the only host art of the three that
        // sets it (`1`; Vulture Blade and Explosive Fist are both `0`),
        // so a sibling in Vahn's slot wore his fire sparks and his
        // afterimage tint through every rewrite the swap makes. That is
        // the "Vahn's fire took over" report.
        //
        // Zeroed rather than re-pointed at the sibling's own element:
        // selector 2 would give Lu the lightning-class spark, but it
        // also switches the afterimages on, and those take their colour
        // from the character table, not the art - so it would trade the
        // host's sparks for the host's ghosts. Removing what is wrong is
        // measured; adding what is right needs a frame capture first.
        offset_edits.push((h.entry_offset + IMPACT_CLASS_OFFSET, 0));
    }
    // The battle idle rides the SAME record0 write - it is the only
    // record0 edit that adds bytes, so batching it means one LZS re-fit
    // instead of two that each have to clear the footprint alone.
    match winpose::rebuild_idle_stream(retail_player, rig, archive, source_id, natural_wrist_hand) {
        Ok(idle) => {
            offset_edits.extend(
                idle.bytes
                    .iter()
                    .enumerate()
                    .map(|(i, &b)| (idle.offset + i, b)),
            );
            notes.push(format!(
                "{who} idle: {}'s own combat stance over {} frames, cycling {:.2}x its authored speed",
                sibling.display_name(),
                idle.frames,
                idle.pace
            ));
        }
        Err(e) => notes.push(format!("{who} idle: stays the host's ({e:#})")),
    }

    if !char_edits.is_empty() || !offset_edits.is_empty() {
        // `None` here is indistinguishable from "nothing needed
        // changing", and the combo needle was proven present above, so
        // at this point it can only mean the recompressed block missed
        // its LZS footprint. Failing loudly matters more than usual:
        // a silent skip would leave the art displaying its new combo in
        // the menu while still answering to the old one in battle.
        let (lzs_off, recompressed) =
            crate::arts::patch_player_record0_full(&entry, &char_edits, &offset_edits).ok_or_else(
                || {
                    anyhow::anyhow!(
                        "{who}'s record0 will not fit its LZS footprint with the \
                         signature-art and idle edits applied"
                    )
                },
            )?;
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

    // 3a2 (retired). The sibling's spell-table row (0x79/0x7A/0x7B) once
    // took the host art's retail name so the Nivora duel's mirrored-hero
    // CAST announced the hero art. Two later changes inverted the
    // ownership: the enemy-side signature is now a physical attack
    // (`delilas_signature_attack` rewrites the AI picker's cast arm in
    // place, so no enemy ever casts these ids), and the state-0x28
    // spell-name label is un-gated for player Magic casts
    // (`delilas_cast::install_cast_label_gate`), which reads exactly this
    // row when the converted signature fires. The retail bytes - the
    // sibling special's own name - are what that banner must show, so
    // the row is left retail.

    // 3b. The swing camera. Not a retarget - a re-time. See
    // [`retime_camera_arm`] for why the arm the art already dispatches to
    // is the right one to edit and the wrong one to replace.
    if let Some(r) = &rebuilt {
        match retime_camera_arm(patcher, slot, &art, r) {
            Ok(why) => notes.push(format!("{} camera: {why}", String::from_utf8_lossy(new))),
            Err(e) => notes.push(format!(
                "{} camera: left retail ({e:#})",
                String::from_utf8_lossy(new)
            )),
        }
    }

    // 4. Fanfare duration: the art's pair channels are SILENCED (the
    // cast bed carries the special's audio - see `delilas_xa_voice`),
    // so the duration rows shrink to a token 0.1 s: the entry is
    // CENTISECONDS of channel audio (measured against retail across 24
    // ids; `dur = entry * 0.6` is a 60 Hz tick budget, not the
    // 75-sectors/s physical span an earlier reading assumed). A silent
    // fire that held the retail 3-7 s span would occupy the guarded XA
    // system and swallow any shout fired inside it; 0.1 s releases it
    // immediately. The table is indexed by jingle id - 0x100; the rows
    // are the art's `base_id` pair, NOT a fixed {4, 7}.
    if let Some(fanfare) = signature_fanfare(slot) {
        let toff = legaia_art::hyper_fanfare::dur_table_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("fanfare duration table not found in SCUS"))?;
        let entry_val = 10u16.to_le_bytes();
        let base = (fanfare.base_id - 0x100) as usize;
        for n in [base, base + 3] {
            patcher
                .patch_named_file(crate::arts::SCUS_NAME, (toff + n * 2) as u64, &entry_val)
                .context("write fanfare duration")?;
        }
        notes.push(format!(
            "{} fanfare pair silenced; duration rows -> 0.1 s",
            String::from_utf8_lossy(new)
        ));
    }
    Ok(notes)
}

/// Bytes of one effect-script record, and where the eight of them start
/// inside an action entry (`[frame_gate, effect_id, i16 x, i16 y, i16 z]`,
/// walked by `FUN_801DEA50`).
const FX_RECORD: usize = 8;
const FX_BASE: usize = 0x14;
const FX_RECORDS: usize = 8;

/// Action-entry offset of the impact-effect class byte: a 1..5 selector
/// (`0` = none) read by `FUN_801EC3E4`, which stores it at
/// `actor[+0x21F]` and its config row at `actor[+0x04]`. Both of the
/// renderers it drives - the swing-path element spark in `FUN_8004998C`
/// and the tinted afterimages in `FUN_80049348` - draw independently of
/// the art's effect script.
const IMPACT_CLASS_OFFSET: usize = 0x7A;

/// Spell id of a sibling's signature cast.
///
/// `FUN_801E9FD4`'s `0xA2`/`0xA3`/`0xA4` arms fire on the round counter
/// (`% 3 == 2`) and write `actor[+0x1DF] = monster_id - 0x29`, so Gi's
/// `162` becomes `0x79`, Che's `163` `0x7A` and Lu's `164` `0x7B`. The
/// subtraction is a literal in the raw battle overlay at file `0x1CFFC`
/// (`0x2442FFD7` = `addiu v0,v0,-0x29`).
fn signature_spell_id(sibling: Sibling) -> u8 {
    (sibling.monster_id() - 0x29) as u8
}
/// PROT 0898 file offset of each character's attack-camera jump table
/// (`0x801CEA88` / `0x801CEAD0` / `0x801CEB20` less the overlay base) and
/// how many art constants it admits - the `sltiu` bounds at `0x801D72E0`
/// / `0x801D76C4` / `0x801D7B24`. Reading past them walks into the next
/// character's table.
const CAMERA_TABLES: [(usize, usize); 3] = [(0x270, 17), (0x2B8, 20), (0x308, 17)];
/// Load base the overlay's own addresses are printed against.
const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;
/// The shared epilogue every unused table slot points at. Not an arm.
const CAMERA_EPILOGUE: u32 = 0x801D_828C;
/// `actor[+0x22C][+0x68]`, the animation cursor in sixteenths of a
/// keyframe - the displacement an arm loads it from.
const CURSOR_DISP: u32 = 0x0068;

/// Re-time the attack camera to the length of the swing it now films.
///
/// Each arm is a cascade of `slti` tests on the animation cursor
/// (`actor[+0x22C][+0x68]`, sixteenths of a keyframe), which is how a
/// swing gets several framings instead of one: Gala's Explosive Fist arm
/// changes shot at keyframes 4, 7 and 10, Noa's Vulture Blade arm at 14.
/// Those thresholds are literals sized for the retail clip - the highest
/// in the whole dispatcher is keyframe 17 - and
/// the signature chains run 75 to 100 frames - so the camera finishes its
/// whole choreography inside the wind-up and then holds one shot for the
/// rest of the move. Scaling each threshold by the length ratio spreads
/// the same shots across the same *fractions* of the new swing.
///
/// The arm is edited, never replaced. Retargeting a table slot to a
/// better-choreographed arm is possible and was tried, but every arm in
/// the overlay is already live in some character's table, so a retarget
/// can only alias an arm that another art still uses - and the re-time
/// would then follow the alias into that art and mistune it. Editing the
/// arm the art already dispatches to keeps the blast radius at exactly
/// one (character, art constant) pair, which this checks rather than
/// assumes: the arm must be reachable from exactly one live table slot
/// across all three tables.
///
/// An arm with no cursor test (Vahn's Burning Flare arm reads only the
/// `ctx[+0x26E]` ramp) has no choreography to mistime and is left alone.
fn retime_camera_arm(
    patcher: &mut DiscPatcher,
    slot: usize,
    art: &HostArt,
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
) -> Result<String> {
    let (frames, retail) = (rebuilt.frames, rebuilt.retail_frames);
    if retail == 0 || (frames == retail && rebuilt.stages == 1) {
        return Ok("unchanged (the swing is its retail shape)".into());
    }
    let (base, len) = *CAMERA_TABLES
        .get(slot)
        .ok_or_else(|| anyhow::anyhow!("no camera table for party slot {slot}"))?;
    let row = (art.action_constant as usize)
        .checked_sub(0x1A)
        .filter(|&r| r < len)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "art constant {:#04X} is outside the table",
                art.action_constant
            )
        })?;
    let mut swapped = String::new();
    if let Some(want) = art.camera_swap {
        swapped = swap_camera_arms(patcher, base + row * 4, want)?;
    }
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay")?;
    let word = |off: usize| -> Option<u32> {
        overlay
            .get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let arm = word(base + row * 4)
        .ok_or_else(|| anyhow::anyhow!("camera table row {row} out of range"))?;
    if arm == 0 || arm == CAMERA_EPILOGUE {
        return Ok("the art dispatches straight to the shared epilogue".into());
    }

    // Every live arm across all three tables: the exclusivity check, and
    // the address list that bounds this arm's body.
    let mut live: Vec<u32> = Vec::new();
    let mut uses = 0usize;
    for &(b, n) in &CAMERA_TABLES {
        for r in 0..n {
            let Some(w) = word(b + r * 4) else { continue };
            if w == arm {
                uses += 1;
            }
            if w != 0 && w != CAMERA_EPILOGUE {
                live.push(w);
            }
        }
    }
    if uses != 1 {
        bail!(
            "arm {arm:#010X} is reached from {uses} table slots, so re-timing it would mistune another art"
        );
    }
    live.sort_unstable();
    live.dedup();
    // The arms are laid out in dispatch order, so the next one up is this
    // one's end. The epilogue closes the last arm.
    let end_va = live
        .iter()
        .copied()
        .find(|&a| a > arm)
        .unwrap_or(CAMERA_EPILOGUE);
    let (start, end) = (
        (arm - BATTLE_OVERLAY_BASE) as usize,
        (end_va - BATTLE_OVERLAY_BASE) as usize,
    );
    if end <= start || end > overlay.len() {
        bail!("arm {arm:#010X} spans {start:#X}..{end:#X}, outside the overlay");
    }

    // Which register holds the cursor, and every `slti` against it.
    // Linear clobber tracking would be wrong here: the arms are branch
    // cascades, and the path that reaches a later test skips the block
    // that reuses the register, so the register is live on the path even
    // though a straight-line read says otherwise. The test is instead
    // shape-based - a `slti` (the dispatcher's own bounds checks are
    // `sltiu`, a different opcode) against a register some `lh`/`lhu`
    // loaded from `+0x68`, with a threshold in the keyframe range.
    let mut cursor_regs = [false; 32];
    let mut sites: Vec<(usize, u32)> = Vec::new();
    for off in (start..end).step_by(4) {
        let Some(w) = word(off) else { continue };
        let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F, w & 0xFFFF);
        match op {
            // lh / lhu rt, 0x68(rs)
            0x21 | 0x25 if imm == CURSOR_DISP => cursor_regs[rt as usize] = true,
            // slti rt, rs, imm
            0x0A if cursor_regs[rs as usize] && (0x10..=0x400).contains(&imm) => {
                sites.push((off, imm))
            }
            _ => {}
        }
    }
    let Some(&last_shot) = sites.iter().map(|(_, i)| i).max() else {
        return Ok(format!(
            "{swapped}arm {arm:#010X} has no cursor-gated shot change to re-time"
        ));
    };
    // Anchor the LAST shot change on the frame the payoff stage begins,
    // and scale the earlier ones by the same factor so their spacing is
    // preserved. Anchoring beats scaling by the raw length ratio: the
    // final framing is the one that films the strike, so it should start
    // when the strike does, and a chain can be its host's length while
    // still opening with a wind-up the retail thresholds know nothing
    // about - Lu's two-stage strike is 58 frames either way, so a length
    // ratio of 1 would leave her final shot in the wind-up. The ratio is
    // the fallback for a stream with no wind-up to clear.
    let (num, den) = if rebuilt.payoff_start > 0 && last_shot > 0 {
        (rebuilt.payoff_start * 16, last_shot as usize)
    } else {
        (frames, retail)
    };
    let edits: Vec<(usize, u16, u16)> = sites
        .iter()
        .filter_map(|&(off, imm)| {
            let scaled = ((imm as usize * num + den / 2) / den).min(0x7FFF) as u16;
            (scaled != imm as u16).then_some((off, imm as u16, scaled))
        })
        .collect();
    if edits.is_empty() {
        return Ok(format!(
            "{swapped}arm {arm:#010X} is already timed for this swing"
        ));
    }
    let shots: Vec<String> = edits
        .iter()
        .map(|(_, o, n)| format!("kf {}->{}", o / 16, n / 16))
        .collect();
    for (off, _, scaled) in &edits {
        // The immediate is the instruction word's low halfword, and the
        // word is stored little-endian, so it is the two bytes AT the
        // instruction - not two bytes into it.
        patcher
            .patch_prot_entry(BATTLE_OVERLAY_ENTRY, *off as u64, &scaled.to_le_bytes()[..])
            .context("write a re-timed camera threshold")?;
    }
    Ok(format!(
        "{swapped}arm {arm:#010X} re-timed over {frames} frames (host had {retail}), \
         final shot on the strike at kf {}: {}",
        rebuilt.payoff_start,
        shots.join(", ")
    ))
}

/// Exchange a table slot's camera arm with another arm already live in
/// the dispatcher, giving every slot that held the wanted arm this
/// slot's own in return.
///
/// A plain retarget would leave the wanted arm reachable from two arts,
/// which is exactly the condition [`retime_camera_arm`] refuses to edit
/// under. The exchange keeps the set of live arms unchanged - only which
/// art dispatches to which - and leaves the wanted arm reachable from
/// this slot alone. Idempotent: a slot that already holds the wanted arm
/// is left alone, so a re-apply cannot swap the pair back.
fn swap_camera_arms(patcher: &mut DiscPatcher, slot_off: usize, want: u32) -> Result<String> {
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay for the camera swap")?;
    let word = |off: usize| -> Option<u32> {
        overlay
            .get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let mine = word(slot_off).ok_or_else(|| anyhow::anyhow!("camera slot out of range"))?;
    if mine == want {
        return Ok(String::new()); // already swapped
    }
    let mut ceded = Vec::new();
    for &(b, n) in &CAMERA_TABLES {
        for r in 0..n {
            let off = b + r * 4;
            if off != slot_off && word(off) == Some(want) {
                ceded.push(format!("{:#04X}", 0x1A + r));
                patcher
                    .patch_prot_entry(BATTLE_OVERLAY_ENTRY, off as u64, &mine.to_le_bytes())
                    .context("cede the borrower's arm")?;
            }
        }
    }
    patcher
        .patch_prot_entry(BATTLE_OVERLAY_ENTRY, slot_off as u64, &want.to_le_bytes())
        .context("take the borrowed arm")?;
    Ok(format!(
        "took arm {want:#010X}, ceded {mine:#010X} to art(s) {}; ",
        ceded.join(", ")
    ))
}

/// Map a frame index from the host stream's timeline onto the rebuilt
/// one, never past the last frame.
fn rescale_frame(f: u8, from: usize, to: usize) -> u8 {
    if from == 0 || to == 0 {
        return f;
    }
    let scaled = (f as usize * to).div_ceil(from);
    scaled.min(to - 1) as u8
}

/// One connect in the **rebuilt stream's** frame space.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Contact {
    /// The value to write into a hit slot / effect gate for this connect.
    frame: usize,
    /// How hard the stage decelerates at this connect, in that stage's own
    /// units. `f64::INFINITY` when the stage's beats are authored on disc,
    /// so an authored stage can never be filtered out by a measured one.
    force: f64,
}

/// The frame a clip's whole body stops moving on, and how hard it stops.
///
/// A strike is the frame the body's translation speed falls fastest: the
/// wind-up accelerates, the connect arrests. Speed is the mean per-part
/// translation delta over consecutive keyframes, so it reads the whole body
/// rather than one bone that may or may not be the weapon.
///
/// The returned frame is already a **hit value**, not a visual frame: a hit
/// fires on the first tick where `frame >= value - 1` (`FUN_801EC3E4`
/// `0x801ec468`-`0x801ec478`), so landing damage on the connect means
/// writing `connect + 1`.
///
/// `None` when the clip is too short to have a speed profile or never
/// decelerates at all (a pure approach).
fn principal_impact(clip: &legaia_asset::monster_archive::MonsterAnimation) -> Option<Contact> {
    if clip.part_count == 0 || clip.frame_count < 4 {
        return None;
    }
    let speed: Vec<f64> = (1..clip.frame_count)
        .map(|i| {
            let (a, b) = (&clip.frames[i - 1], &clip.frames[i]);
            let sum: f64 = (0..clip.part_count)
                .map(|p| {
                    let (u, v) = (a[p], b[p]);
                    let dx = f64::from(v.tx) - f64::from(u.tx);
                    let dy = f64::from(v.ty) - f64::from(u.ty);
                    let dz = f64::from(v.tz) - f64::from(u.tz);
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .sum();
            sum / clip.part_count as f64
        })
        .collect();
    let mut best: Option<(usize, f64)> = None;
    for i in 1..speed.len() {
        let drop = speed[i - 1] - speed[i];
        if drop > 0.0 && best.is_none_or(|(_, f)| drop > f) {
            best = Some((i, drop));
        }
    }
    let (i, force) = best?;
    // `speed[j]` spans keyframes `j..j+1`, so a fall from `speed[i-1]` to
    // `speed[i]` means the body arrived on keyframe `i` - that is the
    // connect - and the `+ 1` is the firing rule above.
    Some(Contact {
        frame: i + 1,
        force,
    })
}

/// Every frame the sibling's chain actually connects on, in the rebuilt
/// stream's coordinates.
///
/// Authority order per stage: the stage's OWN authored beats when it is in
/// the damaging tag band (`0x0C..=0x1F` - the castable/attack actions, the
/// only ones retail gives `+0x10..+0x13` to) and carries any; otherwise its
/// measured [`principal_impact`]. Lu's strike stages are authored; Gi's and
/// Che's signature stages are tag `0x23`, whose beats are all zero because
/// as enemies their damage comes from the PROT 958 / 959 cast modules, so
/// theirs are measured.
///
/// A measured stage whose deceleration is under 40% of the chain's hardest
/// is dropped: that is a wind-up settling, not a connect. Authored stages
/// carry infinite force and so are never dropped.
///
/// The per-stage length expression is the rebuild's own
/// (`(frame_count * rate).div_ceil(stage.rate)`), and the front stages are
/// the ones the rebuild drops when the slot is tight, so only the last
/// [`RebuiltArtSlot::stages`](legaia_asset::party_swap::winpose::RebuiltArtSlot)
/// of the chain are walked.
fn chain_contacts(
    chain: &[&legaia_asset::monster_archive::MonsterAnimation],
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
) -> Vec<Contact> {
    let kept = chain.len().saturating_sub(rebuilt.stages);
    let rate = rebuilt.rate.max(1) as usize;
    let last = rebuilt.frames.saturating_sub(1).max(1);

    // Pass 1: every stage's own beats, its own measured impact, and where
    // it sits in the concatenated stream. The measured impact is taken even
    // for a stage whose beats are authored, because it is the yardstick the
    // wind-up filter below is measured against.
    struct Stage<'a> {
        clip: &'a legaia_asset::monster_archive::MonsterAnimation,
        start: usize,
        len: usize,
        authored: Vec<u8>,
        measured: Option<Contact>,
    }
    let mut stages: Vec<Stage<'_>> = Vec::new();
    let mut start = 0usize;
    for stage in &chain[kept.min(chain.len())..] {
        let len = (stage.frame_count * rate)
            .div_ceil(stage.rate.max(1) as usize)
            .max(1);
        let authored: Vec<u8> = if (0x0C..=0x1F).contains(&stage.action_id) {
            (0..4)
                .filter_map(|i| stage.effect_script.get(0x10 + i).copied())
                .filter(|&f| f != 0)
                .collect()
        } else {
            Vec::new()
        };
        stages.push(Stage {
            clip: stage,
            start,
            len,
            authored,
            measured: principal_impact(stage),
        });
        start += len;
    }

    // Pass 2: a stage with no beats of its own only counts as a connect if
    // it stops the body hard against the chain's own hardest stop. That is
    // what tells Lu's opening lunge (a wind-up she authored no beat for)
    // from the two swings she did.
    let hardest = stages
        .iter()
        .filter_map(|s| s.measured.map(|c| c.force))
        .fold(0.0f64, f64::max);
    let mut out: Vec<Contact> = Vec::new();
    for s in &stages {
        let local: Vec<Contact> = if s.authored.is_empty() {
            s.measured
                .filter(|c| c.force >= 0.4 * hardest)
                .into_iter()
                .collect()
        } else {
            s.authored
                .iter()
                .map(|&f| Contact {
                    frame: f as usize,
                    force: f64::INFINITY,
                })
                .collect()
        };
        for c in local {
            let mapped = s.start + (c.frame * s.len).div_ceil(s.clip.frame_count.max(1));
            out.push(Contact {
                frame: mapped.clamp(1, last),
                force: c.force,
            });
        }
    }
    out.sort_by_key(|c| c.frame);
    out.dedup_by_key(|c| c.frame);
    out
}

/// Force a hit / gate list strictly ascending inside `1..=last`, keeping
/// its length. A tail that collides with the cap is pushed back down rather
/// than collapsed onto one frame.
fn ascending_within(frames: &[usize], last: usize) -> Vec<u8> {
    let last = last.max(1);
    let mut out: Vec<usize> = Vec::with_capacity(frames.len());
    let mut prev = 0usize;
    for &f in frames {
        let v = f.max(prev + 1).clamp(1, last);
        out.push(v);
        prev = v;
    }
    for i in (0..out.len().saturating_sub(1)).rev() {
        if out[i] >= out[i + 1] {
            out[i] = out[i + 1].saturating_sub(1).max(1);
        }
    }
    out.into_iter().map(|v| v.min(0xFF) as u8).collect()
}

/// Re-time the art's hit events (`entry +0x10..0x13`) onto the frames the
/// sibling's chain actually connects on.
///
/// A proportional rescale across the WHOLE stream is wrong for a chain -
/// the host's hits were spaced against a single swing, so spreading them
/// over wind-up-plus-payoff drops most of them into the wind-up. But
/// anchoring them on the payoff stage's *start* is wrong too, and that is
/// what shipped: the payoff stage opens with its own approach, so the first
/// application landed 1.1-3.0 seconds after the body connected in all nine
/// sibling/host pairings. The anchor has to be the connect itself
/// ([`chain_contacts`]), not a stage boundary.
///
/// The number of non-zero slots is always the host's. `entry[+0x00 + i]`
/// (power) and `entry[+0x10 + i]` (frame) are parallel arrays walked by one
/// cursor (`actor[+0x1F4]`), so moving the count would re-pair the power
/// bytes, and a zero slot ends the walk outright (`0x801EC47C`) - which is
/// why nothing here may write `0`.
///
/// With more connects than hits the hits sample the connects evenly with
/// both endpoints included, so the first application lands on the first
/// connect and the last (biggest) power byte on the finisher. With fewer,
/// the extras ride the same impact on consecutive frames - retail's own
/// multi-hit idiom (Burning Flare is `11 12 13 14` on one swing).
fn retimed_hit_frames(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
    contacts: &[Contact],
) -> (Vec<(usize, u8)>, String) {
    if rebuilt.frames == rebuilt.retail_frames && rebuilt.stages == 1 {
        return (Vec::new(), "unchanged (retail shape)".into());
    }
    let head = &host.effect_script;
    let k = (0..4)
        .filter_map(|i| head.get(0x10 + i).copied())
        .filter(|&f| f != 0)
        .count();
    if k == 0 {
        return (Vec::new(), "none scheduled".into());
    }
    let last = rebuilt.frames.saturating_sub(1).max(1);
    let (raw, why) = if contacts.is_empty() {
        // No measurable connect anywhere in the chain: keep the old
        // behaviour rather than scheduling on nothing - the host's rhythm,
        // compressed into the payoff stage.
        let start = rebuilt.payoff_start.min(last);
        let span = rebuilt.frames - start;
        let f = (0..4)
            .filter_map(|i| head.get(0x10 + i).copied())
            .filter(|&f| f != 0)
            .map(|h| start + rescale_frame(h, rebuilt.retail_frames, span) as usize)
            .collect::<Vec<_>>();
        (
            f,
            format!("no measurable connect - the host's {k} hit(s) re-spaced over the payoff"),
        )
    } else {
        let n = contacts.len();
        let mut f = Vec::with_capacity(k);
        if n >= k {
            for i in 0..k {
                let idx = if k == 1 { n - 1 } else { i * (n - 1) / (k - 1) };
                f.push(contacts[idx].frame);
            }
        } else {
            let (per, rem) = (k / n, k % n);
            for (i, c) in contacts.iter().enumerate() {
                for j in 0..per + usize::from(i < rem) {
                    f.push(c.frame + j);
                }
            }
        }
        let measured = contacts.iter().filter(|c| c.force.is_finite()).count();
        (
            f,
            format!(
                "{k} hit(s) on the chain's {n} connect(s) at {:?} ({} authored, {measured} measured)",
                contacts.iter().map(|c| c.frame).collect::<Vec<_>>(),
                n - measured,
            ),
        )
    };
    let frames = ascending_within(&raw, last);
    let edits = (0..4)
        .map(|i| {
            (
                host.entry_offset + 0x10 + i,
                frames.get(i).copied().unwrap_or(0),
            )
        })
        .collect();
    (edits, why)
}

/// Whether an effect-script record actually spawns something: id `0` is
/// an empty slot and `id & 0x7F == 0x7F` terminates the walk.
fn fx_record_spawns(script: &[u8], i: usize) -> bool {
    script
        .get(FX_BASE + i * FX_RECORD + 1)
        .is_some_and(|&id| id != 0 && id & 0x7F != 0x7F)
}

/// How many of a script's eight records spawn.
fn fx_live_count(script: &[u8]) -> usize {
    if script.len() < FX_BASE + FX_RECORDS * FX_RECORD {
        return 0;
    }
    (0..FX_RECORDS)
        .filter(|&i| fx_record_spawns(script, i))
        .count()
}

/// Give the art the SIBLING's own visual effects.
///
/// The host art's script is eight `[frame_gate, effect_id, x, y, z]`
/// records - for Vahn's Burning Flare, eight spawns of flame `0x96`
/// sweeping forward through the swing. That flame is why a reskinned art
/// still reads as the host's move however faithful the body animation
/// gets, so it has to go.
///
/// The staged special clips carry **no** script of their own: as an
/// enemy, a sibling's signature move gets its visuals from a per-spell
/// code module (PROT 960 for Lu's), which spawns from module-resident
/// parameter blocks the art record's one-byte id cannot name. What the
/// siblings do have is their ordinary casts, whose entries carry real
/// scripts in exactly this format - so the effects come from there,
/// re-timed onto the rebuilt stream.
///
/// With nothing to borrow, the host's script is suppressed instead by
/// deferring every gate to `0xFF`: the walker never advances its cursor
/// past a gate it has not reached, so nothing spawns and no terminator
/// arm runs. That leaves the art quiet but honest - the body still
/// swings and the hits still land, with no borrowed fire.
///
/// `burst` is the prototype id of a record transplanted out of the
/// sibling's own cast module ([`crate::delilas_effects`]) - the genuine
/// enemy-side spectacle rather than an approximation of it. It replaces
/// the spawning records' effect id while keeping the donor's offsets,
/// because both spawn paths apply the record's own XYZ. The gate must stay
/// non-zero: the walker terminates on `record[0] == 0`, not on the id.
///
/// The gates come from `contacts` - the same connect list the hit frames
/// are scheduled on ([`chain_contacts`]) - so the burst, the damage and the
/// body all run off ONE clock. Rescaling the donor's own gates instead
/// (which is what shipped) puts the burst on a third timeline: the donor is
/// an unrelated cast of the sibling's, and a whole-stream proportional
/// rescale of its beats lands nowhere near either the strike or the
/// contact. The walker's gate rule is the hit rule (`frame + 1 >= gate`,
/// `0x801decb4`), so a gate equal to a hit value fires on the same frame.
fn effect_script_edits(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    clip: &legaia_asset::monster_archive::MonsterAnimation,
    siblings_clips: &[legaia_asset::monster_archive::MonsterAnimation],
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
    burst: Option<u8>,
    contacts: &[Contact],
) -> (Vec<(usize, u8)>, String) {
    // The clip's own script first; failing that, the sibling's richest
    // other one (their casts spawn the projectile art they own).
    let donor = if fx_live_count(&clip.effect_script) > 0 {
        Some((clip, "its own"))
    } else {
        // Only the offensive band: the reaction tags (idle, walk, flinch,
        // knockdown, block - everything below 0x0C) carry scripts too,
        // and a knockdown's dust cloud is not what a signature move
        // should spawn.
        siblings_clips
            .iter()
            .filter(|a| a.action_id >= 0x0C && fx_live_count(&a.effect_script) > 0)
            .max_by_key(|a| fx_live_count(&a.effect_script))
            .map(|a| (a, "borrowed from the sibling's own casts"))
    };
    let Some((donor, origin)) = donor else {
        let edits = (0..FX_RECORDS)
            .map(|i| (host.entry_offset + FX_BASE + i * FX_RECORD, 0xFFu8))
            .collect();
        return (
            edits,
            "host art's flame suppressed (the sibling has no script to give)".into(),
        );
    };
    let src = &donor.effect_script;
    // Each spawning record takes the connect at the same ordinal, cycling
    // when the donor spawns more times than the chain connects. Falling
    // back to the donor's own rescaled beat keeps a chain with no
    // measurable connect firing something.
    let gates: Vec<u8> = if contacts.is_empty() {
        Vec::new()
    } else {
        ascending_within(
            &contacts.iter().map(|c| c.frame).collect::<Vec<_>>(),
            rebuilt.frames.saturating_sub(1).max(1),
        )
    };
    let mut spawned = 0usize;
    let mut edits = Vec::with_capacity(FX_RECORDS * FX_RECORD);
    for i in 0..FX_RECORDS {
        let rec = &src[FX_BASE + i * FX_RECORD..FX_BASE + (i + 1) * FX_RECORD];
        let dst = host.entry_offset + FX_BASE + i * FX_RECORD;
        // An empty or terminating record is copied verbatim - its gate is
        // what ends the walk.
        let gate = if fx_record_spawns(src, i) {
            let g = if gates.is_empty() {
                rescale_frame(rec[0], donor.frame_count, rebuilt.frames)
            } else {
                gates[spawned % gates.len()]
            };
            spawned += 1;
            g.max(1)
        } else {
            rec[0]
        };
        edits.push((dst, gate));
        for (k, &b) in rec.iter().enumerate().skip(1) {
            // A spawning record hands its slot to the transplanted burst;
            // its own gate and offsets stand, since both spawn paths read
            // the record's XYZ.
            let byte = match burst {
                Some(id) if k == 1 && fx_record_spawns(src, i) => id,
                _ => b,
            };
            edits.push((dst + k, byte));
        }
    }
    let live = fx_live_count(src);
    let when = if gates.is_empty() {
        "on the donor's own rescaled beats".to_string()
    } else {
        format!(
            "on the chain's connects {:?}",
            &gates[..gates.len().min(live)]
        )
    };
    let why = match burst {
        Some(id) => format!(
            "{live} spawn(s) of the sibling's own cast-module burst \
             (transplanted id {id}) {when}"
        ),
        None => {
            let ids: Vec<String> = (0..FX_RECORDS)
                .filter(|&i| fx_record_spawns(src, i))
                .map(|i| format!("0x{:02X}", src[FX_BASE + i * FX_RECORD + 1]))
                .collect();
            format!("{live} spawn(s) {origin} ({}) {when}", ids.join(", "))
        }
    };
    (edits, why)
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

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_art::queue::Character;

    #[test]
    fn move_mode_round_trips_and_defaults_to_hybrid() {
        assert_eq!(DelilasMoveMode::default(), DelilasMoveMode::Hybrid);
        for m in [DelilasMoveMode::Hybrid, DelilasMoveMode::Delilas] {
            assert_eq!(m.to_string().parse::<DelilasMoveMode>().unwrap(), m);
        }
        assert_eq!(
            "  DELILAS ".parse::<DelilasMoveMode>().unwrap(),
            DelilasMoveMode::Delilas
        );
        assert!("purist".parse::<DelilasMoveMode>().is_err());
    }

    /// The Super trigger table is static, so the row set each character's
    /// Supers depend on is too - and it is the thing a blank would cost.
    #[test]
    fn super_critical_rows_come_from_the_trigger_table() {
        // Vahn's Tri-Somersault chains arts 0x27, 0x1F, 0x27, so rows
        // 0x17 and 0x0F must both be in the set.
        let vahn = super_critical_rows(Character::Vahn);
        assert!(vahn.contains(&0x17) && vahn.contains(&0x0F));
        assert_eq!(vahn.len(), 8);
        assert_eq!(super_critical_rows(Character::Noa).len(), 10);
        assert_eq!(super_critical_rows(Character::Gala).len(), 8);
        // Every row is a real bank row above the matcher's start.
        for ch in [Character::Vahn, Character::Noa, Character::Gala] {
            for row in super_critical_rows(ch) {
                assert!(row > MIRACLE_BANK_ROW, "{ch:?}: row {row}");
            }
        }
    }

    #[test]
    fn retained_rows_hold_the_miracle_the_host_and_the_innate_block() {
        // Vahn's shape on the USA disc: 33 bank records, innate cap 3,
        // the signature hosted on row 12.
        let keep = retained_bank_rows(Character::Vahn, 3, 12, 33);
        assert!(keep.contains(&MIRACLE_BANK_ROW), "the Miracle row");
        assert!(keep.contains(&12), "the signature host");
        // Ids 1..=3 are the script-granted Hyper block.
        for row in 12..=14 {
            assert!(keep.contains(&row), "innate row {row}");
        }
        // Rows 16 / 19 / 20 are ordinary arts no Super names.
        for row in [16, 19, 20] {
            assert!(!keep.contains(&row), "row {row} should be hidden");
        }
        assert!(keep.iter().all(|&r| r < 33), "no row past the bank");
        // A cap of 0 keeps only the Miracle, the host and the Supers.
        let tight = retained_bank_rows(Character::Vahn, 0, 12, 33);
        assert!(tight.len() < keep.len());
        assert!(tight.contains(&MIRACLE_BANK_ROW) && tight.contains(&12));
    }

    #[test]
    fn every_sibling_has_a_label_per_swing_clip() {
        // The archive carries at most four swings per sibling, and the
        // menu field they are written into is seven bytes at its
        // tightest.
        for sib in [Sibling::Gi, Sibling::Che, Sibling::Lu] {
            let labels = swing_labels(sib);
            assert!(labels.len() >= 4, "{sib:?}: only {} label(s)", labels.len());
            for l in labels {
                assert!(
                    l.len() <= LABEL_MAX,
                    "{sib:?}: {l:?} will not fit a {LABEL_MAX}-byte field"
                );
                assert!(l.starts_with(sib.display_name()), "{sib:?}: {l:?}");
            }
        }
    }
}
