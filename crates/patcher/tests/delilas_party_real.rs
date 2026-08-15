//! Disc oracle for `--delilas-party`: apply on a scratch copy, re-decode
//! everything the swap claims to change, prove idempotence + determinism.
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_patcher::delilas_party::{PartyMapping, Sibling, apply_delilas_party};
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
    let report =
        apply_delilas_party(&mut patcher, &mapping, ArtsVoiceMode::default()).expect("apply");
    assert!(report.changed);
    // The field forms must come from the siblings' own NPC meshes at
    // full detail - no battle-model fallback, no decimation ladder, no
    // head-texture downscale.
    for note in &report.notes {
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
    let report2 =
        apply_delilas_party(&mut second, &mapping, ArtsVoiceMode::default()).expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    assert_eq!(second.into_image(), patched, "re-apply changed bytes");

    // Determinism: a fresh run over the retail image is byte-identical.
    let mut third = DiscPatcher::open(original).expect("open again");
    apply_delilas_party(&mut third, &mapping, ArtsVoiceMode::default()).expect("apply again");
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
    let report =
        apply_delilas_party(&mut patcher, &mapping, ArtsVoiceMode::default()).expect("apply");
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
    apply_delilas_party(&mut patcher, &mapping, ArtsVoiceMode::default()).expect("apply");
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
        for h in &now {
            assert!(
                (*h as usize) >= payoff_start,
                "slot {slot}: hit at frame {h} is before the payoff stage starts \
                 ({payoff_start}) - the damage fires during the wind-up. hits {now:?}"
            );
            assert!(
                (*h as usize) < after.1,
                "slot {slot}: hit at frame {h} is past the {}-frame stream",
                after.1
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
        // anchored so frame 0 still puts the body where the host's rest
        // pose does - every clip the swap does NOT rebuild (walk,
        // flinch, block, get-up) still starts from there, so an
        // un-anchored idle pops the whole body at every transition.
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
                (was.tx, was.ty, was.tz),
                (now.tx, now.ty, now.tz),
                "slot {slot}: idle frame 0 moved the torso off the host rest pose"
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
        let rep = apply_delilas_party(&mut p, &mapping, mode).expect("apply");
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
        assert_eq!(o, r, "original mode must not touch the fanfare bank");
    }
}
