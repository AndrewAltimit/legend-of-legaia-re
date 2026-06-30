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
--   LEGAIA_MASH       optional auto-advance: "<BTN>:<period>" pulses BTN (held
--                     ~6 vsyncs) every <period> vsyncs - the robust headless way
--                     to drive title -> NEW GAME -> through opening dialog /
--                     cutscenes without guessing the exact vsync timing.
--                     e.g. "CROSS:40". Composes with LEGAIA_INPUTS.
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
local MASH_SPEC   = probe.getenv("LEGAIA_MASH", "")
local OUT_PATH    = probe.out_path("trace_segment.csv")
local MODES_PATH  = OUT_PATH:gsub("%.csv$", "") .. ".modes.txt"

if NO_SSTATE then
    -- probe.run (lib/probe/sm.lua) loads the start state via the *sstate
    -- submodule's* `load`, NOT probe.load_save_state - patch the function it
    -- actually resolves so cold boot truly skips the load. (Loading a save
    -- mid-BIOS-boot segfaults PCSX-Redux; a real no-op lets the disc boot.)
    require("probe.sstate").load = function(_)
        PCSX.log("[trace] LEGAIA_NO_SSTATE=1 -- cold boot; sstate load skipped")
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

-- ---- parse the optional mash spec -----------------------------------------
-- "<BTN>:<period>" -> pulse BTN for ~6 vsyncs every <period> vsyncs.
local function parse_mash(spec)
    if spec == nil or spec == "" then return nil end
    local name, period = spec:match("^%s*(%u%d?%a*)%s*:%s*(%d+)%s*$")
    if name and probe.BTN[name] ~= nil and tonumber(period) then
        return { button = probe.BTN[name], name = name, period = tonumber(period) }
    end
    PCSX.log("[trace] WARN: bad LEGAIA_MASH '" .. spec .. "' (ignored)")
    return nil
end

local WORK   = load_worklist(WORKLIST)
local INPUTS = parse_inputs(INPUTS_SPEC)
local MASH   = parse_mash(MASH_SPEC)

-- Apply the BP cap after the address filter, so --bucket-style ADDR windows
-- compose with a smoke-test cap.
if MAX_BPS > 0 and #WORK > MAX_BPS then
    local trimmed = {}
    for i = 1, MAX_BPS do trimmed[i] = WORK[i] end
    WORK = trimmed
end

-- Write the hit table to the CSV. Called periodically during capture (NOT just
-- at on_done) so a late PCSX crash - this build aborts a few hundred vsyncs into
-- some resumed saves - still leaves the captured hits on disk.
local function flush_csv(descs)
    local fh = io.open(OUT_PATH, "w")
    if not fh then return end
    fh:write("addr,hits,first_frame,first_mode,first_ra,stem\n")
    for _, d in ipairs(descs or {}) do
        local n = d.hits_ref and d.hits_ref.n or 0
        if n > 0 then
            local f = d.first or { frame = -1, mode = 0xFF, ra = 0 }
            fh:write(string.format("0x%08X,%d,%d,0x%02X,0x%08X,%s\n",
                d.addr, n, f.frame, f.mode, f.ra, d.stem))
        end
    end
    fh:close()
end

-- Live elapsed vsync, updated each capture tick; read inside BP callbacks.
local g_elapsed = 0
local _mode_err_logged = false

-- Per-desc breakpoint callback: count hits + capture first-hit detail.
local function make_callback(d)
    return function()
        d.hits_ref.n = d.hits_ref.n + 1
        if d.first == nil then
            -- First-hit detail only: frame + resident mode + caller.
            local r  = PCSX.getRegisters()
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            if ra < 0 then ra = ra + 0x100000000 end -- -> clean unsigned for %08X
            local md = probe.in_ram(GAME_MODE_ADDR)
                and probe.read_u8(GAME_MODE_ADDR) or 0xFF
            d.first = { frame = g_elapsed, mode = md, ra = ra }
        end
    end
end

-- NOTE ON BREAKPOINT COUNT (this build, headless): arming the full 780-entry
-- gap-set in one on_arm call stalls the emulator before capture; arming the set
-- *after* the save resume segfaults. The reliable path is arming a SMALL set
-- (<= ~150) in on_arm BEFORE the load. Capture the whole gap-set as a UNION of
-- windowed passes via LEGAIA_ADDR_LO/HI + LEGAIA_MAX_BPS, each pass under the
-- stable count. See docs/tooling/playthrough-coverage.md.

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
    -- read_u8 returns nil while the memory handle is transiently invalid (the
    -- game's BIOS ResetCallback during boot init). Skip cleanly - a nil here
    -- otherwise reaches string.format("%02X", nil) and throws every vsync.
    if m == nil then return end
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
            probe.arm_breakpoint(w.addr, "Exec", 4, d.name, make_callback(d))
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[trace] %d gap-set exec probes armed", #descs))
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        local ok, err = pcall(poll_mode)
        if not ok and not _mode_err_logged then
            PCSX.log("[trace] poll_mode error (once): " .. tostring(err))
            _mode_err_logged = true
        end
        -- Periodic CSV flush (crash-survival; ~every 60 vsyncs).
        if (elapsed % 60) == 0 then flush_csv(ctx.descs) end
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
        -- Drive the optional auto-advance mash: pulse for ~6 vsyncs each period.
        if MASH then
            local phase = elapsed % MASH.period
            if phase == 0 then
                probe.pad_force(MASH.button)
            elseif phase == 6 then
                probe.pad_release(MASH.button)
            end
        end
    end,

    on_done = function(_, descs)
        flush_csv(descs)
        local n_hit = 0
        for _, d in ipairs(descs) do
            local n = d.hits_ref and d.hits_ref.n or 0
            if n > 0 then
                n_hit = n_hit + 1
            end
        end
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
