-- autorun_opening_spawn_probe.lua  (uncommitted RE probe)
--
-- Pins, for the retail New-Game opening chain
-- (opdeene -> opstati -> opurud -> map01 -> town01):
--   1. every call of the partition-record -> VM-context dispatcher
--      FUN_8003BDE0(x, y, p2_index, gate): args, ra, scene, ticks --
--      attribution of WHO spawns each opening cutscene record;
--   2. the player actor's world X/Z + tile (X>>7, Z>>7) for the first
--      ~120 field ticks of each scene (player = *(0x8007C364), X at
--      +0x14, Z at +0x18, i16 -- per docs/subsystems/field-locomotion.md;
--      NB 0x8007C34C is the actor-LIST head table, not the player ptr);
--   3. whether/when the opurud `4C 9F`-registered actor-list callback
--      LAB_801DA930 fires, relative to the record-9 spawn.
--
-- Boot/navigation logic reused from autorun_opening_capture.lua:
-- cold boot (-fastboot) -> title pulse pattern START/UP/CROSS on the
-- title-tick exec BP (FUN_801DD35C) until game_mode==3 with scene
-- "opdeene"; then hands-off. NO screenshots (takeScreenShot in BP
-- context segfaults); ticks driven by exec BPs, never vsync (the
-- title XA blinds GPU::Vsync).
--
-- Outputs (LEGAIA_OUT_DIR):
--   spawn_hits.csv   every FUN_8003BDE0 hit
--   cb_hits.csv      LAB_801DA930 hits (rate-limited during CAPTURE only;
--                    title-phase hits only counted -- the title overlay
--                    aliases that VA)
--   pos.csv          per-tick player pos for the first POS_WINDOW ticks
--                    of every scene
--   spawn_ctx.txt    call-context dumps for first hits
--   opening.log      timeline
--
-- Run (cold boot; NO save state):
--   timeout --kill-after=15s 2400 ~/Tools/pcsx-redux/pcsx-redux \
--     -interpreter -debugger -fastboot -bios ~/.mednafen/firmware/SCPH1001.BIN \
--     -iso "$LEGAIA_DISC_BIN" -run -stdout \
--     -dofile scripts/pcsx-redux/autorun_opening_spawn_probe.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env  = require("probe.env")
local mem  = require("probe.mem")
local pad  = require("probe.pad")
local bp   = require("probe.bp")
local snap = require("probe.snapshot")

local GM         = 0x8007B83C -- game_mode (low byte)
local SCENE_NAME = 0x8007050C -- active scene-name buffer
local TITLE_BP   = 0x801DD35C -- title overlay per-frame tick
local FIELD_BP   = 0x8001698C -- default mode handler per-frame vsync-sync
local SPAWN_FN   = 0x8003BDE0 -- partition-record -> VM-context dispatcher
local CB_FN      = 0x801DA930 -- opurud 4C 9F actor-list callback (overlay VA)
local PLAYER_PTR = 0x8007C364 -- -> player actor; +0x14 X, +0x18 Z (i16)

local OUT_DIR     = env.getenv("LEGAIA_OUT_DIR", "captures/opening_spawn_probe")
local STALL_TICKS = tonumber(env.getenv("LEGAIA_STALL_TICKS", "4200")) or 4200
local TOWN_TICKS  = tonumber(env.getenv("LEGAIA_TOWN_TICKS", "3600")) or 3600
local MAX_TICKS   = tonumber(env.getenv("LEGAIA_MAX_TICKS", "30000")) or 30000
local TITLE_MAX   = tonumber(env.getenv("LEGAIA_TITLE_MAX", "40000")) or 40000
local MASH_EVERY  = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20
local POS_WINDOW  = tonumber(env.getenv("LEGAIA_POS_WINDOW", "130")) or 130

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/opening.log", "w")
local SPAWN_CSV = io.open(OUT_DIR .. "/spawn_hits.csv", "w")
local CB_CSV    = io.open(OUT_DIR .. "/cb_hits.csv", "w")
local POS_CSV   = io.open(OUT_DIR .. "/pos.csv", "w")
local CTX_PATH  = OUT_DIR .. "/spawn_ctx.txt"
if SPAWN_CSV then
    SPAWN_CSV:write("phase,vsync,title_tick,field_tick,scene,scene_age,mode," ..
        "a0,a1,a2,a3,ra,player_x,player_z,tile_x,tile_z\n")
