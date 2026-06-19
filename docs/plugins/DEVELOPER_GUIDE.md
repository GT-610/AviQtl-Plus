# AviQtl Plugin Developer Guide

This guide explains how to develop plugins for AviQtl.

## Prerequisites

- Basic knowledge of Lua 5.1/LuaJIT
- A text editor
- AviQtl installed

## Basic Plugin Structure

### Directory-Based (Recommended)

```
my_plugin/
├── manifest.lua    # Plugin metadata (required)
├── main.lua        # Main entry point (required)
├── utils.lua       # Additional modules (optional)
└── README.md       # Documentation (optional)
```

### Single-File

```
plugins/
└── my_plugin.lua   # Everything in a single file
```

## Creating manifest.lua

`manifest.lua` defines the plugin's metadata.

```lua
return {
    id = "com.yourname.pluginname",     -- Unique ID (reverse domain notation)
    name = "My Plugin",                 -- Display name
    version = "1.0.0",                  -- Semantic version
    author = "Your Name",              -- Author name
    description = "Plugin description", -- Brief description
    min_app_version = "0.2.0"          -- Minimum AviQtl version required
}
```

### Field Descriptions

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✓ | Unique identifier. Use reverse domain notation |
| `name` | ✓ | Display name shown to users |
| `version` | ✓ | Semantic version (x.y.z) |
| `author` | | Author name |
| `description` | | Brief description of the plugin |
| `min_app_version` | | Minimum AviQtl version required |

## Creating main.lua

`main.lua` contains the plugin's main code.

### Basic Template

```lua
-- Called when the plugin is loaded
function AviQtlOnLoad()
    aviqtl.log("My Plugin loaded!")
end

-- Called approximately every 16ms (60fps)
function AviQtlUpdateHook()
    -- Periodic tasks go here
end

-- Called when a project is opened
function AviQtlOnProjectOpen(path)
    aviqtl.log("Project opened: " .. path)
end

-- Called when a project is saved
function AviQtlOnProjectSave(path)
    aviqtl.log("Project saved: " .. path)
end
```

## Lifecycle Hooks

Define these global functions to respond to application events.

| Function | Arguments | Description |
|----------|-----------|-------------|
| `AviQtlOnLoad()` | None | Called after plugin is loaded |
| `AviQtlOnUnload()` | None | Called when plugin is unloaded |
| `AviQtlUpdateHook()` | None | Called approximately every 16ms |
| `AviQtlOnProjectOpen(path)` | `path`: string | Called when project is opened |
| `AviQtlOnProjectSave(path)` | `path`: string | Called when project is saved |
| `AviQtlOnClipChange()` | None | Called when clips are modified |

## Using the API

### Transport Control

```lua
-- Start playback
aviqtl.transport.play()

-- Pause playback
aviqtl.transport.pause()

-- Toggle play/pause
aviqtl.transport.toggle()

-- Seek to specific frame
aviqtl.transport.seek(100)

-- Get current frame
local frame = aviqtl.transport.get_frame()

-- Check if playing
local playing = aviqtl.transport.is_playing()
```

### Clip Operations

```lua
-- List all clips
local clips = aviqtl.clip.list()
for i, clip in ipairs(clips) do
    aviqtl.log(string.format("Clip %d: %s at layer %d", 
        clip.id, clip.type, clip.layer))
end

-- Create a new clip
aviqtl.clip.create("text", 0, 0)  -- type, startFrame, layer

-- Delete a clip
aviqtl.clip.delete(clipId)

-- Update clip properties
aviqtl.clip.update(clipId, newLayer, newStart, newDuration)

-- Select a clip
aviqtl.clip.select(clipId)

-- Split a clip
aviqtl.clip.split(clipId, frame)

-- Clipboard operations
aviqtl.clip.copy(clipId)
aviqtl.clip.cut(clipId)
aviqtl.clip.paste(clipId, layer)
```

### Effect Operations

```lua
-- Add effect to clip
aviqtl.effect.add(clipId, "blur")

-- Remove effect
aviqtl.effect.remove(clipId, effectIndex)

-- Set effect parameter
aviqtl.effect.set_param(clipId, effectIndex, "radius", 10.0)
```

### Project Information

```lua
-- Get project info
local width = aviqtl.project.width()
local height = aviqtl.project.height()
local fps = aviqtl.project.fps()

-- Save project
aviqtl.project.save("/path/to/project.aviqtl")

-- Load project
aviqtl.project.load("/path/to/project.aviqtl")
```

