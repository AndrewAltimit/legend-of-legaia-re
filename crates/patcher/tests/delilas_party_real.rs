//! Disc oracle for `--delilas-party`: apply on a scratch copy, re-decode
//! everything the swap claims to change, prove idempotence + determinism.
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_art::queue::Character;
use legaia_patcher::delilas_party::{
    DelilasMoveMode as MoveMode, PartyMapping, Sibling, apply_delilas_party,
};
use legaia_patcher::delilas_voice_fx::ArtsVoiceMode;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

fn block_name(patcher: &DiscPatcher, id: u16) -> String {
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    legaia_asset::monster_archive::record(&archive, id)
        .expect("record")
        .expect("populated")
        .name
}

#[test]
fn default_mapping_swaps_models_names_and_is_idempotent() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");

    // Baseline: retail block names.
    assert_eq!(block_name(&patcher, 162), "Gi Delilas");
    assert_eq!(block_name(&patcher, 163), "Che Delilas");
    assert_eq!(block_name(&patcher, 164), "Lu Delilas");

    let mapping = PartyMapping::default();
    let report = apply_delilas_party(
        &mut patcher,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
    )
    .expect("apply");
    assert!(report.changed);
    // The field forms must come from the siblings' own NPC meshes at
    // full detail - no battle-model fallback, no decimation ladder, no
    // head-texture downscale. Scoped to the PROT 0874 field pass ("field:"
    // notes): the nilboa scene mirror ("nilboa field:" notes) legitimately
    // packs its non-face head islands at half resolution by design (face
    // fronts stay full-res), which is not this guard's regression.
    for note in report
        .notes
        .iter()
        .filter(|n| !n.starts_with("nilboa field:"))
    {
        assert!(
            !note.contains("NPC-mesh source unavailable")
                && !note.contains("detail reduced")
                && !note.contains("resolution"),
            "field quality regression: {note}"
        );
    }
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");

    // Re-open validates EDC/ECC on every touched sector.
    let reopened = DiscPatcher::open(patched.clone()).expect("re-open patched");

    // Default mapping: Gi<->Vahn, Lu<->Noa, Che<->Gala.
    assert_eq!(block_name(&reopened, 162), "Vahn");
    assert_eq!(block_name(&reopened, 164), "Noa");
    assert_eq!(block_name(&reopened, 163), "Gala");

    // The swapped blocks still decode as 15-part meshes and the retail
    // animation streams still cover them.
    let archive = reopened
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    for id in [162u16, 163, 164] {
        let mesh = legaia_asset::monster_archive::mesh(&archive, id)
            .expect("mesh")
            .expect("populated");
        let tmd = legaia_tmd::parse(mesh.tmd_bytes()).expect("swapped TMD");
        assert_eq!(tmd.objects.len(), 15, "monster {id} part count");
        let anims = legaia_asset::monster_archive::animations(&archive, id)
            .expect("anims")
            .expect("populated");
        assert!(anims.iter().all(|a| a.part_count == 15));
    }

    // The rebuilt player files re-assemble through the retail chain.
    for entry in [863usize, 864, 865] {
        let file = reopened.read_entry_footprint(entry).expect("player file");
        let pack = legaia_asset::battle_data_pack::parse(&file).expect("pack reparse");
        legaia_asset::battle_char_assembly::assemble_character(&file, &pack, &[0; 5])
            .expect("assemble");
        legaia_asset::battle_char_assembly::battle_animations(&file).expect("anims");
    }

    // New-game template names follow the mapping.
    let scus = reopened
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let tmpl = legaia_asset::new_game::party_template_file_offset(&scus).expect("template");
    let name_at = |slot: usize| -> String {
        let off = tmpl + slot * legaia_asset::new_game::RECORD_STRIDE + 16;
        let field = &scus[off..off + legaia_asset::new_game::NAME_LEN];
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..len]).into_owned()
    };
    assert_eq!(name_at(0), "Gi");
    assert_eq!(name_at(1), "Lu");
    assert_eq!(name_at(2), "Che");

    // Battle voices, both directions: program 7 (Vahn) tone 0 now decodes
    // to RETAIL Gi's grunt, and the patched monster.snd Gi bank tone 0
    // decodes to RETAIL Vahn's grunt (the samples are byte-copies, so the
    // decoded prefix over the copied span must match exactly).
    {
        use legaia_patcher::delilas_voice::{BATTLE_BANK_ENTRY, MONSTER_SND_ENTRY};
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let tone_vag = |vab: &legaia_vab::VabReport, prog: usize, tone: usize| -> usize {
            let page = vab
                .programs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tones > 0)
                .position(|(i, _)| i == prog)
                .expect("program populated");
            vab.tones[page][tone].vag as usize
        };
        // Bodies compare on the TRUE ADPCM grid (the parser's spans sit
        // 4 bytes early; the splice writes at +4).
        let body = |buf: &[u8], off: usize, prog: usize, tone: usize| -> Vec<u8> {
            let vab = legaia_vab::parse(buf, off).expect("VAB parses");
            let span = vab.vag_samples[tone_vag(&vab, prog, tone) - 1];
            buf[span.byte_offset + 4..(span.byte_offset + 4 + span.size).min(buf.len())].to_vec()
        };
        let gi_off = |snd: &[u8]| -> usize {
            u32::from_le_bytes(snd[8 + 161 * 4..12 + 161 * 4].try_into().unwrap()) as usize * 0x800
                + 4
        };

        let bank = reopened.read_entry(BATTLE_BANK_ENTRY).expect("bank");
        let bank_off = *legaia_vab::find_vabs(&bank).first().expect("bank VAB");
        let retail_bank = retail.read_entry(BATTLE_BANK_ENTRY).expect("retail bank");
        let snd = reopened.read_entry(MONSTER_SND_ENTRY).expect("monster.snd");
        let retail_snd = retail.read_entry(MONSTER_SND_ENTRY).expect("retail snd");

        let patched_party = body(&bank, bank_off, 7, 0);
        let retail_gi = body(&retail_snd, gi_off(&retail_snd), 62, 0);
        let n = patched_party.len().min(retail_gi.len());
        assert!(n > 500, "spliced sample too short ({n} bytes)");
        assert_eq!(
            &patched_party[..n / 16 * 16],
            &retail_gi[..n / 16 * 16],
            "program 7 tone 0 does not carry Gi's sample"
        );

        let patched_gi = body(&snd, gi_off(&snd), 62, 0);
        let retail_party = body(&retail_bank, bank_off, 7, 0);
        let n = patched_gi.len().min(retail_party.len()) / 16 * 16;
        assert!(n > 500, "mirrored sample too short ({n} bytes)");
        // The mirror truncates into the smaller sibling slot; the copied
        // prefix (minus the re-flagged final block) matches byte-exact.
        assert_eq!(
            &patched_gi[..n - 16],
            &retail_party[..n - 16],
            "Gi's duel bank does not carry Vahn's sample"
        );

        // Sharing rules: retail's silent placeholder tone (prog 7 tone 2,
        // vol 0) stays silent, and the SHARED vag-1 body it points at
        // (also a battle SFX + other programs' placeholders) stays
        // byte-identical to retail.
        let vab = legaia_vab::parse(&bank, bank_off).expect("patched bank");
        let retail_vab = legaia_vab::parse(&retail_bank, bank_off).expect("retail bank");
        let page7 = vab
            .programs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tones > 0)
            .position(|(i, _)| i == 7)
            .expect("program 7 page");
        assert_eq!(
            vab.tones[page7][2].vol, 0,
            "silent placeholder tone woke up"
        );
        let (r1, p1) = (retail_vab.vag_samples[0], vab.vag_samples[0]);
        assert_eq!(
            &retail_bank[r1.byte_offset..r1.byte_offset + r1.size],
            &bank[p1.byte_offset..p1.byte_offset + p1.size],
            "shared vag 1 body was overwritten"
        );
    }

    // The container header words the battle scene loader registers as
    // battle-VDF pointers at every battle load (FUN_800520F0 state 0xc:
    // meta[0], meta[1], type<<24|size0, offset0) must stay byte-exact -
    // meta[1] is the offset of the VDF tail past the LZS payload, and a
    // recomputed value points the effect system at garbage (battle-load
    // hang). PROT 0874 is a dual-consumer entry: field player pack AND
    // battle VDF carrier.
    {
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let r = retail.read_entry(874).expect("retail 874");
        let p = reopened.read_entry(874).expect("patched 874");
        assert_eq!(
            &r[..16],
            &p[..16],
            "PROT 0874 registered header words changed"
        );
    }

    // The hero XA voice slots first mute, then take the siblings'
    // XA-encoded grunts. Decode-verify the channels that are known
    // 4-bit mono: the payload must stream through the XA decoder with
    // real signal energy. A helper collects one channel's payload in
    // stream order.
    let channel_payload = |name: &str, chan: u8| -> Vec<u8> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&patched, name).expect(name);
        let sectors = (size as usize).div_ceil(2048);
        let mut out = Vec::new();
        for i in 0..sectors {
            let base = (lba as usize + i) * 2352;
            let sector = &patched[base..base + 2352];
            if sector[0x12] & 0x20 != 0 && sector[0x11] == chan {
                out.extend_from_slice(&sector[0x18..0x918]);
            }
        }
        out
    };
    let decodes_with_energy = |name: &str, chan: u8| {
        let payload = channel_payload(name, chan);
        let mut dec = legaia_xa::StreamingDecoder::new(legaia_xa::DecodeOptions {
            channels: legaia_xa::Channels::Mono,
            bits: legaia_xa::BitsPerSample::Four,
            sample_rate: 18900,
        });
        let mut pcm = Vec::new();
        dec.feed(&payload, &mut pcm).expect("XA decode");
        let peak = pcm.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0);
        assert!(
            pcm.len() > 1000 && peak > 2000,
            "{name} chan {chan}: {} samples, peak {peak} - no grunt audio",
            pcm.len()
        );
    };
    // Arts shout (Gi is Vahn under the default mapping), swing grunt,
    // victory bark.
    decodes_with_energy("XA/XA2.XA", 0);
    decodes_with_energy("XA/XA30.XA", 0);
    decodes_with_energy("XA/XA21.XA", 2);
    // XA21's unattributed channels 0/1 stay silent.
    for chan in [0u8, 1] {
        assert!(
            channel_payload("XA/XA21.XA", chan).iter().all(|&b| b == 0),
            "XA21 chan {chan} should be silent"
        );
    }
    // Every hero voice bank differs from retail everywhere it is
    // non-silent (mute-then-fill leaves no retail audio behind).
    for name in [
        "XA/XA2.XA",
        "XA/XA4.XA",
        "XA/XA6.XA",
        "XA/XA1.XA",
        "XA/XA3.XA",
        "XA/XA5.XA",
        "XA/XA27.XA",
        "XA/XA28.XA",
        "XA/XA29.XA",
        "XA/XA21.XA",
        "XA/XA30.XA",
    ] {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&patched, name).expect(name);
        let sectors = (size as usize).div_ceil(2048);
        for i in 0..sectors {
            let base = (lba as usize + i) * 2352;
            let (r, p) = (&original[base..base + 2352], &patched[base..base + 2352]);
            if p[0x12] & 0x20 == 0 {
                continue;
            }
            let silent = p[0x18..0x92C].iter().all(|&b| b == 0);
            assert!(
                silent || p[0x18..0x92C] != r[0x18..0x92C],
                "{name} sector {i} still carries retail audio"
            );
            assert_ne!(
                &p[0x10..0x18],
                &[0u8; 8],
                "{name} sector {i} subheader wiped"
            );
        }
    }

    // XA20/XA22: only bark channel 7 mutes; every other channel (the
    // music) stays byte-identical.
    for name in ["XA/XA20.XA", "XA/XA22.XA"] {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&patched, name).expect(name);
        let sectors = (size as usize).div_ceil(2048);
        let (mut muted, mut kept) = (0usize, 0usize);
        for i in 0..sectors {
            let base = (lba as usize + i) * 2352;
            let sector = &patched[base..base + 2352];
            if sector[0x12] & 0x20 == 0 {
                continue;
            }
            if sector[0x11] == 7 {
                assert!(
                    sector[0x18..0x92C].iter().all(|&b| b == 0)
                        || sector[0x18..0x92C] != original[base + 0x18..base + 0x92C],
                    "{name} bark channel sector {i} still carries retail audio"
                );
                muted += 1;
            } else {
                assert_eq!(
                    &original[base..base + 2352],
                    sector,
                    "{name} music channel sector {i} was touched"
                );
                kept += 1;
            }
        }
        assert!(muted > 10, "{name}: only {muted} bark sectors muted");
        assert!(kept > 100, "{name}: only {kept} music sectors survive");
    }

    // XA12 is untouched: its only captured battle fire was the
    // results-music jingle path, not a hero voice line.
    {
        let (lba, size) =
            legaia_iso::iso9660::find_path_in_image(&patched, "XA/XA12.XA").expect("XA12.XA");
        let sectors = (size as usize).div_ceil(2048);
        for i in 0..sectors {
            let base = (lba as usize + i) * 2352;
            assert_eq!(
                &original[base..base + 2352],
                &patched[base..base + 2352],
                "XA12.XA sector {i} was touched"
            );
        }
    }

    // Win poses: each character's base "ME" archive (readef slot
    // 3*char+2) reparses with the retail entry count and frame counts,
    // and its streams decode through the retail codec.
    {
        use legaia_patcher::delilas_party::READEF_ENTRY;
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let r_readef = retail.read_entry_footprint(READEF_ENTRY).expect("readef");
        let p_readef = reopened.read_entry_footprint(READEF_ENTRY).expect("readef");
        for slot in [2usize, 5, 8] {
            let span = slot * 0x10800..(slot + 1) * 0x10800;
            let ra = legaia_asset::me_archive::parse(&r_readef[span.clone()]).expect("retail ME");
            let pa = legaia_asset::me_archive::parse(&p_readef[span]).expect("patched ME");
            assert_eq!(ra.len(), pa.len(), "slot {slot} entry count");
            for i in 0..ra.len() {
                let r = ra.entry(i).expect("retail entry");
                let p = pa.entry(i).expect("patched entry");
                assert_eq!(r[0], p[0], "slot {slot} entry {i} part count");
                assert_eq!(r[1], p[1], "slot {slot} entry {i} frame count");
                assert_ne!(r, p, "slot {slot} entry {i} still retail frames");
                if (4..=5).contains(&i) {
                    // The weak-victory entries loop in retail; the
                    // rebuild carries the sibling's IDLE cycle there -
                    // animated (not a frozen hold), and cycle-closed
                    // enough that the loop seam stays subtle.
                    let (parts, frames) = (p[0] as usize, p[1] as usize);
                    let stride = parts * 9;
                    let first = &p[2..2 + stride];
                    let animated =
                        (1..frames).any(|f| &p[2 + f * stride..2 + (f + 1) * stride] != first);
                    assert!(animated, "slot {slot} entry {i} is a frozen hold");
                }
            }
        }
    }

    // Victory-voice clips: the heroes' bands in monster.snd's sector
    // TOC now carry the siblings' SPU-ADPCM grunts - every clip differs
    // from retail and its head still decodes as SPU-ADPCM with energy.
    {
        use legaia_patcher::delilas_voice::MONSTER_SND_ENTRY;
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let r_snd = retail.read_entry(MONSTER_SND_ENTRY).expect("monster.snd");
        let p_snd = reopened.read_entry(MONSTER_SND_ENTRY).expect("monster.snd");
        let rd32 =
            |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
        assert_eq!(rd32(&p_snd, 4), rd32(&r_snd, 4), "clip TOC count changed");
        for (lo, hi) in [(0xB8usize, 0xBCusize), (0xC4, 0xCB), (0xBD, 0xC3)] {
            for clip in lo..=hi {
                let s = rd32(&p_snd, (clip + 2) * 4) * 2048;
                let e = rd32(&p_snd, (clip + 3) * 4) * 2048;
                // Each clip is a mini VAB the game REGISTERS at victory
                // time: the patched clip must still parse (a clobbered
                // header softlocks the results sequence), its header
                // block must stay byte-identical to retail EXCEPT each
                // tone's center/shift pair - the fill re-pitches the
                // destination tone so the spliced body plays at its
                // recorded rate - and only the VAG voice bodies may
                // differ, which they must, still decoding with energy.
                let rv = legaia_vab::parse(&r_snd, s + 4).expect("retail clip VAB");
                let pv = legaia_vab::parse(&p_snd, s + 4).expect("patched clip VAB parses");
                // The parser's spans sit 4 bytes before the real block
                // grid (see `decode_vag_aligned`); the header block AND
                // that lead word must stay retail (minus the re-pitch
                // bytes), so the checked prefix extends to first_vag + 4.
                let first_vag = rv.vag_samples.iter().map(|v| v.byte_offset).min().unwrap();
                let tone_base =
                    s + 4 + legaia_vab::VAB_HEADER_SIZE + legaia_vab::PROGRAMS_TABLE_SIZE;
                let repitch: std::collections::BTreeSet<usize> = (0..16)
                    .flat_map(|t| {
                        let a = tone_base + t * legaia_vab::TONE_SIZE;
                        [a + 4, a + 5]
                    })
                    .collect();
                for off in s..first_vag + 4 {
                    if repitch.contains(&off) {
                        continue;
                    }
                    assert_eq!(
                        r_snd[off], p_snd[off],
                        "victory clip {clip:#x} header byte {off:#x} changed outside re-pitch"
                    );
                }
                assert_eq!(
                    rv.vag_samples.len(),
                    pv.vag_samples.len(),
                    "victory clip {clip:#x} VAG count changed"
                );
                // Every keyed tone's implied playback rate must be sane
                // (an XA-sourced line resamples per clip to fit, so the
                // intervals legitimately differ across a band).
                for tone in pv.tones.iter().flatten() {
                    if tone.vag == 0 {
                        continue;
                    }
                    let semis = tone.min as f64 - tone.center as f64 - tone.shift as f64 / 128.0;
                    let rate = 44100.0 * (semis / 12.0).exp2();
                    assert!(
                        (4000.0..=48000.0).contains(&rate),
                        "victory clip {clip:#x} implies a wild rate {rate:.0}"
                    );
                }
                for (i, vag) in rv.vag_samples.iter().enumerate() {
                    let span = vag.byte_offset + 4..vag.byte_offset + 4 + vag.size;
                    assert!(span.end <= e, "victory clip {clip:#x} VAG {i} out of span");
                    assert_ne!(
                        &r_snd[span.clone()],
                        &p_snd[span.clone()],
                        "victory clip {clip:#x} VAG {i} still retail"
                    );
                    // On the true grid: every block flag legal, and the
                    // body ends with the END-mute terminal (flags 0x01,
                    // envelope release) followed only by zeros - NOT
                    // retail's self-looping 0x07, which leaves the
                    // voice alive for the field load to corrupt.
                    let body = &p_snd[span.clone()];
                    let mut terminal = None;
                    for (bi, block) in body.chunks_exact(16).enumerate() {
                        assert!(
                            block[1] <= 7,
                            "victory clip {clip:#x} VAG {i} block {bi} bad flags {:#x}",
                            block[1]
                        );
                        if block[1] == 0x01 && terminal.is_none() {
                            terminal = Some(bi);
                        }
                    }
                    let t = terminal
                        .unwrap_or_else(|| panic!("victory clip {clip:#x} VAG {i} no terminal"));
                    assert!(
                        body[(t + 1) * 16..].iter().all(|&b| b == 0),
                        "victory clip {clip:#x} VAG {i} data after terminal"
                    );
                    let head = span.start..span.start + 8192.min(span.len());
                    let pcm =
                        legaia_vab::decode_vag_aligned(&p_snd[head]).expect("VAG body decodes");
                    let peak = pcm.iter().map(|&v| (v as i32).abs()).max().unwrap_or(0);
                    assert!(peak > 500, "victory clip {clip:#x} VAG {i} is silent");
                }
            }
        }
    }

    // Idempotence: a second apply is a no-op and changes no bytes.
    let mut second = DiscPatcher::open(patched.clone()).expect("open patched");
    let report2 = apply_delilas_party(
        &mut second,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
    )
    .expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    assert_eq!(second.into_image(), patched, "re-apply changed bytes");

    // Determinism: a fresh run over the retail image is byte-identical.
    let mut third = DiscPatcher::open(original).expect("open again");
    apply_delilas_party(
        &mut third,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
    )
    .expect("apply again");
    assert_eq!(third.into_image(), patched, "apply is deterministic");
}

