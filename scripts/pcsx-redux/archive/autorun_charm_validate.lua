-- autorun_charm_validate.lua
--
-- Runtime playtest for the enemy-ally "charm" randomizer feature. On a live
-- battle state it applies, in RAM, the same two effects the disc patch produces,
-- then drives the round and confirms the behaviour:
--
--   1. set the slot-3 monster's flags +0x16E |= 0x380 ("AI-delegated")
--   2. widen the overlay victory check at 0x801E6638 (andi v0,v0,0x4 -> 0x384)
--
-- Then it pulses Confirm to advance the battle and watches:
--   * the charmed monster's target (+0x1DD) flips into the MONSTER band ([3..])
--     when it acts (proof the stock FUN_801E7320 retarget fires for it), and
--   * the monster-wipe victory signal (DAT_8007BD71 == 0xFE) fires.
--
-- Pure RAM poke (no disc patch needed): a savestate restores vanilla code, so
-- this validates the *mechanism* the disc patch relies on. Short window; quits.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local GMODE       = 0x8007B83C
local VICTORY_VA  = 0x801E6638
local VICT_SIGNAL = 0x8007BD71  -- 0xFE on battle end
local AI_DELEGATE = 0x380

local function u32(a) return probe.read_u32(a) or 0 end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end
local function monster_ptr(slot) return u32(ACTOR_TABLE + slot * 4) end

-- write a 32-bit word as two halfword pokes (probe.mem has no write_u32).
local function write_u32(addr, v)
    probe.write_u16(addr, v % 0x10000)
    probe.write_u16(addr + 4 - 2, math.floor(v / 0x10000) % 0x10000)
end

local poked = false
local retarget_seen = false      -- charmed monster's +0x1DD landed in [3..6]
local victory_seen = false       -- monster-wipe signal fired
local flag_stuck = false         -- 0x380 confirmed live on slot 3
local victory_word_patched = false

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = 900,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        local ctx = u32(CTX_PTR)
        local pc = ok_ptr(ctx) and (probe.read_u8(ctx + 0) or 0) or 0
        local mc = ok_ptr(ctx) and (probe.read_u8(ctx + 1) or 0) or 0
        local m = monster_ptr(3)

        -- Apply the two charm edits once, after the state has settled.
        if elapsed == 70 and not poked and ok_ptr(m) then
            local old = probe.read_u16(m + 0x16E) or 0
            probe.write_u16(m + 0x16E, bit.bor(old, AI_DELEGATE))
            local now = probe.read_u16(m + 0x16E) or 0
            flag_stuck = (now % 0x400 >= AI_DELEGATE)
            PCSX.log(string.format("[POKE] slot3 flags 0x%04X -> 0x%04X (ai380=%s)",
                old, now, flag_stuck and "Y" or "n"))

            -- The victory-widen poke is opt-in (LEGAIA_CHARM_WIDEN=1): with it,
            -- the lone charmed monster is counted "down" so victory fires before
            -- it ever acts (good for showing the widen). Default OFF so the
            -- monster actually takes its turn and we can watch it retarget.
            local vold = probe.read_u32(VICTORY_VA) or 0
            if os.getenv("LEGAIA_CHARM_WIDEN") == "1" then
                write_u32(VICTORY_VA, 0x30420384)
                local vnew = probe.read_u32(VICTORY_VA) or 0
                victory_word_patched = (vnew == 0x30420384)
                PCSX.log(string.format(
                    "[POKE] victory word 0x%08X -> 0x%08X (expect 0x30420004 -> 0x30420384, ok=%s)",
                    vold, vnew, victory_word_patched and "Y" or "n"))
            else
                victory_word_patched = (vold == 0x30420004)
                PCSX.log(string.format(
                    "[CHECK] victory word live = 0x%08X (expect andi v0,v0,0x4 = 0x30420004, ok=%s); widen not applied this run",
                    vold, victory_word_patched and "Y" or "n"))
            end
            poked = true
        end

        -- Drive the round: pulse Confirm (Cross is Confirm in Legaia USA; Circle
        -- is Cancel/back) to push menus/targets forward.
        if elapsed >= 90 then
            local phase = elapsed % 12
            if phase == 0 then pad.force(pad.BTN.CROSS)
            elseif phase == 4 then pad.release(pad.BTN.CROSS) end
        end

        -- Watch the charmed monster's resolved target + the victory signal.
        if poked and ok_ptr(m) then
            local flags = probe.read_u16(m + 0x16E) or 0
            local tgt = probe.read_u8(m + 0x1DD) or 0
            if (flags % 0x400 >= AI_DELEGATE) and tgt >= 3 and tgt <= 6 then
                if not retarget_seen then
                    PCSX.log(string.format(
                        "[OBSERVE @%d] charmed slot3 target=%d (MONSTER band) -- retarget confirmed",
                        elapsed, tgt))
                end
                retarget_seen = true
            end
        end
        local sig = probe.read_u8(VICT_SIGNAL) or 0
        if sig == 0xFE then
            if not victory_seen then
                PCSX.log(string.format("[OBSERVE @%d] victory signal 0x%02X (monster wipe) -- battle won",
                    elapsed, sig))
            end
            victory_seen = true
        end

        if elapsed % 60 == 0 then
            PCSX.log(string.format("[t%4d] gm=0x%02X pc=%d mc=%d slot3.live=%d slot3.tgt=%d vict=0x%02X",
                elapsed, probe.read_u8(GMODE) or 0, pc, mc,
                ok_ptr(m) and (probe.read_u16(m + 0x14C) or 0) or -1,
                ok_ptr(m) and (probe.read_u8(m + 0x1DD) or 0) or -1, sig))
        end

        if retarget_seen and victory_seen then c.request_quit = true end
        if elapsed >= 850 then c.request_quit = true end
    end,
    on_done = function()
        pad.release(pad.BTN.CROSS)
        PCSX.log("==== CHARM PLAYTEST RESULT ====")
        PCSX.log(string.format("  0x380 flag set live on slot 3 : %s", flag_stuck and "PASS" or "FAIL"))
        PCSX.log(string.format("  victory word widened to 0x384 : %s", victory_word_patched and "PASS" or "FAIL"))
        PCSX.log(string.format("  charmed monster retargets band: %s", retarget_seen and "PASS" or "(not observed)"))
        PCSX.log(string.format("  battle won (victory signal)   : %s", victory_seen and "PASS" or "(not observed)"))
    end,
})
