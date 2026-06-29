-- autorun_trace_segment.lua
--
-- The trace-driven-coverage segment harness. Arms a non-pausing exec
-- breakpoint on every NOT-YET-UNDERSTOOD function entry (the gap-set
-- worklist), plays one segment of the opening, and records which of those
-- functions actually executed. A hit = "an unexplained function ran in this
-- segment" = a documentation target. See docs/tooling/playthrough-coverage.md.
--
-- This probe is PASSIVE: it records whatever executes. Driving the input is
-- OPTIONAL (LEGAIA_INPUTS, below) - for the cold-boot S1 segment you can also
-- just navigate the title menu by hand while the capture window runs; the
-- tracer records the same gap-set hits either way. Save-state-anchored
-- segments (S2+) get dense, reliable vsync timing, so their inputs script
-- cleanly.
--
-- Gap-set worklist: regenerate with scripts/pcsx-redux/build_gap_worklist.py.
--
-- Env vars:
--   LEGAIA_SSTATE     start save state (ignored when LEGAIA_NO_SSTATE=1).
--   LEGAIA_NO_SSTATE  "1" = cold boot from BIOS (S1). The save-state loader is
--                     monkey-patched to a no-op; the assert in probe.run is
--                     satisfied by the (unopened) LEGAIA_SSTATE string.
--   LEGAIA_WORKLIST   gap-set file (default scripts/pcsx-redux/gap_worklist.txt).
--   LEGAIA_MAX_BPS    cap the number of breakpoints armed (0 = all). For a
--                     quick smoke run, set e.g. 64.
--   LEGAIA_ADDR_LO    only arm addresses >= this (hex/dec). e.g. 0x80010000.
--   LEGAIA_ADDR_HI    only arm addresses <  this. e.g. 0x801C0000 (SCUS-only =
--                     the unambiguous, always-resident signal).
--   LEGAIA_FRAMES     vsyncs to capture (default 3600 = ~60s in-game).
--   LEGAIA_INPUTS     optional input timeline. Comma-separated <frame>:<action>
--                     steps, where <frame> is vsyncs-since-capture-start and
--                     <action> is +BTN (press) / -BTN (release). BTN names:
--                     START SELECT UP DOWN LEFT RIGHT CROSS CIRCLE SQUARE
--                     TRIANGLE L1 L2 R1 R2. Example (NEW GAME at title):
--                       "120:+CROSS,126:-CROSS,300:+CROSS,306:-CROSS"
--   LEGAIA_OUT        trace CSV path (default per-run dir / trace_segment.csv).
--   LEGAIA_BOOT_DELAY vsyncs to wait before arming + load (default 60).
--
-- Output:
--   <OUT>                addr,hits,first_frame,first_mode,first_ra,stem
--   <OUT>.modes.txt      (elapsed, game_mode) on every mode change - the
--                        segment's mode timeline; use it to (a) validate the
--                        segment reached the intended state and (b) attribute
--                        overlay-range (VA-aliased) hits to the resident
--                        overlay window.
--   <OUT>.hits.txt       live snapshot (rewritten every 60 vsyncs).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE_ADDR = 0x8007B83C -- current game-mode byte (0x03 field, 0x15 battle, ...)

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE   = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local WORKLIST    = probe.getenv("LEGAIA_WORKLIST",
    "scripts/pcsx-redux/gap_worklist.txt")
local MAX_BPS     = probe.getenv_num("LEGAIA_MAX_BPS", 0)
local ADDR_LO     = probe.getenv_num("LEGAIA_ADDR_LO", 0)
local ADDR_HI     = probe.getenv_num("LEGAIA_ADDR_HI", 0)
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3600)
local BOOT_DELAY  = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local INPUTS_SPEC = probe.getenv("LEGAIA_INPUTS", "")
local OUT_PATH    = probe.out_path("trace_segment.csv")
local MODES_PATH  = OUT_PATH:gsub("%.csv$", "") .. ".modes.txt"

if NO_SSTATE then
    probe.load_save_state = function(_)
        PCSX.log("[trace] LEGAIA_NO_SSTATE=1 -- cold boot; sstate ignored")
        return true
    end
end

