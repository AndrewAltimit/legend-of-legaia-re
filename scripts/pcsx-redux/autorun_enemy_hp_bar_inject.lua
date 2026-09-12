-- Enemy HP bar (legaia-patcher --enemy-hp-bar) runtime check by RAM injection.
--
-- Loads a mid-battle save state, writes the planned same-size edits straight
-- into main RAM (the four fragments over their unreferenced host bodies + the
-- two-word detour at the damage-popup renderer), then lets the battle run and
-- captures screenshots + a CSV of every gauge / icon emission the routine
-- makes, so the plate geometry can be measured off the frame and off the
-- arguments at once.
--
-- Why RAM injection and not a patched disc: a save state replays the RAM of
-- the disc that booted it, so the resident SCUS + overlay 0898 would mask the
-- disc edits anyway (CLAUDE.md "A save state replays the RAM of the disc that
-- booted it"). Writing the same bytes into RAM is the direct test of the
-- routine; the disc carrier is covered by the disc-gated Rust oracle.
--
-- Inputs (env):
--   LEGAIA_SSTATE / --scenario   a battle state with PROT 0898 resident
--   LEGAIA_HPBAR_EDITS           "RAMEDIT <va> <hex>" lines, from
--                                `cargo test -p legaia-patcher --lib
--                                 enemy_hp_bar::tests::dump_ram_edits
--                                 -- --ignored --nocapture`
--   LEGAIA_FRAMES                capture vsyncs (default 240)
--   LEGAIA_SHOTS                 comma list of vsyncs to screenshot on
--                                (default "40,120,220")
--   LEGAIA_NO_INJECT=1           control run: same state, no edits
--   LEGAIA_HPBAR_ENCOUNTER=1     disc-carrier mode: run on the PATCHED disc,
--                                resume a field state parked one step from a
--                                random encounter (karisto_sol_pre_encounter),
--                                apply LEGAIA_POKES (`legaia-patcher scus-pokes`
--                                output) so the resident SCUS matches the disc,
--                                hold RIGHT into the encounter so PROT 0898 is
--                                read from the disc under test, then settle and
--                                screenshot. Proves the overlay-side edits arrive
--                                from the disc, which a state's stale RAM cannot.
--   LEGAIA_POKES                 the poke list for encounter mode
--   LEGAIA_WALK_FROM             vsync to start holding RIGHT (default 8)
--   LEGAIA_BATTLE_SETTLE         vsyncs after the fight phase opens before the
--                                screenshot (default 300)
--
-- Outputs (probe.out_path): hpbar_<n>.raw(+.meta) screenshots, hpbar_calls.csv
-- (vsync, fn, a0, a1, a2), hpbar_summary.txt.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local EDITS_PATH = probe.getenv("LEGAIA_HPBAR_EDITS", "/tmp/hpbar_ramedits.txt")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 240)
local SHOTS_SPEC = probe.getenv("LEGAIA_SHOTS", "40,120,220")
local NO_INJECT = probe.getenv("LEGAIA_NO_INJECT", "0") == "1"
local ENCOUNTER = probe.getenv("LEGAIA_HPBAR_ENCOUNTER", "0") == "1"
local POKES_PATH = probe.getenv("LEGAIA_POKES", "")
local WALK_FROM = probe.getenv_num("LEGAIA_WALK_FROM", 8)
local BATTLE_SETTLE = probe.getenv_num("LEGAIA_BATTLE_SETTLE", 300)
local GAME_MODE_VA = 0x8007B83C
local PHASE_VA = 0x8007BD71
local HOOK_VA = 0x801DF6B8
local J_FRAG_A = 0x0807CB55 -- `j 0x801F2D54`

