"""Where the catalog reads and writes: the dump corpus, docs, crates, the
output directory and the three data files beside the entry script.
"""

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
# Gitignored, so it exists only in the checkout that dumped it. `--funcs`
# rebinds it by path; main() assigns the global before anything reads it.
DEFAULT_FUNCS = REPO / "ghidra" / "scripts" / "funcs"
FUNCS_DIR = DEFAULT_FUNCS
DOCS_DIR = REPO / "docs"
CRATES_DIR = REPO / "crates"
OUT_DIR = REPO / "target" / "port-catalog"
FEATURES_TOML = REPO / "scripts" / "ci" / "features.toml"
IGNORE_TOML = REPO / "scripts" / "ci" / "port-catalog-ignore.toml"
BASELINE = REPO / "scripts" / "ci" / "port-catalog-baseline.json"
