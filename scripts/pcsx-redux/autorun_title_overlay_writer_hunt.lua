-- autorun_title_overlay_writer_hunt.lua
--
-- Pins the SCUS function that LOADS the title overlay into RAM.
-- Arms Write breakpoints at several points inside the title-overlay
-- code region (0x801D5000..0x801E5000). The first write to each
-- address fires the BP; the callback captures PC + RA + a0..a3 +
-- s0..s7 (caller's args and the typical "src pointer in s0/s2,
-- dst pointer in s1, len in s2/s3" register-save convention) and
-- writes a per-probe CSV row plus a first-hit call-context detail
-- block (full GPRs + code window around PC + stack).
--
-- This works regardless of compression path -- raw DMA, LZS, custom
-- decoder, or any combination of the above will eventually write
-- bytes into the destination, and that write fires the BP. The PC
-- of the write instruction + the function it lives in identifies
-- the loader.
--
-- Run in cold-boot mode -- the title overlay is loaded by the SCUS
-- boot sequence before the user touches anything, and no in-game
-- save state captures that load:
--
--   LEGAIA_NO_SSTATE=1 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua \
--   LEGAIA_FRAMES=2400 \
--   LEGAIA_OUT=captures/boot_walk/title_overlay_writer.csv \
--       bash scripts/pcsx-redux/run_world_map_probe.sh
--
-- Output:
--   <OUT>                                 per-write CSV row
--   <OUT>.detail.txt                      first-hit call-context per probe
--   <OUT>.hits.txt                        live snapshot (auto-rotated)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local OUT_PATH    = probe.getenv("LEGAIA_OUT",
    "captures/boot_walk/title_overlay_writer.csv")
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")
local SNAP_PATH   = OUT_PATH:gsub("%.csv$", ".hits.txt")
local NO_SSTATE   = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local MAX_HITS    = probe.getenv_num("LEGAIA_MAX_HITS", 64)

if NO_SSTATE then
    probe.load_save_state = function(_)
        PCSX.log("[writer] LEGAIA_NO_SSTATE=1 -- cold-boot; sstate ignored")
        return true
    end
end

-- Probe addresses sample the title-overlay code region. We pick a few
-- so that whichever loader writes the region first, at least one BP
-- fires early enough to capture meaningful args.
local PROBE_ADDRS = {
    {0x801CC000, "ov_+0C000"},
    {0x801D0000, "ov_+10000"},
    {0x801D5000, "ov_+15000"},
    {0x801DA000, "ov_+1A000"},
    {0x801DD35C, "ov_tick_fn"},  -- FUN_801DD35C entry, the title-tick body
    {0x801E0000, "ov_+20000"},
    {0x801E5000, "ov_+25000"},
    {0x801EF018, "ov_state_struct"},
}

PCSX.log(string.format(
    "[writer] %d Write probes; out=%s frames=%d max_per=%d",
    #PROBE_ADDRS, OUT_PATH, FRAMES, MAX_HITS))

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,addr,name,hit,pc,ra,gp,a0,a1,a2,a3,s0,s1,s2,s3")

-- Wipe the detail sidecar at start.
local fh = io.open(DETAIL_PATH, "w")
if fh then
    fh:write(string.format(
        "# title-overlay writer-hunt detail sidecar\n"))
    fh:write(string.format(
        "# sstate=%s no_sstate=%s frames=%d\n\n",
        SSTATE_PATH, tostring(NO_SSTATE), FRAMES))
    fh:close()
end

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = SNAP_PATH,
    snapshot_every = 120,

    on_arm = function(_)
        local descs = {}
        for i, pa in ipairs(PROBE_ADDRS) do
            local idx, addr, name = i, pa[1], pa[2]
            local d = {
                addr = addr,
                name = name,
                hits_ref = { n = 0 },
                first_detail_written = false,
            }
            probe.arm_breakpoint(addr, "Write", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS then return end

                local r  = PCSX.getRegisters()
                local pc = n32(tonumber(r.pc)  or 0)
                local ra = n32(tonumber(r.GPR.n.ra) or 0)
                local gp = n32(tonumber(r.GPR.n.gp) or 0)
                local a0 = n32(tonumber(r.GPR.n.a0) or 0)
                local a1 = n32(tonumber(r.GPR.n.a1) or 0)
                local a2 = n32(tonumber(r.GPR.n.a2) or 0)
                local a3 = n32(tonumber(r.GPR.n.a3) or 0)
                local s0 = n32(tonumber(r.GPR.n.s0) or 0)
                local s1 = n32(tonumber(r.GPR.n.s1) or 0)
                local s2 = n32(tonumber(r.GPR.n.s2) or 0)
                local s3 = n32(tonumber(r.GPR.n.s3) or 0)

                csv:row(
                    "%d,0x%08X,%s,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
                    idx - 1, addr, name, d.hits_ref.n,
                    pc, ra, gp, a0, a1, a2, a3, s0, s1, s2, s3)

                if d.hits_ref.n <= 3 then
                    PCSX.log(string.format(
                        "[writer] %s hit %d: pc=0x%08X ra=0x%08X (a0=0x%08X a1=0x%08X)",
                        name, d.hits_ref.n, pc, ra, a0, a1))
                end

                if not d.first_detail_written then
                    d.first_detail_written = true
                    local label = string.format(
                        "first write at 0x%08X (%s)", addr, name)
                    local snap = probe.capture_call_context(label)
                    probe.append_call_context(DETAIL_PATH, snap)
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log(string.format("[writer] CSV: %s", OUT_PATH))
        PCSX.log(string.format("[writer] detail: %s", DETAIL_PATH))
        PCSX.log("=== writer-hunt summary ===")
        for _, d in ipairs(descs) do
            PCSX.log(string.format("  %s @ 0x%08X: %d writes",
                d.name, d.addr, d.hits_ref.n))
        end
    end,
})
