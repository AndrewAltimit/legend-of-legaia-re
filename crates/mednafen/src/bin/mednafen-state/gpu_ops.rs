//! GPU VRAM dump / CLUT-trace and SPU reverb-routing subcommands for
//! `mednafen-state`.

use anyhow::{Context, Result};
use legaia_mednafen::{
    PsxGpu, PsxSpu, SaveState, VRAM_HEIGHT, VRAM_WIDTH,
    gpu::{nonzero_rows, vram_to_rgba8},
};
use std::path::{Path, PathBuf};

pub fn cmd_vram_dump(save: &Path, out: &Path, out_bin: Option<&Path>, regs: bool) -> Result<()> {
    let s = SaveState::from_path(save)?;
    let gpu = PsxGpu::new(&s);
    let bytes = gpu
        .vram_bytes()
        .ok_or_else(|| anyhow::anyhow!("save state has no GPU.&GPURAM[0][0] entry"))?;
    let rgba = vram_to_rgba8(bytes);
    write_png(out, &rgba, VRAM_WIDTH as u32, VRAM_HEIGHT as u32)
        .with_context(|| format!("writing PNG to {}", out.display()))?;
    println!(
        "[ok] wrote {} ({}x{} BGR555 + STP-as-alpha, {} non-zero rows of {})",
        out.display(),
        VRAM_WIDTH,
        VRAM_HEIGHT,
        nonzero_rows(bytes),
        VRAM_HEIGHT,
    );
    if let Some(bin) = out_bin {
        std::fs::write(bin, bytes)
            .with_context(|| format!("writing raw VRAM to {}", bin.display()))?;
        println!(
            "[ok] wrote raw VRAM to {} ({} bytes)",
            bin.display(),
            bytes.len()
        );
    }
    if regs {
        let r = gpu.regs();
        println!("[regs] clip            = {:?}", r.clip);
        println!("[regs] draw_offset     = {:?}", r.draw_offset);
        println!(
            "[regs] tex_window      = {:?}  (mask_x, mask_y, off_x, off_y)",
            r.tex_window
        );
        println!("[regs] tex_page (x,y)  = {:?}", r.tex_page);
        println!("[regs] tex_mode        = {:?}", r.tex_mode);
        println!("[regs] display_fb      = {:?}", r.display_fb);
        println!("[regs] display_h_range = {:?}", r.display_h_range);
        println!("[regs] display_v_range = {:?}", r.display_v_range);
        println!("[regs] display_off     = {:?}", r.display_off);
        println!("[regs] display_mode_raw= {:?}", r.display_mode_raw);
    }
    Ok(())
}

