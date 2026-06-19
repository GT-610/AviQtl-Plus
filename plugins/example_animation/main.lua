-- Animation Effect Example Plugin
-- Demonstrates AviUtl-style script parameters for animation control

--group:Position
--track@pos_x:X Position,-1000,1000,0
--track@pos_y:Y Position,-1000,1000,0
--track@pos_z:Z Position,-1000,1000,0

--group:Rotation
--track@rot_x:X Rotation,0,360,0
--track@rot_y:Y Rotation,0,360,0
--track@rot_z:Z Rotation,0,360,0

--group:Scale
--track@scale_x:X Scale,0,10,1,0.01
--track@scale_y:Y Scale,0,10,1,0.01

--group:Appearance
--check@visible:Visible,true
--color@tint:Tint Color,0xffffff
--select@blend_mode:Blend Mode=normal,normal=0,add=1,subtract=2,multiply=3,screen=4

--group:Animation
--check@animate:Enable Animation,false
--track@anim_speed:Animation Speed,0,10,1,0.1
--select@anim_type:Animation Type=0,sine=0,linear=1,square=2,ease_in=3,ease_out=4

local time = 0

function AviQtlOnLoad()
    aviqtl.log("[Animation Example] Loaded with parameters:")
    aviqtl.log("  Position: (" .. pos_x .. ", " .. pos_y .. ", " .. pos_z .. ")")
    aviqtl.log("  Rotation: (" .. rot_x .. ", " .. rot_y .. ", " .. rot_z .. ")")
    aviqtl.log("  Scale: (" .. scale_x .. ", " .. scale_y .. ")")
    aviqtl.log("  Visible: " .. tostring(visible))
    aviqtl.log("  Tint: 0x" .. string.format("%06x", tint))
end

function AviQtlUpdateHook()
    if not visible then return end

    if animate then
        time = time + anim_speed * 0.016

        -- Calculate animation value based on type
        local anim_value = 0
        if anim_type == 0 then -- sine
            anim_value = math.sin(time * math.pi * 2)
        elseif anim_type == 1 then -- linear
            anim_value = (time % 1) * 2 - 1
        elseif anim_type == 2 then -- square
            anim_value = (math.floor(time * 2) % 2 == 0) and 1 or -1
        elseif anim_type == 3 then -- ease_in
            local t = time % 1
            anim_value = t * t * 2 - 1
        elseif anim_type == 4 then -- ease_out
            local t = time % 1
            anim_value = (1 - (1-t) * (1-t)) * 2 - 1
        end

        -- Apply animation to rotation
        local current_rot_z = rot_z + anim_value * 30
        -- In a real implementation, this would update the object
        aviqtl.log("[Animation] rot_z: " .. string.format("%.1f", current_rot_z))
    end
end

-- Commands to modify parameters interactively
function set_position(x, y, z)
    pos_x = x or pos_x
    pos_y = y or pos_y
    pos_z = z or pos_z
    aviqtl.log("[Animation] Position set to: (" .. pos_x .. ", " .. pos_y .. ", " .. pos_z .. ")")
end

function set_tint_color(hex)
    tint = hex
    aviqtl.log("[Animation] Tint set to: 0x" .. string.format("%06x", tint))
end

aviqtl.log("[Animation Example] Commands:")
aviqtl.log("  set_position(x, y, z)  - Set position")
aviqtl.log("  set_tint_color(hex)    - Set tint color (hex)")
