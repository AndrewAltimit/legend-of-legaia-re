-- autorun_w4d_light_kind_hits.lua
--
-- Do the renderer's light-capable prim handlers (kinds 8..11) ever execute?
--
-- Slots 8..11 of the per-kind prim dispatch are the only handlers in
-- `SCUS_942.54` carrying a GTE light op (`NCCS` / `NCCT`), and they are
-- bank-invariant - every fog bank points at the same four bodies
-- (docs/subsystems/renderer.md). Sampling has never caught them running on a
-- battle, a summon, `map01` or a cold-boot `town01` field, and the standing
-- presumption named the world-map kingdom-bundle slot-4 meshes as the consumer.
-- That presumption is doubly suspect now that slot 4 is known to be the scene's
-- ANM animation bank (docs/formats/world-map-overlay.md), so this probe puts
-- exec breakpoints on the four handlers and walks two kingdom overworlds.
--
-- A zero is only worth reporting if the instrument can see a hit, so a
-- **control group** is armed alongside: the eight bank-0 fog-bank handlers for
-- kinds 12..19 plus kind 16 bank 1, all of which are ordinary depth-cued
-- geometry handlers (LEGAIA_CONTROL overrides the comma list). They share one
-- counter and every one of them disarms at LEGAIA_CONTROL_CAP hits, so the
-- interpreter is not dragged through a per-primitive callback for a whole run.
-- A run whose control count is zero proves nothing about kinds 8..11 and says
-- so in the summary. The group is deliberately wide because the world map's
-- bulk terrain swaps in overlay-resident replacements for kinds 12..19
-- (`0x801F7644..0x801F8690`, PROT 0901), so any single SCUS handler could be
-- silent there for a reason that has nothing to do with the light path.
--
-- Pad plan: some catalogued world-map states are parked in the town scene one
-- held press from the kingdom warp, so LEGAIA_WARP_BTN (default DOWN) is held
-- until the scene name changes; the probe then cycles the four D-pad
-- directions LEGAIA_LEG vsyncs each to walk the overworld under the camera.
--
-- Env vars:
--   LEGAIA_SSTATE        save state (--scenario karisto_sol_pre_encounter, ...)
--   LEGAIA_FRAMES        capture vsyncs (default 1200)
--   LEGAIA_WARP_BTN      UP/DOWN/LEFT/RIGHT/NONE held until the scene changes
--                        (default DOWN; NONE = already on the overworld)
--   LEGAIA_LEG           vsyncs per walk direction (default 90)
--   LEGAIA_CONTROL       comma list of control handler addresses (first window)
--   LEGAIA_CONTROL_POST  comma list armed for the post-warp window instead
--   LEGAIA_CONTROL_CAP   control hits before self-disarm, per window (default 40)
--   LEGAIA_CONTROL_REARM vsyncs after the warp before the control group is armed
--                        a second time (default 120)
--   LEGAIA_OUT_DIR       output directory
--
-- Outputs: w4d_light_kinds.csv, w4d_light_kinds.log, w4d_light_kinds.detail.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1200)
local LEG      = probe.getenv_num("LEGAIA_LEG", 90)
local WARP_BTN = probe.getenv("LEGAIA_WARP_BTN", "DOWN")
local CONTROL_S = probe.getenv("LEGAIA_CONTROL",
    "0x80043658,0x80043768,0x800438B8,0x800439E4,0x80043B58,0x80043C6C," ..
    "0x80043DD4,0x80043F10,0x80044C14")
local CTRL_CAP  = probe.getenv_num("LEGAIA_CONTROL_CAP", 40)
-- The post-warp control is a DIFFERENT list on purpose. The kingdom overworld
-- renders its bulk terrain through PROT 0901's eight overlay-resident
-- replacements for kinds 12..19 (docs/subsystems/world-map.md), so the SCUS
-- fog handlers are silent there for a reason that has nothing to do with the
-- light path - they make a useless liveness control on the map itself.
local CONTROL_POST_S = probe.getenv("LEGAIA_CONTROL_POST",
    "0x801F7644,0x801F7838,0x801F7AA4,0x801F7CCC," ..
    "0x801F7F78,0x801F8198,0x801F8454,0x801F8690")

