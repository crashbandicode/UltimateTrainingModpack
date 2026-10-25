local settings, row, area, confirming = ...
if settings:is_selected_com_frame_actor() then
    settings:unselect_com_frame_actor()
end
settings.selector0:set_focus(false)
settings.selector1:set_focus(false)
settings.selector0:select_button(row)
if area == 0 then
    settings.selector0:set_focus(true)
elseif area == 3 then
    settings:select_com_frame_actor()
else
    settings.selector1:select_button(area - 1)
    settings.selector1:set_focus(true)
end
-- confirming == 1 is the unsaved Random prompt, not a Random highlight.
-- This is update_random's open. It does not randomize and it does not answer.
if confirming == 1 then
    AppPopupManager.open_database("ID_POPUP_MMIIF_EDIT_RANDOM")
end
