-- Restore the unfinished Voice cursor. set_voice_type / set_voice_pitch update
-- the page's own labels. They are not the native commit (component flag +0x100).
local page, cursor, voice_type, voice_pitch = ...
page.voice_pitch = voice_pitch
page.voice_type = voice_type
page.selected_id = cursor
page.selector:select_button(cursor)
page:select_visible_arrow(cursor)
page:set_voice_pitch(voice_pitch)
page:set_voice_type(voice_type)
