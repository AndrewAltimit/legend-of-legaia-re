-- autorun_slot4_readers.lua
--
-- Closed-loop probe for "what code reads kingdom slot-4 records".
--
-- Slot-4 container is solved (15 sub-bodies for Drake; byte-verified
-- against live RAM at 0x8011A624..0x80122454), but the record
-- interpretation is open. Static sweep of captured overlays is empty -
-- the consumer reads via runtime-loaded pointer, not LUI+ADDIU.
--
-- This probe arms Read breakpoints at structurally interesting offsets
-- across the slot-4 region and logs PC + ra of every reader. Run during
-- the dev-menu top-view (steady state) AND across a kingdom-bundle
-- scene-load transition - if the dev-menu doesn't read slot 4, only
-- the scene-load path will.
--
-- Output CSV columns: probe_idx, addr, pc, width, value, ra
--
-- ## Modes
--
-- Default (steady-state count): arm Read bps, log every hit to CSV,
-- run for LEGAIA_FRAMES vsyncs, dump hit counts.
--
-- First-hit detail dump (LEGAIA_S4_DETAIL=1): in addition to the CSV,
-- the *first* hit on each probe writes a full call-context block to
-- the .detail.txt sidecar (32 GPRs, code window around PC, 32 stack
-- words at sp). The MIPS calling convention saves ra into a sp-relative
-- prologue slot for any non-leaf function, so the stack window
-- captures the visible ra-chain without DWARF unwind info. Setting
-- LEGAIA_S4_QUIT_AFTER_ALL=1 quits early once every probe has logged
-- at least one hit (so a manual scene-transition drive doesn't have to
-- run the full LEGAIA_FRAMES window).
--
-- Run:
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_readers.lua \
--       LEGAIA_OUT=slot4_readers.csv LEGAIA_FRAMES=300 \
--       ./scripts/pcsx-redux/run_world_map_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 300)
local OUT_PATH    = probe.getenv("LEGAIA_OUT", "slot4_readers.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local SLOT4_BASE  = probe.getenv_num("LEGAIA_SLOT4_BASE", 0x8011A624)
local DETAIL      = probe.getenv_num("LEGAIA_S4_DETAIL", 0) ~= 0
local QUIT_ALL    = probe.getenv_num("LEGAIA_S4_QUIT_AFTER_ALL", 0) ~= 0
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")

-- Offsets relative to SLOT4_BASE. Each probe arms one Read breakpoint.
local PROBE_OFFSETS = {
    0x00000,   -- outer count word
    0x00004,   -- body 0 word_offset
    0x00040,   -- body 0 records start
    0x00118,   -- body 0 record 14 (mid-body)
    0x00188,   -- body 1 records start
    0x00420,   -- body 4 records start (kind=4)
    0x00800,   -- body 4 mid
    0x010C8,   -- body 9 region
    0x018A4,   -- body 12 records start (densest body, ~1200 records)
    0x02000,   -- body 12 mid
    0x02800,   -- body 12 later
    0x037CC,   -- body 13 records start (kind=4)
    0x05400,   -- body 14 region
    0x07000,   -- near end
}
local MAX_HITS_PER_PROBE = 200

local csv = probe.csv_open(OUT_PATH, "probe_idx,addr,pc,width,value,ra")
if DETAIL then
    -- Truncate the detail sidecar at start; subsequent first-hits append.
    local fh = io.open(DETAIL_PATH, "w")
    if fh then
        fh:write(string.format(
            "# slot-4 reader detail; base=0x%08X; sstate=%s\n\n",
            SLOT4_BASE, SSTATE_PATH))
        fh:close()
    end
end

local descs_ref = { list = nil }

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
            local idx          = i
            local watched_addr = SLOT4_BASE + off
            local d            = {
                addr     = watched_addr,
                name     = string.format("slot4+0x%05X", off),
                hits_ref = { n = 0, capped = false },
            }
            probe.arm_breakpoint(watched_addr, "Read", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n > MAX_HITS_PER_PROBE then
                    if not d.hits_ref.capped then
                        PCSX.log(string.format(
                            "[s4r] probe %d cap reached at %d hits; further hits silently counted",
                            idx - 1, MAX_HITS_PER_PROBE))
                        d.hits_ref.capped = true
                    end
                    return
                end
                local r  = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                local val = probe.read_u32(watched_addr) or 0
                csv:row("%d,0x%08X,0x%08X,4,0x%08X,0x%08X",
                    idx - 1, watched_addr, pc, val, ra)
                if d.hits_ref.n <= 3 then
                    PCSX.log(string.format(
                        "[s4r] probe %d (0x%08X) hit %d: pc=0x%08X val=0x%08X ra=0x%08X",
                        idx - 1, watched_addr, d.hits_ref.n, pc, val, ra))
                end
                if DETAIL and d.hits_ref.n == 1 then
                    local label = string.format(
                        "probe %d (slot4+0x%05X = 0x%08X) val=0x%08X",
                        idx - 1, watched_addr - SLOT4_BASE,
                        watched_addr, val)
                    local snap = probe.capture_call_context(label)
                    probe.append_call_context(DETAIL_PATH, snap)
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format(
            "[s4r] %d Read probes armed across slot 4 (base=0x%08X)",
            #descs, SLOT4_BASE))
        descs_ref.list = descs
        return descs
    end,

    on_capture = function(ctx, _elapsed)
        if QUIT_ALL and descs_ref.list then
            local all = true
            for _, d in ipairs(descs_ref.list) do
                if d.hits_ref.n == 0 then all = false; break end
            end
            if all then
                PCSX.log("[s4r] all probes hit; quitting early")
                ctx.request_quit = true
            end
        end
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("[s4r] CSV closed: " .. OUT_PATH)
        if DETAIL then
            PCSX.log("[s4r] detail sidecar: " .. DETAIL_PATH)
        end
        -- The default summary printer in probe.run already dumps hit counts.
        -- We add a one-liner with capped status the default doesn't show.
        for _, d in ipairs(descs) do
            if d.hits_ref.capped then
                PCSX.log(string.format(
                    "[s4r]   %s: %d hits (capped)", d.name, d.hits_ref.n))
            end
        end
    end,
})