#[test]
fn custom_mapping_rearranges_the_assignment() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // "Lu can replace Vahn": lu,gi,che.
    let mapping = PartyMapping::parse("lu,gi,che").expect("parse mapping");
    assert_eq!(mapping.vahn, Sibling::Lu);
    let mut patcher = DiscPatcher::open(original).expect("open disc");
    let report = apply_delilas_party(
        &mut patcher,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
    )
    .expect("apply");
    assert!(report.changed);
    let reopened = DiscPatcher::open(patcher.into_image()).expect("re-open");
    // Lu's block now depicts Vahn, Gi's depicts Noa, Che's depicts Gala.
    assert_eq!(block_name(&reopened, 164), "Vahn");
    assert_eq!(block_name(&reopened, 162), "Noa");
    assert_eq!(block_name(&reopened, 163), "Gala");
}

/// Every hero slot gets its mapped sibling's signature special: renamed,
/// re-combo'd, and driven by that sibling's own choreography instead of
/// the host art's.
///
/// The assertions are chosen to fail on retail. A shape check would not
/// be: the host streams are already 15-part, so "15 parts" passes on an
/// unpatched disc and proves nothing. What cannot happen by accident is
/// the frame count moving from the host's to the sibling clip's, and the
/// effect script losing the host's hand-authored flame.
#[test]
fn every_slot_gets_its_siblings_signature_art() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_asset::battle_char_assembly as bca;
    use legaia_asset::party_swap::winpose;
    use legaia_patcher::delilas_party::READEF_ENTRY;

    /// One hero slot's expected reskin. Combo glyphs: 1 L, 2 R, 3 D, 4 U,
    /// so the three retail combos read R D L D L / L L R L R / R R L L L.
    struct Expect {
        slot: usize,
        /// The host art's retail name, which must lose exactly one site.
        host_name: &'static str,
        /// The signature name replacing it, which must gain exactly one.
        sig_name: &'static str,
        monster_id: u16,
        /// Monster-archive entry indices of the sibling's signature
        /// chain, in play order; the last is the payoff stage.
        chain: &'static [usize],
        /// The host art's retail combo - how its bank record is addressed.
        host_combo: &'static [u8],
        /// The art's action constant - the attack-camera table row.
        action_constant: u8,
    }
    let expect = [
        Expect {
            slot: 0,
            host_name: "Burning Flare",
            sig_name: "Plasma Strike",
            monster_id: 164,
            chain: &[14, 12, 13],
            host_combo: &[2, 3, 1, 3, 1],
            action_constant: 0x1C,
        },
        Expect {
            slot: 1,
            host_name: "Vulture Blade",
            sig_name: "Blazing Slash",
            monster_id: 162,
            chain: &[10, 11, 12],
            host_combo: &[1, 1, 2, 1, 2],
            action_constant: 0x1F,
        },
        Expect {
            slot: 2,
            host_name: "Explosive Fist",
            sig_name: "Megaton Press",
            monster_id: 163,
            chain: &[10, 11],
            host_combo: &[2, 2, 1, 1, 1],
            action_constant: 0x1C,
        },
    ];

    let mapping = PartyMapping::parse("lu,gi,che").expect("parse mapping");
    let retail = DiscPatcher::open(original.clone()).expect("open disc");
    let retail_scus = retail
        .read_named_file("SCUS_942.54")
        .expect("read retail SCUS");
    let retail_readef = retail
        .read_entry_footprint(READEF_ENTRY)
        .expect("read retail readef");
    let archive = retail
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("read archive");

    let mut patcher = DiscPatcher::open(original).expect("open disc");
    apply_delilas_party(
        &mut patcher,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
    )
    .expect("apply");
    let patched = DiscPatcher::open(patcher.into_image()).expect("re-open");
    let scus = patched.read_named_file("SCUS_942.54").expect("read SCUS");
    let readef = patched
        .read_entry_footprint(READEF_ENTRY)
        .expect("read readef");

    let occurrences = |hay: &[u8], needle: &str| -> usize {
        hay.windows(needle.len())
            .filter(|w| *w == needle.as_bytes())
            .count()
    };

    for Expect {
        slot,
        host_name,
        sig_name,
        monster_id,
        chain,
        host_combo,
        action_constant,
    } in expect
    {
        // The two names EXCHANGE places, so neither count moves: the
        // arts-table record gives up the host name to carry the
        // signature, and the sibling's spell row gives up the signature
        // to carry the host name. Holding the counts still is what
        // catches collateral damage - these strings nest (searching for
        // `Hurricane` finds `Hurricane Kick`), so a rename that went
        // through the image as text rather than through each table's own
        // pointer would show up here as a count that drifted.
        for name in [host_name, sig_name] {
            assert_eq!(
                occurrences(&scus, name),
                occurrences(&retail_scus, name),
                "slot {slot}: {name} site count moved - the rename is an exchange, \
                 so every name should end up somewhere exactly once"
            );
        }

        // The host art's stream now carries the sibling's clip at the
        // clip's OWN length - not the host stream's retail length.
        let host_rec = {
            let entry = retail
                .read_entry(863 + slot)
                .expect("read retail player file");
            let rec0 = bca::decode_record0(&entry).expect("decode record0");
            let bank = bca::art_animation_bank(&rec0).expect("art bank");
            // Addressed by the retail combo, never by the inline name -
            // that field is a dev label ("Fiery Miyawaki"), not the art's
            // display name, so a name match silently finds nothing.
            let matches: Vec<_> = bank
                .iter()
                .filter(|r| !r.uses_base_archive() && r.combo == host_combo)
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "slot {slot}: {host_name}'s combo should address exactly one bank record"
            );
            matches[0].clone()
        };
        // The new combo must not occur INSIDE any of that character's
        // other arts, at any offset. Equality is the wrong test: the
        // retail matcher walks the scan start index downward, so a rival
        // art matching at offset >= 1 wins outright, and an ordinary art
        // does not consume its run - which is how `L R L R D` used to
        // fire Gala's Battering Ram (`L R D`, offset 2) plus Back Punch
        // instead of his signature.
        {
            let entry = patched
                .read_entry(863 + slot)
                .expect("read patched player file");
            let rec0 = bca::decode_record0(&entry).expect("decode patched record0");
            let bank = bca::art_animation_bank(&rec0).expect("patched art bank");
            // Glyphs 1 L, 2 R, 3 D, 4 U.
            const SIGNATURE_COMBO: [u8; 5] = [1, 2, 4, 4, 3];
            assert!(
                bank.iter()
                    .any(|r| !r.uses_base_archive() && r.combo == SIGNATURE_COMBO),
                "slot {slot}: no bank record answers to the signature combo"
            );
            assert!(
                !bank.iter().any(|r| r.combo == host_combo),
                "slot {slot}: the retail combo {host_combo:?} still matches something"
            );
            // Art rows start at bank record 11 - where the retail matcher
            // starts its own scan (`li s3,0xb` in `FUN_801EED1C`) - and an
            // art input is at least three directions (the shortest on the
            // disc are `RRL` / `LLD` / `UDL` / `RLD` / `LUU`, one per
            // character). Shorter rows are starters and basic swings;
            // including them flags every combo that contains any single
            // direction, i.e. all of them.
            for other in bank
                .iter()
                .skip(11)
                .filter(|r| !r.uses_base_archive() && r.combo.len() >= 3)
            {
                if other.combo == SIGNATURE_COMBO {
                    continue;
                }
                assert!(
                    !SIGNATURE_COMBO
                        .windows(other.combo.len())
                        .any(|w| w == other.combo.as_slice()),
                    "slot {slot}: {:?} occurs inside the signature combo - it would \
                     match first, and an ordinary art does not consume its run",
                    other.combo
                );
            }
        }

        let anims = legaia_asset::monster_archive::animations(&archive, monster_id)
            .expect("animations")
            .expect("archive slot");
        let stages: Vec<&legaia_asset::monster_archive::MonsterAnimation> = chain
            .iter()
            .map(|&i| anims.get(i).expect("chain stage"))
            .collect();
        assert!(
            !stages.is_empty(),
            "slot {slot}: chain {chain:?} has no stages"
        );

        let off = bca::art_me_slot(slot, false) * winpose::READEF_SLOT;
        let src = host_rec.stream_source as usize;
        let before = winpose::art_entry_shape(&retail_readef[off..off + winpose::READEF_SLOT], src)
            .expect("retail shape");
        let after = winpose::art_entry_shape(&readef[off..off + winpose::READEF_SLOT], src)
            .expect("patched shape");
        assert_eq!(before.0, after.0, "slot {slot}: part count must not move");
        assert_ne!(
            after.1, before.1,
            "slot {slot}: frame count identical to retail - the retarget did not land"
        );
        // The patched art record: the rate the stream is written at, and
        // the hit frames scheduled against it.
        let patched_rec = {
            let entry = patched
                .read_entry(863 + slot)
                .expect("read patched player file");
            let rec0 = bca::decode_record0(&entry).expect("decode patched record0");
            let bank = bca::art_animation_bank(&rec0).expect("patched art bank");
            bank.into_iter()
                .find(|r| !r.uses_base_archive() && r.combo == [1, 2, 4, 4, 3])
                .expect("the signature record")
        };

        // The whole chain, not just the payoff, measured as DURATION
        // rather than frame count. A clip runs `frames * 8 / rate` ticks,
        // so the rebuild is free to trade keyframe density for room (it
        // halves the rate and the count together when a slot is tight)
        // and a raw frame-count test would read that as a lost stage.
        // Duration is what actually has to survive: every stage's
        // authored playing time, still there.
        let want: f64 = stages
            .iter()
            .map(|c| c.frame_count as f64 / c.rate.max(1) as f64)
            .sum();
        let got = after.1 as f64 / patched_rec.rate.max(1) as f64;
        assert!(
            (got - want).abs() / want < 0.10,
            "slot {slot}: stream runs {got:.1} units against the chain's authored {want:.1} \
             - the wind-up stages {chain:?} did not land"
        );

        // Where the payoff stage begins in the rebuilt stream, derived
        // the same way the rebuild derives it: each stage stretched from
        // its own rate to the stream's.
        let rate = patched_rec.rate.max(1) as usize;
        let payoff_start: usize = stages[..stages.len() - 1]
            .iter()
            .map(|c| {
                (c.frame_count * rate)
                    .div_ceil(c.rate.max(1) as usize)
                    .max(1)
            })
            .sum();

        // Every hit lands on the strike, not in the wind-up, and the
        // number of them is untouched - the count is how many times the
        // action applies damage, so moving it would change the move.
        let hits = |r: &bca::ArtAnimRecord| -> Vec<u8> {
            (0..4)
                .filter_map(|i| r.effect_script.get(0x10 + i).copied())
                .filter(|&f| f != 0)
                .collect()
        };
        let (was, now) = (hits(&host_rec), hits(&patched_rec));
        assert_eq!(
            was.len(),
            now.len(),
            "slot {slot}: hit count moved {was:?} -> {now:?}"
        );

        // The host's impact-effect class is off. Entry `+0x7A` drives two
        // renderers that live outside the 8-record effect script - the
        // swing-path element spark (`FUN_8004998C`) and the character-
        // tinted afterimages (`FUN_80049348`) - so a non-zero value here
        // paints the sibling's move in the HOST's element no matter what
        // the effect script says. Vahn's Burning Flare ships `1`; that is
        // what put fire on Lu.
        assert_eq!(
            patched_rec.impact_class, 0,
            "slot {slot}: the host art's impact-effect class survived \
             (retail {}) - the sibling wears the host's element sparks \
             and afterimage tint",
            host_rec.impact_class
        );
        if slot == 0 {
            // Keeps the check above honest: Vahn's Burning Flare is the
            // one host art of the three that sets a class at all, so if
            // retail ever read 0 here the zeroing would assert nothing.
            assert_ne!(
                host_rec.impact_class, 0,
                "retail Burning Flare no longer sets an impact-effect \
                 class - the assertion above has gone vacuous"
            );
        }
        // The hits sit on the frames the rebuilt stream actually
        // CONNECTS on, measured off that stream rather than off the
        // patcher's own chain arithmetic. Anchoring them on the payoff
        // stage's start - the assertion this replaces - was satisfied by
        // exactly the frames that were too late: the payoff stage opens
        // with its own approach, so the first application landed
        // 1.1-3.0 s after the body connected in every pairing.
        //
        // A connect is where the whole body arrests: the per-frame mean
        // part translation delta falls. So every hit must be a frame the
        // stream decelerates INTO, and the first one must land in the
        // stream's first two thirds (the "all four crammed on the tail"
        // signature the old anchor produced).
        {
            let speed = stream_speed(&readef, slot, src);
            assert!(
                speed.len() + 1 >= after.1,
                "slot {slot}: decoded {} speed samples for a {}-frame stream",
                speed.len(),
                after.1
            );
            let mean = speed.iter().sum::<f64>() / speed.len().max(1) as f64;
            let first = *now.first().expect("a first hit") as usize;
            assert!(
                first * 3 < after.1 * 2,
                "slot {slot}: first damage at frame {first} of {} - the whole \
                 pattern sits on the tail. hits {now:?}",
                after.1
            );
            // What a hit must sit on depends on what the chain knows
            // about itself. A stage in the damaging tag band carries its
            // own contact beats on disc (Lu's strike stages do), and those
            // are authority - they need not be deceleration frames, since
            // a lunge connects at speed. A stage retail gave no beats to
            // (Gi's and Che's signature stages are tag 0x23, damaged by
            // their cast modules instead) has only its motion to go on, so
            // there the hit must be a frame the body arrests into.
            let rate = patched_rec.rate.max(1) as usize;
            let mut authored: Vec<usize> = Vec::new();
            let mut start = 0usize;
            for st in &stages {
                let len = (st.frame_count * rate)
                    .div_ceil(st.rate.max(1) as usize)
                    .max(1);
                if (0x0C..=0x1F).contains(&st.action_id) {
                    for i in 0..4 {
                        let f = st.effect_script.get(0x10 + i).copied().unwrap_or(0);
                        if f != 0 {
                            authored
                                .push(start + (f as usize * len).div_ceil(st.frame_count.max(1)));
                        }
                    }
                }
                start += len;
            }
            let mut on_impact = 0;
            for (i, h) in now.iter().enumerate() {
                let h = *h as usize;
                assert!(
                    h >= 1 && h < after.1,
                    "slot {slot}: hit at frame {h} is outside the {}-frame stream",
                    after.1
                );
                let rides_previous = i > 0 && h == now[i - 1] as usize + 1;
                let lands = if authored.is_empty() {
                    h >= 2 && speed[h - 1] < speed[h - 2]
                } else {
                    authored.contains(&h)
                };
                if lands {
                    on_impact += 1;
                }
                assert!(
                    lands || rides_previous,
                    "slot {slot}: hit {i} at frame {h} is not on a connect - \
                     {}. hits {now:?}, stream mean speed {mean:.1}",
                    if authored.is_empty() {
                        format!(
                            "the body is still accelerating there ({:.1} -> {:.1})",
                            speed[h.saturating_sub(2)],
                            speed[h - 1]
                        )
                    } else {
                        format!("the chain's authored beats are {authored:?}")
                    }
                );
            }
            assert!(
                on_impact > 0,
                "slot {slot}: no hit lands on a deceleration at all. hits {now:?}"
            );
        }

        // The enemy half of the same rename. The sibling's block already
        // wears this character's model and name, so the Nivora duel
        // fights the heroes - and the cast it announces must be the
        // host's art, not the sibling's, or Vahn casts Blazing Slash.
        // `FUN_801E9FD4` resolves it as `monster_id - 0x29`.
        {
            let table = legaia_asset::spell_names::SpellNameTable::from_scus(&scus)
                .expect("patched spell table");
            let id = (monster_id - 0x29) as u8;
            assert_eq!(
                table.name(id),
                Some(host_name),
                "slot {slot}: enemy cast {id:#04X} should announce the host art"
            );
        }

        // The battle idle carries the sibling's own combat stance, at
        // exactly its retail byte length (the stream is inline in a
        // record[0] whose every later offset would otherwise move), and
        // anchored back onto the host's rest by one rigid whole-body
        // translation - every clip the swap does NOT rebuild (walk,
        // flinch, block, get-up) still starts from the host's rest, so an
        // un-anchored idle pops the whole body at every transition.
        //
        // The anchor's two references are what this checks (see
        // `winpose::idle_anchor`): the torso's x/z land exactly on the
        // host rest, and the deepest ankle over the cycle lands exactly
        // on the host idle's, so the character stands on the ground. The
        // torso's HEIGHT is deliberately not the host's - that difference
        // is the stance (Lu stands taller than Vahn, Che crouches).
        {
            let retail_idle = bca::idle_battle_animation(
                &retail.read_entry(863 + slot).expect("retail player file"),
            )
            .expect("retail idle")
            .expect("retail has an idle");
            let new_idle = bca::idle_battle_animation(
                &patched.read_entry(863 + slot).expect("patched player file"),
            )
            .expect("patched idle")
            .expect("patched has an idle");
            assert_eq!(
                (retail_idle.part_count, retail_idle.frame_count),
                (new_idle.part_count, new_idle.frame_count),
                "slot {slot}: idle shape moved - the stream is inline, so its length is fixed"
            );
            assert_ne!(
                retail_idle.frames, new_idle.frames,
                "slot {slot}: idle is byte-identical to retail - the sibling stance did not land"
            );
            let (was, now) = (retail_idle.frames[0][0], new_idle.frames[0][0]);
            assert_eq!(
                (was.tx, was.tz),
                (now.tx, now.tz),
                "slot {slot}: idle frame 0 moved the torso off the host rest ground spot"
            );
            let rig = if slot == 1 {
                &legaia_asset::party_swap::RIG_NOA
            } else {
                &legaia_asset::party_swap::RIG_VAHN_GALA
            };
            let feet = [
                rig.channel_for_canonical[11] as usize,
                rig.channel_for_canonical[14] as usize,
            ];
            // GTE space is y-down, so the deepest contact is the LARGEST y.
            let floor = |a: &legaia_asset::monster_archive::MonsterAnimation| {
                a.frames
                    .iter()
                    .map(|f| feet.iter().map(|&c| f[c].ty).max().unwrap())
                    .max()
                    .unwrap()
            };
            assert_eq!(
                floor(&retail_idle),
                floor(&new_idle),
                "slot {slot}: the swapped idle does not plant its feet on the host's floor"
            );
        }

        // The attack camera is re-timed to the swing it now films: the
        // arm this art dispatches to must be reachable from exactly one
        // live table slot (or re-timing it would mistune another art),
        // and its last cursor threshold must sit on the payoff frame.
        {
            const TABLES: [(usize, usize); 3] = [(0x270, 17), (0x2B8, 20), (0x308, 17)];
            const BASE: u32 = 0x801C_E818;
            const EPILOGUE: u32 = 0x801D_828C;
            let ov = patched.read_entry(898).expect("battle overlay");
            let word = |o: usize| u32::from_le_bytes(ov[o..o + 4].try_into().unwrap());
            let (tb, tn) = TABLES[slot];
            let row = action_constant as usize - 0x1A;
            assert!(
                row < tn,
                "slot {slot}: art constant {action_constant:#04X} is past the table"
            );
            let arm = word(tb + row * 4);
            assert!(
                arm != 0 && arm != EPILOGUE,
                "slot {slot}: art has no camera arm"
            );
            let uses = TABLES
                .iter()
                .flat_map(|&(b, n)| (0..n).map(move |r| b + r * 4))
                .filter(|&o| word(o) == arm)
                .count();
            assert_eq!(
                uses, 1,
                "slot {slot}: arm {arm:#010X} is reached from {uses} table slots - \
                 re-timing it would mistune another art"
            );
            let mut ends: Vec<u32> = TABLES
                .iter()
                .flat_map(|&(b, n)| (0..n).map(move |r| b + r * 4))
                .map(word)
                .filter(|&a| a != 0 && a != EPILOGUE)
                .collect();
            ends.sort_unstable();
            let end = ends.iter().copied().find(|&a| a > arm).unwrap_or(EPILOGUE);
            let mut cursor = [false; 32];
            let mut shots = Vec::new();
            for o in ((arm - BASE) as usize..(end - BASE) as usize).step_by(4) {
                let w = word(o);
                let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F, w & 0xFFFF);
                match op {
                    0x21 | 0x25 if imm == 0x68 => cursor[rt as usize] = true,
                    0x0A if cursor[rs as usize] && (0x10..=0x4000).contains(&imm) => {
                        shots.push(imm)
                    }
                    _ => {}
                }
            }
            let last = *shots.iter().max().expect("a cursor-gated shot change");
            // Sixteenths of a keyframe; one keyframe of slack for the
            // integer rounding the scale factor carries.
            let want = (payoff_start * 16) as i64;
            assert!(
                (last as i64 - want).abs() <= 16,
                "slot {slot}: arm {arm:#010X} last shot change at cursor {last} \
                 (kf {}), payoff starts at kf {payoff_start} - the camera finishes \
                 its choreography before the strike. shots {shots:?}",
                last / 16
            );
        }
    }
}

