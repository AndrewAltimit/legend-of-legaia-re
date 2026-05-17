-- autorun_xp_table_reader.lua
--
-- Pin the runtime XP-table reader by Read-watchpoint instead of static
-- LUI+ADDIU scanning.
--
-- The retail XP increment table sits at 0x8007123C (98 u16 LE per-level
-- increments; total cum-sum ~34663). Static scans (find_xp_table_readers.py
-- + find_xp_table_all_overlays.py) return zero LUI+ADDIU pairs that
-- synthesise the address across SCUS + every captured overlay - the
-- reader either lives in an overlay we haven't captured, or it builds the
-- pointer indirectly (gp-relative load, runtime addition). Either way an
-- instruction watchpoint catches it cleanly.
--
-- Run with a save state captured at end-of-battle, just before the
-- level-up cutscene fires. The reader will hit on the per-character
-- "did anyone level up?" check.
--
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_xp_table_reader.lua \
--       LEGAIA_SSTATE=/path/to/end_of_battle.sstate1 \
--       LEGAIA_OUT=xp_table_readers.csv \
--       LEGAIA_FRAMES=600 \
--       ./scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH = probe.out_path("xp_table_readers.csv")
-- The XP table is 98 u16 entries; we cover the whole span by arming
-- four overlapping 4-byte Read bps. PCSX-Redux exposes per-byte width
-- breakpoints; one bp per word gives us the full range without
-- exhausting the breakpoint slot pool.
local XP_TABLE_BASE = probe.getenv_num("LEGAIA_XP_BASE", 0x8007123C)
local XP_TABLE_LEN  = probe.getenv_num("LEGAIA_XP_LEN", 98 * 2)
local DETAIL_PATH   = OUT_PATH:gsub("%.csv$", ".detail.txt")

local csv = probe.csv_open(OUT_PATH,
    "tick,addr,offset,pc,width,value_u16,ra")
local first_hit = { fired = false }

-- Truncate the detail sidecar; first 8 hits will append full call-context.
do
    local fh = io.open(DETAIL_PATH, "w")
    if fh then
        fh:write(string.format(
            "# xp-table reader detail; base=0x%08X len=%d; sstate=%s\n\n",
            XP_TABLE_BASE, XP_TABLE_LEN, SSTATE_PATH))
        fh:close()
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local descs = {}
        -- Walk the table in u32 strides; one Read bp per word.
        local bytes_remaining = XP_TABLE_LEN
        local cursor          = 0
        local probe_idx       = 0
        while bytes_remaining > 0 do
            local width = math.min(4, bytes_remaining)
            local addr  = XP_TABLE_BASE + cursor
            local off   = cursor
            local d     = {
                addr     = addr,
                name     = string.format("xp+0x%03X", off),
                hits_ref = { n = 0 },
            }
            local idx = probe_idx
            probe.arm_breakpoint(addr, "Read", width, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local r  = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                local val = probe.read_u16(addr) or 0
                csv:row("%d,0x%08X,0x%03X,0x%08X,%d,0x%04X,0x%08X",
                    d.hits_ref.n, addr, off, pc, width, val, ra)
                local total_hits = (first_hit.count or 0) + 1
                first_hit.count = total_hits
                if total_hits <= 8 then
                    PCSX.log(string.format(
                        "[xp] hit %d: addr=0x%08X off=0x%03X pc=0x%08X val=0x%04X ra=0x%08X",
                        total_hits, addr, off, pc, val, ra))
                    local label = string.format(
                        "hit %d: %s = 0x%04X", total_hits, d.name, val)
                    probe.append_call_context(DETAIL_PATH,
                        probe.capture_call_context(label))
                end
            end)
            descs[#descs + 1] = d
            cursor          = cursor + width
            bytes_remaining = bytes_remaining - width
            probe_idx       = probe_idx + 1
        end
        PCSX.log(string.format(
            "[xp] %d Read probes armed across XP table 0x%08X..0x%08X",
            #descs, XP_TABLE_BASE, XP_TABLE_BASE + XP_TABLE_LEN))
        return descs
    end,

    on_done = function(_, _descs)
        csv:close()
        PCSX.log("[xp] CSV closed: " .. OUT_PATH)
        PCSX.log("[xp] detail sidecar: " .. DETAIL_PATH)
    end,
})
