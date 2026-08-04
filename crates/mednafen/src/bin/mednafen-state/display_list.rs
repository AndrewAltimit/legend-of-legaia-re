//! `display-list` - read a frame's libgpu ordering table out of a RAM image.
//!
//! The premise is the same one `widget-draw-sweep.py` uses for UI sprites, but
//! generalised past `SPRT`: libgpu builds each frame's primitive packets in a
//! work buffer in main RAM and links them into an ordering table, so **a RAM
//! image is that frame's display list**. Nothing needs to run - the packets
//! retail submitted are sitting in the bytes.
//!
//! What that buys is a way to ask "does retail actually draw this?" about a
//! surface, offline, instead of inferring it from an emitter's gate condition.
//! Two properties of the answer matter:
//!
//! - **Presence.** A surface retail never draws contributes no packet. Counting
//!   packets by `(clut, tpage)` family says which atlases the frame sampled and
//!   how many primitives came off each.
//! - **Order.** The PSX has no depth buffer; the OT *is* the depth policy, and
//!   the packet later in the chain overwrites the earlier one. So for two
//!   coincident surfaces the winner is whichever walks later, which
//!   [`prim_pool::chain_walk`] reports directly.
//!
//! The pool is found rather than assumed ([`prim_pool::find_pools`]) - the old
//! `POOL_BASE_DEFAULT` is a world-map constant and does not hold for scenes.

use anyhow::{Context, Result, bail};
use legaia_mednafen::{
    SaveState, extract::PSX_RAM_KSEG0, game_anchors, prim_pool, prim_pool::Prim,
};
use std::collections::BTreeMap;
use std::path::Path;

/// Load a whole main-RAM image from either a mednafen save state or a raw
/// 2 MiB dump. PCSX-Redux states are handled by `pcsxr-state extract` on the
/// script side (this crate cannot depend on `legaia-pcsxr` - the dependency
/// runs the other way).
fn load_ram(path: &Path) -> Result<Vec<u8>> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if raw.len() == legaia_mednafen::PSX_RAM_SIZE {
        return Ok(raw);
    }
    let st = SaveState::from_path(path).with_context(|| {
        format!(
            "{} is neither a 2 MiB RAM dump nor a mednafen state",
            path.display()
        )
    })?;
    Ok(st.main_ram()?.to_vec())
}

/// Report primitives whose projected screen geometry coincides.
///
/// This is the measurement behind "does retail draw both copies?". Two meshes
/// placed at one translation whose surfaces coincide project to the *same*
/// screen polygon, so if retail submits both, the frame holds pairs of packets
/// with identical vertices - and if it submits only one (because the scripts
/// swap state/morph variants rather than stacking them), there are no pairs at
/// all. The PSX has no depth buffer, so when a pair does exist the one with the
/// higher chain order is the one whose pixels survive.
///
/// Only the walked chain is used, never the raw pool: a packet's draw order is
/// what makes the answer actionable, and address order is not draw order.
fn report_coincident(chained: &[prim_pool::ChainedPrim], top: usize, min_area: i32) {
    if chained.is_empty() {
        println!("\n[coincident] no walked chain - nothing to compare");
        return;
    }
    let mut skipped_small = 0usize;
    let mut groups: BTreeMap<Vec<(i16, i16)>, Vec<usize>> = BTreeMap::new();
    for (i, c) in chained.iter().enumerate() {
        // Sprites are point-like and coincide constantly (glyph cells at one
        // seat); they are not what this question is about.
        if matches!(c.prim, Prim::Sprt8 { .. } | Prim::Sprt16 { .. }) {
            continue;
        }
        // Distant geometry projects to 1-3 pixel slivers, and slivers coincide
        // with each other constantly without saying anything about stacked
        // meshes - a bounding box of 2x0 px is not a surface. The floor keeps
        // the report about surfaces big enough for a stack to be visible.
        let b = c.prim.bounds();
        let area = (b.2 - b.0) as i32 * (b.3 - b.1) as i32;
        if area < min_area {
            skipped_small += 1;
            continue;
        }
        let mut key = c.prim.verts();
        key.sort();
        if key.first() == key.last() {
            continue;
        }
        groups.entry(key).or_default().push(i);
    }
    println!(
        "\n[coincident] min screen area {min_area} px^2 \
         ({skipped_small} sub-threshold packets excluded)"
    );
    let mut dups: Vec<_> = groups.into_iter().filter(|(_, v)| v.len() > 1).collect();
    dups.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));

    let total: usize = dups.iter().map(|(_, v)| v.len()).sum();
    println!(
        "[coincident] {} screen-coincident group(s), {} packets involved",
        dups.len(),
        total,
    );
    if dups.is_empty() {
        println!(
            "    NONE - every surface in this frame is submitted once. \
             Retail does not stack coincident copies here."
        );
        return;
    }
    for (verts, members) in dups.iter().take(top) {
        println!("    verts {:?}", verts);
        for &m in members {
            let c = &chained[m];
            let ct = match c.prim.clut_tpage() {
                Some((cl, Some(t))) => format!("clut={cl:04X} tpage={t:04X}"),
                Some((cl, None)) => format!("clut={cl:04X} tpage=----"),
                None => "untextured".to_string(),
            };
            println!(
                "        order={:<6} {:<9} {}{}",
                c.order,
                c.prim.kind(),
                ct,
                if m == *members.last().unwrap() {
                    "   <- drawn last, wins"
                } else {
                    ""
                }
            );
        }
    }
}

