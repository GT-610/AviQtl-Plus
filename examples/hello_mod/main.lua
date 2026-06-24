-- Hello Mod: Minimal Lua plugin example
-- Demonstrates lifecycle hooks and basic AviQtl APIs

-- AviQtlOnLoad: Called when the plugin is loaded at startup.
-- Use this for initialization, registering commands, loading state.
function AviQtlOnLoad()
    aviqtl.log("[Hello Mod] Loaded! Version " .. aviqtl.settings.get("hello_mod_version") or "unknown")

    -- Save a persistent setting
    aviqtl.settings.set("hello_mod_version", "1.0.0")
    aviqtl.settings.set("hello_mod_load_count",
        (aviqtl.settings.get("hello_mod_load_count") or 0) + 1)

    aviqtl.log("[Hello Mod] Loaded " .. aviqtl.settings.get("hello_mod_load_count") .. " time(s)")
    aviqtl.log("[Hello Mod] Available commands:")
    aviqtl.log("  hello()              - Say hello")
    aviqtl.log("  info()               - Show project info")
    aviqtl.log("  count_clips()        - Count clips in timeline")
end

-- AviQtlUpdateHook: Called every frame during playback.
-- Use this for real-time monitoring or animation logic.
-- WARNING: Keep this lightweight — it runs 60 times per second.
function AviQtlUpdateHook(frame)
    -- Uncomment to log every 60th frame (once per second at 60fps):
    -- if frame % 60 == 0 then
    --     aviqtl.log("[Hello Mod] Frame " .. frame)
    -- end
end

-- AviQtlOnProjectSave: Called when the user saves the project.
function AviQtlOnProjectSave(path)
    aviqtl.log("[Hello Mod] Project saved to: " .. path)
end

-- Custom commands (callable from the Lua console or other plugins)

function hello()
    aviqtl.log("[Hello Mod] Hello from the Lua plugin system!")
    aviqtl.log("[Hello Mod] Current project: " ..
        aviqtl.project.width() .. "x" .. aviqtl.project.height() ..
        " @ " .. aviqtl.project.fps() .. " fps")
end

function info()
    aviqtl.log("[Hello Mod] === Project Info ===")
    aviqtl.log("  Resolution: " .. aviqtl.project.width() .. "x" .. aviqtl.project.height())
    aviqtl.log("  FPS: " .. aviqtl.project.fps())
    aviqtl.log("  Load count: " .. aviqtl.settings.get("hello_mod_load_count"))
end

function count_clips()
    local clips = aviqtl.clip.list()
    aviqtl.log("[Hello Mod] Total clips: " .. #clips)
    for i, clip in ipairs(clips) do
        aviqtl.log(string.format("  [%d] %s (layer %d, frames %d-%d)",
            i, clip.id, clip.layer, clip.startFrame, clip.endFrame))
    end
    return #clips
end
