-- autorun_prim_pool_writers.lua
--
-- Pins which code writes the GPU prim pool at 0x800AD400+ (~341 KB,
-- ~5000 POLY_FT4 packets - the world-map continent emit target). The
-- draw VM at FUN_801D362C does NOT re-execute continent-render ops
-- during play, so the pool must be populated by a different path -
-- either at scene load or by a per-frame refresh outside the move VM.
--
-- Strategy: arm Write breakpoints at 13 offsets spanning OT head +
-- Buffer A (0x2C00..) + Buffer B (0x22C00..). The set of PCs that fire
-- is the geometry emitter. Each probe caps at MAX_HITS_PER_PROBE so
-- per-frame OT cleanup can't drown the log.
--
-- Env vars:
--   LEGAIA_SSTATE        save state (default sstate1)
--   LEGAIA_FRAMES        post-load capture vsyncs (default 120)
--   LEGAIA_OUT           CSV path (default prim_pool_writers.csv)
--   LEGAIA_POOL_BASE     pool base override (default 0x800AD400)
--   LEGAIA_HOLD_UP       D-pad UP hold for transition trigger (default 0)
--
-- Outputs:
--   <OUT>                per-write CSV (probe_idx, addr, pc, width, value, ra)
--   <stem>.hits.txt      live hit counts, rewritten every 60 vsyncs

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 120)
local OUT_PATH    = probe.out_path("prim_pool_writers.csv")
local POOL_BASE   = probe.getenv_num("LEGAIA_POOL_BASE", 0x800AD400)
local HOLD_UP     = probe.getenv_num("LEGAIA_HOLD_UP", 0)

local SNAP_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")

-- Probe at 13 offsets covering OT head + both buffers, so we can tell
-- whether a single loop walks the whole pool or whether camera-relative
-- and static-slab emitters use distinct PCs.
local PROBE_OFFSETS = {
    0x00008, 0x00100, 0x01000,                       -- OT head
    0x02D00, 0x05000, 0x0A000, 0x10000, 0x18000,     -- Buffer A
    0x22D00, 0x28000, 0x30000, 0x38000, 0x40000,     -- Buffer B
}
local MAX_HITS_PER = 50

PCSX.log(string.format("[ppw] sstate=%s frames=%d out=%s pool_base=0x%08X",
    SSTATE_PATH, FRAMES, OUT_PATH, POOL_BASE))

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,addr,pc,width,value,ra")

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = SNAP_PATH,
    snapshot_every = 60,
    hold_button    = (HOLD_UP > 0) and probe.BTN.UP or nil,
    hold_frames    = HOLD_UP,

    on_arm = function(_)
        local descs = {}
        for i, off in ipairs(PROBE_OFFSETS) do
            local idx  = i
            local addr = n32(POOL_BASE + off)
            local d = {
                addr = addr,
                name = string.format("ppw:0x%08X", addr),
                hits_ref = { n = 0 },
            }
            probe.arm_breakpoint(addr, "Write", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS_PER then return end
                local r  = PCSX.getRegisters()
                local pc = n32(tonumber(r.pc) or 0)
                local ra = n32(tonumber(r.GPR.n.ra) or 0)
                -- The value being written is in some store-source GPR
                -- that we can't identify without disassembling. Read
                -- back the address - that IS the value that just landed.
                local v = probe.read_u32(addr) or 0
                csv:row("%d,0x%08X,0x%08X,4,0x%08X,0x%08X",
                    idx - 1, addr, pc, v, ra)
                if d.hits_ref.n <= 3 then
                    PCSX.log(string.format(
                        "[ppw] probe %d (0x%08X) hit %d: pc=0x%08X val=0x%08X ra=0x%08X",
                        idx - 1, addr, d.hits_ref.n, pc, v, ra))
                end
                if d.hits_ref.n == MAX_HITS_PER then
                    PCSX.log(string.format(
                        "[ppw] probe %d cap reached at %d hits",
                        idx - 1, MAX_HITS_PER))
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[ppw] %d Write probes armed across pool",
            #PROBE_OFFSETS))
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("=== prim-pool writer hit counts ===")
        for i, d in ipairs(descs) do
            local capped = d.hits_ref.n > MAX_HITS_PER and " (capped)" or ""
            PCSX.log(string.format("  probe %d  0x%08X  hits=%d%s",
                i - 1, d.addr, d.hits_ref.n, capped))
        end
        PCSX.log("=== end ===")
    end,
})
