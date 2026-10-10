//! The per-character Super Art row table: the arrow code packing, one row's
//! encoding, and the pass that builds every row from the disc's art records.

use super::{
    ARROW_BITS, ARROWS_COUNT_SHIFT, ARROWS_STRIDE, ART_BLOCK_BIAS, ART_CONSTANT_BIAS,
    ART_NAME_FIELD_OFF, ART_RECORD_STRIDE, ARTS_CHARACTERS, FIRST_NORMAL_ORDINAL, GLYPH_HI,
    GLYPH_LO_BASE, MARKER_HI, MARKERS_SHIFT, MAX_INPUT_ARROWS, MAX_MARKERS, PERFORMED_COUNT_SHIFT,
    PERFORMED_MASK, STYLE_ART_END, STYLE_DEFAULT, STYLE_SUPER_END, SUP_STRIDE, SUPER_ARTS_PER_CHAR,
};
use crate::super_art_power::{GRID_BIAS, super_arts_for};
use anyhow::{Result, bail};
use legaia_art::queue::{ActionConstant, Character, Command};

/// Two-bit arrow codes = glyph low byte minus [`GLYPH_LO_BASE`].
pub fn arrow_code(c: Command) -> u8 {
    match c {
        Command::Right => 0,
        Command::Left => 1,
        Command::Up => 2,
        Command::Down => 3,
    }
}

// --- Table derivation --------------------------------------------------------

/// One Super Art's row in the injected tables, derived from the user's own disc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtRow {
    pub character: Character,
    /// Display name (from [`legaia_art::SUPER_ARTS`]) - carried for reporting
    /// only; the injected row chases the name out of RAM.
    pub name: &'static str,
    /// Action constant of the finisher, and therefore the art record's grid key.
    pub finisher: u8,
    /// Row of the character's five in the resident trigger table = the bit the
    /// applier detour sets in the performed byte.
    pub trigger_row: u8,
    /// Position among the character's five once sorted by AP (descending; ties
    /// keep trigger-table order) - the row's index in the injected `SUP` table.
    pub sorted_index: u8,
    /// Trigger-chain arts as **display ids**, in trigger order (duplicates kept).
    pub chain_ids: Vec<u8>,
    /// The chain arts' names as the disc's own arts-name table spells them.
    pub chain_names: Vec<String>,
    /// Sum of the chain arts' AP costs - what the row displays and sorts by.
    pub ap: u8,
    /// Lowest display id of this character whose AP is at or below `ap`: the
    /// Super Art is listed before every learned id `>= thr`.
    pub thr: u8,
    /// Byte offset of the record's `+0x10` name from the art-record array
    /// (`4 + (finisher - 0x10) * 0xD0 + 0x10`).
    pub name_offset: u16,
    /// The physical input - what the player types - derived with the retail
    /// tokenizer from the trigger pattern.
    pub input: Vec<Command>,
    /// Input positions the chain's arts end on (the arrows the tokenizer wrote
    /// the starter over), ascending; the last is always the final arrow.
    pub ends: Vec<usize>,
}

impl SuperArtRow {
    /// The input as `L`/`R`/`D`/`U` letters, for reports.
    pub fn input_letters(&self) -> String {
        self.input
            .iter()
            .map(|c| match c {
                Command::Left => 'L',
                Command::Right => 'R',
                Command::Down => 'D',
                Command::Up => 'U',
            })
            .collect()
    }

    /// The style each arrow is drawn in: default, [`STYLE_ART_END`] where a
    /// sub-art ends, [`STYLE_SUPER_END`] on the final arrow.
    pub fn styles(&self) -> Vec<u8> {
        let n = self.input.len();
        (0..n)
            .map(|k| {
                if k + 1 == n {
                    STYLE_SUPER_END
                } else if self.ends.contains(&k) {
                    STYLE_ART_END
                } else {
                    STYLE_DEFAULT
                }
            })
            .collect()
    }

    /// How many style markers the row's glyph string carries: one per change
    /// of style, starting from the default.
    pub fn marker_count(&self) -> usize {
        let mut cur = STYLE_DEFAULT;
        let mut n = 0;
        for st in self.styles() {
            if st != cur {
                n += 1;
                cur = st;
            }
        }
        n
    }

