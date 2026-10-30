//! `arts.toml`: new Tactical Arts and the chains that end in them.
//!
//! ```toml
//! [arts.ember_cascade]
//! character = "vahn"
//! name = "Ember Cascade"
//! commands = "LRDUR"          # Left Right Down Up Right
//! ap = 30
//! segments = [
//!   { art = "Pyro Pummel", power = ["U22", "L22"] },
//!   { art = 0x27, power = ["L28"] },          # Somersault's clip
//! ]
//! rider = { status = "venom", chance = 40 }
//! learn = { level = 30 }
//!
//! [arts.rebirth_blaze]
//! character = "vahn"
//! name = "Rebirth Blaze"
//! segments = [{ art = "Tornado Flame (Miracle)" }]
//! damage_pct = 160
//!
//! [chains.rebirth_blaze]
//! character = "vahn"
//! arts = ["ember_cascade", 0x27]   # a pack art then retail Somersault
//! finisher = "rebirth_blaze"
//! ```
//!
//! A pack art is performed with the character's **own retail art clips**:
//! each segment names a retail art (by its action constant or its name) whose
//! battle clip plays, with the pack's per-beat power bytes in place of the
//! clip's own. No animation data is carried. The arts input recognises the
//! command string as one more record at the end of the character's art bank,
//! so a retail art keeps matching exactly as before. The engine side is
//! `legaia_engine_expansion::arts`; see `docs/formats/expansion-pack-arts.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::items::{Character, ItemRef, Status};
use crate::quest::Prerequisites;

/// The longest art or chain name the battle label draws.
pub const MAX_NAME_LEN: usize = 20;
/// Segments (retail clips) one pack art may chain.
pub const MAX_SEGMENTS: usize = 8;
/// Shortest command string a pack art may have. Retail's bank walk ends at a
/// one-arrow full match (`0x801EF420`), so a one-arrow art would never fire.
pub const MIN_COMMANDS: usize = 2;
/// Longest command string: the arts entry holds at most nine arrows.
pub const MAX_COMMANDS: usize = 9;
/// The per-character byte budget for pack-art clips: the queue bytes
/// `0x33..=0x7F` above retail's last art constant and below the queue's mark
/// bit.
pub const MAX_SEGMENT_BYTES: usize = 0x7F - 0x33 + 1;
/// The Spirit gauge's top.
pub const MAX_AP: u8 = 100;
/// Highest `damage_pct`.
pub const MAX_DAMAGE_PCT: u16 = 400;
/// Links one chain may list before its finisher.
pub const MAX_CHAIN_LINKS: usize = 6;

/// The `arts.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArtSection {
    /// Pack arts by key.
    pub arts: BTreeMap<String, ArtDef>,
    /// Miracle-style chains by key: a run of arts in one turn that ends in a
    /// pack finisher.
    pub chains: BTreeMap<String, ChainDef>,
}

/// One pack art.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtDef {
    /// Who performs it.
    pub character: Character,
    /// The name the battle label and the arts lists show.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The arrows, `L` / `R` / `U` / `D` (spaces and commas ignored). Empty
    /// for a chain-only finisher.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commands: String,
    /// What the art costs from the Spirit gauge (`0..=100`).
    #[serde(default)]
    pub ap: u8,
    /// The retail clips the art plays, in order.
    pub segments: Vec<SegmentDef>,
    /// Percent of each hit's damage the art lands (`100` = as rolled).
    #[serde(default = "pct_100", skip_serializing_if = "is_100")]
    pub damage_pct: u16,
    /// The element each hit strikes with (`0..=6`, the affinity matrix's
    /// row); none for retail's neutral swing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<u8>,
    /// A status the art's hits may inflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rider: Option<Rider>,
    /// When the art becomes known.
    #[serde(default, skip_serializing_if = "Learn::is_empty")]
    pub learn: Learn,
}

fn pct_100() -> u16 {
    100
}

fn is_100(v: &u16) -> bool {
    *v == 100
}

impl ArtDef {
    /// The command bytes (`1` Left, `2` Right, `3` Down, `4` Up - the art
    /// record's own encoding), `None` when the string holds anything else.
    pub fn command_bytes(&self) -> Option<Vec<u8>> {
        parse_commands(&self.commands)
    }

    /// Whether the art is reached only as a chain's finisher.
    pub fn chain_only(&self) -> bool {
        self.commands.trim().is_empty()
    }
}