end
if CB_CSV then
    CB_CSV:write("phase,vsync,field_tick,scene,scene_age,hit_no_in_scene,ra,a0\n")
end
if POS_CSV then
    POS_CSV:write("field_tick,vsync,scene,scene_age,mode,player_ptr,x,z,tile_x,tile_z\n")
end

local function log(s)
    PCSX.log("[opnspawn] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end

local function u32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local function i16(v)
    if v == nil then return nil end
    if v >= 0x8000 then return v - 0x10000 end
    return v
end

local function read_mode()
    if not mem.in_ram(GM) then return nil end
    return mem.read_u8(GM)
end

local function read_scene()
    if not mem.in_ram(SCENE_NAME) then return "" end
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

-- player pos: returns ptr, x, z, tx, tz (any may be nil)
local function read_player()
    local ptr = mem.read_u32(PLAYER_PTR)
    if ptr == nil or not mem.in_ram(ptr, 0x20) then return nil end
    local x = i16(mem.read_u16(ptr + 0x14))
    local z = i16(mem.read_u16(ptr + 0x18))
    if x == nil or z == nil then return ptr end
    -- tile = coord >> 7 (arithmetic on the signed value)
    local tx = math.floor(x / 128)
    local tz = math.floor(z / 128)
    return ptr, x, z, tx, tz
end

-- ---------------------------------------------------------------- state
local PHASE = "TITLE" -- TITLE -> CAPTURE -> DONE
local g_vsync = 0
local g_title_tick = 0
local g_tick = 0
local g_pulse = 0
local g_release_at = 0
local g_held = {}
local cur_scene = ""
local scene_enter_tick = 0
local assist = false
local g_assist_hold = 0
local g_quit_at = nil
-- probe state
local g_spawn_rows = 0
local g_spawn_ctx = {}          -- scene -> ctx dumps taken
local g_cb_hits_scene = {}      -- scene -> count (CAPTURE phase)
local g_cb_title_hits = 0
local g_cb_first_logged = {}    -- scene -> true once ctx dumped
local g_town_spawn_tick = nil
local g_pos_rows = 0

local function hold(btn)
    pad.force(btn)
    g_held[#g_held + 1] = btn
end
local function release_all()
    for _, b in ipairs(g_held) do pad.release(b) end
    g_held = {}
end

local function finish(code, why)
    if PHASE == "DONE" then return end
    PHASE = "DONE"
    release_all()
    log("done: " .. why)
    log(string.format("spawn rows=%d pos rows=%d cb title-phase hits=%d",
        g_spawn_rows, g_pos_rows, g_cb_title_hits))
    for sc, n in pairs(g_cb_hits_scene) do
        log(string.format("cb hits in %q: %d", sc, n))
    end
    log("=== probe hits ===")
    log("=== end ===")
    if LOG then LOG:close(); LOG = nil end
    if SPAWN_CSV then SPAWN_CSV:close(); SPAWN_CSV = nil end
    if CB_CSV then CB_CSV:close(); CB_CSV = nil end
    if POS_CSV then POS_CSV:close(); POS_CSV = nil end
    g_quit_at = { code = code, at = g_tick + 2, title_at = g_title_tick + 2 }
end

-- ------------------------------------------------------------ spawn probe
bp.arm(SPAWN_FN, "Exec", 4, "spawn_fn", function()
    if PHASE == "DONE" then return end
    g_spawn_rows = g_spawn_rows + 1
    if g_spawn_rows > 4000 then return end
    local r = PCSX.getRegisters()
    local a0 = u32(r.GPR.n.a0)
    local a1 = u32(r.GPR.n.a1)
    local a2 = u32(r.GPR.n.a2)
    local a3 = u32(r.GPR.n.a3)
    local ra = u32(r.GPR.n.ra)
    local scene = read_scene()
    local age = g_tick - scene_enter_tick
    local _, x, z, tx, tz = read_player()
    if SPAWN_CSV then
        SPAWN_CSV:write(string.format(
            "%s,%d,%d,%d,%s,%d,0x%02X,%d,%d,%d,%d,0x%08X,%s,%s,%s,%s\n",
            PHASE, g_vsync, g_title_tick, g_tick, scene, age,
            read_mode() or 0xFF, a0, a1, a2, a3, ra,
            tostring(x), tostring(z), tostring(tx), tostring(tz)))
        SPAWN_CSV:flush()
    end
    log(string.format(
        "SPAWN %s tick=%d scene=%q age=%d args=(%d,%d,%d,%d) ra=0x%08X pos=(%s,%s) tile=(%s,%s)",
        PHASE, g_tick, scene, age, a0, a1, a2, a3, ra,
        tostring(x), tostring(z), tostring(tx), tostring(tz)))
    if scene == "town01" and g_town_spawn_tick == nil then
        g_town_spawn_tick = g_tick
    end
    -- call-context for the first 3 hits per scene (ra-chain attribution)
    g_spawn_ctx[scene] = (g_spawn_ctx[scene] or 0) + 1
    if PHASE == "CAPTURE" and g_spawn_ctx[scene] <= 3 then
        local ok, ctx = pcall(snap.capture_call_context, string.format(
            "spawn scene=%s tick=%d p2=%d gate=%d", scene, g_tick, a2, a3))
        if ok and ctx then pcall(snap.append_call_context, CTX_PATH, ctx) end
    end
end)

-- ------------------------------------------------------------ cb probe
bp.arm(CB_FN, "Exec", 4, "opurud_cb", function()
    if PHASE == "DONE" then return end
    if PHASE ~= "CAPTURE" then
        -- title overlay aliases this VA; count only
        g_cb_title_hits = g_cb_title_hits + 1
        return
    end
    local scene = read_scene()
    local n = (g_cb_hits_scene[scene] or 0) + 1
    g_cb_hits_scene[scene] = n
    if n <= 20 or (n % 500) == 0 then
        local r = PCSX.getRegisters()
        local ra = u32(r.GPR.n.ra)
        local a0 = u32(r.GPR.n.a0)
        if CB_CSV then
            CB_CSV:write(string.format("%s,%d,%d,%s,%d,%d,0x%08X,0x%08X\n",
                PHASE, g_vsync, g_tick, scene, g_tick - scene_enter_tick,
                n, ra, a0))
            CB_CSV:flush()
        end
        if n <= 5 then
            log(string.format("CB801DA930 tick=%d scene=%q age=%d n=%d ra=0x%08X",
                g_tick, scene, g_tick - scene_enter_tick, n, ra))
        end
    end
    if not g_cb_first_logged[scene] then
        g_cb_first_logged[scene] = true
        local ok, ctx = pcall(snap.capture_call_context, string.format(
            "cb_801DA930 first hit scene=%s tick=%d", scene, g_tick))
        if ok and ctx then pcall(snap.append_call_context, CTX_PATH, ctx) end
    end
end)

-- ------------------------------------------------------------ pos logging
local function log_pos(age)
    local ptr, x, z, tx, tz = read_player()
    g_pos_rows = g_pos_rows + 1
    if POS_CSV then
        POS_CSV:write(string.format("%d,%d,%s,%d,0x%02X,%s,%s,%s,%s,%s\n",
            g_tick, g_vsync, cur_scene, age, read_mode() or 0xFF,
            ptr and string.format("0x%08X", ptr) or "nil",
            tostring(x), tostring(z), tostring(tx), tostring(tz)))
    end
end

-- ---------------------------------------------------------------- title
local PATTERN = { { pad.BTN.START }, { pad.BTN.UP }, { pad.BTN.CROSS } }

local function opening_reached()
    return read_mode() == 3 and read_scene() == "opdeene"
end

local function enter_capture(from)
    release_all()
    PHASE = "CAPTURE"
    cur_scene = read_scene()
    scene_enter_tick = g_tick
    log(string.format("CAPTURE start (via %s): scene=%q mode=0x%02X title_tick=%d field_tick=%d",
        from, cur_scene, read_mode() or 0xFF, g_title_tick, g_tick))
end

local function title_tick()
    g_title_tick = g_title_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_title_tick >= g_quit_at.title_at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE ~= "TITLE" then return end
    local scene = read_scene()
    if opening_reached() then enter_capture("title"); return end
    if scene ~= "opdeene" and scene ~= "" and read_mode() == 3 then
        log(string.format("WRONG_PATH: scene %q loaded from title (CONTINUE?)", scene))
        finish(1, "wrong path")
        return
    end
    if g_title_tick >= TITLE_MAX then
        log("TITLE_TIMEOUT")
        finish(1, "title timeout")
        return
    end
    if (g_title_tick % 300) == 0 then
        log(string.format("...title tick %d mode=0x%02X scene=%q pulse=%d",
            g_title_tick, read_mode() or 0xFF, scene, g_pulse))
    end
    if g_release_at > 0 and g_title_tick >= g_release_at then
        release_all()
        g_release_at = 0
    elseif g_release_at == 0 and (g_title_tick % MASH_EVERY) == 0 then
        g_pulse = g_pulse + 1
        for _, b in ipairs(PATTERN[(g_pulse % #PATTERN) + 1]) do hold(b) end
        g_release_at = g_title_tick + 5
    end
end

local function field_tick()
    g_tick = g_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_tick >= g_quit_at.at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE == "TITLE" then
        if opening_reached() then enter_capture("field") end
        return
    end
    -- PHASE == "CAPTURE"
    local scene = read_scene()
    if scene ~= cur_scene and scene ~= "" then
        log(string.format("tick %d: scene %q -> %q (mode 0x%02X)",
            g_tick, cur_scene, scene, read_mode() or 0xFF))
        cur_scene = scene
        scene_enter_tick = g_tick
        assist = false
        release_all()
    end
    local age = g_tick - scene_enter_tick
    if age < POS_WINDOW then log_pos(age) end
    if (g_tick % 600) == 0 then
        log(string.format("...field tick %d scene=%q mode=0x%02X assist=%s",
            g_tick, scene, read_mode() or 0xFF, tostring(assist)))
        if POS_CSV then POS_CSV:flush() end
    end
    if g_tick >= MAX_TICKS then finish(0, "global tick cap"); return end
    if scene == "town01" then
        if age >= TOWN_TICKS or
           (g_town_spawn_tick and g_tick - g_town_spawn_tick >= 600 and age >= POS_WINDOW) then
            finish(0, "town01 window complete")
        end
        return -- terminal scene: never assist-mash
    end
    -- assist-mash only after a full natural observation window
    if not assist and age >= STALL_TICKS then
        assist = true
        log(string.format("tick %d: scene %q stalled %d ticks; starting CROSS assist",
            g_tick, scene, STALL_TICKS))
    end
    if assist then
        if g_assist_hold > 0 and g_tick >= g_assist_hold then
            release_all(); g_assist_hold = 0
        elseif g_assist_hold == 0 and (g_tick % 40) == 0 then
            hold(pad.BTN.CROSS)
            g_assist_hold = g_tick + 5
        end
    end
end

pcall(function() bp.arm(TITLE_BP, "Exec", 4, "title_tick", title_tick) end)
pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
log(string.format("opening spawn probe armed: out=%s pos_window=%d", OUT_DIR, POS_WINDOW))

PCSX.Events.createEventListener("GPU::Vsync", function()
    g_vsync = g_vsync + 1
    if PHASE == "DONE" and g_quit_at then
        g_quit_at.vs_seen = (g_quit_at.vs_seen or 0) + 1
        if g_quit_at.vs_seen > 10 then PCSX.quit(g_quit_at.code) end
    end
end)