#[test]
fn mapping_parser_rejects_non_permutations() {
    assert!(PartyMapping::parse("gi,gi,che").is_err());
    assert!(PartyMapping::parse("gi,lu").is_err());
    assert!(PartyMapping::parse("gi,lu,songi").is_err());
    assert!(PartyMapping::parse("che,gi,lu").is_ok());
}

/// The three `--delilas-arts-voice` modes against the retail shout
/// banks: `original` leaves XA2 byte-identical, `removed` leaves every
/// channel silent, `adjusted` (default) writes processed audio that is
/// voiced, differs from retail, and is byte-deterministic.
#[test]
fn arts_voice_modes_shape_the_shout_banks() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let mapping = PartyMapping::default();

    // pick a retail-voiced channel of Vahn's shout bank as the probe
    let chans = retail.xa_channels("XA/XA2.XA").expect("xa2 channels");
    let probe = *chans
        .iter()
        .find(|&&c| {
            retail
                .read_xa_channel_pcm("XA/XA2.XA", c)
                .map(|(pcm, _)| pcm.iter().any(|&s| s.unsigned_abs() > 1000))
                .unwrap_or(false)
        })
        .expect("XA2 has a voiced channel");
    let (retail_pcm, _) = retail
        .read_xa_channel_pcm("XA/XA2.XA", probe)
        .expect("read");

    let run = |mode: ArtsVoiceMode| -> DiscPatcher {
        let mut p = DiscPatcher::open(original.clone()).expect("open scratch");
        let rep = apply_delilas_party(&mut p, &mapping, mode, MoveMode::default()).expect("apply");
        assert!(rep.changed, "{mode}: apply reported no change");
        p
    };

    // original: the shout bank survives byte-identical
    let p_orig = run(ArtsVoiceMode::Original);
    let (pcm, _) = p_orig
        .read_xa_channel_pcm("XA/XA2.XA", probe)
        .expect("read");
    assert_eq!(pcm, retail_pcm, "original mode must not touch XA2");

    // removed: every channel decodes to silence
    let p_rm = run(ArtsVoiceMode::Removed);
    for &c in &chans {
        if let Ok((pcm, _)) = p_rm.read_xa_channel_pcm("XA/XA2.XA", c) {
            assert!(
                pcm.iter().all(|&s| s.unsigned_abs() <= 24),
                "removed mode left channel {c} audible"
            );
        }
    }

    // adjusted: voiced, different from retail, deterministic
    let p_adj = run(ArtsVoiceMode::Adjusted);
    let (adj, _) = p_adj.read_xa_channel_pcm("XA/XA2.XA", probe).expect("read");
    assert!(
        adj.iter().any(|&s| s.unsigned_abs() > 1000),
        "adjusted mode silenced the probe channel"
    );
    assert_ne!(adj, retail_pcm, "adjusted mode left retail audio untouched");
    let p_adj2 = run(ArtsVoiceMode::Adjusted);
    let (adj2, _) = p_adj2
        .read_xa_channel_pcm("XA/XA2.XA", probe)
        .expect("read");
    assert_eq!(adj, adj2, "adjusted mode must be byte-deterministic");

    // The Super / Hyper / Miracle FANFARE bank. A Hyper Art fires no
    // shout from the pool above, so this cue bed is its only audio -
    // and the bank once took a quarter-second grunt over a 3-7 second
    // channel, which is silence for 90%+ of every cue. Coverage, not
    // mere "differs from retail", is what catches that: a lossy
    // re-encode of a grunt also differs.
    let fan_chans = retail.xa_channels("XA/XA1.XA").expect("xa1 channels");
    for &c in fan_chans.iter().take(8) {
        let Ok((r, _)) = retail.read_xa_channel_pcm("XA/XA1.XA", c) else {
            continue;
        };
        if !r.iter().any(|&s| s.unsigned_abs() > 1000) {
            continue; // never voiced in retail
        }
        let (a, _) = p_adj
            .read_xa_channel_pcm("XA/XA1.XA", c)
            .expect("read patched fanfare");
        let audible = |p: &[i16]| p.iter().filter(|s| s.unsigned_abs() > 200).count();
        let (ra, aa) = (audible(&r), audible(&a));
        assert!(
            aa * 5 >= ra * 4,
            "adjusted fanfare channel {c} covers {aa} of retail's {ra} audible samples \
             - a Hyper Art would play near-silence"
        );
        let (o, _) = p_orig
            .read_xa_channel_pcm("XA/XA1.XA", c)
            .expect("read original fanfare");
        // ...except the signature special's own channel pair. That cue is
        // the sibling's attack soundtrack lifted whole from the sectors
        // their enemy-side cast module plays, not a treatment of the
        // host's bed, so it lands in every arts-voice mode - `original`
        // describes what happens to the HERO's voice. Asserted both ways
        // so neither half can rot: the pair must move, the rest must not.
        let sig = legaia_patcher::delilas_party::signature_fanfare_channels(0)
            .expect("Vahn's slot has a signature fanfare pair");
        if c == sig.0 || c == sig.1 {
            assert_ne!(
                o, r,
                "original mode must still install the signature special's own cue \
                 on channel {c}"
            );
        } else {
            assert_eq!(
                o, r,
                "original mode must not touch the rest of the fanfare bank (channel {c})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// `--delilas-moves`: the two move modes.
// ---------------------------------------------------------------------------

/// Bank row 11 - the Miracle Art's record, and the row the arts matcher
/// starts every scan at.
const MIRACLE_ROW: usize = 0x0B;

fn art_bank(
    patcher: &DiscPatcher,
    ch: Character,
) -> Vec<legaia_asset::battle_char_assembly::ArtAnimRecord> {
    let entry = patcher
        .read_entry(legaia_patcher::arts::player_entry_index(ch))
        .expect("player file");
    let rec0 = legaia_asset::battle_char_assembly::decode_record0(&entry).expect("record0");
    legaia_asset::battle_char_assembly::art_animation_bank(&rec0).expect("art bank")
}

fn me_slot(patcher: &DiscPatcher, slot: usize) -> Vec<u8> {
    use legaia_patcher::delilas_party::READEF_ENTRY;
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .expect("readef.DAT");
    let off = legaia_asset::battle_char_assembly::art_me_slot(slot, false)
        * legaia_asset::party_swap::winpose::READEF_SLOT;
    readef[off..off + legaia_asset::party_swap::winpose::READEF_SLOT].to_vec()
}

fn run_mode(original: &[u8], moves: MoveMode) -> DiscPatcher {
    run_mapped(original, moves, PartyMapping::default()).0
}

fn run_mapped(
    original: &[u8],
    moves: MoveMode,
    mapping: PartyMapping,
) -> (DiscPatcher, Vec<String>) {
    let mut p = DiscPatcher::open(original.to_vec()).expect("open");
    let rep =
        apply_delilas_party(&mut p, &mapping, ArtsVoiceMode::default(), moves).expect("apply");
    assert!(rep.changed);
    (p, rep.notes)
}

/// The character slots in `(party slot, character)` order.
const SLOTS: [(usize, Character); 3] = [
    (0, Character::Vahn),
    (1, Character::Noa),
    (2, Character::Gala),
];

/// Hybrid must leave every coordinate the Delilas pass owns exactly where
/// retail put it - the whole point of the default being a no-change.
///
/// Proved by contrast against the *retail* disc rather than by a stored
/// hash: for every art record other than the one the signature reskin
/// claims, the stream index, the impact class, the inline name and the
/// combo must all still read retail, and every stream body of the art
/// archive but the reskinned one must be byte-identical.
#[test]
fn hybrid_leaves_the_whole_kit_where_retail_put_it() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open");
    let hybrid = run_mode(&original, MoveMode::Hybrid);

    for (slot, ch) in SLOTS {
        let before = art_bank(&retail, ch);
        let after = art_bank(&hybrid, ch);
        assert_eq!(before.len(), after.len(), "{ch:?}: bank length");
        // Exactly one record may differ, and only in the fields the
        // signature reskin declares.
        let changed: Vec<usize> = (0..before.len())
            .filter(|&i| {
                before[i].stream_source != after[i].stream_source
                    || before[i].name != after[i].name
                    || before[i].combo != after[i].combo
            })
            .collect();
        assert_eq!(
            changed.len(),
            1,
            "{ch:?}: hybrid moved {} art record(s); only the signature host may move",
            changed.len()
        );
        let host = changed[0];
        assert_ne!(host, MIRACLE_ROW, "{ch:?}: the Miracle row must not move");
        // The signature keeps the stream it already read - hybrid never
        // repoints.
        assert_eq!(
            before[host].stream_source, after[host].stream_source,
            "{ch:?}: hybrid must not repoint any art stream"
        );
        for i in 0..before.len() {
            if i == host {
                continue;
            }
            assert_eq!(
                before[i].impact_class, after[i].impact_class,
                "{ch:?} row {i}: hybrid must not clear an impact class"
            );
            assert_eq!(
                before[i].rate, after[i].rate,
                "{ch:?} row {i}: hybrid must not re-time an art"
            );
        }

        // The stream archive: same entry count, and every body but the
        // reskinned one byte-identical.
        let a = me_slot(&retail, slot);
        let b = me_slot(&hybrid, slot);
        let ar = legaia_asset::me_archive::parse(&a).expect("retail ME");
        let br = legaia_asset::me_archive::parse(&b).expect("hybrid ME");
        assert_eq!(
            ar.len(),
            br.len(),
            "{ch:?}: hybrid must not resize the archive"
        );
        let sig = after[host].stream_source as usize;
        for i in 0..ar.len() {
            if i == sig {
                continue;
            }
            assert_eq!(
                ar.raw_body(i),
                br.raw_body(i),
                "{ch:?}: hybrid rewrote art stream {i}"
            );
        }
    }
}

/// Delilas mode: every art plays a sibling clip, the Super and Miracle
/// components keep working combos, and what it hides is provably
/// unreachable rather than merely absent.
#[test]
fn delilas_mode_reanimates_the_kit_and_keeps_the_supers_reachable() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open");
    let del = run_mode(&original, MoveMode::Delilas);
    let mapping = PartyMapping::default();
    let siblings = [mapping.vahn, mapping.noa, mapping.gala];

    for (slot, ch) in SLOTS {
        let before = art_bank(&retail, ch);
        let after = art_bank(&del, ch);

        // Baseline contrast: retail spreads its arts over many streams,
        // and the rebuilt archive is a handful.
        let retail_streams: std::collections::BTreeSet<u8> = before
            .iter()
            .filter(|r| !r.uses_base_archive())
            .map(|r| r.stream_source)
            .collect();
        let new_streams: std::collections::BTreeSet<u8> = after
            .iter()
            .filter(|r| !r.uses_base_archive())
            .map(|r| r.stream_source)
            .collect();
        assert!(
            retail_streams.len() > new_streams.len(),
            "{ch:?}: retail read {} streams, the rebuild reads {} - the pass did nothing",
            retail_streams.len(),
            new_streams.len()
        );

        // The archive really is the sibling's: entry count equals what
        // the record set points at, and it shrank from retail's.
        let retail_slot = me_slot(&retail, slot);
        let a = legaia_asset::me_archive::parse(&retail_slot).expect("retail ME");
        let slot_bytes = me_slot(&del, slot);
        let b = legaia_asset::me_archive::parse(&slot_bytes).expect("delilas ME");
        assert!(
            b.len() < a.len(),
            "{ch:?}: the rebuilt archive still carries {} entries",
            b.len()
        );
        for &s in &new_streams {
            assert!(
                (s as usize) < b.len(),
                "{ch:?}: an art record points at stream {s}, past the {}-entry archive",
                b.len()
            );
        }
        // Every emitted stream decodes, and at the rig's own part count.
        let parts = a.entry(0).expect("retail entry 0")[0];
        for i in 0..b.len() {
            let d = b
                .entry(i)
                .unwrap_or_else(|e| panic!("{ch:?} stream {i}: {e:#}"));
            assert_eq!(d[0], parts, "{ch:?} stream {i}: part count");
            assert!(d[1] > 0, "{ch:?} stream {i}: empty");
        }

        // Every Super Art's component arts still carry a working combo,
        // and the Miracle's row still carries its nine-input one.
        let miracle = &after[MIRACLE_ROW];
        assert_eq!(
            miracle.combo, before[MIRACLE_ROW].combo,
            "{ch:?}: the Miracle Art's combo must survive - it is the only \
             route to the wholesale queue overwrite"
        );
        for sup in legaia_art::super_art::SUPER_ARTS
            .iter()
            .filter(|s| s.character == ch)
        {
            for art in sup.art_sequence() {
                let row = art as usize - 0x10;
                assert!(
                    after[row].combo.len() >= 2,
                    "{ch:?}: {} needs art {art:#04X} (row {row}), whose combo was blanked",
                    sup.name
                );
                assert_eq!(
                    after[row].combo, before[row].combo,
                    "{ch:?}: {}'s component art {art:#04X} changed combo",
                    sup.name
                );
            }
        }

        // Everything blanked is a real art that was blanked whole, and
        // nothing retail could still reach: a blanked row's id must sit
        // above the innate cap, so `FUN_801EFBFC` can only have added it
        // through a performance the blank makes impossible.
        let overlay = del.read_entry(898).expect("battle overlay");
        let cap = overlay[(0x801F_686C - 0x801C_E818) + slot] as usize;
        let mut blanked = 0usize;
        for row in MIRACLE_ROW..after.len() {
            if before[row].combo.len() >= 2 && after[row].combo.is_empty() {
                blanked += 1;
                assert!(
                    row - MIRACLE_ROW > cap,
                    "{ch:?} row {row}: art id {} is at or below the innate cap {cap}, \
                     so a script can still grant it and it would list unusable",
                    row - MIRACLE_ROW
                );
            }
        }
        assert!(blanked > 0, "{ch:?}: nothing was hidden");

        // Every re-animated art dropped the host's impact class (the
        // element spark and the character-tinted afterimages both sit
        // outside the effect script, so nothing else removes them).
        for r in after
            .iter()
            .filter(|r| !r.uses_base_archive() && r.index >= MIRACLE_ROW)
        {
            assert_eq!(
                r.impact_class, 0,
                "{ch:?} row {}: still carries the host's impact class",
                r.index
            );
        }

        // The menu reads as the sibling's list: every performable art
        // other than the signature is named after its clip.
        let label_stem = siblings[slot].display_name();
        let scus =
            legaia_iso::iso9660::read_file_in_image(del.image(), "SCUS_942.54").expect("SCUS");
        let recs = legaia_art::arts_table::raw_records_from_scus(&scus).expect("arts table");
        let mut renamed = 0usize;
        for r in recs.iter().filter(|r| r.character == ch && !r.is_miracle) {
            let row = r.index as usize + MIRACLE_ROW;
            if after[row].combo.len() < 2 {
                continue; // hidden
            }
            let f = legaia_art::arts_table::name_field(&scus, r.record_file_offset).expect("name");
            let name = String::from_utf8_lossy(&scus[f.file_offset..f.file_offset + f.len]);
            if name.starts_with(label_stem) {
                renamed += 1;
            }
        }
        assert!(
            renamed >= 5,
            "{ch:?}: only {renamed} performable art(s) carry a {label_stem} name"
        );
    }

    // Footprint + sector integrity for everything the pass writes.
    let patched = del.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");
    let reopened = DiscPatcher::open(patched.clone()).expect("re-open the patched image");
    for entry in [863usize, 864, 865, 894] {
        let lba = reopened.entry_disc_lba(entry).unwrap() as usize;
        let sectors = (reopened.entry_footprint(entry).unwrap() as usize)
            .div_ceil(legaia_iso::raw::USER_DATA_SIZE);
        for s in 0..sectors {
            let sb = (lba + s) * legaia_iso::raw::SECTOR_SIZE;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(
                    &patched[sb..sb + legaia_iso::raw::SECTOR_SIZE]
                ),
                "PROT {entry} sector {s} lost EDC/ECC under the Delilas moveset"
            );
        }
    }

    // Deterministic: the mode takes no seed, so a second run is byte-identical.
    let again = run_mode(&original, MoveMode::Delilas).into_image();
    assert!(
        again == patched,
        "the Delilas moveset must be byte-deterministic"
    );

    // And the two modes really do differ.
    let hybrid = run_mode(&original, MoveMode::Hybrid).into_image();
    assert!(
        hybrid != patched,
        "hybrid and delilas produced the same image"
    );
}

