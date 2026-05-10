//! `gamedata-tool` - inspect the curated game-data tables.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use legaia_gamedata::{ArtKind, Character, Database};

#[derive(Parser, Debug)]
#[command(
    about = "Inspect Legend of Legaia game-data tables (arts, magic, items, shops, etc.).",
    long_about = "All data is baked in via include_str!; nothing on disc is required.\n\
                  See data/gamedata/README.md for the source attribution."
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// List entries in a specific table.
    List {
        /// Which table to list: arts | magic | items | weapons | armor |
        ///                       accessories | enemies | bosses | shops |
        ///                       slots | muscle | baka | fishing | characters
        table: String,
        /// Filter `arts` by character (Vahn/Noa/Gala).
        #[arg(long)]
        character: Option<String>,
        /// Filter `arts` by kind (regular/hyper/super/miracle).
        #[arg(long)]
        kind: Option<String>,
        /// Filter `magic` by element.
        #[arg(long)]
        element: Option<String>,
    },
    /// Find one entry by name across the arts / magic / items / weapons /
    /// armor / accessories / enemies tables (case-insensitive substring).
    Find {
        /// Substring to search for.
        query: String,
    },
    /// Resolve a comma-separated player input sequence ("Arms,Ra-Seru,High")
    /// for a character to one or more matching arts.
    ArtsByCommand {
        /// Character (Vahn/Noa/Gala).
        character: String,
        /// Comma-separated tokens (`Arms`, `Ra-Seru`, `High`, `Low`).
        sequence: String,
    },
    /// Show one shop's resolved inventory.
    Shop {
        /// Town name.
        town: String,
        /// Optional shop name (otherwise lists every shop in the town).
        #[arg(long)]
        name: Option<String>,
    },
    /// Dump one table to JSON on stdout.
    DumpJson {
        /// Which table to dump.
        table: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = Database::load();
    match cli.command {
        Cmd::List {
            table,
            character,
            kind,
            element,
        } => list(
            &db,
            &table,
            character.as_deref(),
            kind.as_deref(),
            element.as_deref(),
        ),
        Cmd::Find { query } => find(&db, &query),
        Cmd::ArtsByCommand {
            character,
            sequence,
        } => arts_by_command(&db, &character, &sequence),
        Cmd::Shop { town, name } => shop(&db, &town, name.as_deref()),
        Cmd::DumpJson { table } => dump_json(&db, &table),
    }
}