local function read_pokes(path)
    local out = {}
    local f = io.open(path, "r")
    if not f then return nil end
    for line in f:lines() do
        local a, w = line:match("^%s*0x(%x+)%s*:%s*0x(%x+)")
        if a then out[#out + 1] = { addr = tonumber(a, 16), word = tonumber(w, 16) } end
    end
    f:close()
    return out
end

local GAUGE_FN = 0x8002C0B0
local ICON_FN = 0x8002C488
local FRAG_A_VA = 0x801F2D54
local PROJECT_FN = 0x800195A8

local shots = {}
for n in string.gmatch(SHOTS_SPEC, "[^,]+") do shots[tonumber(n)] = true end

local function read_edits()
    local list = {}
    local fh = io.open(EDITS_PATH, "r")
    if not fh then error("cannot open " .. EDITS_PATH) end
    for line in fh:lines() do
        local va, hex = line:match("RAMEDIT%s+(%x+)%s+(%x+)")
        if va then list[#list + 1] = { va = tonumber(va, 16), hex = hex } end
    end
    fh:close()
    return list
end

local function inject(list)
    local total = 0
    for _, e in ipairs(list) do
        for i = 1, #e.hex, 2 do
            local b = tonumber(e.hex:sub(i, i + 1), 16)
            probe.write_u8(e.va + (i - 1) / 2, b)
            total = total + 1
        end
    end
    return total
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

local csv = probe.csv_open(probe.out_path("hpbar_calls.csv"), "vsync,fn,a0,a1,a2,v0_or_depth")
local summary = io.open(probe.out_path("hpbar_summary.txt"), "w")
local function w(msg)
    PCSX.log("[hpbar] " .. msg)
    if summary then summary:write(msg, "\n"); summary:flush() end
end

local vsync_now = 0
local counts = { frag_a = 0, gauge = 0, icon = 0, project = 0 }
local battle_at = nil   -- vsync the game mode turned 0x15
local fight_at = nil    -- vsync the fighting phase opened
local shot_done = false

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = FRAMES,
    snapshot_path = probe.out_path("hpbar_snapshot.txt"),
    on_arm = function()
        local descs = {}
        local function arm(addr, name, key, log_v0)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                counts[key] = counts[key] + 1
                local r = PCSX.getRegisters()
                local g = r.GPR.n
                csv:row("%d,%s,%d,%d,%d,%d", vsync_now, name,
                    tonumber(g.a0), tonumber(g.a1), tonumber(g.a2), tonumber(g.v0))
            end)
            descs[#descs + 1] = d
        end
        -- Just past the projector's return in fragment B: v0 = depth, and the
        -- projected (x, y) sits at sp+0x28 of the routine's frame.
        local after_project = 0x801F463C + 22 * 4
        local d = { addr = after_project, name = "projected", hits_ref = { n = 0 } }
        probe.arm_breakpoint(after_project, "Exec", 4, "projected", function()
            d.hits_ref.n = d.hits_ref.n + 1
            local r = PCSX.getRegisters()
            local g = r.GPR.n
            local sp = tonumber(g.sp)
            -- This render pass runs with the stack in the scratchpad.
            local rd = probe.in_ram(sp, 4) and probe.read_u32 or probe.read_scratch_u32
            local ok, xy = pcall(rd, sp + 0x28)
            if not ok then xy = 0xFFFFFFFF end
            local ok2, svx = pcall(rd, sp + 0x20)
            local ok3, svz = pcall(rd, sp + 0x24)
            csv:row("%d,projected,%d,%d,%d,%d", vsync_now,
                bit.band(xy, 0xFFFF), bit.rshift(xy, 16), sp, tonumber(g.v0))
            csv:row("%d,svec,%d,%d,%d,%d", vsync_now,
                ok2 and bit.band(svx, 0xFFFF) or -1, ok2 and bit.rshift(svx, 16) or -1,
                ok3 and bit.band(svz, 0xFFFF) or -1, 0)
        end)
        descs[#descs + 1] = d
        arm(FRAG_A_VA, "frag_a", "frag_a")
        arm(GAUGE_FN, "gauge", "gauge")
        arm(ICON_FN, "icon", "icon")
        arm(PROJECT_FN, "project", "project")
        return descs
    end,
    on_capture = function(ctx, el)
        vsync_now = el
        if ENCOUNTER then
            if el == 2 then
                local pokes = read_pokes(POKES_PATH)
                if not pokes or #pokes == 0 then
                    w(string.format("FAIL: no pokes read from %q", POKES_PATH))
                    ctx.request_quit = true
                    return
                end
                for _, p in ipairs(pokes) do probe.write_u32(p.addr, p.word) end
                w(string.format("applied %d SCUS pokes; fragment S head now %08X", #pokes, probe.read_u32(0x8005126C)))
                w(string.format("field state: mode=%02X hook word (field overlay) %08X", probe.read_u8(GAME_MODE_VA), probe.read_u32(HOOK_VA)))
            elseif el == WALK_FROM then
                probe.pad_force(probe.BTN.RIGHT)
                w("holding RIGHT to roll an encounter")
            end
            local mode = probe.read_u8(GAME_MODE_VA)
            if battle_at == nil and mode == 0x15 then
                battle_at = el
                probe.pad_release(probe.BTN.RIGHT)
                w(string.format("battle mode at vsync %d", el))
            end
            if battle_at and fight_at == nil and probe.read_u8(PHASE_VA) == 0xFF then
                fight_at = el
                local hook = probe.read_u32(HOOK_VA)
                w(string.format("fighting phase at vsync %d; hook word from disc = %08X (%s)", el, hook,
                    hook == J_FRAG_A and "PATCHED" or "RETAIL - overlay edit did not arrive"))
                for slot = 3, 6 do
                    local a = probe.read_u32(0x801C9370 + slot * 4)
                    if a ~= 0 then
                        w(string.format("slot %d actor=%08X hp=%d/%d", slot, a,
                            probe.read_u16(a + 0x14C), probe.read_u16(a + 0x14E)))
                    end
                end
            end
            if fight_at and not shot_done and el >= fight_at + BATTLE_SETTLE then
                shot_done = true
                w(string.format("screenshot at vsync %d: %s", el, tostring(screenshot("hpbar_encounter"))))
                w(string.format("hits: frag_a=%d gauge=%d icon=%d project=%d",
                    counts.frag_a, counts.gauge, counts.icon, counts.project))
                ctx.request_quit = true
            end
            if el == FRAMES - 1 then
                w(string.format("TIMEOUT: battle_at=%s fight_at=%s mode=%02X", tostring(battle_at), tostring(fight_at), mode))
            end
            return
        end
        if el == 2 then
            if NO_INJECT then
                w("control run: no edits injected")
            else
                local list = read_edits()
                local n = inject(list)
                w(string.format("injected %d bytes over %d edits from %s", n, #list, EDITS_PATH))
                for _, e in ipairs(list) do
                    w(string.format("  %08X <- %d bytes", e.va, #e.hex / 2))
                end
            end
            local ctxp = probe.read_u32(0x8007BD24)
            w(string.format("ctx=%08X phase=%02X hud_parked=%d", ctxp,
                probe.read_u8(0x8007BD71),
                ctxp ~= 0 and probe.read_u16(ctxp + 0x6CE) or -1))
            for slot = 3, 6 do
                local a = probe.read_u32(0x801C9370 + slot * 4)
                if a ~= 0 then
                    w(string.format("slot %d actor=%08X hp=%d/%d anchor=(%d,%d,%d) hidden=%02X",
                        slot, a, probe.read_u16(a + 0x14C), probe.read_u16(a + 0x14E),
                        probe.read_u16(a + 0x3C), probe.read_u16(a + 0x3E), probe.read_u16(a + 0x40),
                        probe.read_u8(a + 0x21C)))
                end
            end
        end
        if shots[el] then
            local n = 0
            for _ in pairs(shots) do n = n + 1 end
            w(string.format("screenshot at vsync %d: %s", el,
                tostring(screenshot(string.format("hpbar_%03d", el)))))
        end
        if el == FRAMES - 1 then
            w(string.format("hits: frag_a=%d gauge=%d icon=%d project=%d",
                counts.frag_a, counts.gauge, counts.icon, counts.project))
        end
    end,
    on_done = function()
        csv:close()
        if summary then summary:close() end
    end,
})