/// The menu labels are per-**sibling** but the name field they overwrite
/// is per-**slot**, and the mapping is a free permutation - so a label
/// sized against the slot a sibling usually lands in silently keeps the
/// retail name under a rearranged party. Proved on the rearrangement,
/// not on the default.
#[test]
fn delilas_labels_fit_every_slot_under_a_rearranged_mapping() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Che onto Vahn's slot, whose tightest retained field is the
    // seven-byte "Cyclone".
    let mapping = PartyMapping {
        vahn: Sibling::Che,
        noa: Sibling::Gi,
        gala: Sibling::Lu,
    };
    let (_, notes) = run_mapped(&original, MoveMode::Delilas, mapping);
    let tight: Vec<&String> = notes.iter().filter(|n| n.contains("too tight")).collect();
    assert!(
        tight.is_empty(),
        "a menu label did not fit its slot's name field: {tight:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("art names:")),
        "the rename pass did not run at all"
    );
}

/// The mode string parses both ways and round-trips through `Display`,
/// which is what the CLI flag and the browser dropdown both hand it.
#[test]
fn move_mode_parses_both_ways() {
    assert_eq!("hybrid".parse::<MoveMode>().unwrap(), MoveMode::Hybrid);
    assert_eq!("DELILAS".parse::<MoveMode>().unwrap(), MoveMode::Delilas);
    assert_eq!(MoveMode::default(), MoveMode::Hybrid);
    for m in [MoveMode::Hybrid, MoveMode::Delilas] {
        assert_eq!(m.to_string().parse::<MoveMode>().unwrap(), m);
    }
    assert!("purist".parse::<MoveMode>().is_err());
}

