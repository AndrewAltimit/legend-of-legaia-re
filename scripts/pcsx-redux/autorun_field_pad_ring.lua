-- autorun_field_pad_ring.lua
--
-- Retail's camera-relative pad-direction remap, measured live: for every
-- rotation index and every held direction, what `FUN_800467E8` turns the
-- held d-pad nibble into.
--
-- The routine is 34 instructions of `SCUS_942.54` (`ghidra/scripts/funcs/
-- 800467e8.txt`). It takes a pointer to the per-frame pad word, and when the
-- rotation index `gp+0x2D8` is non-zero it finds the held direction in the
-- eight-entry compass ring `DAT_800766FC`, adds the index, wraps `& 7` and
-- writes the ring entry back over the word's `0xF000` nibble. The field
-- free-movement controller `FUN_801D01B0` calls it every field frame with
-- `&_DAT_8007B850` (`jal` at `0x801D03E4`, `addiu a0,s1,-0x47B0` in the delay
-- slot), immediately before the wall-slide resolver `FUN_80046494`.
--
-- The port implements it as `World::remap_pad_direction`; this probe is the
-- oracle that pairs the two over the whole input space.
--
-- **The index is scene content, so the probe supplies it.** `gp+0x2D8`'s only
-- writers disc-wide are the field VM's `0x4C` outer-nibble-2 arm and the
-- tile-board walker, so a free-roam save sits at whatever its scene's script
-- authored - one value, not eight. To cover the ring the probe writes the
-- index itself, once per cell and again on every vsync of that cell (nothing
-- else writes it while a field scene just walks). That makes the eight
-- rotations synthetic and the eight directions, the ring walk and the written
-- mask real; the row that matches the save's own authored index is the one
-- cell of the sweep that is entirely unforced, and is marked `authored` in
-- the log.
--
-- Directions are held with `pad.force`, not by writing the mask: the pad word
-- is rebuilt from the real pad every frame by `FUN_8001822C`, so a RAM write
-- to `0x8007B850` does not survive to the call.
--
-- Cells are ordered in opposite pairs (Up then Down, Right then Left, ...) so
-- the sweep's net walk is close to zero and the player stays in the open.
-- Scene word and game mode ride every row, so a cell that walked into a
-- transition is visible rather than silently mixed in.
--
-- Env vars:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 1400)
--   LEGAIA_WARMUP      vsyncs before the first cell (default 60)
--   LEGAIA_CELL_LEN    vsyncs per (rotation, direction) cell (default 8)
--   LEGAIA_CELL_GAP    vsyncs of released pad between cells (default 6)
--   LEGAIA_ROTS        comma-separated rotation indices (default 0,1,2,3,4,5,6,7)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: field_pad_ring.csv, field_pad_ring.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1400)
local WARMUP   = probe.getenv_num("LEGAIA_WARMUP", 60)
local CELL_LEN = probe.getenv_num("LEGAIA_CELL_LEN", 8)
local CELL_GAP = probe.getenv_num("LEGAIA_CELL_GAP", 6)
local ROTS_SPEC = probe.getenv("LEGAIA_ROTS", "0,1,2,3,4,5,6,7")

local OUT_CSV = probe.out_path("field_pad_ring.csv")
local OUT_LOG = probe.out_path("field_pad_ring.log")

-- ---------------------------------------------------------------- addresses
local PAD_WORD   = 0x8007B850   -- `_DAT_8007b850`, the word the routine edits
local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local PLAYER_P   = 0x8007C364   -- pointer to the player actor record
local RING_TABLE = 0x800766FC   -- `DAT_800766fc`, eight u32 direction masks

-- The two taps. `want` is the instruction word the extracted `SCUS_942.54`
-- carries there, so a run that somehow breaks elsewhere is reported instead
-- of counted.
local TAP_IN  = { addr = 0x800467E8, want = 0x8F8202D8, kind = "in" }
local TAP_OUT = { addr = 0x80046868, want = 0x03E00008, kind = "out" }

-- The eight cells, in opposite pairs so the sweep's net walk cancels.
local DIRS = {
    { name = "UP",         btns = { "UP" } },
    { name = "DOWN",       btns = { "DOWN" } },
    { name = "RIGHT",      btns = { "RIGHT" } },
    { name = "LEFT",       btns = { "LEFT" } },
    { name = "UP_RIGHT",   btns = { "UP", "RIGHT" } },
    { name = "DOWN_LEFT",  btns = { "DOWN", "LEFT" } },
    { name = "UP_LEFT",    btns = { "UP", "LEFT" } },
    { name = "DOWN_RIGHT", btns = { "DOWN", "RIGHT" } },
}

