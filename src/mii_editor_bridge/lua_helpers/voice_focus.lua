-- Restore the unfinished Voice cursor. set_voice_type is a transition: it
-- reads the page's current voice_type and voice_pitch, disables pitch button
-- 1 when the requested pair is None, then enables it when the old pair was
-- None. Those fields stay untouched until that call. set_voice_pitch only
-- writes labels. Pitch stays selectable only when the finished pair is not
-- None. Button 1 is the listing's pitch id (set_parts_01). This does not
-- write the native commit flag at component +0x100.
local page, cursor, voice_type, voice_pitch = ...
page.selected_id = cursor
page.selector:select_button(cursor)
page:select_visible_arrow(cursor)
page:set_voice_type(voice_type)
page:set_voice_pitch(voice_pitch)
local none = UiScriptPlayer.invoke("is_voice_none", page.voice_type, page.voice_pitch)
page.selector:set_selectable(1, none ~= true)
