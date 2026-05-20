-- autorun_town01_script_flow.lua
--
-- Capture how a field scene (default: town01 / Rim Elm) runs its
-- field-VM scripts at runtime, to pin the scene-script EXECUTION MODEL
-- that authors collision walls. The committed RE has shown the
-- per-scene collision grid (_DAT_1f8003ec + 0x4000) has exactly ONE
-- writer: the field-VM 0x4C nibble-7 op in FUN_801de840. So the only
-- path to walls is running the scene's scripts correctly. This probe
-- observes that running from a live save.
--
-- It arms five execution breakpoints:
--   FUN_8003aeb0 (0x8003aeb0)  scene-entry map-init (SCUS, fires on a
--                              scene transition / re-entry)
--   FUN_8003ab2c (0x8003ab2c)  scene-entry system-script prologue
--                              runner (SCUS; builds the channel-0xFB
--                              script from the MAN and runs its head)
--   FUN_801de840 (0x801de840)  per-frame field-VM single-op dispatch
--                              (overlay 0897). a0=bytecode ptr, a1=pc,
--                              a2=ctx. Deduped into a per-context table
--                              keyed by ctx pointer: script_id (ctx+0x50),
--                              bytecode ptr (ctx+0x90), pc range, hits.
--                              This is the live multi-context set.
--   nibble-7 grid writes       0x801e1d00 / 0x801e1d74 / 0x801e1e84
--                              (the sb ...,0x4000(reg) stores). Each hit
--                              is a wall-paint; full call context logged.
--
-- It also dumps the live collision grid (field_buf+0x4000, 0x4000 bytes)
-- at the first and last capture frame, with a wall-tile count + an ASCII
-- map, so we can confirm town01 actually has painted walls (unlike the
-- world-map-class map03, whose grid is legitimately near-empty).
--
-- The field buffer pointer lives in scratchpad at 0x1F8003EC.
--
-- Steady-state run (no transition; observes the live context set + grid):
--   timeout --kill-after=30s 600s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --scenario v0_1_pre_battle_tetsu \
--     --lua scripts/pcsx-redux/autorun_town01_script_flow.lua \
--     --frames 300
--
-- To capture the LOAD-time paint flow (FUN_8003aeb0 / nibble-7 firing),
-- drive a scene transition: pass LEGAIA_HOLD_BUTTON to walk toward an
-- exit, or start from a pre-transition save. See the doc note.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 300)
local OUT_PATH    = probe.out_path("town01_script_flow.csv")
local HITS_PATH   = OUT_PATH:gsub("%.csv$", ".hits.txt")
local CTX_PATH    = OUT_PATH:gsub("%.csv$", ".contexts.txt")
local GRID_PATH   = OUT_PATH:gsub("%.csv$", ".grid.txt")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local VM_CAP      = probe.getenv_num("LEGAIA_VM_CAP", 200000)

local FIELD_BUF_PTR_SCRATCH = 0x1F8003EC
local GRID_OFF              = 0x4000
local GRID_LEN              = 0x4000  -- 0x80 * 0x80 tiles
local GRID_STRIDE          = 0x80

-- Walk the field buffer pointer -> collision grid; return (base, bytes).
local function read_grid()
    local base = probe.mem.read_scratch_u32(FIELD_BUF_PTR_SCRATCH)
    if not base or base < 0x80000000 then return nil, nil end
    local bytes = probe.read_bytes(base + GRID_OFF, GRID_LEN)
    return base, bytes
end