local ROTS = {}
for tok in string.gmatch(ROTS_SPEC, "[^,]+") do
    local v = tonumber(tok)
    if v then ROTS[#ROTS + 1] = v % 8 end
end
if #ROTS == 0 then ROTS = { 0 } end

-- ------------------------------------------------------------------ helpers
local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[pad_ring] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function gp() return n32(PCSX.getRegisters().GPR.n.gp) end
local function rot_addr() return n32(gp() + 0x2D8) end

local function player_pos()
    local pp = probe.read_u32(PLAYER_P)
    if pp == nil or not probe.in_ram(pp) then return 0, 0, 0 end
    return s16(probe.read_u16(pp + 0x14) or 0),
           s16(probe.read_u16(pp + 0x18) or 0),
           s16(probe.read_u16(pp + 0x26) or 0)
end

-- ----------------------------------------------------------------- csv shape
local CSV_HEADER = table.concat({
    "seq", "vsync", "cell", "rot_written", "dir",
    "tap", "pad_word", "dir_nibble", "in_nibble", "rot_live", "ra",
    "px", "pz", "facing", "scene", "mode",
}, ",")

local csv = nil
local seq = 0
local g_elapsed = 0
local held = {}
local cell_idx = -1        -- -1 = warmup
local cell_rot, cell_dir = 0, nil
local authored_rot = nil
local fingerprint = {}
local hits = { ["in"] = 0, out = 0, in_alias = 0, out_alias = 0 }
local ring_words = {}
-- One PAIR of rows per cell, both from the same call of the routine: `taken`
-- marks a cell that has already contributed its pair, `pair_open` names the
-- cell whose `in` tap is waiting for its `out`, and `pair_in` carries that
-- call's input word onto the `out` row so one row holds both sides.
local taken = {}
local pair_open = nil
local pair_in = 0

local function row(kind, ra)
    seq = seq + 1
    local w = n32(probe.read_u32(PAD_WORD) or 0)
    local px, pz, fa = player_pos()
    local vals = {
        seq, g_elapsed, cell_idx, cell_rot,
        cell_dir and cell_dir.name or "NONE",
        kind, hex8(w), hex8(bit.band(w, 0xF000)),
        hex8(bit.band(pair_in, 0xF000)),
        n32(probe.read_u32(rot_addr()) or 0), hex8(ra),
        px, pz, fa, scene_name(),
        string.format("0x%02X", probe.read_u8(GAME_MODE) or 0),
    }
    for i, v in ipairs(vals) do vals[i] = tostring(v) end
    if csv then csv:row("%s", table.concat(vals, ",")) end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV, CSV_HEADER)
        probe.write_manifest("autorun_field_pad_ring.lua", {
            sstate = SSTATE, frames = FRAMES, warmup = WARMUP,
            cell_len = CELL_LEN, cell_gap = CELL_GAP, rots = ROTS_SPEC,
        })
        local descs = {}
        for _, t in ipairs({ TAP_IN, TAP_OUT }) do
            local tap = t
            local d = { addr = tap.addr, hits_ref = { n = 0 }, name = "ring_" .. tap.kind }
            probe.arm_breakpoint(tap.addr, "Exec", 4,
                string.format("ring_%08X", tap.addr), function()
                -- `n32` is `bit.band`, which in LuaJIT yields a SIGNED 32-bit
                -- integer, so a want word with bit 31 set (`0x8F8202D8`, the
                -- entry's `lw v0,0x2d8(gp)`) never equals the Lua number
                -- literal. Normalise BOTH sides or the tap reports every hit
                -- as an alias and records nothing.
                if n32(probe.read_u32(tap.addr) or 0) ~= n32(tap.want) then
                    hits[tap.kind .. "_alias"] = hits[tap.kind .. "_alias"] + 1
                    return
                end
                hits[tap.kind] = hits[tap.kind] + 1
                d.hits_ref.n = hits[tap.kind]
                -- One PAIR of rows per cell, and they must come from the same
                -- call: the `in` tap opens the pair when the pad word already
                -- carries a direction (the first call of a cell still sees
                -- the pre-press word - the press lands a frame later), and
                -- the `out` tap closes it on the very next hit. Taking the
                -- two taps independently pairs an `in` with the `out` of some
                -- other call, which is how the first run recorded an `out`
                -- whose direction nibble was zero.
                if cell_idx < 0 then return end
                local w = n32(probe.read_u32(PAD_WORD) or 0)
                if tap.kind == "in" then
                    if taken[cell_idx] then return end
                    if bit.band(w, 0xF000) == 0 then return end
                    taken[cell_idx] = true
                    pair_open = cell_idx
                    pair_in = w
                elseif pair_open ~= cell_idx then
                    return
                else
                    pair_open = nil
                end
                row(tap.kind, PCSX.getRegisters().GPR.n.ra)
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            for _, t in ipairs({ TAP_IN, TAP_OUT }) do
                local w = n32(probe.read_u32(t.addr) or 0)
                local ok = (w == n32(t.want))
                fingerprint[t.addr] = ok
                logf("fingerprint [0x%08X] = 0x%s (want 0x%s) %s",
                     t.addr, hex8(w), hex8(t.want), ok and "OK" or "MISMATCH")
            end
            for i = 0, 7 do
                ring_words[i] = n32(probe.read_u32(RING_TABLE + i * 4) or 0)
            end
            logf("ring DAT_800766FC = %s %s %s %s %s %s %s %s",
                 hex8(ring_words[0]), hex8(ring_words[1]), hex8(ring_words[2]),
                 hex8(ring_words[3]), hex8(ring_words[4]), hex8(ring_words[5]),
                 hex8(ring_words[6]), hex8(ring_words[7]))
            authored_rot = n32(probe.read_u32(rot_addr()) or 0)
            local px, pz, fa = player_pos()
            logf("start: scene=%s mode=0x%02X gp=0x%s gp+0x2D8=%d player=(%d,%d) facing=%d",
                 scene_name(), probe.read_u8(GAME_MODE) or 0, hex8(gp()),
                 authored_rot, px, pz, fa)
            return
        end
        if elapsed < WARMUP then return end

        -- Cell schedule: CELL_LEN vsyncs held, CELL_GAP released.
        local period = CELL_LEN + CELL_GAP
        local t = elapsed - WARMUP
        local idx = math.floor(t / period)
        local phase = t % period
        local total = #ROTS * #DIRS
        if idx >= total then
            for _, b in ipairs(held) do probe.pad_release(probe.BTN[b]) end
            held = {}
            cell_idx = -2
            return
        end
        local rot = ROTS[math.floor(idx / #DIRS) + 1]
        local dir = DIRS[(idx % #DIRS) + 1]

        if phase == 0 then
            -- New cell: release the last one's buttons, then press this one's.
            for _, b in ipairs(held) do probe.pad_release(probe.BTN[b]) end
            held = {}
            cell_idx = idx
            cell_rot = rot
            cell_dir = dir
            for _, b in ipairs(dir.btns) do
                probe.pad_force(probe.BTN[b]); held[#held + 1] = b
            end
        elseif phase == CELL_LEN - 1 then
            -- Settled facing: the routine's own taps read the actor heading
            -- BEFORE this frame's facing write (`FUN_801D01B0` calls the remap
            -- at its top), so a tap row's `facing` is the previous frame's and
            -- lags the cell by one. This row is taken at the cell's last held
            -- vsync instead, by which point the heading has been written from
            -- the remapped direction for several frames.
            row("settle", 0)
        elseif phase == CELL_LEN then
            for _, b in ipairs(held) do probe.pad_release(probe.BTN[b]) end
            held = {}
        end
        -- The rotation index is written every vsync of the cell: retail's own
        -- writers are script arms that a walking field scene does not run, so
        -- one write would hold - but a write per frame costs nothing and
        -- survives a script that does.
        if phase < CELL_LEN then
            probe.write_u32(rot_addr(), rot)
        end
    end,

    on_summary = function(ctx, descs)
        for _, b in ipairs(held) do probe.pad_release(probe.BTN[b]) end
        held = {}
        logf("hits: in=%d(+%d alias) out=%d(+%d alias)",
             hits["in"], hits.in_alias, hits.out, hits.out_alias)
        logf("cells scheduled: %d (%d rotations x %d directions)",
             #ROTS * #DIRS, #ROTS, #DIRS)
        logf("save's own authored gp+0x2D8 = %s",
             authored_rot and tostring(authored_rot) or "unread")
        local px, pz, fa = player_pos()
        logf("end: scene=%s mode=0x%02X player=(%d,%d) facing=%d",
             scene_name(), probe.read_u8(GAME_MODE) or 0, px, pz, fa)
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n")); fh:write("\n"); fh:close() end
        if csv then csv:close() end
    end,
})
