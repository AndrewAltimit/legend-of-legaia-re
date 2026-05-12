-- autorun_lzs_and_bundle_probe.lua
--
-- Closed-loop probe for the second leg of the world-map terrain load.
--
-- autorun_cd_dma_probe.lua showed 12 CD-DMA reads during the town ->
-- world-map transition, but none land in the prim pool at 0x800AD400.
-- Two reads of 215 KB each land at 0x80184BD0 (PROTs 990 + 1107, ra =
-- 0x8001FD18 in both cases). The leading hypothesis is now:
--
--   DMA -> 0x80184BD0 (compressed bundle, 215 KB)
--   CPU -> LZS decoder FUN_8001A55C reads from 0x80184BD0, writes the
--          POLY_FT4 chain into 0x800AD400+
--
-- This script captures both halves in one run:
--
--   * Exec probe on 0x8001A55C   - LZS decoder entry; logs (a0=src,
--                                  a1=dst, ra=caller). If dst lands
--                                  inside [0x800AD400, 0x800EFFFF] AND
--                                  src equals one of the CD staging
--                                  addresses, the second leg is proven.
--   * Exec probe on 0x8003E800   - CD setup; correlates LZS calls
--                                  against the staging-write timeline.
--   * Write probes at 0x80184BD0  - confirms the DMA actually lands
--     and three deeper offsets     bytes here (Write breakpoints may
--                                  or may not fire on DMA writes -
--                                  worth measuring).
--   * Write probes at 0x800AD408, - pool head + deep offsets in both
--     0x800B5000, 0x800D5000        double-buffered halves. Catches
--                                  the second-leg CPU writer's PC if
--                                  it isn't LZS (e.g. if LZS targets a
--                                  scratch buffer and another routine
--                                  copies into the pool).
--
-- Output CSV unifies both probe kinds:
--   vsync,probe_idx,name,kind,pc,a0,a1,a2,a3,ra,val,bin_a0,bin_a1
--
-- For Exec hits, val=0 and pc=entry PC.
-- For Write hits, a0..a3 are still snapshot at the write but the
-- significant fields are pc (writing instruction) and val (what landed).
--
-- Run via the wrapper:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_lzs_and_bundle_probe.lua \
--     LEGAIA_OUT=lzs_bundle_probe.csv \
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
local OUT_PATH    = getenv("LEGAIA_OUT",  "lzs_bundle_probe.csv")
local HOLD_UP_FRAMES = tonumber(getenv("LEGAIA_HOLD_UP", "0"))
local BTN_UP = 4

-- Heterogeneous probe table. Each probe is either Exec (entry-PC trap)
-- or Write (memory-write trap with width = 4).
local PROBES = {
    -- Exec — second-leg candidates
    { addr = 0x8001A55C, name = "lzs_decoder",      kind = "Exec",  cap = 400 },
    { addr = 0x8003E800, name = "cd_setup",         kind = "Exec",  cap = 100 },
    -- Write — source bundle staging (DMA target from cd_dma_probe vsync 268+322)
    { addr = 0x80184BD0, name = "bundle_start",     kind = "Write", cap = 50  },
    { addr = 0x80194BD0, name = "bundle_mid",       kind = "Write", cap = 50  },
    { addr = 0x801B4BD0, name = "bundle_late",      kind = "Write", cap = 50  },
    -- Write — destination prim pool. 0x800AD408 = past the 8-byte chain
    -- header into the first prim's body. 0x800B5000 is mid Buffer A,
    -- 0x800D5000 is mid Buffer B.
    { addr = 0x800AD408, name = "pool_head",        kind = "Write", cap = 100 },
    { addr = 0x800B5000, name = "pool_buffer_A",    kind = "Write", cap = 100 },
    { addr = 0x800D5000, name = "pool_buffer_B",    kind = "Write", cap = 100 },
}

PCSX.log(string.format("[lbp] sstate=%s frames=%d out=%s",
    SSTATE_PATH, FRAMES, OUT_PATH))

------------------------------------------------------------------
-- Region classifier — turns any pointer into a one-word bin.