/// Parse an arrow string into art-record command bytes.
pub fn parse_commands(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for c in s.chars() {
        out.push(match c.to_ascii_uppercase() {
            'L' => 1,
            'R' => 2,
            'D' => 3,
            'U' => 4,
            ' ' | ',' | '-' => continue,
            _ => return None,
        });
    }
    Some(out)
}

/// The arrow string of command bytes (the inverse of [`parse_commands`]).
pub fn commands_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| match b {
            1 => 'L',
            2 => 'R',
            3 => 'D',
            4 => 'U',
            _ => '?',
        })
        .collect()
}

/// One retail clip of a pack art.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentDef {
    /// The retail art whose clip plays: its action constant (`0x1B..=0x32`)
    /// or its name.
    pub art: RetailArt,
    /// Per-beat power bytes in place of the clip's own, one per hit beat
    /// (at most four). Empty keeps the clip's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub power: Vec<PowerDef>,
    /// The clip frames its hits land on, in place of the clip's own (at
    /// most four, ascending). Empty keeps the clip's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<u8>,
}

/// A retail art, by action constant or name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RetailArt {
    Constant(u8),
    Name(String),
}

impl Default for RetailArt {
    fn default() -> Self {
        Self::Constant(0x1B)
    }
}

impl RetailArt {
    /// The action constant for `character`, resolving a name against the
    /// character's art table.
    pub fn constant(&self, character: Character) -> Option<u8> {
        let ch = art_character(character);
        match self {
            Self::Constant(c) => legaia_art::ActionConstant::from_byte(*c)
                .filter(|a| legaia_art::tables::is_art(ch, *a))
                .map(|a| a.as_byte()),
            Self::Name(n) => (0x1Bu8..=0x32).find(|&c| {
                legaia_art::ActionConstant::from_byte(c)
                    .and_then(|a| legaia_art::tables::art_name(ch, a))
                    .is_some_and(|name| name.eq_ignore_ascii_case(n.trim()))
            }),
        }
    }
}

/// The `legaia_art` character of a pack character.
pub fn art_character(c: Character) -> legaia_art::Character {
    match c {
        Character::Vahn => legaia_art::Character::Vahn,
        Character::Noa => legaia_art::Character::Noa,
        Character::Gala => legaia_art::Character::Gala,
    }
}

/// One power byte: the raw art-record byte, or `U12`..`U28` / `L12`..`L28`
/// (the facet the hit tests - upper or lower defence - and its multiplier).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PowerDef {
    Byte(u8),
    Text(String),
}

/// The five multipliers a power byte can carry
/// (`MULT = [12, 18, 20, 22, 28]`, overlay `0898` `0x801F64EC`).
pub const POWER_MULTIPLIERS: [u8; 5] = [12, 18, 20, 22, 28];

impl PowerDef {
    /// The art-record power byte: `U` multipliers on `0x16..=0x1A`, `L` on
    /// `0x1B..=0x1F` (docs/formats/art-data.md, power encoding).
    pub fn byte(&self) -> Option<u8> {
        match self {
            Self::Byte(b) => (0x0C..=0x1F).contains(b).then_some(*b),
            Self::Text(t) => {
                let t = t.trim();
                let (facet, n) = t.split_at(t.char_indices().nth(1).map_or(t.len(), |(i, _)| i));
                let base = match facet.to_ascii_uppercase().as_str() {
                    "U" => 0x16,
                    "L" => 0x1B,
                    _ => return None,
                };
                let m: u8 = n.parse().ok()?;
                let i = POWER_MULTIPLIERS.iter().position(|&x| x == m)?;
                Some(base + i as u8)
            }
        }
    }
}

/// The `U28`-style label of a power byte.
pub fn power_label(b: u8) -> Option<String> {
    if !(0x0C..=0x1F).contains(&b) {
        return None;
    }
    let k = b - 0x0C;
    let facet = if k % 10 < 5 { 'U' } else { 'L' };
    Some(format!("{facet}{}", POWER_MULTIPLIERS[usize::from(k % 5)]))
}

/// A status a pack art or spell may inflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rider {
    pub status: Status,
    /// Percent per landed hit (`1..=100`).
    pub chance: u8,
}

/// When a pack art or spell becomes known: every set condition must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Learn {
    /// The character's own level must be at least this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// The pack gate (quests, flags, bosses, party level).
    #[serde(skip_serializing_if = "Prerequisites::is_empty")]
    pub requires: Prerequisites,
    /// An item the party must hold (a scroll, a book); kept, not used up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemRef>,
}

