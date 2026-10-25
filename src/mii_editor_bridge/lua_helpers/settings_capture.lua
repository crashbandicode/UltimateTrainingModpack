local settings = ...
if settings.is_end or settings.exit_value ~= EXIT_VALUE_DECIDE then
    return 1, 0, 0, 0
end
-- update_random yields inside its popup while visible_popup_random is still
-- true. A Random highlight has that flag and is not busy. is_busy is also
-- true for Save and Cancel; this is the Random prompt only when selector1
-- is still on Random (id 0) and neither the list nor the Save footer has
-- focus. The popup result is not read.
local random_pending = settings.visible_popup_random == true
    and AppPopupManager.is_busy() == true
    and settings.selector1:get_selected_button_id() == 0
local area = -1
if settings.selector0:has_focus() then
    area = 0
elseif settings.selector1:has_focus() then
    area = settings.selector1:get_selected_button_id() + 1
elseif settings:is_selected_com_frame_actor() then
    area = 3
elseif random_pending then
    area = 1
end
local confirming = 0
if random_pending and area == 1 then
    confirming = 1
end
return 0, settings.selector0:get_selected_button_id(), area, confirming
