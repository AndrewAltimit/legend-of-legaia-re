use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "tim", about = "PSX TIM texture parser + PNG exporter")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print metadata for a TIM file (header, CLUT block, image dims).
    Info { path: PathBuf },
    /// Convert a single TIM to PNG.
    ///
    /// For CLUT-bearing TIMs, --clut selects the palette row (default 0).
    /// With --all-cluts, emit one PNG per CLUT row.
    Convert {
        path: PathBuf,
        /// Output PNG path. Defaults to `<path>.png` (or `<path>_clut<N>.png` with --all-cluts).
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        clut: usize,
        #[arg(long)]
        all_cluts: bool,
    },
    /// Recursively convert every .tim under a directory to .png.
    ConvertDir {
        dir: PathBuf,
        /// Output directory. Defaults to mirroring the input layout next to it.
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        all_cluts: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info { path } => info(&path),
        Cmd::Convert {
            path,
            out,
            clut,
            all_cluts,
        } => convert(&path, out.as_deref(), clut, all_cluts),
        Cmd::ConvertDir {
            dir,
            out,
            all_cluts,
        } => convert_dir(&dir, out.as_deref(), all_cluts),
    }
}

fn info(path: &Path) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let tim = legaia_tim::parse(&buf)?;
    println!("file:     {}", path.display());
    println!("flags:    0x{:08x}", tim.flags);
    println!("mode:     {:?}", tim.mode);
    if let Some(c) = &tim.clut {
        println!(
            "clut:     fb=({},{}) w={} h={} entries={} (palettes={})",
            c.fb_x,
            c.fb_y,
            c.w,
            c.h,
            c.entries.len(),
            c.n_palettes(tim.mode)
        );
    } else {
        println!("clut:     none");
    }
    println!(
        "image:    fb=({},{}) fb_w={} h={} pixel_dims={}x{} bytes={}",
        tim.image.fb_x,
        tim.image.fb_y,
        tim.image.fb_w,
        tim.image.h,
        tim.pixel_width(),
        tim.pixel_height(),
        tim.image.data.len()
    );
    Ok(())
}

fn convert(path: &Path, out: Option<&Path>, clut: usize, all_cluts: bool) -> Result<()> {
    let buf = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let tim = legaia_tim::parse(&buf)?;
    let n = tim
        .clut
        .as_ref()
        .map(|c| c.n_palettes(tim.mode))
        .unwrap_or(1);

    if all_cluts && tim.clut.is_some() {
        let stem = derive_stem(path);
        for i in 0..n {
            let target = match out {
                Some(o) if o.is_dir() => o.join(format!("{}_clut{:02}.png", stem, i)),
                Some(o) => {
                    // Treat user-given out as a stem template if all_cluts.
                    let parent = o.parent().unwrap_or_else(|| Path::new("."));
                    let s = o.file_stem().and_then(|s| s.to_str()).unwrap_or(&stem);
                    parent.join(format!("{}_clut{:02}.png", s, i))
                }
                None => default_out(path, Some(i)),
            };
            write_decoded(&tim, i, &target)?;
            println!("{} (clut {})", target.display(), i);
        }
    } else {
        let target: PathBuf = out
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| default_out(path, None));
        write_decoded(&tim, clut, &target)?;
        println!("{}", target.display());
    }
    Ok(())
}

fn write_decoded(tim: &legaia_tim::Tim, clut: usize, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let rgba = legaia_tim::decode_rgba8(tim, clut)?;
    legaia_tim::write_png(target, tim.pixel_width(), tim.pixel_height(), &rgba)?;
    Ok(())
}

fn convert_dir(dir: &Path, out: Option<&Path>, all_cluts: bool) -> Result<()> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", dir.display()))?;
    let mut tims = vec![];
    walk_tims(&dir, &mut tims)?;
    if tims.is_empty() {
        bail!("no .tim files found under {}", dir.display());
    }
    let mut ok = 0usize;
    let mut fail = 0usize;
    for t in &tims {
        let target_root = match out {
            Some(o) => o.to_path_buf(),
            None => dir.clone(),
        };
        let rel = t.strip_prefix(&dir).unwrap_or(t);
        let target = target_root.join(rel).with_extension("png");
        let buf = match std::fs::read(t) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {}: {}", t.display(), e);
                fail += 1;
                continue;
            }
        };
        let tim = match legaia_tim::parse(&buf) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("parse {}: {}", t.display(), e);
                fail += 1;
                continue;
            }
        };
        let n = if all_cluts {
            tim.clut
                .as_ref()
                .map(|c| c.n_palettes(tim.mode))
                .unwrap_or(1)
        } else {
            1
        };
        for i in 0..n {
            let target_i = if all_cluts && tim.clut.is_some() {
                let stem = target
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("tim")
                    .to_string();
                let parent = target.parent().unwrap_or_else(|| Path::new("."));
                parent.join(format!("{}_clut{:02}.png", stem, i))
            } else {
                target.clone()
            };
            match write_decoded(&tim, i, &target_i) {
                Ok(()) => {
                    ok += 1;
                }
                Err(e) => {
                    eprintln!("write {}: {}", target_i.display(), e);
                    fail += 1;
                }
            }
        }
    }
    eprintln!(
        "converted {} OK, {} failed (out of {} TIMs)",
        ok,
        fail,
        tims.len()
    );
    Ok(())
}

fn walk_tims(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_tims(&path, out)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("tim"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

fn derive_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tim")
        .to_string()
}

fn default_out(path: &Path, clut_idx: Option<usize>) -> PathBuf {
    match clut_idx {
        Some(i) => {
            let stem = derive_stem(path);
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            parent.join(format!("{}_clut{:02}.png", stem, i))
        }
        None => path.with_extension("png"),
    }
}
