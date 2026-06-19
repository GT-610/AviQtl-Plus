-- Transport Control Example Plugin
-- This plugin demonstrates how to use the AviUtl-style script parameters

--track@monitor_interval:Monitor Interval,1,120,60
--check@auto_play:Auto Play on Load,false
--select@log_level:Log Level=info,debug=0,info=1,warning=2,error=3

local frame_counter = 0
local is_monitoring = true

-- Log level names for display
local log_levels = {[0]="DEBUG", [1]="INFO", [2]="WARNING", [3]="ERROR"}

-- Helper function to log with level
function log_with_level(level, msg)
    if log_level >= level then
        aviqtl.log("[" .. log_levels[level] .. "] " .. msg)
    end
end

-- Called when plugin is loaded
function AviQtlOnLoad()
    log_with_level(1, "Transport Example loaded!")
    log_with_level(1, "Monitor interval: " .. monitor_interval .. " frames")
    log_with_level(1, "Auto play: " .. tostring(auto_play))
    log_with_level(1, "Log level: " .. log_levels[log_level])

    if auto_play then
        aviqtl.transport.play()
        log_with_level(1, "Auto-play enabled, starting playback")
    end
end

-- Called every ~16ms
function AviQtlUpdateHook()
    frame_counter = frame_counter + 1

    -- Monitor based on user-defined interval
    if is_monitoring and frame_counter % monitor_interval == 0 then
        local ok_frame, current_frame = pcall(aviqtl.transport.get_frame)
        local ok_playing, is_playing = pcall(aviqtl.transport.is_playing)
        if not ok_frame or not ok_playing then
            return
        end
        log_with_level(0, string.format("Frame: %d, Playing: %s", current_frame, is_playing and "yes" or "no"))
    end
end

-- Called when a project is opened
function AviQtlOnProjectOpen(path)
    log_with_level(1, "Project opened: " .. path)
end

-- Helper function to demonstrate transport control
function demo_play_pause()
    aviqtl.transport.toggle()
    local state = aviqtl.transport.is_playing() and "playing" or "paused"
    log_with_level(1, "Toggled to: " .. state)
end

-- Helper function to seek to a specific frame
function demo_seek(frame)
    aviqtl.transport.seek(frame)
    log_with_level(1, "Seeked to frame: " .. frame)
end

-- Toggle monitoring
function toggle_monitoring()
    is_monitoring = not is_monitoring
    log_with_level(1, "Monitoring: " .. (is_monitoring and "enabled" or "disabled"))
end

log_with_level(1, "Commands available:")
log_with_level(1, "  demo_play_pause()    - Toggle play/pause")
log_with_level(1, "  demo_seek(frame)     - Seek to frame")
log_with_level(1, "  toggle_monitoring()  - Toggle frame monitoring")