impl Learn {
    /// Whether nothing is required: known from the start.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A chain: when one turn's arts end with [`Self::arts`], the finisher
/// plays after them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainDef {
    pub character: Character,
    /// The arts the turn must end with, in order: a retail action constant
    /// or a pack art key.
    pub arts: Vec<ArtLink>,
    /// The pack art that plays after them.
    pub finisher: String,
}

/// One link of a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArtLink {
    /// A retail art constant (`0x1B..=0x32`).
    Retail(u8),
    /// A pack art key.
    Pack(String),
}

impl crate::validate::SectionRules for ArtSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for key in self.arts.keys() {
            cx.declare("art", key, &format!("arts.{key}"));
        }
        for key in self.chains.keys() {
            cx.declare("art_chain", key, &format!("chains.{key}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        let mut combos: BTreeMap<(Character, Vec<u8>), &str> = BTreeMap::new();
        let mut budget: BTreeMap<Character, usize> = BTreeMap::new();
        for (key, a) in &self.arts {
            let at = format!("arts.{key}");
            check_name(cx, &at, &a.name);
            if !a.description.is_ascii() {
                cx.warning(&at, "the dialog font draws ASCII only");
            }
            match a.command_bytes() {
                None => cx.error(&format!("{at}.commands"), "commands are L, R, U and D only"),
                Some(c) if c.is_empty() => {
                    if !self.chains.values().any(|ch| ch.finisher == *key) {
                        cx.warning(
                            &format!("{at}.commands"),
                            "an art with no commands is reached only as a chain finisher, and no chain names it",
                        );
                    }
                }
                Some(c) if c.len() < MIN_COMMANDS => cx.error(
                    &format!("{at}.commands"),
                    format!(
                        "at least {MIN_COMMANDS} arrows (a one-arrow match ends the bank walk)"
                    ),
                ),
                Some(c) if c.len() > MAX_COMMANDS => cx.error(
                    &format!("{at}.commands"),
                    format!("at most {MAX_COMMANDS} arrows"),
                ),
                Some(c) => {
                    if let Some(prev) = combos.insert((a.character, c), key) {
                        cx.error(
                            &format!("{at}.commands"),
                            format!("the same commands as art {prev:?}"),
                        );
                    }
                }
            }
            if a.ap > MAX_AP {
                cx.error(&format!("{at}.ap"), "AP is 0..=100");
            }
            if a.segments.is_empty() {
                cx.error(
                    &format!("{at}.segments"),
                    "an art needs at least one segment",
                );
            } else if a.segments.len() > MAX_SEGMENTS {
                cx.error(
                    &format!("{at}.segments"),
                    format!("at most {MAX_SEGMENTS} segments"),
                );
            }
            *budget.entry(a.character).or_default() += a.segments.len();
            for (i, s) in a.segments.iter().enumerate() {
                let sat = format!("{at}.segments[{i}]");
                if s.art.constant(a.character).is_none() {
                    cx.error(
                        &format!("{sat}.art"),
                        format!("{:?} is not one of {:?}'s retail arts", s.art, a.character),
                    );
                }
                if s.power.len() > 4 {
                    cx.error(&format!("{sat}.power"), "at most four power bytes");
                }
                for (j, p) in s.power.iter().enumerate() {
                    if p.byte().is_none() {
                        cx.error(
                            &format!("{sat}.power[{j}]"),
                            "a power byte is 0x0C..=0x1F or U12..U28 / L12..L28 (12, 18, 20, 22, 28)",
                        );
                    }
                }
                if s.beats.len() > 4 {
                    cx.error(&format!("{sat}.beats"), "at most four beats");
                }
                if s.beats.contains(&0) || s.beats.windows(2).any(|w| w[0] >= w[1]) {
                    cx.error(
                        &format!("{sat}.beats"),
                        "beats are ascending clip frames from 1",
                    );
                }
                if !s.beats.is_empty() && !s.power.is_empty() && s.beats.len() != s.power.len() {
                    cx.warning(
                        &format!("{sat}.beats"),
                        "beats and power differ in length; each beat takes the power byte at its index",
                    );
                }
            }
            if a.damage_pct == 0 || a.damage_pct > MAX_DAMAGE_PCT {
                cx.error(
                    &format!("{at}.damage_pct"),
                    format!("damage_pct is 1..={MAX_DAMAGE_PCT}"),
                );
            }
            if a.element.is_some_and(|e| e > 6) {
                cx.error(&format!("{at}.element"), "element must be 0..=6");
            }
            if let Some(r) = &a.rider {
                check_rider(cx, &format!("{at}.rider"), r);
            }
            check_learn(cx, &format!("{at}.learn"), &a.learn);
        }
        for (c, n) in budget {
            if n > MAX_SEGMENT_BYTES {
                cx.error(
                    "arts",
                    format!(
                        "{c:?}'s arts use {n} segments; a character has {MAX_SEGMENT_BYTES} clip slots"
                    ),
                );
            }
        }
        for (key, c) in &self.chains {
            let at = format!("chains.{key}");
            if c.arts.is_empty() {
                cx.error(&format!("{at}.arts"), "a chain lists at least one art");
            } else if c.arts.len() > MAX_CHAIN_LINKS {
                cx.error(
                    &format!("{at}.arts"),
                    format!("at most {MAX_CHAIN_LINKS} arts"),
                );
            }
            for (i, l) in c.arts.iter().enumerate() {
                let lat = format!("{at}.arts[{i}]");
                match l {
                    ArtLink::Retail(b) => {
                        if RetailArt::Constant(*b).constant(c.character).is_none() {
                            cx.error(
                                &lat,
                                format!("0x{b:02X} is not one of {:?}'s retail arts", c.character),
                            );
                        }
                    }
                    ArtLink::Pack(k) => {
                        cx.reference("art", k, &lat);
                        if let Some(a) = self.arts.get(k)
                            && a.character != c.character
                        {
                            cx.error(&lat, format!("art {k:?} is {:?}'s", a.character));
                        }
                    }
                }
            }
            cx.reference("art", &c.finisher, &format!("{at}.finisher"));
            if let Some(a) = self.arts.get(&c.finisher)
                && a.character != c.character
            {
                cx.error(
                    &format!("{at}.finisher"),
                    format!("art {:?} is {:?}'s", c.finisher, a.character),
                );
            }
        }
    }
}

pub(crate) fn check_name(cx: &mut crate::validate::Checker<'_>, at: &str, name: &str) {
    let n = name.trim();
    if n.is_empty() {
        cx.error(&format!("{at}.name"), "no name");
    } else if n.chars().count() > MAX_NAME_LEN {
        cx.error(
            &format!("{at}.name"),
            format!("the name is longer than {MAX_NAME_LEN} characters"),
        );
    } else if !n.is_ascii() {
        cx.warning(&format!("{at}.name"), "the dialog font draws ASCII only");
    }
}

pub(crate) fn check_rider(cx: &mut crate::validate::Checker<'_>, at: &str, r: &Rider) {
    if r.chance == 0 || r.chance > 100 {
        cx.error(&format!("{at}.chance"), "chance is 1..=100");
    }
}

pub(crate) fn check_learn(cx: &mut crate::validate::Checker<'_>, at: &str, l: &Learn) {
    if l.level.is_some_and(|v| v == 0 || v > 99) {
        cx.error(&format!("{at}.level"), "level is 1..=99");
    }
    l.requires.check(cx, &format!("{at}.requires"));
    if let Some(i) = &l.item {
        i.check(cx, &format!("{at}.item"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_power_parse() {
        assert_eq!(parse_commands("L R-d,U"), Some(vec![1, 2, 3, 4]));
        assert_eq!(parse_commands("LX"), None);
        assert_eq!(commands_string(&[1, 2, 3, 4]), "LRDU");
        assert_eq!(PowerDef::Text("U28".into()).byte(), Some(0x1A));
        assert_eq!(PowerDef::Text("l20".into()).byte(), Some(0x1D));
        assert_eq!(PowerDef::Text("L19".into()).byte(), None);
        assert_eq!(PowerDef::Byte(0x1F).byte(), Some(0x1F));
        assert_eq!(PowerDef::Byte(0x20).byte(), None);
        for b in 0x16..=0x1F {
            let label = power_label(b).unwrap();
            assert_eq!(PowerDef::Text(label).byte(), Some(b));
        }
    }

    #[test]
    fn retail_art_names_resolve_per_character() {
        let som = RetailArt::Name("somersault".into());
        assert_eq!(som.constant(Character::Vahn), Some(0x27));
        assert_eq!(RetailArt::Constant(0x31).constant(Character::Vahn), None);
        assert_eq!(
            RetailArt::Constant(0x31).constant(Character::Noa),
            Some(0x31)
        );
        assert_eq!(
            RetailArt::Name("Love You".into()).constant(Character::Noa),
            Some(0x31)
        );
        assert_eq!(RetailArt::Constant(0x19).constant(Character::Vahn), None);
    }

    fn issues(arts: &str, spells: &str) -> Vec<String> {
        let mut files = crate::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\nschema = 1\ndisc = \"SCUS-94254\"\n"
                .to_vec(),
        );
        files.insert(
            "battle.toml".into(),
            b"[monsters.drake]\nbase = { kind = \"monster\", id = 9 }\n".to_vec(),
        );
        files.insert("arts.toml".into(), arts.as_bytes().to_vec());
        files.insert("spells.toml".into(), spells.as_bytes().to_vec());
        crate::validate::validate_files(&files, None)
            .into_iter()
            .filter(|i| i.severity == crate::validate::Severity::Error)
            .map(|i| format!("{} {}", i.field, i.message))
            .collect()
    }

    #[test]
    fn validation_catches_what_the_runtime_cannot_play() {
        let ok = r#"
            [arts.a]
            character = "vahn"
            name = "A"
            commands = "RURDU"
            segments = [{ art = "Somersault", power = ["L28"] }]
            [arts.fin]
            character = "vahn"
            name = "Fin"
            segments = [{ art = 0x2A }]
            [chains.c]
            character = "vahn"
            arts = ["a", 0x27]
            finisher = "fin"
        "#;
        assert!(issues(ok, "").is_empty(), "{:?}", issues(ok, ""));
        let bad = r#"
            [arts.a]
            character = "vahn"
            name = "A"
            commands = "RUX"
            ap = 101
            segments = [{ art = 0x31, power = ["L19"], beats = [9, 4] }]
            damage_pct = 0
            [arts.b]
            character = "vahn"
            name = "B"
            commands = "L"
            segments = []
            [arts.n]
            character = "noa"
            name = "N"
            commands = "UDUDU"
            segments = [{ art = "Love You" }]
            [chains.c]
            character = "vahn"
            arts = ["n", 0x31]
            finisher = "missing"
        "#;
        let got = issues(bad, "");
        for want in [
            "arts.a.commands",
            "arts.a.ap",
            "arts.a.segments[0].art",
            "arts.a.segments[0].power[0]",
            "arts.a.segments[0].beats",
            "arts.a.damage_pct",
            "arts.b.commands",
            "arts.b.segments",
            "chains.c.arts[0]",
            "chains.c.arts[1]",
            "chains.c.finisher",
        ] {
            assert!(got.iter().any(|g| g.starts_with(want)), "{want}: {got:#?}");
        }
        assert!(
            !got.iter().any(|g| g.starts_with("arts.n")),
            "Noa's Love You is hers: {got:#?}"
        );
        let spells = r#"
            [spells.good]
            name = "Good"
            base = 0x81
            learn = { capture = "drake" }
            [spells.bad]
            name = "Bad"
            base = 0x40
            power = 0
            element = 9
            status = { status = "venom", chance = 0 }
            learn = { capture = "nobody" }
        "#;
        let got = issues("", spells);
        for want in [
            "spells.bad.base",
            "spells.bad.power",
            "spells.bad.element",
            "spells.bad.status.chance",
            "spells.bad.learn.capture",
        ] {
            assert!(got.iter().any(|g| g.starts_with(want)), "{want}: {got:#?}");
        }
        assert!(
            !got.iter().any(|g| g.starts_with("spells.good")),
            "{got:#?}"
        );
    }

    #[test]
    fn toml_shape() {
        let s: ArtSection = toml::from_str(
            r#"
            [arts.ember]
            character = "vahn"
            name = "Ember"
            commands = "LRDU"
            ap = 20
            segments = [{ art = "Pyro Pummel", power = ["U22", 0x1D] }, { art = 0x27 }]
            rider = { status = "venom", chance = 40 }
            learn = { level = 30, requires = { bosses = ["x"] } }

            [chains.c]
            character = "vahn"
            arts = ["ember", 0x27]
            finisher = "ember"
            "#,
        )
        .unwrap();
        let a = &s.arts["ember"];
        assert_eq!(a.command_bytes(), Some(vec![1, 2, 3, 4]));
        assert_eq!(a.segments[0].art.constant(a.character), Some(0x23));
        assert_eq!(a.damage_pct, 100);
        assert_eq!(s.chains["c"].arts[1], ArtLink::Retail(0x27));
        let back: ArtSection = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