/// `(clut, tpage)` key for the texture-family census. `tpage` is `-1` for
/// sprites, which inherit the last `DR_TPAGE` rather than carrying one.
type TextureFamily = (u16, i32);
/// Per-family roll-up: packet count plus the union of their screen bounds
/// `(min_x, min_y, max_x, max_y)`.
type FamilyStats = (usize, (i16, i16, i16, i16));

#[allow(clippy::too_many_arguments)]
pub fn cmd_display_list(
    save: &Path,
    pool_base: Option<u32>,
    pool_end: Option<u32>,
    min_prims: usize,
    top: usize,
    list: bool,
    coincident: bool,
    min_area: i32,
    all_ots: bool,
    ot_addr: Option<u32>,
    json_out: Option<&Path>,
) -> Result<()> {
    let ram = load_ram(save)?;
    let id = game_anchors::identify(&ram);

    println!("[display-list] {}", save.display());
    println!(
        "[display-list] scene={} mode={} ({:#04x})",
        if id.scene.is_empty() {
            "(none)"
        } else {
            &id.scene
        },
        game_anchors::game_mode_label(id.game_mode),
        id.game_mode
    );

    // Locate the pool: explicit window if given, otherwise scan.
    let region = match (pool_base, pool_end) {
        (Some(b), Some(e)) => {
            if e <= b {
                bail!("pool_end <= pool_base");
            }
            prim_pool::PoolRegion {
                start: b,
                end: e,
                prims: 0,
            }
        }
        _ => {
            let pools = prim_pool::find_pools(&ram, PSX_RAM_KSEG0, min_prims);
            println!("[display-list] {} candidate pool run(s):", pools.len());
            for (i, p) in pools.iter().take(8).enumerate() {
                println!(
                    "    #{i} 0x{:08X}..0x{:08X}  {:>6} prims  ({} KB)",
                    p.start,
                    p.end,
                    p.prims,
                    p.len() / 1024
                );
            }
            match pools.into_iter().next() {
                Some(p) => p,
                None => {
                    println!(
                        "[display-list] NO primitive pool found (min_prims={min_prims}). \
                         A state captured mid-load can legitimately have no frame built yet."
                    );
                    return Ok(());
                }
            }
        }
    };

    let lo = (region.start - PSX_RAM_KSEG0) as usize;
    let hi = ((region.end - PSX_RAM_KSEG0) as usize).min(ram.len());
    let pool = &ram[lo..hi];
    println!(
        "\n[display-list] using pool 0x{:08X}..0x{:08X} ({} KB)",
        region.start,
        region.end,
        pool.len() / 1024
    );

    let prims = prim_pool::decode(pool, region.start);
    let topo = prim_pool::chain_topology(pool, region.start);
    println!("[display-list] {} packets decoded", prims.len());
    println!(
        "[topology] {} tagged, {} head(s), {} terminator(s), {} linked",
        topo.total_tags,
        topo.heads.len(),
        topo.terminators,
        topo.linked
    );

    // Per-opcode census.
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &prims {
        *by_kind.entry(p.kind()).or_insert(0) += 1;
    }
    println!("\n[kinds]");
    for (k, v) in &by_kind {
        println!("    {k:<10} {v}");
    }

    // Per-(clut, tpage) census: the texture-family view. A "does retail draw
    // this surface?" question is usually a question about one atlas family, so
    // this is the table that answers it.
    let mut fam: BTreeMap<TextureFamily, FamilyStats> = BTreeMap::new();
    for p in &prims {
        if let Some((clut, tpage)) = p.clut_tpage() {
            let key = (clut, tpage.map(|t| t as i32).unwrap_or(-1));
            let b = p.bounds();
            let e = fam
                .entry(key)
                .or_insert((0, (i16::MAX, i16::MAX, i16::MIN, i16::MIN)));
            e.0 += 1;
            e.1.0 = e.1.0.min(b.0);
            e.1.1 = e.1.1.min(b.1);
            e.1.2 = e.1.2.max(b.2);
            e.1.3 = e.1.3.max(b.3);
        }
    }
    let mut fams: Vec<_> = fam.into_iter().collect();
    fams.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    println!("\n[texture families]  (clut, tpage) -> count, screen bounds");
    for ((clut, tpage), (n, b)) in fams.iter().take(top) {
        let tp = if *tpage < 0 {
            "  (inherited)".to_string()
        } else {
            format!("0x{tpage:04X}")
        };
        println!(
            "    clut=0x{clut:04X} tpage={tp:<12} {n:>6}  x[{},{}] y[{},{}]",
            b.0, b.2, b.1, b.3
        );
    }

    // Untextured families: colour only. These are the flat/Gouraud polys, which
    // a textured-only reader misses entirely.
    let untextured = prims.iter().filter(|p| !p.is_textured()).count();
    println!("\n[untextured] {untextured} flat/Gouraud packets (no texture page)");

    // Draw order along the chain. The head lives in the ordering-table array,
    // not in the packet pool, so walk over the whole RAM image starting there -
    // a walk confined to the pool reports one spurious head per packet whose
    // predecessor sits outside the window.
    let ots = prim_pool::find_ot_arrays(&ram, PSX_RAM_KSEG0, 64);
    println!("\n[ot] {} ordering-table array(s):", ots.len());
    for (i, o) in ots.iter().take(6).enumerate() {
        println!(
            "    #{i} 0x{:08X}..0x{:08X}  {} buckets  head=0x{:08X}",
            o.start, o.end, o.buckets, o.head
        );
    }
    // A frame is built from more than one ordering table (retail keeps a pair
    // per double-buffer half), and each is handed to its own `DrawOTag`. Walking
    // just one under-reports the frame, so walk every table that belongs to the
    // selected pool's buffer half - i.e. sits below the pool and within
    // `SAME_BUFFER` bytes of it - and concatenate them in address order.
    //
    // Caveat worth stating: cross-table order is the order the `DrawOTag` calls
    // are made, which is assumed here to match address order. Within one table
    // the order is exact.
    const SAME_BUFFER: u32 = 256 * 1024;
    let mut mine: Vec<&prim_pool::OtArray> = ots
        .iter()
        .filter(|o| o.end <= region.start && region.start - o.start <= SAME_BUFFER)
        .collect();
    mine.sort_by_key(|o| o.start);
    // Retail double-buffers: the two ordering tables of a pair hold frame N and
    // frame N-1, and their packet counts come out near-identical. Merging them
    // makes every primitive appear twice, which is exactly the false positive a
    // coincidence test must not have - the twin is the same surface in the other
    // buffer, not a second mesh. So walk ONE table by default and require
    // `--all-ots` to opt into the merged view.
    if let Some(want) = ot_addr {
        mine.retain(|o| o.start == want || o.head == want);
        if mine.is_empty() {
            println!("[chain] no ordering table at 0x{want:08X}");
        }
    } else if !all_ots && mine.len() > 1 {
        let chosen = *mine.iter().max_by_key(|o| o.buckets).unwrap();
        println!(
            "[chain] {} adjacent tables (double buffer); walking one - \
             pass --all-ots to merge",
            mine.len()
        );
        mine = vec![chosen];
    }
    let mut chained: Vec<prim_pool::ChainedPrim> = Vec::new();
    if mine.is_empty() {
        println!("[chain] no ordering table adjacent to the selected pool");
    }
    for ot in &mine {
        let part = prim_pool::chain_walk(&ram, PSX_RAM_KSEG0, (ot.head - PSX_RAM_KSEG0) as usize);
        println!(
            "[chain] OT 0x{:08X} (head 0x{:08X}) -> {} packets",
            ot.start,
            ot.head,
            part.len()
        );
        for mut c in part {
            c.order = chained.len();
            chained.push(c);
        }
    }
    if !chained.is_empty() {
        println!(
            "[chain] {} packets total in draw order (later index wins)",
            chained.len()
        );
    }

    if coincident {
        report_coincident(&chained, top, min_area);
    }

    if list {
        println!("\n[packets] order  offset    kind        clut/tpage      bounds");
        let src: Vec<(usize, usize, &Prim)> = if !chained.is_empty() {
            chained
                .iter()
                .map(|c| (c.order, c.offset, &c.prim))
                .collect()
        } else {
            prims
                .iter()
                .enumerate()
                .map(|(i, p)| (i, 0usize, p))
                .collect()
        };
        for (order, offset, p) in src {
            let b = p.bounds();
            let ct = match p.clut_tpage() {
                Some((c, Some(t))) => format!("{c:04X}/{t:04X}"),
                Some((c, None)) => format!("{c:04X}/----"),
                None => "----/----".to_string(),
            };
            println!(
                "  {order:>6}  0x{offset:06X}  {:<10}  {ct:<14}  x[{},{}] y[{},{}]",
                p.kind(),
                b.0,
                b.2,
                b.1,
                b.3
            );
        }
    }

    if let Some(path) = json_out {
        #[derive(serde::Serialize)]
        struct Report<'a> {
            file: String,
            scene: &'a str,
            game_mode: u8,
            pool_start: u32,
            pool_end: u32,
            packets: usize,
            kinds: BTreeMap<&'a str, usize>,
            chain: &'a [prim_pool::ChainedPrim],
        }
        let rep = Report {
            file: save.display().to_string(),
            scene: &id.scene,
            game_mode: id.game_mode,
            pool_start: region.start,
            pool_end: region.end,
            packets: prims.len(),
            kinds: by_kind,
            chain: &chained,
        };
        std::fs::write(path, serde_json::to_string_pretty(&rep)?)?;
        println!("\n[display-list] wrote {}", path.display());
    }
    Ok(())
}
