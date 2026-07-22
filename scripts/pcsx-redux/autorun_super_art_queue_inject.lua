-- autorun_super_art_queue_inject.lua
--
-- Live-executes the retail Super-Art tail-replace (FUN_801EF9E4) for every
-- Super in the trigger table, capturing the per-actor action queue at
-- actor[+0x1DF..+0x1F2] after the replace runs.
--
-- Method (see docs/tooling/super-art-queue-capture.md):
--   The arts queue-builder FUN_801EED1C ends with `jal 0x801EF9E4`
--   (call site 0x801EF9AC) passing a0 = actor slot and
--   a1 = party_char_id[0x8007BD10 + slot] - 1 (0=Vahn 1=Noa 2=Gala; the
--   delay slot does the -1). The applier is table-driven off (a0, a1) only:
--   it zero-scans the queue at actor[+0x1DF], tail-matches the five
--   resident `find` rows at 0x801F6524 + a1*65, and on match overwrites the
--   tail from 0x801F65E8 + a1*80 (sb at 0x801EFB7C). So an Exec breakpoint
--   at 0x801EF9E4 can (1) overwrite the queue with a target Super's exact
--   `find` bytes and (2) set register a1 to the owning character's index,
--   and the *retail* applier performs the byte-exact find->tail-replace for
--   any of the 15 Supers from a single battle state. A second Exec
--   breakpoint at the return site 0x801EF9B4 (the builder epilogue) reads
--   the replaced queue back.
--
-- Base state: any battle state where a party member commits an attack
-- input (the builder runs at ActionSeed state 0x0C of FUN_801E295C).
-- `party_basic_attack_vs_gobu_gobu` is the canonical one: Vahn parked on
-- the Begin/Reselect confirm; forcing CROSS starts the turn.
--
-- Run (from repo root; see docs/tooling/super-art-queue-capture.md):
--   xvfb-run -a timeout --kill-after=20s 900s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario party_basic_attack_vs_gobu_gobu \
--       --lua scripts/pcsx-redux/autorun_super_art_queue_inject.lua \
--       --frames 20000
--
-- Env knobs:
--   LEGAIA_SUPER_LIST   comma-separated 0-based indices into the table
--                       below (default "0,1,...,14" = all 15)
--   LEGAIA_DRY=1        no injection: log the natural (a0, a1) + queue at
--                       the applier hit, then quit (method validation)
--   LEGAIA_PRESS_FRAME  vsync (per iteration) to force CROSS (default 30)
--   LEGAIA_ITER_FRAMES  per-iteration frame budget before TIMEOUT (2500)
--   LEGAIA_SAVE_STATES=1  autosave a post-replace .sstate per Super
--
-- Output: captures/super_art_queue_inject/<ts>/super_art_queue_inject.csv
--   idx,name,char,slot,find_hex,queue_after_hex,pass

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 20000)
local DRY         = probe.getenv_num("LEGAIA_DRY", 0)
local PRESS_FRAME = probe.getenv_num("LEGAIA_PRESS_FRAME", 30)
local PRESS_HOLD  = probe.getenv_num("LEGAIA_PRESS_HOLD", 10)
local ITER_FRAMES = probe.getenv_num("LEGAIA_ITER_FRAMES", 2500)
local SAVE_STATES = probe.getenv_num("LEGAIA_SAVE_STATES", 0)

local APPLIER_PC  = 0x801EF9E4 -- FUN_801EF9E4 entry (Super find/tail-replace)
local RETURN_PC   = 0x801EF9B4 -- FUN_801EED1C epilogue (jal return site)
local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local Q_OFF       = 0x1DF      -- action-parameter byte stream head
local Q_LEN       = 0x10       -- the 16-byte queue proper (+0x1DF..+0x1EE);
                               -- FUN_801DA34C preseeds exactly 16 bytes and
                               -- the applier's zero-scan caps at 0x10.
                               -- +0x1EF.. holds unrelated actor fields
                               -- (02 03 04 05 in the Gobu Gobu state) - do
                               -- not write past +0x1EE.
local SNAP_LEN    = 0x14       -- read-back window +0x1DF..+0x1F2 (pinned)

