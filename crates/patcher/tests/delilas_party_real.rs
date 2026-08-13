//! Disc oracle for `--delilas-party`: apply on a scratch copy, re-decode
//! everything the swap claims to change, prove idempotence + determinism.
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_patcher::delilas_party::{PartyMapping, Sibling, apply_delilas_party};
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
    let report = apply_delilas_party(&mut patcher, &mapping).expect("apply");
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
            let mut band_interval: Option<i32> = None;
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
                // Every keyed tone in the band plays the SAME sibling
                // body, so the note-to-center interval (which pins the
                // playback rate) must agree across the whole band.
                for tone in pv.tones.iter().flatten() {
                    if tone.vag == 0 {
                        continue;
                    }
                    let interval =
                        tone.min as i32 * 128 - tone.center as i32 * 128 - tone.shift as i32;
                    let prev = band_interval.get_or_insert(interval);
                    assert_eq!(
                        *prev, interval,
                        "victory clip {clip:#x} plays the body at a different rate"
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
    let report2 = apply_delilas_party(&mut second, &mapping).expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    assert_eq!(second.into_image(), patched, "re-apply changed bytes");

    // Determinism: a fresh run over the retail image is byte-identical.
    let mut third = DiscPatcher::open(original).expect("open again");
    apply_delilas_party(&mut third, &mapping).expect("apply again");
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
    let report = apply_delilas_party(&mut patcher, &mapping).expect("apply");
    assert!(report.changed);
    let reopened = DiscPatcher::open(patcher.into_image()).expect("re-open");
    // Lu's block now depicts Vahn, Gi's depicts Noa, Che's depicts Gala.
    assert_eq!(block_name(&reopened, 164), "Vahn");
    assert_eq!(block_name(&reopened, 162), "Noa");
    assert_eq!(block_name(&reopened, 163), "Gala");
}

#[test]
fn mapping_parser_rejects_non_permutations() {
    assert!(PartyMapping::parse("gi,gi,che").is_err());
    assert!(PartyMapping::parse("gi,lu").is_err());
    assert!(PartyMapping::parse("gi,lu,songi").is_err());
    assert!(PartyMapping::parse("che,gi,lu").is_ok());
}
