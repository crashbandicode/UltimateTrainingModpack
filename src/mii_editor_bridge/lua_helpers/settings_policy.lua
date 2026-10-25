-- Retained selector disabler. Training and Quickplay do not install it.
-- Name, Special Moves, Test, Save and Cancel stay native either way. Headgear,
-- Outfit, Color, Voice and Random keep the game's own rows in both modes.
local settings = ...
local selector0, selector1 = settings.selector0, settings.selector1
if selector0 == nil or selector1 == nil then
    return 2
end

-- selector0 rows: 0 Name, 1 Headgear, 2 Outfit, 3 Color, 4 Special Moves, 5 Voice.
-- selector1 ids: 0 Random, 1 Test.
local function cosmetic(row)
    return row == 1 or row == 2 or row == 3 or row == 5
end

local fresh = settings.training_only_cosmetics == nil
if fresh then
    local update_selector0 = settings.update_selector0
    local update_com_frame = settings.update_com_frame
    settings.training_only_cosmetics = { update_selector0, update_com_frame }
    -- Gate the decision before it can become a native child request, then
    -- keep a Right move off the disabled Random button.
    settings.update_selector0 = function(self)
        local list = self.selector0
        if not list:has_focus() then
            return update_selector0(self)
        end
        if cosmetic(list:get_decided_button_id()) then
            return false
        end
        local decided = update_selector0(self)
        if not decided and self.selector1:has_focus()
            and self.selector1:get_selected_button_id() ~= 1 then
            self.selector1:select_button(1)
        end
        return decided
    end
    -- Leaving the Save footer upward natively lands on Voice; use Special Moves.
    settings.update_com_frame = function(self)
        local from_footer = self:is_selected_com_frame_actor()
        local decided = update_com_frame(self)
        if from_footer and not decided and self.selector0:has_focus()
            and cosmetic(self.selector0:get_selected_button_id()) then
            self.selector0:select_button(4)
        end
        return decided
    end
    -- Random confirms inside Lua before native set_random; drop the action.
    settings.update_random = function() end
end

-- initialize rebuilds both selectors on every Settings entry; reapply.
selector0:set_selectable(1, false)
selector0:set_selectable(2, false)
selector0:set_selectable(3, false)
selector0:set_selectable(5, false)
selector1:set_selectable(0, false)
selector1:select_button(1)
return fresh and 1 or 0