-- The 15-entry Super table, in resident trigger-table order (find
-- 0x801F6524 / replace 0x801F65E8) = crates/art/src/super_art.rs order.
-- char: 0=Vahn 1=Noa 2=Gala. Byte strings are the modeled find/replace.
local SUPERS = {
    { char = 0, name = "Tri-Somersault",
      find = "19270F191F0E1927",     replace = "19270F191F0E1A2B2B2B" },
    { char = 0, name = "Maximum Blow",
      find = "19280E19260C1925",     replace = "19280E19260C1A2C" },
    { char = 0, name = "Fire Tackle",
      find = "19290C19250D1928",     replace = "19290C19250D1A2D" },
    { char = 0, name = "Power Slash",
      find = "19280E19270E1926",     replace = "19280E19270E1A2E" },
    { char = 0, name = "Rolling Combo",
      find = "19220C19250F0F1921",   replace = "19220C19250F0F1A2F30" },
    { char = 1, name = "Triple Lizard",
      find = "19250F19240E192B",     replace = "19250F19240E1A2E2E2E" },
    { char = 1, name = "Super Javelin",
      find = "19220E1929",           replace = "19220E1A2F" },
    { char = 1, name = "Super Tempest",
      find = "19260D0C0F0F1921",     replace = "19260D0C0F0F1A30" },
    { char = 1, name = "Love You",
      find = "19270E192B0E0C1923",   replace = "19270E192B0E0C1A31" },
    { char = 1, name = "Dragon Fangs",
      find = "192B0F19240E192A",     replace = "192B0F19240E1A32" },
    { char = 2, name = "Back Punch x3",
      find = "19270F19290D1926",     replace = "19270F19290D1A2B2B2B" },
    { char = 2, name = "Super Ironhead",
      find = "19290F19240E1927",     replace = "19290F19240E1A2C" },
    { char = 2, name = "Rushing Crush",
      find = "19280F19290F1924",     replace = "19280F19290F1A2D" },
    { char = 2, name = "Heaven's Drop",
      find = "19290F19240C0E1922",   replace = "19290F19240C0E1A2E" },
    { char = 2, name = "Neo Static Raising",
      find = "19260F19250C0D0F1921", replace = "19260F19250C0D0F1A2F" },
}

