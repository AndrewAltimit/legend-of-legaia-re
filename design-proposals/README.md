# Site redesign proposals

Five design-variant mockups for rehauling the static site, plus the shared
brief they were built from. **Not shipped** — this directory is comparison
material on the `site-redesign-proposals` branch; the winning direction gets
implemented into `site/` and this directory deleted.

View: open `index.html` in a browser (everything is self-contained; `file://`
works), or serve the directory:

```bash
cd design-proposals && python3 -m http.server 8750
```

| File | Variant | Thesis |
|---|---|---|
| `refined-terminal.html` | A | Evolution of the current dark/cyan look; ergonomics fixed, identity kept. |
| `ra-seru.html` | B | Game-forward cinematic dark; ember/indigo palette from the game. |
| `archive-light.html` | C | Light editorial "museum catalog" identity. |
| `console-dashboard.html` | D | App-shell launcher with tile grid and status badges. |
| `split-portal.html` | E | Two deliberate identities: playful Explore, terminal Docs. |

All five implement the same structural fixes (`_shared/BRIEF.md` items C1–C8)
on the same frozen content (`_shared/content.md`): two-zone Explore/Docs IA,
tool-first interactive pages, one-line visual cards, header disc-status chip,
stat-strip progress, docs metadata strips. Each mockup file shows three
sections: the homepage, the Enemy table page (interactive chrome), and the
Legaia TMD spec page (docs chrome).

No Sony imagery anywhere — thumbnails are abstract inline SVG placeholders.
