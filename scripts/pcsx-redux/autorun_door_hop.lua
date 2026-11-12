-- autorun_door_hop.lua
--
-- Cross a chain of scene doors on the retail disc by standing the player on
-- each door's walk-on tile, and save a raw state in every scene it arrives
-- in. The fast sibling of autorun_walk_route.lua: the door's own record runs
-- (its gates, its flag writes, its arrival cutscene), only the walk to the
-- tile is skipped, so no random encounter is fought and no NPC blocks the way.
-- A state saved here carries the source state's story flags plus what the
-- doors' records wrote, and its scenario must say it was reached by tile pokes.
-- docs/tooling/capture-coverage.md, "Filling the worklist".
--
-- LEGAIA_HOPS is a `;` list of `<scene>:<tile x>:<tile z>[:<wait>]`: while
-- <scene> is loaded and the field has run <wait> vsyncs (default 60), write
-- player +0x14 / +0x18 to the tile's centre (held LEGAIA_POKE_FOR, 20, vsyncs).
-- A door whose record refuses
-- (a gate) leaves the scene loaded: after LEGAIA_REFUSE (240) vsyncs the probe
-- logs REFUSED and ends the run with what it saved. An empty list only waits
-- for the field and saves it (a state that plays forward into its scene).
--
-- Between hops: an open dialogue box (*0x801C6EA4 +0x62, or the pager
-- _DAT_801F2734 waiting on a page at 0x19) is paged with Cross,
-- and a mode other than the field (a scripted fight, a talk screen) is mashed
-- with Cross, so an arrival cutscene runs out. Saved per scene, once
-- LEGAIA_SCENE_SETTLE (150) vsyncs after the field resumed in it; then a press
-- of LEGAIA_NUDGE (default UP, 16 vsyncs) logs CONTROL when it moves the player
-- and NO_CONTROL when LEGAIA_STUCK (1500) vsyncs pass without it.
--
-- LEGAIA_FLAGS_SET / LEGAIA_FLAGS_CLEAR: comma lists of story flags (hex, the
-- 0x80085758 bank) written once as the run starts, for a door whose record
-- tests a flag the source state does not hold. A state made so carries a poke:
-- its scenario must name the flags.
--
-- Env: LEGAIA_SSTATE, LEGAIA_HOPS, LEGAIA_OUT_DIR, LEGAIA_PREFIX (default
-- "hop"), LEGAIA_FRAMES (default 6000). No breakpoints, --fast is fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local PREFIX = probe.getenv("LEGAIA_PREFIX", "hop")
local SETTLE = probe.getenv_num("LEGAIA_SCENE_SETTLE", 150)
local REFUSE = probe.getenv_num("LEGAIA_REFUSE", 240)
local STUCK = probe.getenv_num("LEGAIA_STUCK", 1500)
local POKE_FOR = probe.getenv_num("LEGAIA_POKE_FOR", 20)
local NUDGE = probe.BTN[probe.getenv("LEGAIA_NUDGE", "UP")] or probe.BTN.UP

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA = 0x8007050c
local PLAYER_PTR = 0x8007c364
local FIELD_CTL = 0x801c6ea4