local function hex_to_bytes(s)
    local t = {}
    for h in string.gmatch(s, "%x%x") do t[#t + 1] = tonumber(h, 16) end
    return t
end

-- Iteration list.
local LIST = {}
local list_env = probe.getenv("LEGAIA_SUPER_LIST", "")
if list_env ~= "" then
    for tok in string.gmatch(list_env, "[^,]+") do
        LIST[#LIST + 1] = tonumber(tok)
    end
else
    for i = 0, #SUPERS - 1 do LIST[#LIST + 1] = i end
end

local OUT_PATH = probe.out_path("super_art_queue_inject.csv")
local csv = probe.csv_open(OUT_PATH,
    "idx,name,char,slot,find_hex,queue_after_hex,pass")

local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local function actor_ptr(slot)
    return tou32(probe.read_u32(ACTOR_TABLE + slot * 4))
end

local function queue_hex(actor)
    local b = probe.read_bytes(actor + Q_OFF, SNAP_LEN)
    if b == nil then return "?" end
    return probe.bytes_to_hex(b):gsub("%s+", "")
end

-- Per-iteration state machine: "wait_hit" -> "wait_ret" -> "post" -> reload.
local iter        = 1     -- index into LIST
local stage       = "wait_hit"
local iter_frame  = 0
local pressed     = false
local pending     = nil   -- {slot=, actor=} between the two breakpoints
local passes, fails = 0, 0

local function current()
    local idx = LIST[iter]
    if idx == nil then return nil, nil end
    return idx, SUPERS[idx + 1]
end

local function finish_iter(idx, sup, verdict, after_hex, slot)
    csv:row("%d,%s,%d,%s,%s,%s,%s",
        idx, sup.name, sup.char, tostring(slot or "?"),
        sup.find, after_hex or "?", verdict)
    PCSX.log(string.format("[sq] #%d %-18s char=%d -> %s  queue=%s",
        idx, sup.name, sup.char, verdict, after_hex or "?"))
    if verdict == "PASS" then passes = passes + 1 else fails = fails + 1 end
    stage = "post"
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(APPLIER_PC, "Exec", 4, "sq_applier", function()
            local r = PCSX.getRegisters()
            local a0 = tou32(r.GPR.n.a0)
            local a1 = tou32(r.GPR.n.a1)
            if a0 > 2 then return end
            local actor = actor_ptr(a0)
            if not probe.in_ram(actor) then return end
            if DRY ~= 0 then
                PCSX.log(string.format(
                    "[sq] DRY applier hit: slot=%d char_idx=%d queue=%s (frame %d)",
                    a0, a1, queue_hex(actor), iter_frame))
                stage = "dry_done"
                return
            end
            if stage ~= "wait_hit" then return end
            local idx, sup = current()
            if idx == nil then return end
            -- Overwrite the queue with the Super's exact `find` bytes,
            -- zero-filling the rest of the pinned window.
            local find = hex_to_bytes(sup.find)
            for i = 0, Q_LEN - 1 do
                mem.write_u8(actor + Q_OFF + i, find[i + 1] or 0)
            end
            -- Retarget the applier at the owning character's table rows.
            r.GPR.n.a1 = sup.char
            pending = { slot = a0, actor = actor, nat_a1 = a1 }
            stage = "wait_ret"
            PCSX.log(string.format(
                "[sq] #%d %s: injected find=%s slot=%d nat_char=%d -> char=%d",
                idx, sup.name, sup.find, a0, a1, sup.char))
        end)
        probe.arm_breakpoint(RETURN_PC, "Exec", 4, "sq_return", function()
            if stage ~= "wait_ret" or pending == nil then return end
            local idx, sup = current()
            if idx == nil then return end
            local after = queue_hex(pending.actor)
            -- PASS iff the 16-byte queue proper is exactly `replace` ++ zero
            -- fill. Bytes beyond +0x1EE in the snapshot are neighbor fields
            -- and are recorded but not judged.
            local want = sup.replace
            local wlen = #want
            local qhex = (after ~= "?") and after:sub(1, Q_LEN * 2) or "?"
            local ok = (qhex ~= "?") and (qhex:sub(1, wlen) == want)
            if ok then
                local rest = qhex:sub(wlen + 1)
                ok = rest == string.rep("0", #rest)
            end
            finish_iter(idx, sup, ok and "PASS" or "FAIL", after, pending.slot)
        end)
        return {}
    end,

    on_capture = function(hctx, _elapsed)
        iter_frame = iter_frame + 1
        if stage == "dry_done" then
            hctx.request_quit = true
            return
        end
        -- Drive the Begin confirm.
        if stage == "wait_hit" and not pressed and iter_frame >= PRESS_FRAME then
            pad.force(pad.BTN.CROSS)
            pressed = true
        end
        if pressed and iter_frame >= PRESS_FRAME + PRESS_HOLD then
            pad.release(pad.BTN.CROSS)
        end
        -- Per-iteration budget.
        if stage ~= "post" and iter_frame > ITER_FRAMES then
            local idx, sup = current()
            if idx ~= nil then
                finish_iter(idx, sup, "TIMEOUT", nil, nil)
            else
                hctx.request_quit = true
                return
            end
        end
        -- Post-capture: optionally autosave, then reload for the next Super.
        if stage == "post" then
            local idx, sup = current()
            if SAVE_STATES ~= 0 and idx ~= nil and pending ~= nil then
                sstate.save(probe.out_path(string.format(
                    "super_%02d_%s.sstate", idx,
                    sup.name:lower():gsub("[^%w]+", "_"))))
            end
            pending = nil
            iter = iter + 1
            if LIST[iter] == nil then
                hctx.request_quit = true
                return
            end
            if not sstate.load(SSTATE_PATH) then
                PCSX.log("[sq] FATAL: reload failed")
                hctx.request_quit = true
                return
            end
            iter_frame = 0
            pressed = false
            stage = "wait_hit"
            pad.release(pad.BTN.CROSS)
        end
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "=== super-art queue-inject: %d PASS / %d FAIL (of %d requested) ===",
            passes, fails, #LIST))
    end,
})
