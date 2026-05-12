-- autorun_cd_dma_probe.lua
--
-- Closed-loop probe for "is the continent prim pool loaded by CD-DMA?"
--
-- The autorun_prim_pool_writers run came back empty deep in the pool
-- during play, and FUN_801D362C never dispatched the 0x2B-0x2E render
-- ops. Two independent Exec/Write measurements both say the CPU does
-- not rebuild the terrain. The hypothesis is that the build happens
-- via the BIOS CD-DMA path: CdReadSector / DslReadN copies a pre-baked
-- POLY_FT4 chain straight from disc into Buffer A, bypassing CPU
-- breakpoints entirely (the writes are landed by the DMA controller).
--
-- This script catches the entry of every documented CD-loader function
-- and snapshots a0..a3 + ra. The crucial telemetry: a0 of FUN_8003E800
-- IS the destination address of the upcoming DMA. If we see calls with
-- dst inside [0x800AD400, 0x800EFFFF] during a town->world-map
-- transition, the DMA-load hypothesis is confirmed and ra tells us
-- which high-level caller orchestrated it.
--
-- The probe also classifies dst by region in a "bin" column so a quick
-- glance at the CSV tells the story without a calculator.
--
-- Run via the same wrapper:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_cd_dma_probe.lua \
--     LEGAIA_OUT=cd_dma_probe.csv \
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
local OUT_PATH    = getenv("LEGAIA_OUT",  "cd_dma_probe.csv")
local HOLD_UP_FRAMES = tonumber(getenv("LEGAIA_HOLD_UP", "0"))
local BTN_UP = 4

-- Probes — every documented CD loader-chain entry in functions.md
-- "Disc / loader chain" table. Cap reasonably high so we don't truncate
-- a transition that streams dozens of sectors in a burst.
local PROBES = {
    { addr = 0x8003E6BC, name = "path_opener"        },  -- by name
    { addr = 0x8003E800, name = "cd_setup"           },  -- dst+size stored here
    { addr = 0x8003E8A8, name = "lba_resolver"       },  -- prot_idx -> start_lba
    { addr = 0x8003EB98, name = "by_index_loader"    },  -- wraps resolver + setup
    { addr = 0x8003F128, name = "async_kickoff"      },  -- BIOS handoff
}
local MAX_HITS_PER_PROBE = 200

PCSX.log(string.format("[cdd] sstate=%s frames=%d out=%s",
    SSTATE_PATH, FRAMES, OUT_PATH))

------------------------------------------------------------------
-- Region classifier — turns a destination pointer into a one-word bin
-- so the resulting CSV is human-readable at a glance.

local function classify_dst(addr)
    if addr == 0 then return "null" end
    local ka = bit.band(addr, 0x1FFFFFFF)  -- strip kseg flags
    if ka >= 0x000AD400 and ka < 0x000F0000 then return "prim_pool"  end
    if ka >= 0x000B0000 and ka < 0x000D0000 then return "buffer_A"   end
    if ka >= 0x000D0000 and ka < 0x000F0000 then return "buffer_B"   end
    if ka >= 0x001C0000 and ka < 0x001F0000 then return "overlay"    end
    if ka >= 0x00080000 and ka < 0x000A0000 then return "low_data"   end
    if ka >= 0x00100000 and ka < 0x00200000 then return "high_data"  end
    return "other"
end

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("vsync,probe_idx,name,pc,a0,a1,a2,a3,ra,bin_a0\n")
    csv_fh:flush()
