-- autorun_slot4_transcoder_hunt.lua
--
-- Finds the function that writes the world-map working buffer at
-- ~0x801BA000 - the place cluster A (FUN_80043390) walks per-frame
-- for slot-4-derived rendering.
--
-- Background: cross-kingdom Exec-bp captures
-- (autorun_slot4_consumer_pcs.lua) show cluster A's a1 (command
-- stream) and a2 (vertex pool) pointing at 0x801BA8E4 / 0x801BA7F8 -
-- NOT slot 4's documented RAM range (0x8011A624..0x80122454 for
-- Drake). The combination of FUN_8001ada4's mesh-table walk at
-- actor+0x44 + the working-buffer pointers strongly suggests slot 4
-- is **transcoded** into TMD-style mesh structs in 0x801BA000-ish
-- during scene load. This probe arms Write breakpoints at structural
-- offsets in the working buffer to surface the transcoder function.
--
-- Run for the kingdom warp transition (held UP from sstate1 / held
-- DOWN from sstate4/5):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
--   LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=60 LEGAIA_FRAMES=600 \
--   LEGAIA_OUT=/tmp/slot4_transcoder.csv \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_transcoder_hunt.lua \
--       bash scripts/pcsx-redux/run_world_map_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH    = probe.getenv("LEGAIA_OUT", "slot4_transcoder.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")

-- Working-buffer address offsets. The cross-kingdom capture captured
-- mesh-struct vertex_base @ 0x801BA7F8 and command_stream @ 0x801BA8E4,
-- so the buffer occupies the 0x801BA000 region. Arm bps at a spread
-- of offsets so we catch the transcoder writing anywhere in the buffer.
local WB_BASE = probe.getenv_num("LEGAIA_WB_BASE", 0x801BA000)
local WB_OFFSETS = {
    0x000,    -- buffer start
    0x100,    -- first mesh struct's vertex_base region
    0x7F8,    -- exactly where cross-kingdom probe saw a2 (vertex_base)
    0x8E4,    -- exactly where cross-kingdom probe saw a1 (command_stream)
    0x1000,   -- 4 KB in
    0x2000,   -- 8 KB in
    0x4000,   -- 16 KB in
    0x6000,   -- 24 KB in
}
local MAX_HITS_PER_PROBE = probe.getenv_num("LEGAIA_WB_CAP", 50)

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,addr,pc,width,value,ra,a0,a1,a2,a3")

local fh = io.open(DETAIL_PATH, "w")
if fh then
    fh:write(string.format(
        "# slot-4 transcoder-hunt probe; wb_base=0x%08X; sstate=%s\n\n",
        WB_BASE, SSTATE_PATH))
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
        for i, off in ipairs(WB_OFFSETS) do
            local idx  = i
            local addr = WB_BASE + off
            local d    = {
                addr     = addr,
                name     = string.format("WB_+0x%05X", off),
                offset   = off,
                hits_ref = { n = 0, capped = false },
            }
            probe.arm_breakpoint(addr, "Write", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS_PER_PROBE then
                    if not d.hits_ref.capped then
                        PCSX.log(string.format(
                            "[xcdr] probe %d cap reached at %d writes",
                            idx - 1, MAX_HITS_PER_PROBE))
                        d.hits_ref.capped = true
                    end
                    return
                end
                local r  = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                local a0 = tonumber(r.GPR.n.a0) or 0
                local a1 = tonumber(r.GPR.n.a1) or 0
                local a2 = tonumber(r.GPR.n.a2) or 0
                local a3 = tonumber(r.GPR.n.a3) or 0
                csv:row("%d,0x%08X,0x%08X,4,0,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
                    idx - 1, addr, pc, ra, a0, a1, a2, a3)
                if d.hits_ref.n <= 3 then
                    PCSX.log(string.format(
                        "[xcdr] probe %d (%s) hit %d: pc=0x%08X ra=0x%08X",
                        idx - 1, d.name, d.hits_ref.n, pc, ra))
                end
                if d.hits_ref.n == 1 then
                    local label = string.format(
                        "probe %d addr=0x%08X (%s) first write",
                        idx - 1, addr, d.name)
                    local snap = probe.capture_call_context(label)
                    probe.append_call_context(DETAIL_PATH, snap)
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format(
            "[xcdr] %d Write probes armed across working buffer at 0x%08X",
            #descs, WB_BASE))
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("[xcdr] CSV closed: " .. OUT_PATH)
        PCSX.log("[xcdr] detail sidecar: " .. DETAIL_PATH)
        PCSX.log("=== transcoder-hunt summary ===")
        for _, d in ipairs(descs) do
            local cap = d.hits_ref.capped and " (capped)" or ""
            PCSX.log(string.format(
                "  %s: %d writes%s", d.name, d.hits_ref.n, cap))
        end
    end,
})