    /// The glyph string routine (F) builds: `[glyphs]`, then per arrow a style
    /// marker whenever the style changes and the arrow glyph itself.
    pub fn glyph_string(&self) -> Vec<u8> {
        let mut out = vec![(self.input.len() + self.marker_count()) as u8];
        let mut cur = STYLE_DEFAULT;
        for (c, st) in self.input.iter().zip(self.styles()) {
            if st != cur {
                out.extend_from_slice(&[MARKER_HI, st]);
                cur = st;
            }
            out.push(GLYPH_HI);
            out.push(GLYPH_LO_BASE + arrow_code(*c));
        }
        out
    }

    /// The packed arrows word (F) expands.
    pub fn packed_arrows(&self) -> [u8; ARROWS_STRIDE as usize] {
        let mut w: u32 = (self.input.len() as u32) << ARROWS_COUNT_SHIFT;
        for (k, &c) in self.input.iter().enumerate() {
            let end = u32::from(self.ends.contains(&k));
            w |= (u32::from(arrow_code(c)) | (end << 2)) << (k as u32 * ARROW_BITS);
        }
        w.to_le_bytes()
    }

    /// The 4-byte `SUP` record: AP, `thr | trigger_row << 5`, and the name
    /// offset with the marker count in its top three bits.
    pub fn sup_record(&self) -> [u8; SUP_STRIDE as usize] {
        let word = self.name_offset | ((self.marker_count() as u16) << MARKERS_SHIFT);
        let [lo_, hi_] = word.to_le_bytes();
        [
            self.ap,
            self.thr | (self.trigger_row << PERFORMED_COUNT_SHIFT),
            lo_,
            hi_,
        ]
    }
}

