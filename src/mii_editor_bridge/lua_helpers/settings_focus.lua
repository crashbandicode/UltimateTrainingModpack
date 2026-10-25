local settings, row, area = ...
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
