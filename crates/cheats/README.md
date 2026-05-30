# legaia-cheats

Parser + classifier for third-party GameShark / Pro-Action-Replay
cheat code databases targeting *Legend of Legaia*. Two formats are
supported:

- **GameShark text dump** (`legaia-ntsc-u.gs.txt`): one line per
  effect, fields separated by whitespace
  (`R I <width> L 0 <addr> <value> <description>`).
- **Mednafen `.cht`** (`legaia-ntsc-u.cht`): TOML-shaped triples
  (`cheatN_desc / cheatN_code / cheatN_enable`); multi-write codes
  use `+` as the separator.

The crate exposes:

- [`CheatCode`] — a single `(addr, value, width, op)` write.
- [`CheatEntry`] — one named effect (description + one or more
  writes; conditional codes are first-class).
- [`Database`] — a collection of entries plus deduplication
  helpers.
- [`parse_gs_text`] / [`parse_mednafen_cht`] — format-specific
  parsers.
- [`classify`] — assign each address a [`Category`] (per-character
  record, inventory slot, battle actor pool, engine global,
  mini-game scratch, …) plus a stable subtype label.

The companion `cheat-tool` CLI exposes:

```
cheat-tool parse <PATH>           # parse + dump as JSON
cheat-tool list <PATH>            # one line per entry
cheat-tool classify <PATH>        # group by category, with citations
cheat-tool diff <A> <B>           # show entries unique to each
cheat-tool extract-offsets <PATH> # per-character record offsets only
```

The runtime cheat applier wired into `legaia-engine play-window
--cheat-file <PATH>` lives in `legaia_engine_core::cheat_applier`
and uses this crate plus the `ram_map` registry to dispatch each
write to the appropriate engine cell.

## Write taxonomy

`taxonomy::classify_writes(addrs)` rolls a set of changed RAM
addresses up into per-region buckets via `classify::classify_address`,
flagging writes that land outside every known data region
(`Category::Unknown`) or in the `0x8007Bxxx` script-VM / build-flag
scratch. It is the classification half of a gameplay-driven write
tracer; feed it the per-byte deltas from a pair of save states (see
`mednafen-state write-taxonomy LEFT RIGHT`) to see *what* changed,
bucketed by subsystem. Pure and capture-free — unit-tested with
synthetic deltas.