-- Count wall tiles (high nibble != 0) and render a coarse ASCII map.
local function dump_grid(label, frame)
    local base, bytes = read_grid()
    local f = io.open(GRID_PATH, "a")
    if not f then return end
    f:write(string.format("== %s (frame %d) field_buf=0x%08X ==\n",
        label, frame, base or 0))
    if not bytes then
        f:write("  <grid unreadable>\n\n")
        f:close()
        return
    end
    local s = tostring(bytes)
    local walls = 0
    for i = 1, #s do
        if bit.band(s:byte(i), 0xF0) ~= 0 then walls = walls + 1 end
    end
    f:write(string.format("  wall tiles (high-nibble != 0): %d / %d\n",
        walls, GRID_LEN))
    -- Coarse 64x64 ASCII map (sample every 2nd tile on each axis).
    for row = 0, GRID_STRIDE - 1, 2 do
        local line = {}
        for col = 0, GRID_STRIDE - 1, 2 do
            local b = s:byte(row * GRID_STRIDE + col + 1) or 0
            line[#line + 1] = (bit.band(b, 0xF0) ~= 0) and "#" or "."
        end
        f:write("  " .. table.concat(line) .. "\n")
    end
    f:write("\n")
    f:close()
    PCSX.log(string.format("[town01] %s grid: %d wall tiles", label, walls))
end

local csv = probe.csv_open(OUT_PATH,
    "frame,site,ctx,script_id,pc,bytecode,grid_addr,grid_val")

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits_summary.txt"),
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function(ctx)
        ctx.frame = 0
        ctx.contexts = {}     -- ctx_ptr -> { script_id, bytecode, pc_min, pc_max, hits }
        ctx.vm_hits = { n = 0 }
        local descs = {}

        local function desc(addr, name)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            descs[#descs + 1] = d
            return d
        end

        -- scene-entry init + prologue runner: full call context on each
        -- hit (rare; only on a transition), so we see which MAN drives
        -- the scene and the args (a0/a1/a2 = partition counts).
        for _, t in ipairs({
            { 0x8003aeb0, "FUN_8003aeb0_scene_init" },
            { 0x8003ab2c, "FUN_8003ab2c_prologue_runner" },
        }) do
            local d = desc(t[1], t[2])
            probe.arm_breakpoint(t[1], "Exec", 4, t[2], function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n <= 4 then
                    probe.append_call_context(HITS_PATH,
                        probe.snapshot.capture_call_context(string.format(
                            "%s frame=%d hit=%d", t[2], ctx.frame, d.hits_ref.n)))
                end
            end)
        end

        -- per-frame VM dispatch: dedup into the per-context table.
        local dvm = desc(0x801de840, "FUN_801de840_vm_step")
        probe.arm_breakpoint(0x801de840, "Exec", 4, "vm_step", function()
            dvm.hits_ref.n = dvm.hits_ref.n + 1
            ctx.vm_hits.n = ctx.vm_hits.n + 1
            if ctx.vm_hits.n > VM_CAP then return end
            local r  = PCSX.getRegisters()
            local a2 = tonumber(r.GPR.n.a2) or 0  -- ctx ptr
            local a0 = tonumber(r.GPR.n.a0) or 0  -- bytecode ptr
            local a1 = tonumber(r.GPR.n.a1) or 0  -- pc
            local rec = ctx.contexts[a2]
            if not rec then
                local sid = probe.read_u16(a2 + 0x50) or 0
                rec = { script_id = sid, bytecode = a0,
                        pc_min = a1, pc_max = a1, hits = 0 }
                ctx.contexts[a2] = rec
            end
            rec.hits = rec.hits + 1
            if a1 < rec.pc_min then rec.pc_min = a1 end
            if a1 > rec.pc_max then rec.pc_max = a1 end
        end)

        -- nibble-7 grid writes: each is a wall paint. Log a CSV row +
        -- (first few) full call contexts.
        for _, t in ipairs({
            { 0x801e1d00, "paint_sb_1d00" },
            { 0x801e1d74, "paint_sb_1d74" },
            { 0x801e1e84, "paint_sb_1e84" },
        }) do
            local d = desc(t[1], t[2])
            probe.arm_breakpoint(t[1], "Exec", 4, t[2], function()
                d.hits_ref.n = d.hits_ref.n + 1
                local r  = PCSX.getRegisters()
                -- base reg differs per site: 1d00/1d74 use v1, 1e84 uses a0.
                local v1 = tonumber(r.GPR.n.v1) or 0
                local a0 = tonumber(r.GPR.n.a0) or 0
                local v0 = tonumber(r.GPR.n.v0) or 0
                local base = (t[1] == 0x801e1e84) and a0 or v1
                local grid_addr = base + 0x4000
                local val = probe.read_u8(grid_addr) or 0
                csv:row("%d,%s,,,,,0x%08X,0x%02X",
                    ctx.frame, t[2], grid_addr, val)
                if d.hits_ref.n <= 6 then
                    probe.append_call_context(HITS_PATH,
                        probe.snapshot.capture_call_context(string.format(
                            "%s frame=%d hit=%d grid_addr=0x%08X",
                            t[2], ctx.frame, d.hits_ref.n, grid_addr)))
                end
            end)
        end

        PCSX.log("[town01] armed scene-init + prologue + vm-step + 3 paint BPs")
        return descs
    end,

    on_capture = function(ctx, elapsed)
        ctx.frame = elapsed
        if elapsed == 1 then dump_grid("initial", elapsed) end
    end,

    on_done = function(ctx, descs)
        csv:close()
        dump_grid("final", ctx.frame)

        -- Write the per-context table: the live multi-context set.
        local f = io.open(CTX_PATH, "w")
        if f then
            f:write("# town01 field-VM live contexts (via FUN_801de840 a2)\n")
            f:write("# ctx_ptr  script_id  bytecode_ptr  pc_min  pc_max  hits\n")
            -- stable order by ctx ptr
            local keys = {}
            for k in pairs(ctx.contexts) do keys[#keys + 1] = k end
            table.sort(keys)
            for _, k in ipairs(keys) do
                local r = ctx.contexts[k]
                f:write(string.format(
                    "0x%08X  0x%04X  0x%08X  0x%04X  0x%04X  %d\n",
                    k, r.script_id, r.bytecode, r.pc_min, r.pc_max, r.hits))
            end
            f:close()
            PCSX.log(string.format("[town01] %d distinct contexts -> %s",
                #keys, CTX_PATH))
        end

        PCSX.log("=== town01 script-flow summary ===")
        for _, d in ipairs(descs or {}) do
            PCSX.log(string.format("  0x%08X  %10d  %s",
                d.addr, d.hits_ref.n, d.name))
        end
        PCSX.log(string.format("  vm-step total hits: %d (cap %d)",
            ctx.vm_hits.n, VM_CAP))
        PCSX.log("=== end ===")
    end,
})