/// Each hero slot fights in the **mapped sibling's** element, not the
/// host character's.
///
/// The battle overlay's per-character element table (`0x801F5480`) is the
/// only per-character element on the disc and the one both affinity
/// readers index (`FUN_801DD864` at `0x801dd8ac`/`0x801dd900`,
/// `FUN_801EC3E4` at `0x801ecf38`/`0x801ecf94`), so until it moves a
/// swapped party deals and takes the *host's* element - Lu's Plasma
/// Strike landing as fire out of Vahn's slot, Che's Megaton Press as
/// thunder out of Gala's.
///
/// Proved by contrast against retail rather than against a literal table:
/// every slot must read the sibling's own `+0x1D` byte, at least one must
/// have MOVED off retail (the assertion is vacuous if the two agree
/// everywhere), and the five characters the swap does not touch must be
/// byte-identical to retail.
///
/// Run on a rearranged mapping as well as the default: the table is
/// indexed by slot and the value comes from the sibling, so a pass that
/// wrote the right element to the wrong slot would still satisfy the
/// default if the default happened to be the identity permutation.
#[test]
fn every_slot_fights_in_its_siblings_element() {
    use legaia_asset::element_affinity as ea;
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let retail_ov = retail.read_entry(898).expect("retail battle overlay");
    let archive = retail
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let sibling_element = |s: Sibling| -> u8 {
        legaia_asset::monster_archive::record(&archive, s.monster_id())
            .expect("record")
            .expect("populated")
            .element
    };

    for mapping in [
        PartyMapping::default(),
        PartyMapping {
            vahn: Sibling::Lu,
            noa: Sibling::Gi,
            gala: Sibling::Che,
        },
    ] {
        let (patched, notes) = run_mapped(&original, MoveMode::Hybrid, mapping);
        let ov = patched.read_entry(898).expect("patched battle overlay");
        let want = [mapping.vahn, mapping.noa, mapping.gala];
        let mut moved = 0;
        for (slot, sibling) in want.iter().copied().enumerate() {
            let at = ea::CHARACTER_ELEMENTS_FILE_OFFSET + slot;
            let got = ov[at];
            let expect = sibling_element(sibling);
            assert_eq!(
                got,
                expect,
                "slot {slot}: element {got} ({:?}), expected {}'s own {expect} ({:?})",
                ea::Element::from_id(got).map(|e| e.name()),
                sibling.display_name(),
                ea::Element::from_id(expect).map(|e| e.name()),
            );
            if got != retail_ov[at] {
                moved += 1;
            }
        }
        assert!(
            moved > 0,
            "no slot's element moved off retail - the assertion is vacuous"
        );
        // The characters outside the party swap keep retail's elements:
        // the table has eight rows and the pass owns exactly three.
        for i in 3..ea::CHARACTER_ELEMENTS_LEN {
            let at = ea::CHARACTER_ELEMENTS_FILE_OFFSET + i;
            assert_eq!(
                ov[at],
                retail_ov[at],
                "character {} element moved, and the swap owns only slots 0..3",
                i + 1
            );
        }
        // The affinity matrix itself is untouched - this pass moves who
        // is what element, never what an element does.
        let m = ea::AFFINITY_MATRIX_FILE_OFFSET;
        let n = ea::ELEMENT_COUNT * ea::ELEMENT_COUNT;
        assert_eq!(
            &ov[m..m + n],
            &retail_ov[m..m + n],
            "the element-affinity matrix moved"
        );
        assert_eq!(
            notes.iter().filter(|n| n.contains(" element: ")).count(),
            3,
            "the report should name one element per slot: {notes:?}"
        );
    }
}

