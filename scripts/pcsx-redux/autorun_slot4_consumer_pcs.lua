-- autorun_slot4_consumer_pcs.lua
--
-- Cross-kingdom verification that the slot-4 reader PCs identified
-- from the Drake transition capture also fire during Sebucus / Karisto
-- transitions. The original Read-breakpoint probe (autorun_slot4_readers.lua)
-- is tied to Drake's slot-4 base address and per-body offsets, so it
-- can't directly validate the same consumer is reused.
--
-- This probe arms Exec breakpoints at the function entry points + the
-- specific LW instructions inside cluster A / B / C from the Drake
-- capture. If those PCs hit on Sebucus / Karisto with non-zero hit
-- counts, the consumer is cross-kingdom; if a cluster goes dark the
-- per-kingdom world-map overlay re-implements it.
--
-- Run:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate4 \
--   LEGAIA_HOLD_BUTTON=6 LEGAIA_HOLD=60 \
--   LEGAIA_FRAMES=1800 \
--   LEGAIA_OUT=/tmp/slot4_pcs_sebucus.csv \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua \
--       bash scripts/pcsx-redux/run_world_map_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local OUT_PATH    = probe.getenv("LEGAIA_OUT", "slot4_consumer_pcs.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")

-- PCs surfaced by autorun_slot4_readers.lua during the Drake transition
-- capture (sstate1, held UP). Each entry is a specific LW instruction
-- inside the reader's body that touched slot-4 data; the comments name
-- the slot-4 offset that PC was reading at the time the breakpoint
-- fired. The clusters A / B / C are the three distinct consumer PCs
-- identified from the Read-breakpoint detail dump.
local CONSUMER_PCS = {
    -- Cluster A - GTE-driven primitive emitter; RA 0x801F78D4 (world-map overlay)
    { pc = 0x800455E4, name = "A_lw_count_word",        cluster = "A" },
    { pc = 0x800455E8, name = "A_lw_body0_offset",      cluster = "A" },
    { pc = 0x80044B00, name = "A_lw_body0_mid",         cluster = "A" },
    { pc = 0x8004561C, name = "A_lw_body1_records",     cluster = "A" },
    { pc = 0x80045658, name = "A_lw_body0_record_14",   cluster = "A" },
    { pc = 0x80044E08, name = "A_lw_body12_records",    cluster = "A" },
    { pc = 0x80045418, name = "A_lw_body13_records",    cluster = "A" },
    -- Cluster B - SCUS mid-body reader; RA 0x80059C00
    { pc = 0x80059DE4, name = "B_lw_midbody",           cluster = "B" },
    -- Cluster C - SCUS near-end consumer; RA 0x8001BC8C
    { pc = 0x80044C70, name = "C_lw_near_end",          cluster = "C" },
}
-- Per-probe cap. Defaults to 200 (fast for kingdom verification);
-- set LEGAIA_PC_CAP to a larger value (e.g. 5000) to surface uncapped
-- totals for per-kind delta analysis.
local MAX_HITS_PER_PROBE = probe.getenv_num("LEGAIA_PC_CAP", 200)

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,cluster,pc,name,ra,a0,a1,a2,a3,s8")

local fh = io.open(DETAIL_PATH, "w")
if fh then
    fh:write(string.format(
        "# slot-4 consumer-PC verification probe; sstate=%s\n\n",
        SSTATE_PATH))
    fh:close()
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function()
        local descs = {}
        for i, p in ipairs(CONSUMER_PCS) do
            local idx       = i
            local pc        = p.pc
            local d         = {
                addr     = pc,
                name     = string.format("%s_%08X", p.cluster, pc),
                cluster  = p.cluster,
                label    = p.name,
                hits_ref = { n = 0, capped = false },
            }
            probe.arm_breakpoint(pc, "Exec", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS_PER_PROBE then
                    if not d.hits_ref.capped then
                        PCSX.log(string.format(
                            "[s4pc] probe %d cap reached at %d hits",
                            idx - 1, MAX_HITS_PER_PROBE))
                        d.hits_ref.capped = true
                    end
                    return
                end
                local r  = PCSX.getRegisters()
                local ra = tonumber(r.GPR.n.ra) or 0
                local a0 = tonumber(r.GPR.n.a0) or 0
                local a1 = tonumber(r.GPR.n.a1) or 0
                local a2 = tonumber(r.GPR.n.a2) or 0
                local a3 = tonumber(r.GPR.n.a3) or 0
                local s8 = tonumber(r.GPR.n.s8) or 0
                csv:row("%d,%s,0x%08X,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
                    idx - 1, p.cluster, pc, p.name, ra, a0, a1, a2, a3, s8)
                if d.hits_ref.n <= 3 then
                    PCSX.log(string.format(
                        "[s4pc] probe %d (%s) hit %d: pc=0x%08X ra=0x%08X",
                        idx - 1, d.name, d.hits_ref.n, pc, ra))
                end
                if d.hits_ref.n == 1 then
                    local label = string.format(
                        "probe %d cluster=%s pc=0x%08X (%s)",
                        idx - 1, p.cluster, pc, p.name)
                    local snap = probe.capture_call_context(label)
                    probe.append_call_context(DETAIL_PATH, snap)
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format(
            "[s4pc] %d Exec probes armed at slot-4 consumer PCs", #descs))
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("[s4pc] CSV closed: " .. OUT_PATH)
        PCSX.log("[s4pc] detail sidecar: " .. DETAIL_PATH)
        -- Group by cluster so the summary makes the cross-kingdom
        -- comparison obvious at a glance.
        local by_cluster = {}
        for _, d in ipairs(descs) do
            local c = d.cluster or "?"
            by_cluster[c] = (by_cluster[c] or 0) + (d.hits_ref and d.hits_ref.n or 0)
        end
        PCSX.log("=== slot-4 consumer-PC cluster totals ===")
        for c, n in pairs(by_cluster) do
            PCSX.log(string.format("  cluster %s: %d hits", c, n))
        end
    end,
})
