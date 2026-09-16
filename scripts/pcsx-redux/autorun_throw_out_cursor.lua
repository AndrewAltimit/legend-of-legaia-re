-- autorun_throw_out_cursor.lua
--
-- Is the pause-menu item cursor `_DAT_8007BB88` a BAG SLOT or a LIST ROW?
--
-- The Throw Out confirm (`FUN_801D8734` in PROT 0899, state `3`) zeroes the
-- selected entry with two `sb zero` of its own at `0x801D88FC` / `0x801D8910`,
-- both over `0x80084140 + 0x1818 + _DAT_8007BB88 * 2` - i.e. it indexes the
-- 256-slot bag **directly** by the cursor word. The port removes by list row
-- instead. The two agree only while every displayed row is its own bag slot,
-- so the question is whether retail's item list can ever show a row whose
-- index is not its slot.
--
-- The experiment does not need the Throw Out flow at all. It punches holes
-- into the bag (`id == 0` mid-run), walks into the pause Items screen, then
-- steps the cursor with DOWN and logs, every vsync, the cursor word together
-- with the id the bag holds AT that index. A cursor that is a slot index never
-- rests on a hole; a cursor that is a row index does as soon as the list
-- renderer skips one. Retail's own compaction is the confound, so the
-- normalizer `FUN_800423E0` is tapped too: if it runs on menu open, the holes
-- are gone before the list is drawn and the two readings are indistinguishable
-- *by construction* - which is itself the answer.
--
-- The two Throw Out `sb` sites are tapped as well, so a run that does reach the
-- confirm records the slot it zeroed.
--
-- Env vars:
--   LEGAIA_SSTATE      field save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 900)
--   LEGAIA_HOLES       comma list of bag slots to zero (default 1,3,6)
--   LEGAIA_FILL        comma list of item ids to write into slots 0.. (default
--                      a nine-id run); count 1 each
--   LEGAIA_POKE_AT     vsync to write the bag pattern (default 12)
--   LEGAIA_MENU_AT     vsync of the SELECT press (default 60)
--   LEGAIA_ITEMS_AT    vsync of the CROSS press on Items (default 200)
--   LEGAIA_STEP_FROM   first vsync of the DOWN walk (default 300)
--   LEGAIA_STEP_EVERY  vsyncs between DOWN presses (default 24)
--   LEGAIA_STEPS       number of DOWN presses (default 12)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: throw_out_cursor.csv, throw_out_cursor.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLES_S    = probe.getenv("LEGAIA_HOLES", "1,3,6")
local FILL_S     = probe.getenv("LEGAIA_FILL",
    "0x77,0x78,0x79,0x7A,0x7B,0x7C,0x7D,0x7E,0x7F")
local POKE_AT    = probe.getenv_num("LEGAIA_POKE_AT", 12)
local MENU_AT    = probe.getenv_num("LEGAIA_MENU_AT", 60)
local ITEMS_AT   = probe.getenv_num("LEGAIA_ITEMS_AT", 200)
local STEP_FROM  = probe.getenv_num("LEGAIA_STEP_FROM", 300)
local STEP_EVERY = probe.getenv_num("LEGAIA_STEP_EVERY", 24)
local STEPS      = probe.getenv_num("LEGAIA_STEPS", 12)

local OUT_CSV = probe.out_path("throw_out_cursor.csv")
local OUT_LOG = probe.out_path("throw_out_cursor.log")

local BAG        = 0x80085958        -- 0x80084140 + 0x1818, 256 slots x 2 bytes
local CURSOR     = 0x8007BB88
local GAME_MODE  = 0x8007B83C
-- `gp` is 0x8007B318 (pinned by `lhu v1,0x478(gp)` reaching the camera
-- rotation trio at 0x8007B790), so the active-window trio is here.
local WIN_LO     = 0x8007B5EA        -- gp[+0x2D2]
local WIN_HI     = 0x8007B5EC        -- gp[+0x2D4]
local WIN_3      = 0x8007B5EE        -- gp[+0x2D6]
local NORMALIZE  = 0x800423E0        -- SCUS compaction helper
local WINDOW_SET = 0x8004313C        -- the sole writer of the window trio
local THROW_ID   = 0x801D88FC        -- sb zero,0x1818(v0)
local THROW_CNT  = 0x801D8910        -- sb zero,0x1819(v0)
local THROW_FN   = 0x801D8734        -- FUN_801D8734 entry