/// The runtime reads the signature art's effect script out of exactly the
/// bytes the reskin edits.
///
/// The chain is `DAT_801C9360[char]` -> the decoded `record[0]` image ->
/// `record0[+0x58]` -> `bank + 4 + row*0xD0` -> `+0x24` -> `+0x14`
/// (`FUN_8004AD80` at `0x8004b708`/`0x8004bc84`, handed to `FUN_801DEA50`
/// as `node[+0x4C]` by `FUN_80047430`). This asserts the *disc* half of
/// that chain: the whole art bank the runtime walks is a verbatim image of
/// the player file's `record[0]`, so a same-size edit inside a bank record
/// is an edit the runtime sees.
///
/// Cross-checked against a live mid-battle RAM capture (mednafen scenario
/// `party_battle_gobu_gobu`): every one of Vahn's 33 bank rows in RAM
/// matches this decode byte for byte, effect scripts included.
#[test]
fn the_signature_effect_script_lives_in_the_bank_the_runtime_walks() {
    use legaia_asset::battle_char_assembly as bca;
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let (patched, _) = run_mapped(&original, MoveMode::Hybrid, PartyMapping::default());
    // Glyphs 1 L, 2 R, 3 D, 4 U - the combo the reskin writes.
    const SIGNATURE_COMBO: [u8; 5] = [1, 2, 4, 4, 3];
    for (slot, ch) in SLOTS {
        let entry = patched
            .read_entry(legaia_patcher::arts::player_entry_index(ch))
            .expect("player file");
        let rec0 = bca::decode_record0(&entry).expect("record0");
        let bank = bca::art_animation_bank(&rec0).expect("art bank");
        let row = bank
            .iter()
            .find(|r| !r.uses_base_archive() && r.combo == SIGNATURE_COMBO)
            .expect("the signature row");
        // The bank record is addressed off record[0] by the same
        // expression the runtime uses, so read the script straight out of
        // the image and require the parser's view to agree with it.
        let at = row.entry_offset + 0x14;
        let live: Vec<u8> = rec0[at..at + 8 * 8].to_vec();
        assert_eq!(
            &live[..],
            &row.effect_script[0x14..0x14 + 8 * 8],
            "slot {slot}: the parsed script is not the bytes at record0 +{at:#x}"
        );
        // Exactly one record spawns, and it is not a retail id: the host
        // scripts are 8 x 0x96 (Vahn), 2 x 0x93 (Noa), 4 x 0x93 (Gala).
        let spawns: Vec<u8> = (0..8)
            .map(|i| live[i * 8 + 1])
            .filter(|&id| id != 0 && id & 0x7F != 0x7F)
            .collect();
        assert_eq!(
            spawns.len(),
            1,
            "slot {slot}: {} spawning record(s), expected the sibling's single burst",
            spawns.len()
        );
        assert!(
            !matches!(spawns[0], 0x96 | 0x93),
            "slot {slot}: the burst is still a host-element id {:#04X}",
            spawns[0]
        );
        // And the retail row it replaced really did carry those ids, so
        // the check above cannot go vacuous on a future disc.
        let retail_entry = retail
            .read_entry(legaia_patcher::arts::player_entry_index(ch))
            .expect("retail player file");
        let retail_rec0 = bca::decode_record0(&retail_entry).expect("retail record0");
        let retail_bank = bca::art_animation_bank(&retail_rec0).expect("retail art bank");
        let retail_row = retail_bank
            .iter()
            .find(|r| r.entry_offset == row.entry_offset)
            .expect("the retail row at the same offset");
        let retail_spawns: Vec<u8> = (0..8)
            .map(|i| retail_row.effect_script[0x14 + i * 8 + 1])
            .filter(|&id| id != 0 && id & 0x7F != 0x7F)
            .collect();
        assert!(
            retail_spawns.iter().all(|&id| matches!(id, 0x96 | 0x93)),
            "slot {slot}: retail host script is {retail_spawns:02X?}, not the \
             hand-authored flame/spark this test contrasts against"
        );
    }
}

