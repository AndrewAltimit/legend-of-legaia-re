-- autorun_deep_pool_probe.lua
--
-- Closed-loop probe for the deep-Buffer-A / Buffer-B prim region during a
-- town -> world-map transition. The earlier autorun_lzs_and_bundle_probe.lua
-- pinned Buffer-A 0x800B5000 hits to FUN_801F8AB0 / FUN_801F8D0C - per-entity
-- sprite renderers, NOT the terrain POLY_FT4 emitter. The terrain prims
-- live deeper in the pool (offset >= 0x18000, address >= 0x800C8000).
--
-- Complications:
--   * The vsync-139 LZS (350 KB into 0x800C8D24) overwrites that exact
--     region with TIM-pack texture data. Any naive Write probe at
--     0x800CC000+ will record thousands of LZS-decoder hits per run and
--     drown the real signal.
--
-- Discriminators built in here:
--
--   1. One probe BELOW the LZS-139 destination range:
--        0x800C8000 = 0x800C8D24 - 0xD24
--      Writes here cannot come from LZS-139 (its dst floor is 0x800C8D24).
--      Any hit at 0x800C8000 is a terrain-emitter candidate by elimination.
--
--   2. PC-range filter for the in-LZS-range probes. The LZS decoder body
--      (FUN_8001A55C) lives in 0x8001A55C..~0x8001A800. Each Write row
--      gets a `pc_in_lzs` column (1 = noise to filter offline, 0 = real
--      candidate emitter).
--
--   3. Concurrent LZS Exec probe maintains a global `lzs_call_count`.
--      Each Write row records the running LZS count + the current vsync,
--      so post-processing can time-bracket terrain emission against the
--      known LZS timeline (LZS #1 @ vsync 139, LZS #2 @ vsync 164, etc.).
--
-- Output CSV columns:
--   vsync,probe_idx,name,kind,pc,a0,a1,a2,a3,ra,val,lzs_count,pc_in_lzs
--
-- Run via the wrapper:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_deep_pool_probe.lua \
--     LEGAIA_OUT=deep_pool_probe.csv \
--     LEGAIA_HOLD_UP=30 \
--     ./scripts/pcsx-redux/run_world_map_probe.sh

------------------------------------------------------------------
-- Configuration

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = tonumber(getenv("LEGAIA_FRAMES", "600"))
local OUT_PATH    = getenv("LEGAIA_OUT",  "deep_pool_probe.csv")
local HOLD_UP_FRAMES = tonumber(getenv("LEGAIA_HOLD_UP", "0"))
local BTN_UP = 4

-- LZS decoder body range. The function starts at 0x8001A55C; pad up to
-- 0x8001A900 generously to absorb the dispatcher's inlined helper
-- (ring-buffer copy loop). Anything in this range writing into the pool
-- is by definition the texture decompressor, not a geometry emitter.
local LZS_PC_LO = 0x8001A55C
local LZS_PC_HI = 0x8001A900

-- Write probe addresses. See big block-comment above for the rationale
-- behind 0x800C8000 (the only address guaranteed to sit OUTSIDE the
-- vsync-139 LZS destination range, [0x800C8D24, 0x80120D24)).
local PROBES = {
    -- Exec — LZS call counter (drives the lzs_count discriminator column).
    { addr = 0x8001A55C, name = "lzs_decoder",  kind = "Exec",  cap = 50   },
    -- Sub-LZS-range terrain candidate. Writes here cannot be LZS-139.
    { addr = 0x800C8000, name = "pool_pre_lzs", kind = "Write", cap = 1500 },
    -- In-LZS-range probes — need PC filter offline to strip LZS noise.
    { addr = 0x800CC000, name = "pool_d8_a",    kind = "Write", cap = 1500 },
    { addr = 0x800CE000, name = "pool_d8_b",    kind = "Write", cap = 1500 },
    { addr = 0x800D0010, name = "pool_buf_b0",  kind = "Write", cap = 1500 },
    { addr = 0x800D8000, name = "pool_d8_c",    kind = "Write", cap = 1500 },
    { addr = 0x800E0000, name = "pool_d8_d",    kind = "Write", cap = 1500 },
    { addr = 0x800E8000, name = "pool_d8_e",    kind = "Write", cap = 1500 },
    -- Post-LZS-range — beyond the vsync-139 350 KB dst tail (~0x80120D24).
    { addr = 0x80120D24, name = "pool_post_lzs",kind = "Write", cap = 1500 },
}

PCSX.log(string.format("[dpp] sstate=%s frames=%d out=%s",
    SSTATE_PATH, FRAMES, OUT_PATH))

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("vsync,probe_idx,name,kind,pc,a0,a1,a2,a3,ra,val,lzs_count,pc_in_lzs\n")
    csv_fh:flush()
else
    PCSX.log("[dpp] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

------------------------------------------------------------------
-- Memory + register helpers

local mem_file
local RAM_SIZE = 2 * 1024 * 1024

local function read_u32_safe(addr)
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE - 4 then return nil end
    local ok, v = pcall(function() return mem_file:readU32At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

------------------------------------------------------------------
-- Probe state

local hits = {}
local bps  = {}
local vsync_count = 0
local lzs_count = 0
for i = 1, #PROBES do hits[i] = 0 end

local function pc_in_lzs_range(pc)
    return pc >= LZS_PC_LO and pc < LZS_PC_HI
end

------------------------------------------------------------------
-- Arm + disarm

local function arm_probes()
    for i, p in ipairs(PROBES) do
        local idx = i
        local probe = p
        local cap = probe.cap or 100
        local cb = function(_, _, _)
            hits[idx] = hits[idx] + 1
            if hits[idx] > cap then return end
            local ok, info = pcall(function()
                local r = PCSX.getRegisters()
                local out = {
                    pc = tonumber(r.pc) or 0,
                    a0 = tonumber(r.GPR.n.a0) or 0,
                    a1 = tonumber(r.GPR.n.a1) or 0,
                    a2 = tonumber(r.GPR.n.a2) or 0,
                    a3 = tonumber(r.GPR.n.a3) or 0,
                    ra = tonumber(r.GPR.n.ra) or 0,
                    val = 0,
                }
                if probe.kind == "Write" then
                    out.val = read_u32_safe(probe.addr) or 0
                end
                return out
            end)
            if not ok then return end

            -- LZS Exec probe maintains the timeline marker for everyone
            -- else. Increment BEFORE writing the row so the row reflects
            -- the count just established (i.e. the LZS call this Exec is
            -- entering, 1-indexed).
            if probe.kind == "Exec" and probe.name == "lzs_decoder" then
                lzs_count = lzs_count + 1
            end

            local in_lzs = pc_in_lzs_range(info.pc) and 1 or 0
            if csv_fh then
                csv_fh:write(string.format(
                    "%d,%d,%s,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%d,%d\n",
                    vsync_count, idx - 1, probe.name, probe.kind,
                    info.pc, info.a0, info.a1, info.a2, info.a3, info.ra,
                    info.val, lzs_count, in_lzs))
                -- NOTE: do NOT flush per-row. At ~5000 prims/frame the
                -- flush call was hot enough to SEGV pcsx-redux mid-frame.
                -- Vsync handler does a periodic flush every SNAPSHOT_EVERY
                -- frames instead.
            end

            -- Inline log: every LZS Exec hit, every non-LZS write to a
            -- discriminator probe (pool_pre_lzs / pool_post_lzs), and the
            -- first 3 of every other probe to keep noise bounded.
            local should_log = false
            if probe.name == "lzs_decoder" then
                should_log = true
            elseif probe.kind == "Write" and in_lzs == 0 then
                should_log = (hits[idx] <= 20)
            elseif hits[idx] <= 3 then
                should_log = true
            end

            if should_log then
                if probe.kind == "Exec" then
                    PCSX.log(string.format(
                        "[dpp] vsync=%d LZS #%d at pc=0x%08X a0=0x%08X a1=0x%08X a2=0x%08X ra=0x%08X",
                        vsync_count, lzs_count, info.pc,
                        info.a0, info.a1, info.a2, info.ra))
                else
                    PCSX.log(string.format(
                        "[dpp] vsync=%d %s #%d pc=0x%08X val=0x%08X ra=0x%08X lzs_count=%d in_lzs=%d",
                        vsync_count, probe.name, hits[idx], info.pc,
                        info.val, info.ra, lzs_count, in_lzs))
                end
            end

            if hits[idx] == cap then
                PCSX.log(string.format(
                    "[dpp] %s cap reached at %d hits; further hits silently counted",
                    probe.name, cap))
            end
        end
        local bp = PCSX.addBreakpoint(
            probe.addr, probe.kind, 4,
            "dpp:" .. probe.name, cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[dpp] %d probes armed", #PROBES))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function dump_summary()
    PCSX.log("=== deep pool probe hit counts ===")
    for i, p in ipairs(PROBES) do
        local cap = p.cap or 100
        PCSX.log(string.format(
            "  %-16s  %-6s 0x%08X  hits=%d%s",
            p.name, p.kind, p.addr, hits[i],
            hits[i] > cap and " (capped)" or ""))
    end
    PCSX.log(string.format("  total LZS calls observed: %d", lzs_count))
    PCSX.log("=== end ===")
    if csv_fh then csv_fh:flush(); csv_fh:close(); csv_fh = nil end
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> ARM_THEN_LOAD -> CAPTURE -> DONE

local STATE_WAIT_BOOT     = 1
local STATE_ARMED_LOADED  = 2
local STATE_DONE          = 3

local state           = STATE_WAIT_BOOT
local capture_start   = nil

local BOOT_DELAY_VSYNCS = 60
local CAPTURE_VSYNCS    = FRAMES

local function try_load_save_state()
    local fh = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log("[dpp] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[dpp] save state loaded")
    return true
end

local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

local HITS_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")
local function write_live_hits(label)
    local f = io.open(HITS_PATH, "w")
    if not f then return end
    f:write(string.format("# %s  vsync=%d  capture_start=%s  lzs_count=%d\n",
        label, vsync_count, tostring(capture_start), lzs_count))
    for i, p in ipairs(PROBES) do
        local cap = p.cap or 100
        f:write(string.format("  %-16s  %-6s 0x%08X  hits=%d%s\n",
            p.name, p.kind, p.addr, hits[i],
            hits[i] > cap and " (capped)" or ""))
    end
    f:close()
end
local SNAPSHOT_EVERY = 60

local function on_vsync()
    vsync_count = vsync_count + 1
    if vsync_count % SNAPSHOT_EVERY == 0 then
        write_live_hits("live")
        if csv_fh then csv_fh:flush() end
    end

    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            arm_probes()
            if try_load_save_state() then
                state         = STATE_ARMED_LOADED
                capture_start = vsync_count
                PCSX.log("[dpp] probes armed before load; capture started")
                if HOLD_UP_FRAMES > 0 then
                    pad_force(BTN_UP)
                    pad_held = true
                    PCSX.log(string.format(
                        "[dpp] forcing D-pad UP held for %d vsyncs",
                        HOLD_UP_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if pad_held and vsync_count - capture_start >= HOLD_UP_FRAMES then
            pad_release(BTN_UP)
            pad_held = false
            PCSX.log(string.format(
                "[dpp] released D-pad UP at vsync %d",
                vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(BTN_UP); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            state = STATE_DONE
            PCSX.log("[dpp] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[dpp] vsync listener installed; waiting for boot")
