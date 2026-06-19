-- Transport Control Example Plugin
-- This plugin demonstrates how to use the AviQtl transport API

local frame_counter = 0
local is_monitoring = false

-- Called when plugin is loaded
function AviQtlOnLoad()
    aviqtl.log("[Transport Example] Plugin loaded!")
    aviqtl.log("[Transport Example] Use 'aviqtl.transport.play()' etc. in the console")
end

-- Called every ~16ms
function AviQtlUpdateHook()
    frame_counter = frame_counter + 1

    -- Example: Log current frame every 60 frames (about once per second at 60fps)
    if is_monitoring and frame_counter % 60 == 0 then
        local current_frame = aviqtl.transport.get_frame()
        local is_playing = aviqtl.transport.is_playing()
        aviqtl.log(string.format("[Transport Example] Frame: %d, Playing: %s", current_frame, is_playing and "yes" or "no"))
    end
end

-- Called when a project is opened
function AviQtlOnProjectOpen(path)
    aviqtl.log("[Transport Example] Project opened: " .. path)
end

-- Helper function to demonstrate transport control
function demo_play_pause()
    aviqtl.transport.toggle()
    local state = aviqtl.transport.is_playing() and "playing" or "paused"
    aviqtl.log("[Transport Example] Toggled to: " .. state)
end

-- Helper function to seek to a specific frame
function demo_seek(frame)
    aviqtl.transport.seek(frame)
    aviqtl.log("[Transport Example] Seeked to frame: " .. frame)
end

-- Toggle monitoring
function toggle_monitoring()
    is_monitoring = not is_monitoring
    aviqtl.log("[Transport Example] Monitoring: " .. (is_monitoring and "enabled" or "disabled"))
end

aviqtl.log("[Transport Example] Commands available:")
aviqtl.log("  demo_play_pause()    - Toggle play/pause")
aviqtl.log("  demo_seek(frame)     - Seek to frame")
aviqtl.log("  toggle_monitoring()  - Toggle frame monitoring")
