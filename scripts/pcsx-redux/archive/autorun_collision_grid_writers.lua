-- autorun_collision_grid_writers.lua
--
-- Pin EVERY writer of the per-scene collision grid (_DAT_1f8003ec +
-- 0x4000) during a scene transition, via a memory Write watchpoint on
-- the grid region. Catches block-loaders + nibble-7 paints alike,
-- regardless of which overlay they live in (an instruction-address BP
-- only catches one overlay's copy).
--
-- Motivation: a Drake-Castle->world-map transition repainted the grid
-- from 2093 to 6805 wall tiles while only 6 nibble-7 tile-writes fired
-- (autorun_town01_script_flow.lua). So the BASE collision is written by
-- something other than the 0x4C nibble-7 op - most likely the un-dumped
-- scene-boot allocator that sets _DAT_1f8003ec. This probe finds its PC.
--
-- The grid pointer lives in scratchpad at 0x1F8003EC; the watchpoint is
-- armed lazily after the save loads (the target is a runtime deref).
--
-- Run (drive Up to trigger the transition):
--   LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=30 \
--   timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario drake_castle_to_worldmap \
--     --lua scripts/pcsx-redux/autorun_collision_grid_writers.lua \
--     --frames 260

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local OUT_PATH    = probe.out_path("collision_grid_writers.csv")
local HITS_PATH   = OUT_PATH:gsub("%.csv$", ".hits.txt")
local PCS_PATH    = OUT_PATH:gsub("%.csv$", ".writer_pcs.txt")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 260)
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
-- Watch a window inside the grid. A grid is 0x4000 bytes; watching a
-- 0x400-byte slice catches any full-grid block write while keeping the
-- per-store callback rate manageable.
local WATCH_OFF   = probe.getenv_num("LEGAIA_WATCH_OFF", 0x4000)
local WATCH_WIDTH = probe.getenv_num("LEGAIA_WATCH_WIDTH", 0x400)
local CAP         = probe.getenv_num("LEGAIA_GRID_CAP", 100000)

local FIELD_BUF_PTR_SCRATCH = 0x1F8003EC

local csv = probe.csv_open(OUT_PATH, "frame,pc,ra,addr,val")

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = FRAMES,
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function(ctx)
        ctx.frame = 0
        ctx.armed = false
        ctx.hits = { n = 0 }
        ctx.pcs = {}        -- pc -> { ra, count, first_frame }
        return {}
    end,

    on_capture = function(ctx, elapsed)
        ctx.frame = elapsed
        if ctx.armed then return end
        -- Lazy-arm once the field buffer pointer is populated.
        local base = probe.mem.read_scratch_u32(FIELD_BUF_PTR_SCRATCH)
        if not base or base < 0x80000000 then return end
        local watch = base + WATCH_OFF
        probe.arm_breakpoint(watch, "Write", WATCH_WIDTH, "grid_write", function()
            ctx.hits.n = ctx.hits.n + 1
            if ctx.hits.n > CAP then return end
            local r  = PCSX.getRegisters()
            local pc = tonumber(r.pc) or 0
            local ra = tonumber(r.GPR.n.ra) or 0
            local rec = ctx.pcs[pc]
            if not rec then
                rec = { ra = ra, count = 0, first_frame = ctx.frame }
                ctx.pcs[pc] = rec
                -- Full call context the first time we see a new writer PC.
                probe.append_call_context(HITS_PATH,
                    probe.snapshot.capture_call_context(string.format(
                        "grid_writer pc=0x%08X frame=%d", pc, ctx.frame)))
            end
            rec.count = rec.count + 1
            if ctx.hits.n <= 4000 then
                csv:row("%d,0x%08X,0x%08X,0x%08X,", ctx.frame, pc, ra, watch)
            end
        end)
        ctx.armed = true
        PCSX.log(string.format(
            "[grid-writers] armed Write bp at 0x%08X width 0x%X (field_buf=0x%08X)",
            watch, WATCH_WIDTH, base))
    end,

    on_done = function(ctx)
        csv:close()
        local f = io.open(PCS_PATH, "w")
        if f then
            f:write("# distinct collision-grid (+0x4000) writer PCs\n")
            f:write("# pc  ra  count  first_frame  classification\n")
            local keys = {}
            for k in pairs(ctx.pcs) do keys[#keys + 1] = k end
            table.sort(keys)
            for _, pc in ipairs(keys) do
                local r = ctx.pcs[pc]
                -- Classify against the known field-overlay nibble-7 sites.
                local cls = "OTHER (candidate block-loader)"
                if pc >= 0x801E1C00 and pc <= 0x801E1F00 then
                    cls = "nibble-7 paint (0x4C op)"
                end
                f:write(string.format("0x%08X  0x%08X  %8d  %5d  %s\n",
                    pc, r.ra, r.count, r.first_frame, cls))
            end
            f:close()
        end
        PCSX.log(string.format(
            "[grid-writers] %d total writes, %d distinct PCs -> %s",
            ctx.hits.n, (function() local n=0 for _ in pairs(ctx.pcs) do n=n+1 end return n end)(),
            PCS_PATH))
        for pc, r in pairs(ctx.pcs) do
            PCSX.log(string.format("  writer 0x%08X  ra=0x%08X  count=%d", pc, r.ra, r.count))
        end
    end,
})
