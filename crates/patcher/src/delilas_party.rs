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
/// where the per-character attack-camera jump tables live.
const BATTLE_OVERLAY_ENTRY: usize = 898;

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
                lines: &victory_lines,
            };
            let notes = reskin_signature_art(patcher, &ctx, &mut cave_taken)
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
    lines: &'a crate::delilas_xa_voice::VictoryLines,
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
        lines,
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
        let (hits, why) = retimed_hit_frames(h, r, c);
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

        let (fx, why) = effect_script_edits(h, c, &sibling_clips, r, burst);
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
    }
    // The battle idle rides the SAME record0 write - it is the only
    // record0 edit that adds bytes, so batching it means one LZS re-fit
    // instead of two that each have to clear the footprint alone.
    match winpose::rebuild_idle_stream(retail_player, rig, archive, source_id) {
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

    // 3a2. The other half of the same rename, on the enemy side. The
    // sibling's own block now wears this character's model and name, so
    // the Nivora duel already fights the heroes - but the cast it
    // announces is still the sibling's, which reads as Vahn casting
    // Blazing Slash. The enemy AI resolves that name through the spell
    // table (`FUN_801E9FD4` sets `actor+0x1DF = monster_id - 0x29` on
    // every third round), so pointing the sibling's row at the host
    // art's retail name completes the exchange: the party art gave up
    // "Burning Flare" to become "Blazing Slash", and the enemy row gives
    // up "Blazing Slash" to become "Burning Flare".
    match rename_enemy_signature_spell(patcher, sibling, art.retail_name) {
        Ok(why) => notes.push(format!("{who} enemy cast: {why}")),
        Err(e) => notes.push(format!("{who} enemy cast: left retail ({e:#})")),
    }

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

/// Bytes of one effect-script record, and where the eight of them start
/// inside an action entry (`[frame_gate, effect_id, i16 x, i16 y, i16 z]`,
/// walked by `FUN_801DEA50`).
const FX_RECORD: usize = 8;
const FX_BASE: usize = 0x14;
const FX_RECORDS: usize = 8;

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

/// Rename the sibling's signature cast to the host art's retail name.
///
/// The spell-name table is the same one the party path uses, so this is
/// the enemy half of the art rename and not a second mechanism. Written
/// through the record's own `+8` pointer into the measured NUL padding -
/// never grown, never found by searching the image for the old text,
/// which is how the `Hurricane` / `Hurricane Kick` class of neighbour
/// corruption happens.
fn rename_enemy_signature_spell(
    patcher: &mut DiscPatcher,
    sibling: Sibling,
    host_name: &[u8],
) -> Result<String> {
    let id = signature_spell_id(sibling);
    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
    let field = legaia_asset::spell_names::name_field(&scus, id)
        .ok_or_else(|| anyhow::anyhow!("spell {id:#04X} has no reachable name field"))?;
    let current = &scus[field.file_offset..field.file_offset + field.len];
    if current == host_name {
        return Ok(format!("spell {id:#04X} already renamed"));
    }
    if host_name.len() + 1 > field.budget {
        bail!(
            "spell {id:#04X}'s name slot holds {} bytes, {} needs {}",
            field.budget,
            String::from_utf8_lossy(host_name),
            host_name.len() + 1
        );
    }
    let was = String::from_utf8_lossy(current).into_owned();
    let mut bytes = vec![0u8; field.budget];
    bytes[..host_name.len()].copy_from_slice(host_name);
    patcher
        .patch_named_file(crate::arts::SCUS_NAME, field.file_offset as u64, &bytes)
        .context("write the enemy signature cast name")?;
    Ok(format!(
        "spell {id:#04X} {was:?} -> {:?}",
        String::from_utf8_lossy(host_name)
    ))
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

/// Re-time the art's hit events (`entry +0x10..0x13`) onto the rebuilt
/// stream. These are the frames the strike lands on.
///
/// A proportional rescale across the WHOLE stream is wrong for a chain.
/// The stream is now wind-up stages followed by the payoff, and the host's
/// hits were spaced against a single swing, so spreading them over the
/// full length drops most of them into the wind-up: Burning Flare's four
/// hits at frames 11..14 of its own clip land at 42..53 of a 75-frame
/// chain whose strike does not begin until frame 52. The damage fires
/// while the character is still winding up.
///
/// Every hit therefore lands inside the payoff stage, and the payoff's
/// OWN event frames are preferred when the clip carries them - Lu's
/// strike stage is authored with its contacts, and authored beats
/// interpolated. Only the placement moves; the number of non-zero slots
/// is always the host's, because that count is how many times the action
/// applies damage and changing it would change the move.
fn retimed_hit_frames(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
    payoff: &legaia_asset::monster_archive::MonsterAnimation,
) -> (Vec<(usize, u8)>, String) {
    if rebuilt.frames == rebuilt.retail_frames && rebuilt.stages == 1 {
        return (Vec::new(), "unchanged (retail shape)".into());
    }
    let head = &host.effect_script;
    let host_hits: Vec<u8> = (0..4)
        .filter_map(|i| head.get(0x10 + i).copied())
        .filter(|&f| f != 0)
        .collect();
    if host_hits.is_empty() {
        return (Vec::new(), "none scheduled".into());
    }
    let last = rebuilt.frames.saturating_sub(1) as u8;
    let start = rebuilt.payoff_start.min(last as usize);
    // The payoff's own contacts, lifted onto the chain: its frame indices
    // are clip-local and it was stretched to the chain's common rate.
    let own: Vec<u8> = (0..4)
        .filter_map(|i| payoff.effect_script.get(0x10 + i).copied())
        .filter(|&f| f != 0)
        .map(|f| {
            let scaled = (f as usize * rebuilt.payoff_frames).div_ceil(payoff.frame_count.max(1));
            (start + scaled).min(last as usize) as u8
        })
        .collect();
    let (frames, why) = if own.is_empty() {
        // Nothing authored: keep the host's rhythm, compressed into the
        // payoff stage so the whole pattern lands on the strike.
        let span = rebuilt.frames - start;
        let f = host_hits
            .iter()
            .map(|&h| {
                let scaled = rescale_frame(h, rebuilt.retail_frames, span) as usize;
                (start + scaled).min(last as usize) as u8
            })
            .collect::<Vec<_>>();
        (
            f,
            format!(
                "the host's {} hit(s) re-spaced inside the payoff stage \
                 (frames {start}..{})",
                host_hits.len(),
                rebuilt.frames
            ),
        )
    } else {
        // Authored contacts, extended with their own spacing when the
        // host schedules more hits than the stage carries.
        let step = own
            .windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .max()
            .unwrap_or(2)
            .max(1);
        let mut f = own.clone();
        while f.len() < host_hits.len() {
            let next = f.last().unwrap().saturating_add(step).min(last);
            f.push(next);
        }
        f.truncate(host_hits.len());
        (
            f,
            format!(
                "the payoff clip's own {} authored contact(s) (tag {:#04X}){}",
                own.len(),
                payoff.action_id,
                if host_hits.len() > own.len() {
                    format!(", extended to the host's {}", host_hits.len())
                } else {
                    String::new()
                }
            ),
        )
    };
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
/// the spawning records' effect id while keeping the donor's frame gates
/// and offsets, because both spawn paths apply the record's own XYZ. The
/// gate must stay non-zero: the walker terminates on `record[0] == 0`,
/// not on the id.
fn effect_script_edits(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    clip: &legaia_asset::monster_archive::MonsterAnimation,
    siblings_clips: &[legaia_asset::monster_archive::MonsterAnimation],
    rebuilt: &legaia_asset::party_swap::winpose::RebuiltArtSlot,
    burst: Option<u8>,
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
    let mut edits = Vec::with_capacity(FX_RECORDS * FX_RECORD);
    for i in 0..FX_RECORDS {
        let rec = &src[FX_BASE + i * FX_RECORD..FX_BASE + (i + 1) * FX_RECORD];
        let dst = host.entry_offset + FX_BASE + i * FX_RECORD;
        // The gate is a frame index in the DONOR's timeline; the rebuilt
        // stream is a different length, so it moves with the ratio. An
        // empty or terminating record is copied verbatim.
        let gate = if fx_record_spawns(src, i) {
            rescale_frame(rec[0], donor.frame_count, rebuilt.frames)
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
    let why = match burst {
        Some(id) => {
            format!("{live} spawn(s) of the sibling's own cast-module burst (transplanted id {id})")
        }
        None => {
            let ids: Vec<String> = (0..FX_RECORDS)
                .filter(|&i| fx_record_spawns(src, i))
                .map(|i| format!("0x{:02X}", src[FX_BASE + i * FX_RECORD + 1]))
                .collect();
            format!("{live} spawn(s) {origin} ({})", ids.join(", "))
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