/// The whole-body speed profile of a rebuilt art stream, measured off the
/// readef slot itself: `speed[j]` is the mean per-part translation delta
/// between keyframes `j` and `j + 1`.
///
/// An ME entry decodes to `[u8 parts][u8 frames]` then `frames * parts`
/// nine-byte part records, six 12-bit fields each (low bytes at
/// `[0,1,3,4,6,7]`, high nibbles packed into `[2,5,8]` - the layout
/// `FUN_8004998C` unpacks). Only the three translations are read here, and
/// they are unpacked locally rather than through the crate's own decoder so
/// the assertion does not measure the patcher with the patcher's tools.
fn stream_speed(readef: &[u8], slot: usize, entry_index: usize) -> Vec<f64> {
    use legaia_asset::party_swap::winpose::READEF_SLOT;
    let off = legaia_asset::battle_char_assembly::art_me_slot(slot, false) * READEF_SLOT;
    let ar = legaia_asset::me_archive::parse(&readef[off..off + READEF_SLOT]).expect("ME archive");
    let body = ar.entry(entry_index).expect("art entry");
    let (parts, frames) = (body[0] as usize, body[1] as usize);
    let sx12 = |v: u16| -> i32 {
        if v & 0x800 != 0 {
            (v | 0xF000) as i16 as i32
        } else {
            v as i32
        }
    };
    let pose = |f: usize, p: usize| -> [i32; 3] {
        let b = &body[2 + (f * parts + p) * 9..][..9];
        [
            sx12(b[0] as u16 | ((b[2] as u16 & 0x0F) << 8)),
            sx12(b[1] as u16 | ((b[2] as u16 & 0xF0) << 4)),
            sx12(b[3] as u16 | ((b[5] as u16 & 0x0F) << 8)),
        ]
    };
    (1..frames)
        .map(|f| {
            let sum: f64 = (0..parts)
                .map(|p| {
                    let (a, b) = (pose(f - 1, p), pose(f, p));
                    let d: Vec<f64> = (0..3).map(|k| (b[k] - a[k]) as f64).collect();
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .sum();
            sum / parts.max(1) as f64
        })
        .collect()
}