fn list(
    db: &Database,
    table: &str,
    character: Option<&str>,
    kind: Option<&str>,
    element: Option<&str>,
) -> Result<()> {
    match table {
        "arts" => {
            let want_char = character
                .map(|s| Character::parse(s).unwrap_or_else(|| panic!("unknown character {s:?}")));
            let want_kind = kind.map(|k| match k.to_ascii_lowercase().as_str() {
                "regular" => ArtKind::Regular,
                "hyper" => ArtKind::Hyper,
                "super" => ArtKind::Super,
                "miracle" => ArtKind::Miracle,
                other => panic!("unknown art kind {other:?}"),
            });
            for art in db.arts() {
                if let Some(c) = want_char
                    && art.character != c
                {
                    continue;
                }
                if let Some(k) = want_kind
                    && art.kind != k
                {
                    continue;
                }
                println!(
                    "{:?} {:>3} AP  [{:?}]  {}  {}",
                    art.character,
                    art.ap,
                    art.kind,
                    art.command.join(" "),
                    art.name,
                );
            }
        }
        "magic" => {
            let want_elem = element.map(|s| s.to_ascii_lowercase());
            for sp in db.spells() {
                if let Some(ref e) = want_elem
                    && &sp.element != e
                {
                    continue;
                }
                println!(
                    "{:>20}  {:>4} MP  [{}]  {}  -> {}",
                    sp.name, sp.mp, sp.element, sp.attack, sp.target
                );
            }
        }
        "items" => {
            for it in db.items() {
                let price = it.price.map(|p| format!("{p}G")).unwrap_or_default();
                println!(
                    "{:>22}  [{:>14}]  {:>8}  {}",
                    it.name, it.category, price, it.effect
                );
            }
        }
        "weapons" => {
            for w in db.weapons() {
                let price = w.price.map(|p| format!("{p}G")).unwrap_or_default();
                println!(
                    "{:>20}  ATK {:>3}  best:{:<6}  {:>8}",
                    w.name, w.attack, w.equip_best, price
                );
            }
        }
        "armor" => {
            for a in db.armor() {
                let price = a.price.map(|p| format!("{p}G")).unwrap_or_default();
                println!(
                    "{:>20}  [{:>6}]  UDF {:>3}  LDF {:>3}  {:<6}  {:>8}",
                    a.name, a.slot, a.udf, a.ldf, a.equip, price
                );
            }
        }
        "accessories" => {
            for a in db.accessories() {
                let price = a.price.map(|p| format!("{p}G")).unwrap_or_default();
                let class = a.effect_class.as_deref().unwrap_or("-");
                println!("{:>20}  {:>16}  {:>8}  {}", a.name, class, price, a.effect);
            }
        }
        "enemies" => {
            for e in db.enemies() {
                let elem = e.element.as_deref().unwrap_or("-");
                let drop = e.drop.as_deref().unwrap_or("-");
                let steal = e.steal.as_deref().unwrap_or("-");
                let boss = if e.boss { "[BOSS] " } else { "" };
                println!(
                    "{}{:>26}  @ {:<28}  {}  drop:{}  steal:{}",
                    boss, e.name, e.location, elem, drop, steal
                );
            }
        }
        "bosses" => {
            for b in db.bosses() {
                let arena = b.tournament.as_deref().unwrap_or("-");
                println!(
                    "{:>22}  @ {:<24}  HP {:>6}-{:<6}  arena:{}",
                    b.name, b.location, b.hp_min, b.hp_max, arena
                );
            }
        }
        "shops" => {
            for s in db.shops() {
                let name = s.name.as_deref().or(s.merchant.as_deref()).unwrap_or("-");
                let phase = s.phase.as_deref().unwrap_or("-");
                println!(
                    "{:>20}  {:<24}  phase:{:<14}  {} items",
                    s.town,
                    name,
                    phase,
                    s.inventory.len()
                );
            }
        }
        "slots" => {
            for p in db.slot_prizes() {
                println!(
                    "{:<6}  {:>20}  {:>8} coins",
                    p.location, p.item, p.cost_coins
                );
            }
        }
        "muscle" => {
            for c in db.muscle_dome() {
                println!(
                    "{:<10}  fee:{}  reward:{}  bonus:{}",
                    c.name,
                    c.entry_fee,
                    c.reward_coins,
                    c.reward_first_clear.as_deref().unwrap_or("-")
                );
                for (i, e) in c.enemies.iter().enumerate() {
                    println!("    {:>2}. {}", i + 1, e);
                }
            }
        }
        "baka" => {
            for r in db.baka_fighter() {
                let notes = r.notes.as_deref().unwrap_or("");
                println!("{:>2}: {}  {}", r.round, r.buttons.join(" "), notes);
            }
        }
        "fishing" => {
            for p in db.fishing_prizes() {
                println!(
                    "{:<6}  {:>20}  {:>6} pts",
                    p.location, p.item, p.cost_points
                );
            }
        }
        "characters" => {
            for c in db.characters() {
                println!(
                    "{}  ra-seru:{}  strong:{:?}  weak:{:?}",
                    c.name, c.ra_seru, c.affinity_strong, c.affinity_weak
                );
            }
        }
        other => bail!("unknown table {other:?}"),
    }
    Ok(())
}

