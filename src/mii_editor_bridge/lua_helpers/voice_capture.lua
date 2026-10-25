-- Voice page table shared by the fingerprinted main and initialize closures.
-- Reads the list cursor and the two axes. Does not decide, save, or play a test.
local page = ...
return 0, page.selector:get_selected_button_id(), page.voice_type, page.voice_pitch
