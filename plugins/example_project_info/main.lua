-- Project Info Example Plugin
-- This plugin demonstrates how to use the AviQtl project and settings API

-- Called when plugin is loaded
function AviQtlOnLoad()
    aviqtl.log("[Project Info Example] Plugin loaded!")

    -- Load saved settings
    local saved_greeting = aviqtl.settings.get("example_greeting")
    if saved_greeting and saved_greeting ~= "" then
        aviqtl.log("[Project Info Example] Saved greeting: " .. saved_greeting)
    else
        aviqtl.log("[Project Info Example] No saved greeting found")
    end
end

-- Get current project information
function get_project_info()
    local width = aviqtl.project.width()
    local height = aviqtl.project.height()
    local fps = aviqtl.project.fps()

    aviqtl.log("[Project Info Example] Project Info:")
    aviqtl.log(string.format("  Resolution: %dx%d", width, height))
    aviqtl.log(string.format("  FPS: %.2f", fps))

    return {width = width, height = height, fps = fps}
end

-- Save a custom greeting
function save_greeting(greeting)
    if greeting == nil then
        aviqtl.log("[Project Info Example] Usage: save_greeting('Hello World')")
        return
    end
    aviqtl.settings.set("example_greeting", greeting)
    aviqtl.log("[Project Info Example] Saved greeting: " .. greeting)
end

-- Load and display the saved greeting
function show_greeting()
    local greeting = aviqtl.settings.get("example_greeting")
    if greeting and greeting ~= "" then
        aviqtl.log("[Project Info Example] Greeting: " .. greeting)
    else
        aviqtl.log("[Project Info Example] No greeting saved")
    end
end

-- Save project to a specific path
function save_project_to(path)
    if path == nil then
        aviqtl.log("[Project Info Example] Usage: save_project_to('/path/to/project.aviqtl')")
        return
    end
    local success = aviqtl.project.save(path)
    if success then
        aviqtl.log("[Project Info Example] Project saved to: " .. path)
    else
        aviqtl.log("[Project Info Example] Failed to save project")
    end
end

-- Create a new scene
function create_new_scene(name)
    if name == nil then
        name = "Scene " .. os.date("%H:%M:%S")
    end
    aviqtl.scene.create(name)
    aviqtl.log("[Project Info Example] Created scene: " .. name)
end

-- Called when project is saved
function AviQtlOnProjectSave(path)
    aviqtl.log("[Project Info Example] Project saved to: " .. path)
    get_project_info()
end

aviqtl.log("[Project Info Example] Commands available:")
aviqtl.log("  get_project_info()           - Show project info")
aviqtl.log("  save_greeting('text')        - Save a greeting")
aviqtl.log("  show_greeting()              - Show saved greeting")
aviqtl.log("  save_project_to(path)        - Save project to path")
aviqtl.log("  create_new_scene(name)       - Create a new scene")