local function parse(str)
    local out = {}
    for tok in string.gmatch(str, "[^,%s]+") do
        local v = tonumber(tok)
        if v then out[#out + 1] = v end
    end
    return out
end

local HOLES = parse(HOLES_S)
local FILL  = parse(FILL_S)

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[throwout] " .. s)
end

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
-- The confirm reads the cursor with `lw` at 0x801D88EC, so it is a word.
local function cursor() return u32(CURSOR) end
local function bag_id(slot) return u8(BAG + slot * 2) end
local function bag_cnt(slot) return u8(BAG + slot * 2 + 1) end

local function bag_head(n)
    local out = {}
    for i = 0, n - 1 do
        out[#out + 1] = string.format("%02X:%d", bag_id(i), bag_cnt(i))
    end
    return table.concat(out, " ")
end

local csv
local g_elapsed = 0
local n_norm, n_winset, n_throw, n_throw_fn = 0, 0, 0, 0
local cursor_on_hole, cursor_samples = 0, 0
local seen_cursor = {}
local holes_at_list = nil

local function row(ev, note)
    local c = cursor()
    csv:row("%d,%s,%d,0x%02X,%d,%d,%d,%d,0x%02X,%s",
        g_elapsed, ev, c, bag_id(c), bag_cnt(c),
        u16(WIN_LO), u16(WIN_HI), u16(WIN_3),
        u8(GAME_MODE), note or "")
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "vsync,event,cursor,bag_id_at_cursor,bag_count_at_cursor," ..
            "win_lo,win_hi,win_3,game_mode,note")
        probe.env.write_manifest("autorun_throw_out_cursor.lua", {
            sstate = SSTATE, frames = FRAMES, holes = HOLES_S, fill = FILL_S,
            menu_at = MENU_AT, items_at = ITEMS_AT, step_from = STEP_FROM,
            step_every = STEP_EVERY, steps = STEPS,
        })
        local descs = {}
        local function tap(addr, label, name, cb)
            local d = { addr = addr, hits_ref = { n = 0 }, name = name }
            probe.arm_breakpoint(addr, "Exec", 4, label, function()
                d.hits_ref.n = d.hits_ref.n + 1
                cb()
            end)
            descs[#descs + 1] = d
        end

        tap(NORMALIZE, "normalize", "FUN_800423E0 bag normalize", function()
            n_norm = n_norm + 1
            row("normalize", bag_head(10))
        end)
        tap(WINDOW_SET, "window_set", "FUN_8004313C window setup", function()
            n_winset = n_winset + 1
            row("window_set")
        end)
        tap(THROW_FN, "throw_fn", "FUN_801D8734 entry", function()
            n_throw_fn = n_throw_fn + 1
            row("throw_fn")
        end)
        tap(THROW_ID, "throw_id", "Throw Out: sb zero over the id byte", function()
            n_throw = n_throw + 1
            local r = PCSX.getRegisters()
            local v0 = bit.band(tonumber(r.GPR.n.v0) or 0, 0xFFFFFFFF)
            row("throw_zero_id", string.format(
                "v0=0x%s target=0x%s cursor=%d bag=%s",
                string.upper(bit.tohex(v0)),
                string.upper(bit.tohex(bit.band(v0 + 0x1818, 0xFFFFFFFF))),
                cursor(), bag_head(10)))
        end)
        tap(THROW_CNT, "throw_cnt", "Throw Out: sb zero over the count byte", function()
            row("throw_zero_count")
        end)
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed

        if elapsed == 2 then
            -- PROT 0899's VAs alias other overlays, so fingerprint the two
            -- stores this whole measurement rests on: `sb zero,0x1818(v0)` and
            -- `sb zero,0x1819(v0)`.
            local w1 = u32(THROW_ID)
            local w2 = u32(THROW_CNT)
            logf("fingerprint [0x%08X]=0x%s (want A0401818) [0x%08X]=0x%s " ..
                 "(want A0401819) mode=0x%02X",
                 THROW_ID, string.upper(bit.tohex(w1)),
                 THROW_CNT, string.upper(bit.tohex(w2)), u8(GAME_MODE))
        end

        if elapsed == POKE_AT then
            for i, id in ipairs(FILL) do
                probe.write_u8(BAG + (i - 1) * 2, id)
                probe.write_u8(BAG + (i - 1) * 2 + 1, 1)
            end
            for _, h in ipairs(HOLES) do
                probe.write_u8(BAG + h * 2, 0)
                probe.write_u8(BAG + h * 2 + 1, 0)
            end
            logf("poked bag at vsync %d: %s", elapsed, bag_head(10))
            row("poke", bag_head(10))
        end

        -- pad script
        local function press(at, btn, name)
            if elapsed == at then
                probe.pad_force(btn)
                logf("vsync %d press %s (mode 0x%02X)", elapsed, name, u8(GAME_MODE))
            elseif elapsed == at + 8 then
                probe.pad_release(btn)
            end
        end
        press(MENU_AT, probe.BTN.SELECT, "SELECT")
        press(ITEMS_AT, probe.BTN.CROSS, "CROSS (Items)")

        for i = 0, STEPS - 1 do
            press(STEP_FROM + i * STEP_EVERY, probe.BTN.DOWN,
                  string.format("DOWN #%d", i + 1))
        end

        if elapsed == ITEMS_AT + 60 then
            holes_at_list = 0
            for _, h in ipairs(HOLES) do
                if bag_id(h) == 0 then holes_at_list = holes_at_list + 1 end
            end
            logf("bag when the list is up (vsync %d): %s (%d of %d holes survive)",
                 elapsed, bag_head(10), holes_at_list, #HOLES)
            row("list_up", bag_head(10))
        end

        if elapsed >= STEP_FROM then
            local c = cursor()
            cursor_samples = cursor_samples + 1
            if c < 128 and bag_id(c) == 0 then
                cursor_on_hole = cursor_on_hole + 1
            end
            if not seen_cursor[c] then
                seen_cursor[c] = true
                row("cursor_new")
            end
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN.DOWN)
        logf("normalize=%d window_set=%d throw_fn=%d throw_zero_id=%d",
             n_norm, n_winset, n_throw_fn, n_throw)
        local cs = {}
        for c in pairs(seen_cursor) do cs[#cs + 1] = c end
        table.sort(cs)
        local parts = {}
        for _, c in ipairs(cs) do
            parts[#parts + 1] = string.format("%d(id 0x%02X)", c, bag_id(c))
        end
        logf("cursor values while the list was up: %s", table.concat(parts, " "))
        logf("cursor rested on a zero-id slot on %d of %d sampled vsyncs",
             cursor_on_hole, cursor_samples)
        logf("final bag head: %s", bag_head(12))
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