local hops = {}
for sc, x, z, w in string.gmatch(probe.getenv("LEGAIA_HOPS", ""), "([%w_]+):(%d+):(%d+):?(%d*)") do
    hops[#hops + 1] = { scene = sc, x = tonumber(x), z = tonumber(z), wait = tonumber(w) or 60 }
end

local FLAG_BANK = 0x80085758
local function flag_list(name)
    local out = {}
    for f in string.gmatch(probe.getenv(name, ""), "[%x]+") do
        out[#out + 1] = tonumber(f, 16)
    end
    return out
end
local flags_set, flags_clear = flag_list("LEGAIA_FLAGS_SET"), flag_list("LEGAIA_FLAGS_CLEAR")

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[hop] " .. s)
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end
local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end
local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function player_xz()
    local pp = probe.read_u32(PLAYER_PTR)
    if not ok_ptr(pp) then return nil end
    return s16(pp + 0x14), s16(pp + 0x18), pp
end
local function dialog_open()
    local c = probe.read_u32(FIELD_CTL)
    if ok_ptr(c) and (probe.read_u8(c + 0x62) or 0) ~= 0 then return true end
    -- A scripted beat's box leaves +0x62 clear; its page wait shows on the
    -- pager state _DAT_801F2734 (0x19).
    return (probe.read_u32(0x801F2734) or 0) == 0x19
end

local function save_checkpoint(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
end

local hi = 1
local last_mode, last_scene = nil, nil
local field_since = 0
local saved, ckpted = {}, {}
local poked_at, held_tile = nil, nil
local tap_until, tap_btn, next_tap = 0, nil, 0
local nudge_at, nudge_from = nil, nil
local finished = false
local flags_done = false

local function tap(btn, vsync, hold)
    probe.pad.force(btn)
    tap_btn, tap_until = btn, vsync + (hold or 6)
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        logf("sstate=%s hops=%d frames=%d", SSTATE_PATH, #hops, FRAMES)
        return {}
    end,

    on_capture = function(ctx, vsync)
        if finished then return end
        -- After the state load, which on_arm runs ahead of.
        if not flags_done then
            flags_done = true
            for _, f in ipairs(flags_set) do
                local va = FLAG_BANK + bit.rshift(f, 3)
                probe.write_u8(va, bit.bor(probe.read_u8(va) or 0, bit.rshift(0x80, bit.band(f, 7))))
                logf("FLAG set 0x%03X", f)
            end
            for _, f in ipairs(flags_clear) do
                local va = FLAG_BANK + bit.rshift(f, 3)
                probe.write_u8(va, bit.band(probe.read_u8(va) or 0, 0xFF - bit.rshift(0x80, bit.band(f, 7))))
                logf("FLAG clear 0x%03X", f)
            end
        end
        if tap_btn and vsync >= tap_until then
            probe.pad.release(tap_btn)
            tap_btn = nil
        end
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        local label = scene_name()
        if mode ~= last_mode or label ~= last_scene then
            local px, pz = player_xz()
            logf("vsync=%d game_mode=0x%02x scene=%q player=(%s,%s) hop=%d", vsync, mode, label,
                tostring(px), tostring(pz), hi)
            field_since = vsync
            if label ~= last_scene then poked_at, nudge_at = nil, nil end
            last_mode, last_scene = mode, label
        end

        -- Run an arrival out: page a box, mash through a non-field mode.
        if vsync >= next_tap and not tap_btn and (mode ~= 0x03 or dialog_open()) then
            tap(probe.BTN.CROSS, vsync)
            next_tap = vsync + 20
            field_since = vsync
            return
        end
        if mode ~= 0x03 then return end
        local since = vsync - field_since

        -- Save this scene before testing the pad: a press can walk the player
        -- straight back through the door he came in by. The press then says
        -- whether the state was a free field run (log CONTROL) or a cutscene
        -- that keeps the pad (log NO_CONTROL after LEGAIA_STUCK vsyncs).
        local arrival = hi > 1 or #hops == 0
        if arrival and not saved[label] then
            if since >= SETTLE and not nudge_at and not tap_btn then
                if not ckpted[label] then
                    ckpted[label] = true
                    save_checkpoint(string.format("%s_scene_%s_v%d", PREFIX, label, vsync), vsync)
                end
                nudge_from = { player_xz() }
                nudge_at = vsync
                tap(NUDGE, vsync, 16)
            elseif nudge_at and vsync >= nudge_at + 40 and not tap_btn then
                local x, z = player_xz()
                if x ~= nudge_from[1] or z ~= nudge_from[2] then
                    saved[label] = true
                    logf("CONTROL in %q at vsync=%d", label, vsync)
                elseif since >= STUCK then
                    saved[label] = true
                    logf("NO_CONTROL in %q at vsync=%d", label, vsync)
                else
                    nudge_at = nil
                    field_since = vsync - SETTLE + 60
                end
            end
            if not saved[label] then return end
        end

        -- A poked door has the scene to change; a gate leaves it loaded. The
        -- tile is held: the dispatcher's per-tick compare only runs when no
        -- lock holds the player, and a walking actor rewrites its position.
        if poked_at ~= nil then
            if vsync - poked_at < POKE_FOR and held_tile then
                local _, _, pp = player_xz()
                if pp then
                    probe.write_u16(pp + 0x14, held_tile[1])
                    probe.write_u16(pp + 0x18, held_tile[2])
                end
            end
            if vsync - poked_at >= REFUSE then
                logf("REFUSED: door in %q did not change the scene (vsync=%d)", label, vsync)
                finished = true
                ctx.request_quit = true
            end
            return
        end
        local h = hops[hi]
        if h == nil then
            logf("route done at vsync=%d in %q", vsync, label)
            finished = true
            ctx.request_quit = true
            return
        end
        if label ~= h.scene then
            logf("hop %d wants %q, in %q: stop", hi, h.scene, label)
            finished = true
            ctx.request_quit = true
            return
        end
        if since >= h.wait then
            local _, _, pp = player_xz()
            if pp then
                probe.write_u16(pp + 0x14, h.x * 128 + 64)
                probe.write_u16(pp + 0x18, h.z * 128 + 64)
                poked_at = vsync
                held_tile = { h.x * 128 + 64, h.z * 128 + 64 }
                saved[label] = true
                logf("POKE %q tile (%d,%d) vsync=%d", label, h.x, h.z, vsync)
                hi = hi + 1
            end
        end
    end,

    on_done = function(_, _)
        local fh = io.open(probe.out_path(PREFIX .. ".log"), "w")
        if fh then fh:write(table.concat(log_lines, "\n"), "\n"); fh:close() end
    end,
})
