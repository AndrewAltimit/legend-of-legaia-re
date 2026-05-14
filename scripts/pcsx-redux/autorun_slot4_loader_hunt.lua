-- autorun_slot4_loader_hunt.lua
--
-- Finds the function that populates slot 4 in RAM (0x8011A624 for
-- Drake) during the kingdom warp transition. The Drake Read-bp probe
-- (autorun_slot4_readers.lua) pinned the slot-4 READERS but not the
-- writer. This probe arms Write breakpoints at slot-4 RAM offsets and
-- logs PC + ra at each first-write to capture the loader's call chain.
--
-- Run with sstate1 (Drake, held UP for 60 vsyncs into warp):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
--   LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=60 LEGAIA_FRAMES=300 \
--   LEGAIA_OUT=/tmp/slot4_loader_drake.csv \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_loader_hunt.lua \
--       bash scripts/pcsx-redux/run_world_map_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 300)
local OUT_PATH    = probe.getenv("LEGAIA_OUT", "slot4_loader.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local SLOT4_BASE  = probe.getenv_num("LEGAIA_SLOT4_BASE", 0x8011A624)
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")

-- Same offsets as the readers probe so we can directly cross-reference
-- read sites with write sites.
local PROBE_OFFSETS = {
    0x00000,   -- outer count word
    0x00004,   -- body 0 word_offset
    0x00040,   -- body 0 records start
    0x00118,   -- body 0 record 14 (mid-body)
    0x00188,   -- body 1 records start
    0x00420,   -- body 4 records start
    0x00800,   -- body 4 mid
    0x010C8,   -- body 9 region
    0x018A4,   -- body 12 records start
    0x02000,   -- body 12 mid
    0x02800,   -- body 12 later
    0x037CC,   -- body 13 records start
    0x05400,   -- body 14 region
    0x07000,   -- near end
}
local MAX_HITS_PER_PROBE = probe.getenv_num("LEGAIA_LOAD_CAP", 50)

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,addr,pc,width,value,ra,a0,a1,a2,a3")

local fh = io.open(DETAIL_PATH, "w")
if fh then
    fh:write(string.format(
        "# slot-4 loader-hunt probe; base=0x%08X; sstate=%s\n\n",
        SLOT4_BASE, SSTATE_PATH))
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
        for i, off in ipairs(PROBE_OFFSETS) do
            local idx  = i
            local addr = SLOT4_BASE + off
            local d    = {
                addr     = addr,
                name     = string.format("S4_+0x%05X", off),
                offset   = off,
                hits_ref = { n = 0, capped = false },
            }
            probe.arm_breakpoint(addr, "Write", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS_PER_PROBE then
                    if not d.hits_ref.capped then
                        PCSX.log(string.format(
                            "[loader] probe %d cap reached at %d writes",
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
                        "[loader] probe %d (%s) hit %d: pc=0x%08X ra=0x%08X",
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
            "[loader] %d Write probes armed across slot 4 at 0x%08X",
            #descs, SLOT4_BASE))
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("[loader] CSV closed: " .. OUT_PATH)
        PCSX.log("[loader] detail sidecar: " .. DETAIL_PATH)
        PCSX.log("=== slot-4 loader-hunt summary ===")
        for _, d in ipairs(descs) do
            local cap = d.hits_ref.capped and " (capped)" or ""
            PCSX.log(string.format(
                "  %s: %d writes%s", d.name, d.hits_ref.n, cap))
        end
    end,
})
