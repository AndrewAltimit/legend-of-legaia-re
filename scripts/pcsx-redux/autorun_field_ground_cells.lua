-- autorun_field_ground_cells.lua
--
-- Per-cell ground-draw census for a field scene: does retail's ground pass
-- emit a quad for a cell that carries object-grid bit 0x0800 (kind-2
-- tile-trigger / elevation override) but not 0x1000?
--
-- The ground pass is `FUN_801F6D48` in PROT 0900 (the slot-B field render
-- library, base 0x801F69D8). It walks the camera's visible-tile window
-- (scratchpad 0x1F8003E8..EB), reads the cell word at
-- `*(0x1F8003EC) + 0x8000 + (z << 8) + (x << 1)` and skips the cell when
-- `(cell & 0x1000) == 0`. This probe measures both halves live:
--
--   * the live object grid - every cell's bit set, so the 0x0800-only
--     population is counted rather than inferred from the .MAP parse; and
--   * the emitted-cell set - an Exec breakpoint on the gate (0x801F6E10)
--     logs (x, z, cell) per visited cell, and one on the packet commit
--     (0x801F7020) logs the cells that actually produced a POLY_FT4.
--
-- Outputs (LEGAIA_OUT_DIR):
--   grid_census.csv   one row per non-zero object-grid cell
--   emit_census.csv   one row per visited cell on the sampled frame
--   summary.txt       counts + the visible-tile window
--   ram_full.bin      the frame's main RAM (feed to `mednafen-state
--                     display-list`), when LEGAIA_DUMP_RAM=1
--   field.sstate      a save state at the captured frame, when
--                     LEGAIA_SAVE_STATE is a path
--
-- Env vars:
--   LEGAIA_SSTATE       save state to start from
--   LEGAIA_FRAMES       vsyncs to run before giving up (default 900)
--   LEGAIA_SCENE        scene name to wait for (default: any)
--   LEGAIA_WALK_BTN     pad button index held while waiting (default none)
--   LEGAIA_WALK_FROM    vsync at which to start holding it (default 120)
--   LEGAIA_DUMP_RAM     1 = also dump 2 MiB main RAM at the captured frame
--   LEGAIA_SAVE_STATE   path for a save state at the captured frame

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local WANT     = probe.getenv("LEGAIA_SCENE", "")
local WALK_BTN = probe.getenv_num("LEGAIA_WALK_BTN", -1)
local WALK_FROM = probe.getenv_num("LEGAIA_WALK_FROM", 120)
local SETTLE   = probe.getenv_num("LEGAIA_SETTLE", 0)
local DUMP_RAM = probe.getenv_num("LEGAIA_DUMP_RAM", 0)
local SAVE_ST  = probe.getenv("LEGAIA_SAVE_STATE", "")

-- game anchors (legaia_mednafen::game_anchors)
local SCENE_VA = 0x8007050C
local MODE_VA  = 0x8007B83C
-- scratchpad
local GRID_PTR = 0x1F8003EC   -- field-env block base
local WIN_X0   = 0x1F8003E8   -- camera visible-tile window: x0,z0,x1,z1 bytes
-- PROT 0900 ground pass. There are TWO ground emitters with the same body,
-- picked at 0x801F79A0 on `_DAT_8007BB4C`: nonzero -> FUN_801F69EC,
-- zero -> FUN_801F6D48. Both are watched.
local PASSES = {
    { name = "801F69EC", gate = 0x801F6AB4, emit = 0x801F6CE0 },
    { name = "801F6D48", gate = 0x801F6E10, emit = 0x801F7020 },
}
local PASS_SEL = 0x8007BB4C   -- the selector the caller branches on
local CALLER_PC = 0x801F79A0  -- the branch on that selector, one hit per pass

local visited, emitted = {}, {}
local nvisit, nemit = 0, 0
local sampling = false
local captured = false
local walking = false
local field_run = 0
local ncall = 0

local function scene_name()
    local b = probe.read_bytes(SCENE_VA, 12)
    if b == nil then return "?" end
    local s = tostring(b)
    local z = s:find("\0")
    if z then s = s:sub(1, z - 1) end
    return s
end

local function mode() return probe.read_u8(MODE_VA) or 0 end

-- Both breakpoints sit inside a ground pass's inner loop, so they cost a Lua
-- call per visited cell per pass for the whole run; the `sampling` flag plus
-- the caller bracket below is what narrows the recorded census to one pass.
local function arm_emitter_bps()
    -- The vsync event fires several times per rendered frame under the
    -- interpreter, so "one vsync" is NOT "one pass": gating the census on a
    -- single vsync gap captures zero cells most of the time. The caller
    -- breakpoint is what brackets exactly one pass.
    probe.bp.arm(CALLER_PC, "Exec", 4, "ground_caller", function()
        if sampling then ncall = ncall + 1 end
    end)
    for _, p in ipairs(PASSES) do
        local tag = p.name
        probe.bp.arm(p.gate, "Exec", 4, "gate_" .. tag, function()
            if not sampling or ncall ~= 1 then return end
            local r = PCSX.getRegisters().GPR.n
            local x = bit.band(tonumber(r.a0), 0xFFFF)
            local z = bit.band(tonumber(r.a1), 0xFFFF)
            local c = bit.band(tonumber(r.s5), 0xFFFF)
            nvisit = nvisit + 1
            visited[#visited + 1] = { x, z, c, tag }
        end)
        probe.bp.arm(p.emit, "Exec", 4, "emit_" .. tag, function()
            if not sampling or ncall ~= 1 then return end
            local r = PCSX.getRegisters().GPR.n
            local x = bit.band(tonumber(r.a0), 0xFFFF)
            local z = bit.band(tonumber(r.a1), 0xFFFF)
            nemit = nemit + 1
            emitted[x .. "," .. z] = tag
        end)
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        arm_emitter_bps()
        return {
            { addr = PASSES[1].gate, name = "gate_" .. PASSES[1].name },
            { addr = PASSES[2].gate, name = "gate_" .. PASSES[2].name },
        }
    end,

    on_capture = function(ctx, elapsed)
        if captured then return end

        if WALK_BTN >= 0 and not walking and elapsed >= WALK_FROM then
            probe.pad_force(WALK_BTN)
            walking = true
            PCSX.log(string.format("[ground] holding pad %d at vsync %d",
                WALK_BTN, elapsed))
        end

        if elapsed % 30 == 0 then
            PCSX.log(string.format("[ground] vsync=%d mode=0x%02X scene=%s ptr=0x%08X",
                elapsed, mode(), scene_name(),
                probe.read_scratch_u32(GRID_PTR) or 0))
        end

        if sampling then
            -- Wait for the caller to enter a second time: everything between
            -- its first and second entry is exactly one complete pass.
            if ncall < 2 then return end
            sampling = false
            captured = true
            if walking then probe.pad_release(WALK_BTN); walking = false end
            ctx.request_quit = true
            return
        end

        local m = mode()
        if m ~= 0x03 then field_run = 0; return end
        if WANT ~= "" and scene_name() ~= WANT then return end
        local base = probe.read_scratch_u32(GRID_PTR) or 0
        if base == 0 or not probe.in_ram(base, 0x10000) then return end
        if elapsed < WALK_FROM then return end
        -- Settle: require N consecutive field-run frames before sampling, so
        -- the saved state is solidly mid-field rather than one frame past the
        -- init->run flip.
        field_run = field_run + 1
        if field_run <= SETTLE then return end

        -- Grid census, once, on the frame before the sampled frame.
        ctx.grid_base = base
        ctx.win = {
            probe.read_scratch_u32(WIN_X0) or 0,
        }
        ctx.scene = scene_name()
        local cells = {}
        local blob = probe.read_bytes(base + 0x8000, 0x8000)
        if blob ~= nil then
            local s = tostring(blob)
            for i = 0, 0x3FFF do
                local lo = s:byte(i * 2 + 1)
                local hi = s:byte(i * 2 + 2)
                local c = lo + hi * 256
                if c ~= 0 then
                    cells[#cells + 1] = { i % 128, math.floor(i / 128), c }
                end
            end
        end
        ctx.cells = cells
        sampling = true
        PCSX.log(string.format(
            "[ground] sampling frame: scene=%s base=0x%08X nonzero_cells=%d",
            ctx.scene, base, #cells))
    end,

    on_done = function(ctx)
        local cells = ctx.cells or {}
        local n1000, n2000, n800, n800_only, n800_1000 = 0, 0, 0, 0, 0
        local fh = io.open(probe.out_path("grid_census.csv"), "w")
        if fh then fh:write("tile_x,tile_z,cell\n") end
        for _, c in ipairs(cells) do
            local w = c[3]
            if bit.band(w, 0x1000) ~= 0 then n1000 = n1000 + 1 end
            if bit.band(w, 0x2000) ~= 0 then n2000 = n2000 + 1 end
            if bit.band(w, 0x0800) ~= 0 then
                n800 = n800 + 1
                if bit.band(w, 0x1000) ~= 0 then
                    n800_1000 = n800_1000 + 1
                else
                    n800_only = n800_only + 1
                end
            end
            if fh then fh:write(string.format("%d,%d,0x%04X\n", c[1], c[2], w)) end
        end
        if fh then fh:close() end

        local eh = io.open(probe.out_path("emit_census.csv"), "w")
        if eh then
            eh:write("tile_x,tile_z,cell,emitted,pass\n")
            for _, v in ipairs(visited) do
                eh:write(string.format("%d,%d,0x%04X,%d,%s\n", v[1], v[2], v[3],
                    emitted[v[1] .. "," .. v[2]] and 1 or 0, v[4]))
            end
            eh:close()
        end

        local win = (ctx.win and ctx.win[1]) or probe.read_scratch_u32(WIN_X0) or 0
        local sh = io.open(probe.out_path("summary.txt"), "w")
        if sh then
            sh:write(string.format("scene=%s\n", tostring(ctx.scene)))
            sh:write(string.format("grid_base=0x%08X\n", ctx.grid_base or 0))
            sh:write(string.format("pass_selector(0x8007BB4C)=0x%08X\n",
                probe.read_u32(PASS_SEL) or 0))
            sh:write(string.format("visible_tile_window(0x1F8003E8)=0x%08X"
                .. " x0=%d z0=%d x1=%d z1=%d\n", win,
                bit.band(win, 0xFF), bit.band(bit.rshift(win, 8), 0xFF),
                bit.band(bit.rshift(win, 16), 0xFF),
                bit.band(bit.rshift(win, 24), 0xFF)))
            sh:write(string.format("nonzero_cells=%d\n", #cells))
            sh:write(string.format("cells_with_0x1000=%d\n", n1000))
            sh:write(string.format("cells_with_0x2000=%d\n", n2000))
            sh:write(string.format("cells_with_0x0800=%d\n", n800))
            sh:write(string.format("cells_0x0800_and_0x1000=%d\n", n800_1000))
            sh:write(string.format("cells_0x0800_without_0x1000=%d\n", n800_only))
            sh:write(string.format("gate_visits_one_frame=%d\n", nvisit))
            sh:write(string.format("packets_emitted_one_frame=%d\n", nemit))
            sh:close()
        end
        PCSX.log(string.format(
            "[ground] cells=%d 0x1000=%d 0x2000=%d 0x800=%d 0x800-only=%d"
            .. " visits=%d emits=%d",
            #cells, n1000, n2000, n800, n800_only, nvisit, nemit))

        if SAVE_ST ~= "" then
            if probe.sstate.save(SAVE_ST) then
                PCSX.log("[ground] saved state to " .. SAVE_ST)
            end
        end
        if DUMP_RAM == 1 then
            local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
            if buf ~= nil then
                local rh = io.open(probe.out_path("ram_full.bin"), "wb")
                if rh then rh:write(tostring(buf)); rh:close() end
                PCSX.log("[ground] dumped main RAM")
            end
        end
    end,
})
