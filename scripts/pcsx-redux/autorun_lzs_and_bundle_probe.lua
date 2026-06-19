-- autorun_lzs_and_bundle_probe.lua
--
-- Captures both legs of a world-map terrain load in one run:
--
--   1. LZS decoder entry (FUN_8001A55C) - logs (src, dst, ra). If dst
--      lands inside [0x800AD400, 0x800EFFFF] AND src matches a known
--      CD staging address, the LZS→pool hypothesis is proven.
--   2. CD setup entry (FUN_8003E800) - correlates LZS calls against
--      the staging-write timeline.
--   3. Write probes at 0x80184BD0 + two deeper offsets - confirm the
--      DMA actually lands bytes (Write breakpoints may or may not fire
--      on DMA writes; worth measuring).
--   4. Write probes at 0x800AD408, 0x800B5000, 0x800D5000 - pool head
--      + mid Buffer A + mid Buffer B. Catches any non-LZS pool writer.
--
-- Env vars:
--   LEGAIA_SSTATE   save state (default sstate1)
--   LEGAIA_FRAMES   post-load capture vsyncs (default 600)
--   LEGAIA_OUT      CSV path (default lzs_bundle_probe.csv)
--   LEGAIA_HOLD_UP  D-pad UP hold to drive a town → world-map transition

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH    = probe.out_path("lzs_bundle_probe.csv")
local HOLD_UP     = probe.getenv_num("LEGAIA_HOLD_UP", 0)

local SNAP_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")

local PROBES = {
    { addr = 0x8001A55C, name = "lzs_decoder",   kind = "Exec",  cap = 400 },
    { addr = 0x8003E800, name = "cd_setup",      kind = "Exec",  cap = 100 },
    { addr = 0x80184BD0, name = "bundle_start",  kind = "Write", cap = 50 },
    { addr = 0x80194BD0, name = "bundle_mid",    kind = "Write", cap = 50 },
    { addr = 0x801B4BD0, name = "bundle_late",   kind = "Write", cap = 50 },
    { addr = 0x800AD408, name = "pool_head",     kind = "Write", cap = 100 },
    { addr = 0x800B5000, name = "pool_buffer_A", kind = "Write", cap = 100 },
    { addr = 0x800D5000, name = "pool_buffer_B", kind = "Write", cap = 100 },
}

PCSX.log(string.format("[lbp] sstate=%s frames=%d out=%s",
    SSTATE_PATH, FRAMES, OUT_PATH))

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

-- Region classifier - turns any pointer into a one-word bin.
local function classify(addr)
    if addr == 0 then return "null" end
    local ka = bit.band(addr, 0x1FFFFFFF)
    if ka >= 0x000AD400 and ka < 0x000F0000 then return "prim_pool" end
    if ka >= 0x00180000 and ka < 0x001C0000 then return "bundle_stage" end
    if ka >= 0x001C0000 and ka < 0x001F0000 then return "overlay" end
    if ka >= 0x00080000 and ka < 0x000A0000 then return "low_data" end
    if ka >= 0x00130000 and ka < 0x00180000 then return "high_data_l" end
    if ka >= 0x001F0000 and ka < 0x00200000 then return "scratch" end
    return "other"
end

local csv = probe.csv_open(OUT_PATH,
    "vsync,probe_idx,name,kind,pc,a0,a1,a2,a3,ra,val,bin_a0,bin_a1")

local vsync_after_load = 0

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = SNAP_PATH,
    snapshot_every = 60,
    hold_button    = (HOLD_UP > 0) and probe.BTN.UP or nil,
    hold_frames    = HOLD_UP,

    on_arm = function(_)
        local descs = {}
        for i, p in ipairs(PROBES) do
            local idx, cap, pkind, pname, paddr = i, p.cap, p.kind, p.name, p.addr
            local d = { addr = paddr, name = pname, hits_ref = { n = 0 } }
            probe.arm_breakpoint(paddr, pkind, 4, "lbp:" .. pname, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > cap then return end
                local r = PCSX.getRegisters()
                local pc = n32(tonumber(r.pc) or 0)
                local a0 = n32(tonumber(r.GPR.n.a0) or 0)
                local a1 = n32(tonumber(r.GPR.n.a1) or 0)
                local a2 = n32(tonumber(r.GPR.n.a2) or 0)
                local a3 = n32(tonumber(r.GPR.n.a3) or 0)
                local ra = n32(tonumber(r.GPR.n.ra) or 0)
                local val = 0
                if pkind == "Write" then val = probe.read_u32(paddr) or 0 end
                local b0 = classify(a0)
                local b1 = classify(a1)
                csv:row(
                    "%d,%d,%s,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%s,%s",
                    vsync_after_load, idx - 1, pname, pkind,
                    pc, a0, a1, a2, a3, ra, val, b0, b1)
                if pname == "lzs_decoder" or d.hits_ref.n <= 3 then
                    if pkind == "Exec" then
                        PCSX.log(string.format(
                            "[lbp] vsync=%d %s #%d pc=0x%08X a0=0x%08X(%s) a1=0x%08X(%s) ra=0x%08X",
                            vsync_after_load, pname, d.hits_ref.n,
                            pc, a0, b0, a1, b1, ra))
                    else
                        PCSX.log(string.format(
                            "[lbp] vsync=%d %s #%d pc=0x%08X val=0x%08X ra=0x%08X",
                            vsync_after_load, pname, d.hits_ref.n, pc, val, ra))
                    end
                end
                if d.hits_ref.n == cap then
                    PCSX.log(string.format(
                        "[lbp] %s cap reached at %d hits", pname, cap))
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[lbp] %d probes armed", #PROBES))
        return descs
    end,

    on_capture = function(_, elapsed)
        vsync_after_load = elapsed
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("=== lzs+bundle probe hit counts ===")
        for i, p in ipairs(PROBES) do
            local capped = descs[i].hits_ref.n > p.cap and " (capped)" or ""
            PCSX.log(string.format(
                "  %-16s  %-6s 0x%08X  hits=%d%s",
                p.name, p.kind, p.addr, descs[i].hits_ref.n, capped))
        end
        PCSX.log("=== end ===")
    end,
})