fn find(db: &Database, query: &str) -> Result<()> {
    let q = query.to_ascii_lowercase();
    let mut hits = 0;
    for a in db.arts() {
        if a.name.to_ascii_lowercase().contains(&q) {
            println!(
                "[art]        {:?} {} ({} AP, {})",
                a.character,
                a.name,
                a.ap,
                format!("{:?}", a.kind).to_lowercase()
            );
            hits += 1;
        }
    }
    for s in db.spells() {
        if s.name.to_ascii_lowercase().contains(&q) {
            println!("[spell]      {} ({} MP, {})", s.name, s.mp, s.element);
            hits += 1;
        }
    }
    for it in db.items() {
        if it.name.to_ascii_lowercase().contains(&q) || it.key.contains(&q) {
            println!("[item]       {} ({})", it.name, it.category);
            hits += 1;
        }
    }
    for w in db.weapons() {
        if w.name.to_ascii_lowercase().contains(&q) {
            println!("[weapon]     {} (ATK {})", w.name, w.attack);
            hits += 1;
        }
    }
    for a in db.armor() {
        if a.name.to_ascii_lowercase().contains(&q) {
            println!("[armor]      {} ({})", a.name, a.slot);
            hits += 1;
        }
    }
    for a in db.accessories() {
        if a.name.to_ascii_lowercase().contains(&q) {
            println!("[accessory]  {}", a.name);
            hits += 1;
        }
    }
    for e in db.enemies() {
        if e.name.to_ascii_lowercase().contains(&q) {
            println!(
                "[enemy]      {} @ {}{}",
                e.name,
                e.location,
                if e.boss { " (boss)" } else { "" }
            );
            hits += 1;
        }
    }
    if hits == 0 {
        bail!("no matches for {query:?}");
    }
    Ok(())
}

fn arts_by_command(db: &Database, character: &str, sequence: &str) -> Result<()> {
    let chr = Character::parse(character).context("parse character")?;
    let dirs: Vec<u8> = sequence
        .split(',')
        .map(|tok| {
            chr.token_to_byte(tok)
                .with_context(|| format!("unknown token {tok:?}"))
        })
        .collect::<Result<Vec<u8>>>()?;
    if let Some(art) = db.find_art_by_directions(chr, &dirs) {
        println!(
            "{:?} {} ({} AP, {:?}) [action_constant={}]",
            art.character,
            art.name,
            art.ap,
            art.kind,
            art.action_constant
                .map(|c| format!("0x{c:02X}"))
                .unwrap_or_else(|| "-".to_string()),
        );
    } else {
        bail!("no art for {:?} matches {:?} -> {:?}", chr, sequence, dirs);
    }
    Ok(())
}

fn shop(db: &Database, town: &str, name: Option<&str>) -> Result<()> {
    let mut found = false;
    for s in db.shops_in(town) {
        let display_name = s.name.as_deref().or(s.merchant.as_deref()).unwrap_or("-");
        if let Some(want) = name
            && !display_name.eq_ignore_ascii_case(want)
        {
            continue;
        }
        found = true;
        println!(
            "=== {}  /  {}  /  phase:{} ===",
            s.town,
            display_name,
            s.phase.as_deref().unwrap_or("-")
        );
        let entries = db.resolve_inventory(&s.inventory);
        for entry in entries {
            let price = entry
                .price
                .map(|p| format!("{p}G"))
                .unwrap_or_else(|| "(quest)".to_string());
            let featured = if s.featured.iter().any(|f| f == entry.key) {
                "*"
            } else {
                " "
            };
            println!(
                "  {} {:<22}  {:<10}  [{:?}]",
                featured, entry.name, price, entry.category
            );
        }
        println!();
    }
    if !found {
        bail!("no shops match town={town:?} name={name:?}");
    }
    Ok(())
}

fn dump_json(db: &Database, table: &str) -> Result<()> {
    let json = match table {
        "arts" => serde_json::to_string_pretty(db.arts())?,
        "magic" => serde_json::to_string_pretty(db.spells())?,
        "items" => serde_json::to_string_pretty(db.items())?,
        "weapons" => serde_json::to_string_pretty(db.weapons())?,
        "armor" => serde_json::to_string_pretty(db.armor())?,
        "accessories" => serde_json::to_string_pretty(db.accessories())?,
        "enemies" => serde_json::to_string_pretty(db.enemies())?,
        "bosses" => serde_json::to_string_pretty(db.bosses())?,
        "shops" => serde_json::to_string_pretty(db.shops())?,
        "slots" => serde_json::to_string_pretty(db.slot_prizes())?,
        "muscle" => serde_json::to_string_pretty(db.muscle_dome())?,
        "baka" => serde_json::to_string_pretty(db.baka_fighter())?,
        "fishing" => serde_json::to_string_pretty(db.fishing_prizes())?,
        "characters" => serde_json::to_string_pretty(db.characters())?,
        other => bail!("unknown table {other:?}"),
    };
    println!("{json}");
    Ok(())
}