### Scene Operations

```lua
-- Create a scene
aviqtl.scene.create("New Scene")

-- Remove a scene
aviqtl.scene.remove(sceneId)

-- Switch scene
aviqtl.scene.switch(sceneId)
```

### Settings

```lua
-- Save a setting
aviqtl.settings.set("my_key", "my_value")

-- Load a setting
local value = aviqtl.settings.get("my_key")
if value == "" then
    aviqtl.settings.set("my_key", "default_value")
end
```

### Logging

```lua
aviqtl.log("Hello from my plugin!")
aviqtl.log(string.format("Current frame: %d", aviqtl.transport.get_frame()))
```

## Permission Requirements

Plugins must be granted permissions to use certain APIs. Users manage permissions through the Package Manager.

### Permission-to-API Mapping

| Permission | Required APIs |
|------------|---------------|
| `transport.control` | `transport.*` |
| `clip.read` | `clip.list`, `clip.select` |
| `clip.modify` | `clip.create`, `clip.delete`, `clip.update`, `clip.split` |
| `effect.modify` | `effect.add`, `effect.remove`, `effect.set_param` |
| `project.read` | `project.width`, `project.height`, `project.fps` |
| `project.save` | `project.save` |
| `project.load` | `project.load` |
| `scene.manage` | `scene.*` |
| `settings.read` | `settings.get` |
| `settings.write` | `settings.set` |
| `clipboard.access` | `clip.copy`, `clip.cut`, `clip.paste` |
| `log.output` | `log` |

### Error Handling

Calling an API without permission raises a Lua error.

```lua
-- Permission check example
local ok, err = pcall(function()
    aviqtl.transport.play()
end)
if not ok then
    aviqtl.log("Permission denied: " .. err)
end
```

## Best Practices

### 1. Use Unique IDs

Use reverse domain notation to ensure globally unique IDs.

```lua
-- Good
id = "com.yourname.autosave"

-- Bad
id = "autosave"  -- May conflict with other plugins
```

### 2. Keep UpdateHook Lightweight

`AviQtlUpdateHook` is called frequently, so avoid heavy processing.

```lua
-- Good: Use frame counter to throttle
local frame_count = 0
function AviQtlUpdateHook()
    frame_count = frame_count + 1
    if frame_count % 60 == 0 then  -- Approximately once per second
        -- Heavy processing here
    end
end

-- Bad: Calling heavy_work() every frame
function AviQtlUpdateHook()
    heavy_work()  -- Impacts performance
end
```

### 3. Namespace Your Settings

Use your plugin ID as a prefix for setting keys.

```lua
-- Good
aviqtl.settings.set("com.yourname.plugin.interval", "100")

-- Bad
aviqtl.settings.set("interval", "100")  -- May conflict with other plugins
```

### 4. Handle Errors

Handle errors properly so your plugin doesn't crash.

```lua
function AviQtlOnLoad()
    local ok, err = pcall(function()
        -- Initialization code
    end)
    if not ok then
        aviqtl.log("Initialization failed: " .. err)
    end
end
```

### 5. Provide Documentation

Include a README.md with usage instructions and API requirements.

```markdown
# My Plugin

## Overview
This plugin...

## Required Permissions
- transport.control
- clip.read

## Usage
1. Install the plugin
2. Grant permissions
3. ...
```

## Debugging

### Viewing Logs

Logs are output to AviQtl's console. Use `aviqtl.log()` to output debug information.

### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Permission denied` | Missing permission | Grant permission in Package Manager |
| `controller not ready` | TimelineController not initialized | Open a project first |
| `syntax error` | Lua syntax error | Check code syntax |

## Distribution

### Creating a Repository

1. Create a repository on GitHub/Codeberg
2. Create a release with a ZIP file
3. Add package info to the repository JSON

### Repository JSON Format

```json
{
    "repository_name": "My Repository",
    "packages": [
        {
            "id": "com.yourname.pluginname",
            "display_name": "My Plugin",
            "type": "mod",
            "release_feed": "https://github.com/you/plugin/releases.atom"
        }
    ]
}
```

## References

- [API Reference](API_REFERENCE.md) - Complete API documentation
- [User Guide](USER_GUIDE.md) - End-user guide
- Example plugins: `plugins/example_*/`