pub fn cmd_clut_trace(
    pack_path: &Path,
    saves: &[PathBuf],
    json_out: Option<&Path>,
    include_tmd_body: bool,
) -> Result<()> {
    use legaia_asset::battle_data_pack;

    let pack_bytes = std::fs::read(pack_path)
        .with_context(|| format!("reading PROT entry {}", pack_path.display()))?;
    let pack = battle_data_pack::parse(&pack_bytes)
        .with_context(|| format!("parsing {} as battle_data pack", pack_path.display()))?;

    println!(
        "[pack] {}  records={}  data_base=0x{:x}",
        pack_path.display(),
        pack.records.len(),
        pack.data_base
    );

    // Decode every record once.
    struct DecodedRecord {
        record_idx: usize,
        record_id: u32,
        decoded: battle_data_pack::DecodedEntry,
    }
    let mut decoded_records = Vec::new();
    for r in &pack.records {
        match battle_data_pack::decode_record(&pack_bytes, &pack, r.index) {
            Ok(d) => decoded_records.push(DecodedRecord {
                record_idx: r.index,
                record_id: r.id,
                decoded: d,
            }),
            Err(e) => {
                eprintln!("[warn] record {} decode failed: {}", r.index, e);
            }
        }
    }

    #[derive(serde::Serialize)]
    struct CorpusEntry {
        save_state: String,
        record_idx: usize,
        record_id: u32,
        header_u32s: [String; 8],
        record_byte_offset: usize,
        tmd_end: Option<usize>,
        post_tmd_offset: Option<usize>,
        fb_x: u16,
        fb_y: u16,
        vram_byte_offset: usize,
    }
    let mut corpus = Vec::new();
    let mut total_hits = 0usize;

    println!(
        "{:<32} {:>5} {:>5} {:>10} {:>7} {:>7} {:>7}  header[0..8]",
        "save_state", "rec", "id", "rec_off", "post_off", "fb_x", "fb_y"
    );
    println!("{}", "-".repeat(120));

    for save in saves {
        let s = SaveState::from_path(save)
            .with_context(|| format!("loading save state {}", save.display()))?;
        let gpu = PsxGpu::new(&s);
        let Some(vram) = gpu.vram_bytes() else {
            eprintln!("[skip] {} has no GPU.&GPURAM[0][0]", save.display());
            continue;
        };
        let save_label = save
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        for rec in &decoded_records {
            let matches = if include_tmd_body {
                // Construct a fake DecodedEntry that lies about tmd_range so
                // find_clut_in_vram scans the whole record.
                let phony = battle_data_pack::DecodedEntry {
                    record: rec.decoded.record,
                    bytes: rec.decoded.bytes.clone(),
                    tmd_range: None,
                };
                battle_data_pack::find_clut_in_vram(&phony, vram)
            } else {
                battle_data_pack::find_clut_in_vram(&rec.decoded, vram)
            };
            let header = battle_data_pack::record_header_u32s(&rec.decoded);
            let tmd_end = rec.decoded.tmd_range.as_ref().map(|r| r.end);
            for m in &matches {
                total_hits += 1;
                let post_tmd_offset = tmd_end.map(|end| m.record_byte_offset.saturating_sub(end));
                println!(
                    "{:<32} {:>5} 0x{:02x} 0x{:08x} {:>7} {:>7} {:>7}  {}",
                    save_label,
                    rec.record_idx,
                    rec.record_id,
                    m.record_byte_offset,
                    post_tmd_offset
                        .map(|p| format!("0x{:x}", p))
                        .unwrap_or_else(|| "-".into()),
                    m.fb_x,
                    m.fb_y,
                    header
                        .iter()
                        .map(|w| format!("{:08x}", w))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                corpus.push(CorpusEntry {
                    save_state: save_label.clone(),
                    record_idx: rec.record_idx,
                    record_id: rec.record_id,
                    header_u32s: header.map(|w| format!("0x{:08x}", w)),
                    record_byte_offset: m.record_byte_offset,
                    tmd_end,
                    post_tmd_offset,
                    fb_x: m.fb_x,
                    fb_y: m.fb_y,
                    vram_byte_offset: m.vram_byte_offset(),
                });
            }
        }
    }
    println!();
    println!(
        "[done] {} match(es) across {} save state(s) and {} record(s)",
        total_hits,
        saves.len(),
        decoded_records.len()
    );

    if let Some(path) = json_out {
        let json = serde_json::to_string_pretty(&corpus)
            .with_context(|| "encoding corpus JSON".to_string())?;
        std::fs::write(path, json)
            .with_context(|| format!("writing JSON to {}", path.display()))?;
        println!("[ok] wrote corpus to {}", path.display());
    }
    Ok(())
}

fn write_png(out: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let f = std::fs::File::create(out)?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

pub fn cmd_spu(save: &Path, all: bool) -> Result<()> {
    let s = SaveState::from_path(save)?;
    let spu = PsxSpu::new(&s);
    println!("[spu] {}", save.display());

    match spu.reverb_master_enabled() {
        Some(on) => println!(
            "  reverb master (SPUCNT bit 7) : {}",
            if on { "ENABLED" } else { "disabled" }
        ),
        None => println!("  reverb master (SPUCNT bit 7) : <not captured>"),
    }
    match spu.reverb_mode() {
        Some(m) => println!("  reverb mode                  : {m}"),
        None => println!("  reverb mode                  : <not captured>"),
    }
    let reverb_mask = spu.voice_reverb_mask();
    match reverb_mask {
        Some(m) => println!("  voice reverb mask (EON)      : 0x{m:06X}"),
        None => println!("  voice reverb mask (EON)      : <not captured>"),
    }
    if let Some(m) = reverb_mask {
        let reverbed: Vec<usize> = (0..legaia_mednafen::SPU_NUM_VOICES)
            .filter(|i| m & (1 << i) != 0)
            .collect();
        println!("  reverb-routed voices         : {reverbed:?}");
    }

    if let Some(wa) = spu.reverb_work_area() {
        // mednafen stores ReverbWA in 16-bit (halfword) units; the work-area
        // byte base is wa*2 and the size = 0x80000 - wa*2.
        let size = 0x8_0000u32.saturating_sub(wa.wrapping_mul(2));
        println!(
            "  reverb work area (mBASE)     : 0x{:05X}  (size 0x{size:05X} bytes)",
            wa * 2
        );
    }
    if let Some(rr) = spu.reverb_registers() {
        // dAPF1/dAPF2 are the most distinctive per-preset; print the lead
        // registers so the preset can be matched against the known libspu
        // tables (engine_audio::ReverbMode presets).
        println!(
            "  reverb regs dAPF1/dAPF2/vIIR : 0x{:04X} 0x{:04X} 0x{:04X}",
            rr[0], rr[1], rr[2]
        );
        print!("  reverb regs[0..16]           :");
        for r in &rr[..16] {
            print!(" {r:04X}");
        }
        println!();
        print!("  reverb regs[16..32]          :");
        for r in &rr[16..] {
            print!(" {r:04X}");
        }
        println!();
    }

    println!("  voices (idx: active reverb vol_l vol_r pitch):");
    let voices = spu.voices();
    for (i, v) in voices.iter().enumerate() {
        let active = v.is_active();
        if !all && !active {
            continue;
        }
        let rev = reverb_mask.map(|m| m & (1 << i) != 0).unwrap_or(false);
        println!(
            "    {i:>2}: {} {} vol=({:>6},{:>6}) pitch={}",
            if active { "ON " } else { "off" },
            if rev { "REV" } else { "-  " },
            v.vol_left.unwrap_or(0),
            v.vol_right.unwrap_or(0),
            v.pitch.map(|p| format!("0x{p:04X}")).unwrap_or_default(),
        );
    }
    Ok(())
}