local function parse_addrs(str)
    local out = {}
    for tok in string.gmatch(str, "[^,%s]+") do
        local a = tonumber(tok)
        if a then out[#out + 1] = a end
    end
    return out
end
local CONTROLS      = parse_addrs(CONTROL_S)
local CONTROLS_POST = parse_addrs(CONTROL_POST_S)

local OUT_LOG    = probe.out_path("w4d_light_kinds.log")
local OUT_CSV    = probe.out_path("w4d_light_kinds.csv")
local OUT_DETAIL = probe.out_path("w4d_light_kinds.detail.txt")

local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local PLAYER_PTR = 0x8007C364

-- The four bank-invariant lit handlers (renderer.md, prim dispatch table).
local LIT_KINDS = {
    { kind = 8,  addr = 0x8004409C, op = "NCCS tri" },
    { kind = 9,  addr = 0x8004423C, op = "NCCS quad" },
    { kind = 10, addr = 0x80044434, op = "NCCT tri" },
    { kind = 11, addr = 0x800445B0, op = "NCCT+NCCS quad" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[w4d_light] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function player_tile()
    local p = probe.read_u32(PLAYER_PTR)
    if p == nil or p < 0x80000000 or p >= 0x80200000 then return -1, -1 end
    local x = probe.read_u16(p + 0x14) or 0
    local z = probe.read_u16(p + 0x18) or 0
    if x >= 0x8000 then x = x - 0x10000 end
    if z >= 0x8000 then z = z - 0x10000 end
    return bit.rshift(x, 7), bit.rshift(z, 7)
end

local DIRS = {
    { name = "UP",    btn = probe.BTN.UP },
    { name = "RIGHT", btn = probe.BTN.RIGHT },
    { name = "DOWN",  btn = probe.BTN.DOWN },
    { name = "LEFT",  btn = probe.BTN.LEFT },
}

local held_btn, held_name = nil, "none"
local function hold(btn, name)
    if held_btn then probe.pad_release(held_btn) end
    held_btn, held_name = btn, name or "none"
    if held_btn then probe.pad_force(held_btn) end
end

local csv = nil
local g_elapsed = 0
local lit_hits = { 0, 0, 0, 0 }
local ctrl_hits = 0
local ctrl_bps, ctrl_by_addr = {}, {}
local ctrl_rearmed, ctrl_rearm_at = false, nil
local ctrl_window, ctrl_window_base, ctrl_post = {}, 0, {}
local REARM = probe.getenv_num("LEGAIA_CONTROL_REARM", 120)
local arm_controls = nil            -- set in on_arm; re-armed once after the warp
local start_scene, warped = nil, false
local map_frames, field_frames = 0, 0
local leg_idx, leg_start = 0, nil
local scenes_seen = {}

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV, "tick,event,kind,addr,ra,scene,mode,note")
        probe.write_manifest("autorun_w4d_light_kind_hits.lua", {
            sstate = SSTATE, frames = FRAMES, leg = LEG, warp_btn = WARP_BTN,
            control = CONTROL_S, control_cap = CTRL_CAP,
            core = probe.getenv("LEGAIA_CORE", "?"),
        })
        local descs = {}
        for i, k in ipairs(LIT_KINDS) do
            local idx = i
            local d = { addr = k.addr, hits_ref = { n = 0 },
                        name = string.format("kind %d (%s)", k.kind, k.op) }
            probe.arm_breakpoint(k.addr, "Exec", 4,
                string.format("lit_kind_%d", k.kind), function()
                lit_hits[idx] = lit_hits[idx] + 1
                d.hits_ref.n = d.hits_ref.n + 1
                if lit_hits[idx] <= 8 then
                    local r = PCSX.getRegisters()
                    logf("LIT HIT kind %d at 0x%08X vsync %d ra=0x%08X scene=%s mode=0x%02X",
                        k.kind, k.addr, g_elapsed, n32(r.GPR.n.ra), scene_name(),
                        probe.read_u8(GAME_MODE) or 0)
                    csv:row("%d,lit,%d,0x%08X,0x%08X,%s,0x%02X,hit",
                        g_elapsed, k.kind, k.addr, n32(r.GPR.n.ra), scene_name(),
                        probe.read_u8(GAME_MODE) or 0)
                    probe.append_call_context(OUT_DETAIL,
                        probe.capture_call_context(string.format(
                            "lit kind %d hit #%d vsync=%d scene=%s",
                            k.kind, lit_hits[idx], g_elapsed, scene_name())))
                end
            end)
            descs[#descs + 1] = d
        end

        -- Control group: ordinary depth-cued handlers, so a zero above is a
        -- measurement and not a dead instrument. They share one counter and
        -- all disarm together at the cap. The cap is reached in the first frames
        -- of the run, i.e. before the kingdom warp, so the group is re-armed once
        -- LEGAIA_CONTROL_REARM vsyncs after the warp: that second window is what
        -- proves the breakpoints are still live ON THE OVERWORLD, where the lit
        -- set reads zero.
        arm_controls = function(tag)
            ctrl_window, ctrl_window_base = {}, ctrl_hits
            for _, addr in ipairs(tag == "initial" and CONTROLS or CONTROLS_POST) do
            local a = addr
            ctrl_by_addr[a] = ctrl_by_addr[a] or 0
            local post = (tag ~= "initial")
            local d = { addr = a, hits_ref = { n = 0 },
                        name = string.format("control 0x%08X", a) }
            ctrl_bps[#ctrl_bps + 1] = probe.arm_breakpoint(a, "Exec", 4,
                string.format("control_%08X", a), function()
                ctrl_hits = ctrl_hits + 1
                ctrl_by_addr[a] = ctrl_by_addr[a] + 1
                if post then ctrl_post[a] = (ctrl_post[a] or 0) + 1 end
                d.hits_ref.n = ctrl_by_addr[a]
                if ctrl_window[a] == nil then
                    ctrl_window[a] = g_elapsed
                    logf("CONTROL alive (%s): 0x%08X hit at vsync %d (scene=%s mode=0x%02X)",
                        tag, a, g_elapsed, scene_name(), probe.read_u8(GAME_MODE) or 0)
                    csv:row("%d,control,,0x%08X,,%s,0x%02X,first hit",
                        g_elapsed, a, scene_name(), probe.read_u8(GAME_MODE) or 0)
                end
                if (ctrl_hits - ctrl_window_base) >= CTRL_CAP and #ctrl_bps > 0 then
                    for _, b in ipairs(ctrl_bps) do
                        pcall(function() b:remove() end)
                    end
                    ctrl_bps = {}
                    logf("control breakpoints disarmed after %d shared hits", ctrl_hits)
                end
            end)
            if tag == "initial" then descs[#descs + 1] = d end
        end
        end
        arm_controls("initial")
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        local scene = scene_name()
        -- Frames actually spent walking a kingdom overworld, so a run that
        -- spent its budget inside a random encounter can't read as a clean
        -- world-map zero.
        local mode_now = probe.read_u8(GAME_MODE) or 0
        if mode_now == 0x03 then
            field_frames = field_frames + 1
            if string.match(scene, "^map%d%d$") then map_frames = map_frames + 1 end
        end
        if scene ~= "" and not scenes_seen[scene] then
            scenes_seen[scene] = elapsed
            logf("scene '%s' first seen at vsync %d (mode 0x%02X)",
                scene, elapsed, probe.read_u8(GAME_MODE) or 0)
            csv:row("%d,scene,,,,%s,0x%02X,first seen", elapsed, scene,
                probe.read_u8(GAME_MODE) or 0)
        end

        if start_scene == nil then
            start_scene = scene
            if WARP_BTN == "NONE" then
                warped = true
                leg_idx, leg_start = 1, elapsed
                hold(DIRS[1].btn, DIRS[1].name)
                logf("start scene=%s (already on target); walking %s",
                    scene, DIRS[1].name)
            else
                for _, d in ipairs(DIRS) do
                    if d.name == WARP_BTN then hold(d.btn, d.name) end
                end
                logf("start scene=%s; holding %s for the kingdom warp",
                    scene, WARP_BTN)
            end
            return
        end

        if not warped then
            if scene ~= start_scene and scene ~= "" then
                warped = true
                leg_idx, leg_start = 1, elapsed
                hold(DIRS[1].btn, DIRS[1].name)
                logf("WARPED %s -> %s at vsync %d; walking legs of %d vsyncs",
                    start_scene, scene, elapsed, LEG)
            end
            return
        end

        if warped and not ctrl_rearmed then
            if ctrl_rearm_at == nil then ctrl_rearm_at = elapsed + REARM end
            if elapsed >= ctrl_rearm_at then
                ctrl_rearmed = true
                logf("re-arming the control group at vsync %d (scene=%s)",
                    elapsed, scene)
                arm_controls("post-warp")
            end
        end

        if leg_start and (elapsed - leg_start) >= LEG then
            leg_idx = (leg_idx % #DIRS) + 1
            leg_start = elapsed
            hold(DIRS[leg_idx].btn, DIRS[leg_idx].name)
            local tx, tz = player_tile()
            logf("leg %s at vsync %d (scene=%s tile=(%d,%d) lit=%d/%d/%d/%d ctrl=%d)",
                DIRS[leg_idx].name, elapsed, scene, tx, tz,
                lit_hits[1], lit_hits[2], lit_hits[3], lit_hits[4], ctrl_hits)
        end

        if (elapsed % 180) == 0 then
            local tx, tz = player_tile()
            logf("vsync %4d scene=%s mode=0x%02X tile=(%d,%d) held=%s lit=%d/%d/%d/%d ctrl=%d",
                elapsed, scene, probe.read_u8(GAME_MODE) or 0, tx, tz,
                held_name, lit_hits[1], lit_hits[2], lit_hits[3], lit_hits[4],
                ctrl_hits)
        end
    end,

    on_done = function(ctx)
        hold(nil)
        logf("=== light-capable handler hits ===")
        for i, k in ipairs(LIT_KINDS) do
            logf("  kind %2d  0x%08X  %-16s  %d", k.kind, k.addr, k.op, lit_hits[i])
        end
        logf("control-group hits: %d (cap %d per window, re-armed=%s)",
            ctrl_hits, CTRL_CAP, tostring(ctrl_rearmed))
        for _, a in ipairs(CONTROLS) do
            logf("  control        0x%08X  %d", a, ctrl_by_addr[a] or 0)
        end
        for _, a in ipairs(CONTROLS_POST) do
            logf("  post-warp ctrl 0x%08X  %d", a, ctrl_post[a] or 0)
        end
        if ctrl_hits == 0 then
            logf("WARNING: control never fired - this run measures NOTHING about kinds 8..11")
        end
        local names = {}
        for s in pairs(scenes_seen) do names[#names + 1] = s end
        table.sort(names)
        logf("scenes visited: %s", table.concat(names, " "))
        logf("field-mode frames: %d (of which kingdom-overworld mapNN: %d)",
            field_frames, map_frames)
        csv:row("%d,summary,,,,%s,,k8=%d k9=%d k10=%d k11=%d control=%d field=%d map=%d",
            g_elapsed, table.concat(names, "|"),
            lit_hits[1], lit_hits[2], lit_hits[3], lit_hits[4], ctrl_hits,
            field_frames, map_frames)
        if csv then csv:close() end
        local f = io.open(OUT_LOG, "w")
        if f then
            f:write(table.concat(lines, "\n") .. "\n")
            f:close()
            PCSX.log("[w4d_light] wrote " .. OUT_LOG)
        end
    end,
})