else
    PCSX.log("[cdd] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

------------------------------------------------------------------
-- Probe state

local hits = {}        -- probe_idx -> count
local bps  = {}        -- list of bp objects
for i = 1, #PROBES do hits[i] = 0 end

-- For tracking elapsed time at the moment of each hit.
local vsync_count = 0

------------------------------------------------------------------
-- Arm + disarm

local function arm_probes()
    for i, p in ipairs(PROBES) do
        local idx = i
        local probe = p
        local cb = function(_, _, _)
            hits[idx] = hits[idx] + 1
            if hits[idx] > MAX_HITS_PER_PROBE then return end
            local ok, info = pcall(function()
                local r = PCSX.getRegisters()
                return {
                    pc = tonumber(r.pc) or 0,
                    a0 = tonumber(r.GPR.n.a0) or 0,
                    a1 = tonumber(r.GPR.n.a1) or 0,
                    a2 = tonumber(r.GPR.n.a2) or 0,
                    a3 = tonumber(r.GPR.n.a3) or 0,
                    ra = tonumber(r.GPR.n.ra) or 0,
                }
            end)
            if not ok then return end
            local bin = classify_dst(info.a0)
            if csv_fh then
                csv_fh:write(string.format(
                    "%d,%d,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%s\n",
                    vsync_count, idx - 1, probe.name,
                    info.pc, info.a0, info.a1, info.a2, info.a3, info.ra,
                    bin))
                csv_fh:flush()
            end
            -- Log only the cd_setup hits inline (the smoking-gun probe)
            -- plus the first three hits of every other probe.
            if probe.name == "cd_setup" or hits[idx] <= 3 then
                PCSX.log(string.format(
                    "[cdd] vsync=%d %s #%d pc=0x%08X a0=0x%08X(%s) a1=0x%08X ra=0x%08X",
                    vsync_count, probe.name, hits[idx],
                    info.pc, info.a0, bin, info.a1, info.ra))
            end
            if hits[idx] == MAX_HITS_PER_PROBE then
                PCSX.log(string.format(
                    "[cdd] %s cap reached at %d hits; further hits silently counted",
                    probe.name, MAX_HITS_PER_PROBE))
            end
        end
        local bp = PCSX.addBreakpoint(
            probe.addr, "Exec", 4,
            "cdd:" .. probe.name, cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[cdd] %d Exec probes armed", #PROBES))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function dump_summary()
    PCSX.log("=== cd-dma probe hit counts ===")
    for i, p in ipairs(PROBES) do
        PCSX.log(string.format(
            "  %s  0x%08X  hits=%d%s",
            p.name, p.addr, hits[i],
            hits[i] > MAX_HITS_PER_PROBE and " (capped)" or ""))
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

local BOOT_DELAY_VSYNCS    = 60
local CAPTURE_VSYNCS       = FRAMES

local function try_load_save_state()
    local fh = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log("[cdd] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[cdd] save state loaded")
    return true
end

-- Pad helpers (same as the other autorun probes).
local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

-- Live snapshot every 60 vsync so we have hit counts even if pcsx-redux
-- exits early.
local HITS_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")
local function write_live_hits(label)
    local f = io.open(HITS_PATH, "w")
    if not f then return end
    f:write(string.format("# %s  vsync=%d  capture_start=%s\n",
        label, vsync_count, tostring(capture_start)))
    for i, p in ipairs(PROBES) do
        f:write(string.format("  %-16s  0x%08X  hits=%d%s\n",
            p.name, p.addr, hits[i],
            hits[i] > MAX_HITS_PER_PROBE and " (capped)" or ""))
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
                PCSX.log("[cdd] probes armed before load; capture started")
                if HOLD_UP_FRAMES > 0 then
                    pad_force(BTN_UP)
                    pad_held = true
                    PCSX.log(string.format(
                        "[cdd] forcing D-pad UP held for %d vsyncs",
                        HOLD_UP_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if pad_held and vsync_count - capture_start >= HOLD_UP_FRAMES then
            pad_release(BTN_UP)
            pad_held = false
            PCSX.log(string.format(
                "[cdd] released D-pad UP at vsync %d",
                vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(BTN_UP); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            state = STATE_DONE
            PCSX.log("[cdd] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[cdd] vsync listener installed; waiting for boot")
