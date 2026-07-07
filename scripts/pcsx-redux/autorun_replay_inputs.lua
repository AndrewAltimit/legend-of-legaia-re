-- autorun_replay_inputs.lua
--
-- Deterministic input REPLAY. Resumes the same save state and plays back a button
-- timeline recorded by autorun_record_inputs.lua, reproducing a manually-played
-- sequence (e.g. walk to Tetsu + pick the 3rd training-fight option + start the
-- spar) headlessly, then checkpoints the result (S5 battle entry).
--
-- It reconstructs the per-frame held mask from the CSV (hold the last value
-- between transitions) and, at each field tick, DRIVES THE PAD via pad.force /
-- pad.release for the buttons whose bits are in that mask. Direct RAM writes to
-- 0x8007B850 do NOT work - FUN_8001822C rebuilds the mask from the actual pad
-- after the field-tick BP - so replay must go through the pad. The mask is the
-- byte-swapped PSX controller word (button index b -> bit 1<<(b+8) for b<8 else
-- 1<<(b-8); UP=0x1000 DOWN=0x4000 CROSS=0x0040, pinned by autorun_btnmap.lua).
-- Frame 0 = first field tick after load, matching the recorder's clock. A battle
-- (game_mode 0x15 OR battle-ctx 0x8007BD24 != 0) is captured from the field-tick
-- clock (FUN_8001698C keeps firing through this battle; a Vsync-only capture
-- missed it), settling LEGAIA_SETTLE field-ticks before the checkpoint.
--
-- Env: LEGAIA_SSTATE, LEGAIA_INPUTS (CSV path), LEGAIA_OUT_DIR,
--      LEGAIA_CKPT_LABEL, LEGAIA_SETTLE, LEGAIA_MAX_FRAMES, LEGAIA_BOOT_DELAY.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP=0x8001698C; local GM=0x8007B83C; local BATTLE_CTX=0x8007BD24
local SCENE_NAME=0x8007050C; local PLAYER=0x8007C364
local BATTLE_MODE=0x15
-- button index -> its bit in the 0x8007B850 mask (byte-swapped PSX word).
local function btn_bit(b) if b<8 then return 2^(b+8) else return 2^(b-8) end end

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local INPUTS     = env.getenv("LEGAIA_INPUTS", "captures/input_record/inputs.csv")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/input_replay")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s5_tetsu_battle")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "25")) or 25
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "20000")) or 20000
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/replay.log", "w")
local function log(s) PCSX.log("[rep] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or 0 end
local function read_scene()
    local s={}; for i=0,7 do local b=ru8(SCENE_NAME+i); if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
-- drive the pad to match `mask`: force buttons whose bit is set, release others.
local held_now = {}
local function apply_mask(mask)
    for b=0,15 do
        local on = (math.floor(mask / btn_bit(b)) % 2) == 1
        if on and not held_now[b] then pad.force(b); held_now[b]=true
        elseif (not on) and held_now[b] then pad.release(b); held_now[b]=false end
    end
end

-- load the timeline: sorted list of {frame, held}
local timeline = {}
do
    local fh = io.open(INPUTS, "r")
    if fh==nil then log("CANNOT OPEN "..INPUTS) else
        for line in fh:lines() do
            if line:sub(1,1) ~= "#" and #line>0 then
                local f,h = line:match("^(%d+),0[xX](%x+)")
                if f and h then timeline[#timeline+1] = { tonumber(f), tonumber(h,16) } end
            end
        end
        fh:close()
    end
    log(string.format("loaded %d input transitions from %s", #timeline, INPUTS))
end

local vsync, loaded, done = 0, false, false
local frame, ti, cur_held, prev_held = -1, 1, 0, 0
local battle_seen, cap_since = false, nil
local last_eng, lx, lz = false, nil, nil

-- shared battle capture: callable from whichever clock is still ticking in
-- battle (the field-tick BP keeps firing here; GPU::Vsync may not). `clock` is a
-- monotonically-increasing frame counter. Returns true while a battle is up.
local function try_capture(clock)
    local m=ru8(GM); local bc=ru32(BATTLE_CTX)
    if m==BATTLE_MODE or bc~=0 then
        if not battle_seen then battle_seen=true; log(string.format("*** BATTLE mode=0x%02X ctx=0x%08X (clock %d) ***", m, bc, clock)) end
        if cap_since==nil then cap_since=clock
        elseif clock-cap_since>=SETTLE then
            local ok=pcall(function()
                local w=PCSX.createSaveState()
                local fhh=Support.File.open(OUT_DIR.."/"..CKPT_LABEL..".rawsstate","CREATE"); fhh:writeMoveSlice(w); fhh:close()
            end)
            log(ok and ("checkpoint written: "..OUT_DIR.."/"..CKPT_LABEL..".rawsstate") or "checkpoint FAILED")
            done=true; if LOG then LOG:close() end; PCSX.quit(0)
        end
        return true
    end
    cap_since=nil; return false
end

-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE)); return
    end
    -- battle capture is driven from the field-tick clock (it keeps firing in
    -- battle here); the Vsync listener only resumes the save.
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    if not loaded or done then return end
    frame=frame+1
    if try_capture(frame) then return end   -- in battle: capture, stop driving input
    if frame>=MAX_FRAMES and not battle_seen then log("MAX_FRAMES, no battle scene="..read_scene()); if LOG then LOG:close() end; PCSX.quit(0); return end
    -- advance to the active transition for this frame
    while ti<=#timeline and timeline[ti][1]<=frame do cur_held=timeline[ti][2]; ti=ti+1 end
    apply_mask(cur_held)
    -- diagnostics: position, engaged flag, warps, engaged transitions
    local p=ru32(PLAYER); local px,pz,eng=0,0,false
    if p~=0 then
        px=mem.read_u16(p+0x14) or 0; pz=mem.read_u16(p+0x18) or 0
        if px>=0x8000 then px=px-0x10000 end; if pz>=0x8000 then pz=pz-0x10000 end
        local fl=ru32(p+0x10); eng=(math.floor(fl/0x80000)%2==1)
    end
    if eng~=last_eng then log(string.format("[f%d] engaged %s->%s pos=(%d,%d) held=0x%04X", frame, tostring(last_eng), tostring(eng), px, pz, cur_held)); last_eng=eng end
    if lx~=nil and (math.abs(px-lx)+math.abs(pz-lz))>300 then log(string.format("[f%d] WARP (%d,%d)->(%d,%d)", frame, lx,lz,px,pz)) end
    lx,lz=px,pz
    if (frame%120)==0 then
        log(string.format("[f%d] held=0x%04X pos=(%d,%d) eng=%s scene=%q mode=0x%02X", frame, cur_held, px, pz, tostring(eng), read_scene(), ru8(GM)))
    end
end)

log("replay_inputs armed")