local function classify(addr)
    if addr == 0 then return "null" end
    local ka = bit.band(addr, 0x1FFFFFFF)
    if ka >= 0x000AD400 and ka < 0x000F0000 then return "prim_pool"  end
    if ka >= 0x00180000 and ka < 0x001C0000 then return "bundle_stage" end  -- the 0x80184BD0 region
    if ka >= 0x001C0000 and ka < 0x001F0000 then return "overlay"    end
    if ka >= 0x00080000 and ka < 0x000A0000 then return "low_data"   end
    if ka >= 0x00130000 and ka < 0x00180000 then return "high_data_l" end
    if ka >= 0x001F0000 and ka < 0x00200000 then return "scratch"    end
    return "other"
end

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("vsync,probe_idx,name,kind,pc,a0,a1,a2,a3,ra,val,bin_a0,bin_a1\n")
    csv_fh:flush()
else
    PCSX.log("[lbp] FATAL: cannot open " .. OUT_PATH .. ": " ..
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
for i = 1, #PROBES do hits[i] = 0 end

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
            local b0 = classify(info.a0)
            local b1 = classify(info.a1)
            if csv_fh then
                csv_fh:write(string.format(
                    "%d,%d,%s,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%s,%s\n",
                    vsync_count, idx - 1, probe.name, probe.kind,
                    info.pc, info.a0, info.a1, info.a2, info.a3, info.ra,
                    info.val, b0, b1))
                csv_fh:flush()
            end
            -- Inline log: every LZS hit (the smoking-gun probe) plus
            -- first 3 of every other probe.
            if probe.name == "lzs_decoder" or hits[idx] <= 3 then
                if probe.kind == "Exec" then
                    PCSX.log(string.format(
                        "[lbp] vsync=%d %s #%d pc=0x%08X a0=0x%08X(%s) a1=0x%08X(%s) ra=0x%08X",
                        vsync_count, probe.name, hits[idx],
                        info.pc, info.a0, b0, info.a1, b1, info.ra))
                else
                    PCSX.log(string.format(
                        "[lbp] vsync=%d %s #%d pc=0x%08X val=0x%08X ra=0x%08X",
                        vsync_count, probe.name, hits[idx],
                        info.pc, info.val, info.ra))
                end
            end
            if hits[idx] == cap then
                PCSX.log(string.format(
                    "[lbp] %s cap reached at %d hits; further hits silently counted",
                    probe.name, cap))
            end
        end
        local bp = PCSX.addBreakpoint(
            probe.addr, probe.kind, 4,
            "lbp:" .. probe.name, cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[lbp] %d probes armed", #PROBES))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function dump_summary()
    PCSX.log("=== lzs+bundle probe hit counts ===")
    for i, p in ipairs(PROBES) do
        local cap = p.cap or 100
        PCSX.log(string.format(
            "  %-16s  %-6s 0x%08X  hits=%d%s",
            p.name, p.kind, p.addr, hits[i],
            hits[i] > cap and " (capped)" or ""))
    end
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
        PCSX.log("[lbp] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[lbp] save state loaded")
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
    f:write(string.format("# %s  vsync=%d  capture_start=%s\n",
        label, vsync_count, tostring(capture_start)))
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
    end

    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            arm_probes()
            if try_load_save_state() then
                state         = STATE_ARMED_LOADED
                capture_start = vsync_count
                PCSX.log("[lbp] probes armed before load; capture started")
                if HOLD_UP_FRAMES > 0 then
                    pad_force(BTN_UP)
                    pad_held = true
                    PCSX.log(string.format(
                        "[lbp] forcing D-pad UP held for %d vsyncs",
                        HOLD_UP_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if pad_held and vsync_count - capture_start >= HOLD_UP_FRAMES then
            pad_release(BTN_UP)
            pad_held = false
            PCSX.log(string.format(
                "[lbp] released D-pad UP at vsync %d",
                vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(BTN_UP); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            state = STATE_DONE
            PCSX.log("[lbp] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[lbp] vsync listener installed; waiting for boot")