-- ---- parse the gap-set worklist -------------------------------------------
-- Each non-comment line: `0x801xxxxx  <stem>  # ...`. We keep addr + stem.
local function load_worklist(path)
    local list = {}
    local fh = io.open(path, "r")
    if not fh then
        PCSX.log("[trace] FATAL: cannot open worklist " .. path)
        return list
    end
    for line in fh:lines() do
        local addr_s, stem = line:match("^%s*(0[xX]%x+)%s+(%S+)")
        if not addr_s then
            addr_s = line:match("^%s*(0[xX]%x+)")
        end
        if addr_s then
            local addr = tonumber(addr_s)
            if addr then
                if (ADDR_LO == 0 or addr >= ADDR_LO)
                    and (ADDR_HI == 0 or addr < ADDR_HI) then
                    list[#list + 1] = { addr = addr, stem = stem or "?" }
                end
            end
        end
    end
    fh:close()
    return list
end

-- ---- parse the optional input timeline ------------------------------------
-- "<frame>:+BTN,<frame>:-BTN,..." -> sorted list of { frame, press, button }.
local function parse_inputs(spec)
    local steps = {}
    if spec == nil or spec == "" then return steps end
    for chunk in spec:gmatch("[^,]+") do
        local frame_s, sign, name = chunk:match("^%s*(%d+)%s*:%s*([+%-])(%u%d?%a*)%s*$")
        if frame_s and probe.BTN[name] ~= nil then
            steps[#steps + 1] = {
                frame  = tonumber(frame_s),
                press  = (sign == "+"),
                button = probe.BTN[name],
                name   = name,
            }
        else
            PCSX.log("[trace] WARN: bad input step '" .. chunk .. "' (ignored)")
        end
    end
    table.sort(steps, function(a, b) return a.frame < b.frame end)
    return steps
end

local WORK   = load_worklist(WORKLIST)
local INPUTS = parse_inputs(INPUTS_SPEC)

-- Apply the BP cap after the address filter, so --bucket-style ADDR windows
-- compose with a smoke-test cap.
if MAX_BPS > 0 and #WORK > MAX_BPS then
    local trimmed = {}
    for i = 1, MAX_BPS do trimmed[i] = WORK[i] end
    WORK = trimmed
end

local csv = probe.csv_open(OUT_PATH,
    "addr,hits,first_frame,first_mode,first_ra,stem")

-- Live elapsed vsync, updated each capture tick; read inside BP callbacks.
local g_elapsed = 0

-- ---- mode-timeline log -----------------------------------------------------
local modes_fh = io.open(MODES_PATH, "w")
if modes_fh then
    modes_fh:write(string.format(
        "# game-mode timeline (elapsed_vsync, mode) for segment trace\n"
        .. "# worklist=%s armed=%d cold_boot=%s\n",
        WORKLIST, #WORK, tostring(NO_SSTATE)))
    modes_fh:flush()
end
local last_mode = -1
local function poll_mode()
    if not probe.in_ram(GAME_MODE_ADDR) then return end
    local m = probe.read_u8(GAME_MODE_ADDR)
    if m ~= last_mode then
        if modes_fh then
            modes_fh:write(string.format("%6d  0x%02X\n", g_elapsed, m))
            modes_fh:flush()
        end
        PCSX.log(string.format("[trace] mode -> 0x%02X at vsync %d", m, g_elapsed))
        last_mode = m
    end
end

PCSX.log(string.format(
    "[trace] worklist=%s armed=%d (filter lo=0x%X hi=0x%X cap=%d) frames=%d inputs=%d",
    WORKLIST, #WORK, ADDR_LO, ADDR_HI, MAX_BPS, FRAMES, #INPUTS))

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    boot_delay     = BOOT_DELAY,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local descs = {}
        for _, w in ipairs(WORK) do
            local d = {
                addr     = w.addr,
                name     = string.format("gap_%08X", w.addr),
                stem     = w.stem,
                hits_ref = { n = 0 },
                first    = nil,
            }
            probe.arm_breakpoint(w.addr, "Exec", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.first == nil then
                    -- First-hit detail only: frame + resident mode + caller.
                    local r  = PCSX.getRegisters()
                    local ra = tonumber(r.GPR.n.ra) or 0
                    local md = probe.in_ram(GAME_MODE_ADDR)
                        and probe.read_u8(GAME_MODE_ADDR) or 0xFF
                    d.first = {
                        frame = g_elapsed,
                        mode  = md,
                        ra    = bit.band(ra, 0xFFFFFFFF),
                    }
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[trace] %d gap-set exec probes armed", #descs))
        return descs
    end,

    on_capture = function(_, elapsed)
        g_elapsed = elapsed
        poll_mode()
        -- Drive the optional scripted input timeline.
        while INPUTS[1] and INPUTS[1].frame <= elapsed do
            local s = table.remove(INPUTS, 1)
            if s.press then
                probe.pad_force(s.button)
            else
                probe.pad_release(s.button)
            end
            PCSX.log(string.format("[trace] input %s%s at vsync %d",
                s.press and "+" or "-", s.name, elapsed))
        end
    end,

    on_done = function(_, descs)
        local n_hit = 0
        for _, d in ipairs(descs) do
            local n = d.hits_ref and d.hits_ref.n or 0
            if n > 0 then
                n_hit = n_hit + 1
                local f = d.first or { frame = -1, mode = 0xFF, ra = 0 }
                csv:row("0x%08X,%d,%d,0x%02X,0x%08X,%s",
                    d.addr, n, f.frame, f.mode, f.ra, d.stem)
            end
        end
        csv:close()
        if modes_fh then modes_fh:close() end
        PCSX.log(string.format(
            "[trace] %d/%d gap-set functions hit; CSV=%s modes=%s",
            n_hit, #descs, OUT_PATH, MODES_PATH))
    end,

    on_summary = function(_, descs)
        local n_hit = 0
        for _, d in ipairs(descs) do
            if (d.hits_ref and d.hits_ref.n or 0) > 0 then n_hit = n_hit + 1 end
        end
        PCSX.log("=== gap-set trace summary ===")
        PCSX.log(string.format("  armed   : %d", #descs))
        PCSX.log(string.format("  hit     : %d", n_hit))
        PCSX.log(string.format("  last mode: 0x%02X", last_mode))
    end,
})
