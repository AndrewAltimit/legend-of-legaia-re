//! The `translate` subcommand tree.

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum TranslateCmd {
    /// Export every cataloged user-facing string (item / spell / art /
    /// accessory / party names, scene dialog, event-script text) from a disc
    /// into a YAML language pack with empty `translation:` fields.
    ///
    /// The exported pack contains the game's copyrighted text - keep it
    /// local / share only filled translations per your jurisdiction's rules;
    /// never commit it to this repository.
    Export {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// Where to write the pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Produce an empty per-language skeleton from an exported pack: same
    /// keys / sources / budgets, cleared translations, stamped header.
    ///
    /// `--resume` seeds it with the translations of an already-published
    /// (source-less) pack, so a translator can pick up where a shipped pack
    /// left off without anyone redistributing the source text.
    Init {
        /// Target language code (e.g. fr, de, es, it, pt-BR, ja, ru, zh, ko -
        /// note: non-Latin scripts also need a font patch, see the docs).
        #[arg(long)]
        lang: String,
        /// An existing exported pack to derive from...
        #[arg(long, conflicts_with = "input", required_unless_present = "input")]
        from: Option<PathBuf>,
        /// ...or export straight from a disc image.
        #[arg(long, required_unless_present = "from")]
        input: Option<PathBuf>,
        /// Contributor names for the pack header (repeatable).
        #[arg(long)]
        contributor: Vec<String>,
        /// Pre-fill from an existing (working or distributable) pack, matched
        /// by key - e.g. one of the shipped `site/lang/*.yaml` packs.
        #[arg(long)]
        resume: Option<PathBuf>,
        /// Also split the skeleton into chunk files of at most N entries each
        /// (`<output stem>.001.yaml`, ...) for a parallel / bulk fill pass.
        /// Recombine them with `translate merge`.
        #[arg(long, value_name = "N")]
        chunk: Option<usize>,
        /// Where to write the skeleton (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Strip a filled pack down to the **distributable** shape: the filled
    /// entries only, keys + your translations + the byte-budget hint, with
    /// every `source:` / `context:` field (the game's own text) removed.
    ///
    /// This is the shape that is safe to publish / commit.
    Strip {
        /// The filled working pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Where to write the distributable pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
        /// Overwrite the pack's `notes:` header line.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Merge the filled entries of several packs (chunks of a bulk fill, a
    /// shipped pack + your edits, ...) into the first one, matched by key.
    Merge {
        /// Base pack - defines the entry set (keys / sources / budgets).
        #[arg(long)]
        base: PathBuf,
        /// Packs whose translations are merged onto the base, in order.
        #[arg(long = "pack", required = true)]
        packs: Vec<PathBuf>,
        /// Where to write the merged pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Coverage + validation report for a pack: per-section translated/total
    /// counts, plus encodability and budget checks on every filled entry.
    ///
    /// Without `--input` this is an offline check against the pack's own
    /// budgets. With `--input` it is a full dry run against a real disc: every
    /// entry is planned exactly as `import` would (in memory, nothing is
    /// written), which is the only way to validate a distributable pack's
    /// budgets - they are hints until a disc is there to measure.
    Stats {
        /// The language pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Dry-run the pack against this disc image (`.bin`, Mode 2/2352; a
        /// `.cue` is accepted and resolved to its `.bin`).
        #[arg(long)]
        input: Option<PathBuf>,
        /// Print every skipped / over-budget entry individually instead of
        /// the default per-reason summary.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Space report: how much room every translatable string has, how much
    /// English uses, and - with `--pack` - how much the pack uses and what
    /// `import` does with each line (a dry run on an in-memory copy; nothing
    /// is written). Tightest things first: the fullest name regions and
    /// scenes, the entries that would not land. Numbers and keys only - no
    /// game text.
    Space {
        /// The disc image (`.bin`, Mode 2/2352; a `.cue` is accepted).
        #[arg(long)]
        input: PathBuf,
        /// Dry-run this language pack against the disc.
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Only report this pack section (`items`, `scene_dialog`, ...).
        #[arg(long)]
        section: Option<String>,
        /// Dry-run with the whole-sector relayout enabled, as
        /// `import --allow-relayout` would.
        #[arg(long)]
        allow_relayout: bool,
        /// Re-fit one scene MAN (PROT entry) with the pack's lines only - the
        /// editor fast path - and print it with its timing.
        #[arg(long, requires = "pack")]
        scene: Option<usize>,
        /// Print the full report as JSON (schema `legaia-space-v1`).
        #[arg(long)]
        json: bool,
        /// Print every row instead of the tightest few per table.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Cross-region corpus alignment report: compare a **target** disc (the
    /// one the importer would patch, e.g. the retail NTSC/USA build) against an
    /// **official localization** disc (a PAL SCES build) and quantify how well
    /// the dialog corpus aligns id-/order-for-order and how much of the
    /// official text fits the target's same-size budget. Emits counts and byte
    /// values only - no game text - so it is safe to run and log. Use it to
    /// judge whether an official translation can be lifted into a distributable
    /// pack for the target disc.
    DiffDisc {
        /// The target disc the importer patches (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
        /// The other (official-localization) disc to align against.
        #[arg(long)]
        other: PathBuf,
    },
    /// Lift the text of **another Latin-script disc** into a USA-keyed working
    /// pack: name tables id-for-id, dialog by positional segment pairing. Emits
    /// a filled pack (source = USA text, translation = the other disc's text)
    /// to `-o`.
    ///
    /// The source is an official PAL localization (FR/DE/IT measured, ES and
    /// EU-English located at run time) or a **fan-patched** disc of any Latin
    /// build - a patched USA disc included. That is how a community translation
    /// shipped as a binary patch (xdelta / PPF) becomes an editable pack: apply
    /// the patch to the disc it was built for, then lift from the result.
    /// The JP discs are refused (different text encoding).
    ///
    /// The output carries the game's copyrighted text - keep it local, never
    /// commit it. Only `translate strip`-ed distributable packs are shareable.
    LiftOfficial {
        /// The disc to lift from (`.bin`): a PAL SCES build (SCES_019.44 FR /
        /// .45 DE / .46 IT / .47 ES, SCES_017.52 EN) or a fan-patched disc,
        /// USA (SCUS_942.54) included.
        #[arg(long)]
        from: PathBuf,
        /// The USA target disc whose coordinate space the pack is keyed to.
        #[arg(long)]
        target: PathBuf,
        /// Where to write the filled working pack (YAML). Scratchpad only.
        #[arg(long, short)]
        output: PathBuf,
        /// ASCII-fold the accented glyphs the NTSC font lacks (`Epee` for
        /// `Épée`). Without it the lifted text keeps the PAL accent bytes and
        /// the pack is stamped `accents: font`, so the import draws them with
        /// the accent font.
        #[arg(long)]
        fold_accents: bool,
        /// Language code to stamp the pack with (`pt-BR`, ...). Defaults to
        /// the source build's own language; a fan patch's language is not in
        /// the exe name, so pass it here.
        #[arg(long)]
        language: Option<String>,
        /// The retail disc a fan patch was built on (`.bin`). Every line whose
        /// text that disc also carries is blanked, so the pack holds only the
        /// translator's own text and the lines the patch left alone stay
        /// vanilla on import - the shape a fan-patch lift needs before
        /// `translate strip` may publish it.
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
    /// Dialog coverage of `export` on a disc: every line the scene scripts
    /// reach (the script walk) against the lines the exported pack carries,
    /// per scene, with the reason for each missing line. Also counts lines no
    /// walk reaches, untranslated Japanese lines left in a Latin build, and
    /// shop vendor names. Counts and offsets only - no text - so it is safe to
    /// run and log.
    Coverage {
        /// The disc image (`.bin`, or a `.cue` resolved to its `.bin`): any
        /// build - USA, PAL or Japanese.
        #[arg(long)]
        input: PathBuf,
        /// Print the full report as JSON.
        #[arg(long)]
        json: bool,
        /// List every carrier, not only the ones with missing lines.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Measure how much of an official localization fits the USA target under
    /// the per-string vs per-MAN (generalized rewriter) budget, and how many
    /// scene MANs remain sector-crossers. Counts only - no text - so it is safe
    /// to run and log.
    FitReport {
        /// The official-localization disc (PAL SCES build).
        #[arg(long)]
        from: PathBuf,
        /// The USA target disc.
        #[arg(long)]
        target: PathBuf,
    },
    /// Apply a filled pack to a copy of a disc. Untranslated entries are
    /// left byte-identical; every write is same-size in place and each
    /// touched sector's EDC/ECC is re-encoded.
    Import {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// The filled language pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Write the patched image here (contains Sony bytes - local play
        /// only, never redistribute).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write a portable PPF 3.0 patch here (safe to share).
        #[arg(long)]
        patch: Option<PathBuf>,
        /// Allow a whole-sector **disc relayout**: scene MANs whose full-length
        /// dialog overflows their compressed footprint gain `+N` sectors (the
        /// PROT entry grows and the disc is relaid out) so the dialog imports
        /// byte-faithfully instead of being abbreviated. Grows the image, so
        /// it needs `--output` and is refused with `--patch` (a PPF cannot
        /// describe a longer image).
        #[arg(long)]
        allow_relayout: bool,
        /// How typed accents reach the disc, overriding the pack's own
        /// `accents:` header: `strict` (an accent is an error), `fold` (write
        /// `Epee` for `Épée`) or `font` (also draw the accented letters into
        /// the dialog font, built from this disc's own glyphs, so accents
        /// render).
        #[arg(long)]
        accents: Option<String>,
        /// Print every skipped entry individually instead of the default
        /// per-reason summary.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
}
