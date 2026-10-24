local settings = ...
if settings.is_end or settings.exit_value ~= EXIT_VALUE_DECIDE then
    return 1, 0, 0
end
local area = -1
if settings.selector0:has_focus() then
    area = 0
elseif settings.selector1:has_focus() then
    area = settings.selector1:get_selected_button_id() + 1
elseif settings:is_selected_com_frame_actor() then
    area = 3
end
return 0, settings.selector0:get_selected_button_id(), area