/// Derive all fifteen rows from `scus`, in `character * 5 + sorted_index`
/// order. Every chain entry has to land on a real arts-name-table row of the
/// same character, every trigger pattern has to derive a unique physical input
/// through the retail tokenizer, and every threshold has to resolve, or the
/// whole thing is refused - the checks that keep the tables honest against the
/// disc rather than against the curated table.
pub fn super_art_rows(scus: &[u8]) -> Result<Vec<SuperArtRow>> {
    let table = legaia_art::arts_table::raw_records_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: parse the SCUS arts-name table"))?;
    let names = legaia_art::arts_table::parse_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: decode the SCUS arts-name table"))?;
    let mut out = Vec::with_capacity(ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);
    for ch in Character::all() {
        let supers = super_arts_for(ch);
        if supers.len() != SUPER_ARTS_PER_CHAR {
            bail!(
                "show-super-arts: {ch:?} has {} Super Arts, expected {SUPER_ARTS_PER_CHAR}",
                supers.len()
            );
        }
        // This character's rows, in table order (= ascending id = descending AP).
        let mut mine: Vec<_> = table.iter().filter(|r| r.character == ch).collect();
        mine.sort_by_key(|r| r.index);
        if mine.is_empty() {
            bail!("show-super-arts: {ch:?} has no arts-name-table rows");
        }
        // The tokenizer catalog: normal arts only, grid order.
        let catalog: Vec<(ActionConstant, &[Command])> = mine
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal >= FIRST_NORMAL_ORDINAL)
            .filter_map(|(_, r)| {
                let c = ActionConstant::from_byte(r.index.checked_add(ART_CONSTANT_BIAS)?)?;
                Some((c, r.commands.as_slice()))
            })
            .collect();

        let mut rows: Vec<SuperArtRow> = Vec::with_capacity(SUPER_ARTS_PER_CHAR);
        for (trigger_row, s) in supers.iter().enumerate() {
            let chain = s.art_sequence();
            if chain.is_empty() {
                bail!(
                    "show-super-arts: Super Art {} has an empty trigger chain - refusing",
                    s.name
                );
            }
            let mut ap: u32 = 0;
            let mut chain_ids = Vec::with_capacity(chain.len());
            let mut chain_names = Vec::with_capacity(chain.len());
            for c in chain.iter().copied() {
                let id = c.checked_sub(ART_CONSTANT_BIAS).ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: {}'s chain entry {c:#x} is below the art-constant \
                         base {ART_CONSTANT_BIAS:#x}",
                        s.name
                    )
                })?;
                let rec = mine.iter().find(|r| r.index == id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: {}'s chain art (constant {c:#x} -> id {id}) has no \
                         row in this disc's arts-name table - refusing",
                        s.name
                    )
                })?;
                ap += u32::from(rec.ap);
                chain_ids.push(id);
                chain_names.push(
                    names
                        .iter()
                        .find(|e| e.character == ch && e.index == id)
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                );
            }
            let ap = u8::try_from(ap).map_err(|_| {
                anyhow::anyhow!(
                    "show-super-arts: {}'s chain costs {ap} AP, past the one byte the row \
                     draws - refusing",
                    s.name
                )
            })?;
            if s.finisher < GRID_BIAS {
                bail!(
                    "show-super-arts: {}'s finisher constant {:#x} is below the art-record grid \
                     base {GRID_BIAS:#x} - refusing",
                    s.name,
                    s.finisher
                );
            }
            let name_offset = ART_BLOCK_BIAS
                + u32::from(s.finisher - GRID_BIAS) * ART_RECORD_STRIDE
                + ART_NAME_FIELD_OFF;
            let name_offset = u16::try_from(name_offset).map_err(|_| {
                anyhow::anyhow!("show-super-arts: {}'s name offset overflows u16", s.name)
            })?;
            // The Super sits before every id whose AP is at or below its own.
            let thr = mine
                .iter()
                .find(|r| r.ap <= ap)
                .map(|r| r.index)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: no art of {ch:?} costs {ap} AP or less - the \
                         threshold cannot resolve",
                    )
                })?;
            if thr > PERFORMED_MASK {
                bail!(
                    "show-super-arts: {}'s threshold id {thr} does not fit the five bits the \
                     record packs it into - refusing",
                    s.name
                );
            }
            let input = legaia_art::derive_super_input(&catalog, s.find).ok_or_else(|| {
                anyhow::anyhow!(
                    "show-super-arts: {}'s trigger pattern derives no unique physical input \
                     against this disc's arts-name table - refusing",
                    s.name
                )
            })?;
            if input.len() > MAX_INPUT_ARROWS {
                bail!(
                    "show-super-arts: {}'s input is {} arrows, past the {MAX_INPUT_ARROWS} \
                     the packing carries - refusing",
                    s.name,
                    input.len()
                );
            }
            let ends = legaia_art::art_ends(&catalog, &input);
            if ends.len() != chain.len() || ends.last() != Some(&(input.len() - 1)) {
                bail!(
                    "show-super-arts: {}'s input {:?} ends {} arts at {:?}, but its chain has \
                     {} - refusing",
                    s.name,
                    input,
                    ends.len(),
                    ends,
                    chain.len()
                );
            }
            if name_offset >= 1 << MARKERS_SHIFT {
                bail!(
                    "show-super-arts: {}'s name offset {name_offset:#x} collides with the marker \
                     count field - refusing",
                    s.name
                );
            }
            rows.push(SuperArtRow {
                character: ch,
                name: s.name,
                finisher: s.finisher,
                trigger_row: trigger_row as u8,
                sorted_index: 0,
                chain_ids,
                chain_names,
                ap,
                thr,
                name_offset,
                input,
                ends,
            });
        }
        for r in &rows {
            if r.marker_count() > MAX_MARKERS {
                bail!(
                    "show-super-arts: {}'s row needs {} style markers, past the {MAX_MARKERS} \
                     the buffer holds - refusing",
                    r.name,
                    r.marker_count()
                );
            }
        }
        // AP-descending, ties in trigger-table order (stable sort).
        rows.sort_by_key(|r| std::cmp::Reverse(r.ap));
        for (k, r) in rows.iter_mut().enumerate() {
            r.sorted_index = k as u8;
        }
        out.extend(rows);
    }
    Ok(out)
}
