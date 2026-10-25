-- Host stand-in for the Voice listing's set_voice_type / set_voice_pitch.
-- "None" here is whichever pair the stub says is None. It is not a game id.
local helper = assert(loadfile(arg[1]))
local function pair_key(voice_type, voice_pitch)
    return tostring(voice_type) .. ":" .. tostring(voice_pitch)
end
local function run(old_type, old_pitch, new_type, new_pitch, none_key)
    local calls = {}
    local page = {
        voice_type = old_type,
        voice_pitch = old_pitch,
        selected_id = -1,
        selector = {},
    }
    function page.selector:select_button() end
    function page.selector:set_selectable(id, on)
        calls[#calls + 1] = { id = id, on = on }
    end
    function page:select_visible_arrow() end
    function page:set_voice_type(requested)
        local requested_none = UiScriptPlayer.invoke("is_voice_none", requested, self.voice_pitch)
        if requested_none == true then
            self.selector:set_selectable(1, false)
        end
        local old_none = UiScriptPlayer.invoke("is_voice_none", self.voice_type, self.voice_pitch)
        if old_none == true then
            self.selector:set_selectable(1, true)
        end
        self.voice_type = requested
    end
    function page:set_voice_pitch(pitch)
        self.voice_pitch = pitch
    end
    UiScriptPlayer = {
        invoke = function(name, voice_type, voice_pitch)
            assert(name == "is_voice_none")
            if pair_key(voice_type, voice_pitch) == none_key then
                return true
            end
            return false
        end,
    }
    helper(page, 0, new_type, new_pitch)
    local last = calls[#calls]
    return last, calls
end

local function expect(label, last, id, on)
    if last == nil or last.id ~= id or last.on ~= on then
        io.stderr:write(label .. " last set_selectable mismatch\n")
        os.exit(1)
    end
    io.write(label .. " id=" .. tostring(last.id) .. " on=" .. tostring(last.on) .. "\n")
end

local none = "0:0"
local last, calls = run(0, 0, 0, 0, none)
expect("equal-none", last, 1, false)
local sequence = {}
for _, call in ipairs(calls) do
    sequence[#sequence + 1] = tostring(call.on)
end
local joined = table.concat(sequence, ",")
if joined ~= "false,true,false" then
    io.stderr:write("equal-none sequence " .. joined .. "\n")
    os.exit(1)
end
io.write("equal-none sequence " .. joined .. "\n")
last = run(1, 1, 0, 0, none)
expect("into-none", last, 1, false)
last = run(0, 0, 1, 1, none)
expect("out-of-none", last, 1, true)
last, calls = run(1, 1, 1, 2, none)
expect("non-none", last, 1, true)
if #calls ~= 1 then
    io.stderr:write("non-none should reconcile once\n")
    os.exit(1)
end
io.write("ok\n")
