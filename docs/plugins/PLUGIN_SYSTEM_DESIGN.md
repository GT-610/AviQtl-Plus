# AviQtl Plugin System Design

This document describes the complete design of the AviQtl plugin system, including architecture, scripting model, parameter system, permissions, and package management.

## Directory Structure

```
plugins/
├── example_transport/          # Directory-based plugin (recommended)
│   ├── manifest.lua           # Plugin metadata
│   └── main.lua               # Main entry point
├── example_animation/
│   ├── manifest.lua
│   └── main.lua               # With declarative parameters
├── simple.lua                 # Single-file plugin (optional)
└── placeholder
```

## manifest.lua - Metadata Declaration

Every directory-based plugin requires a `manifest.lua` file:

```lua
return {
    id = "com.aviqtl.example.transport",   -- Unique ID (reverse domain notation)
    name = "Transport Control Example",     -- Display name
    version = "1.0.0",                      -- Semantic version
    author = "AviQtl Team",                -- Author name
    description = "Demo plugin",            -- Short description
    min_app_version = "0.2.0"              -- Minimum AviQtl version required
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Unique identifier. Use reverse domain notation |
| `name` | Yes | Display name shown to users |
| `version` | Yes | Semantic version (x.y.z) |
| `author` | No | Author name |
| `description` | No | Brief description |
| `min_app_version` | No | Minimum AviQtl version required |

## Script Parameters (Declarative UI)

AviQtl supports AviUtl-style declarative parameter definitions in script headers. Parameters are automatically exposed as Lua global variables and their values persist across sessions.

### Parameter Types

#### Track (Slider)
```lua
--track@varname:label,min,max,default[,step]
--track@speed:Speed,-100,100,0
--track@scale:Scale,0,10,1,0.01
```

#### Check (Checkbox)
```lua
--check@varname:label,default
--check@visible:Visible,true
--check@debug:Debug Mode,false
```

#### Color
```lua
--color@varname:label,default
--color@tint:Tint Color,0xff0000
--color@bg:Background,0x000000
```

#### Select (Dropdown)
```lua
--select@varname:label=default,option1=value1,option2=value2
--select@mode:Mode=fast,fast=0,normal=1,slow=2
--select@blend:Blend=add,add=subtract=multiply=screen
```

#### Text (Multi-line)
```lua
--text@varname:label,default
--text@content:Content,Default text here
```

#### String (Single-line)
```lua
--string@varname:label,default
--string@name:Name,Default Name
```

#### File Selection
```lua
--file@varname:label
--file@image:Image File
```

#### Folder Selection
```lua
--folder@varname:label
--folder@output:Output Directory
```

#### Value (Generic)
```lua
--value@varname:label,default
--value@count:Count,0
--value@data:Data,{1,2,3}
```

### Groups

Organize parameters into collapsible groups:

```lua
--group:Position
--track@pos_x:X,-1000,1000,0
--track@pos_y:Y,-1000,1000,0

--group:Appearance
--color@color:Color,0xffffff
--check@visible:Visible,true

--group:  -- End group
```

### Metadata Directives

```lua
--information:Script Name v1.0 by Author
--script:lua        -- or --script:luajit (default)
--require:2003500   -- Minimum app version
--filter            -- Mark as filter object
--label:Category    -- Menu label
```

### Complete Example

```lua
--group:Movement
--track@speed_x:X Speed,-10,10,0
--track@speed_y:Y Speed,-10,10,0

--group:Style
--color@color:Color,0xff0000
--check@bounce:Bounce,false
--select@shape:Shape=circle,circle=square=triangle

function AviQtlUpdateHook()
    -- speed_x, speed_y, color, bounce, shape are automatically available
    aviqtl.log("Speed: " .. speed_x .. ", " .. speed_y)
end
```

### Notes

- Parameters are injected as global Lua variables before script execution
- Values persist across sessions (saved in application settings)
- Use `--group:name` to organize parameters, `--group:` to end a group
- Step value for track is optional (auto-detected from range)

## Lifecycle Hooks

Plugins can define these global functions to respond to application events:

| Function | Arguments | Description |
|----------|-----------|-------------|
| `AviQtlOnLoad()` | None | Called after plugin is loaded |
| `AviQtlOnUnload()` | None | Called when plugin is unloaded |
| `AviQtlUpdateHook()` | None | Called approximately every 16ms |
| `AviQtlOnProjectOpen(path)` | `path`: string | Called when project is opened |
| `AviQtlOnProjectSave(path)` | `path`: string | Called when project is saved |
| `AviQtlOnClipChange()` | None | Called when clips are modified |

### Example

```lua
function AviQtlOnLoad()
    aviqtl.log("Plugin loaded!")
end

function AviQtlUpdateHook()
    local frame = aviqtl.transport.get_frame()
    -- Periodic tasks
end

function AviQtlOnProjectOpen(path)
    aviqtl.log("Project opened: " .. path)
end
```

## API Reference

### Transport Control

```lua
aviqtl.transport.play()                     -- Start playback
aviqtl.transport.pause()                    -- Pause playback
aviqtl.transport.toggle()                   -- Toggle play/pause
aviqtl.transport.seek(frame)                -- Seek to frame
aviqtl.transport.get_frame()                -- Get current frame (integer)
aviqtl.transport.is_playing()               -- Check if playing (0 or 1)
```

### Clip Operations

```lua
aviqtl.clip.create(type, startFrame, layer) -- Create clip
aviqtl.clip.delete(clipId)                  -- Delete clip
aviqtl.clip.update(clipId, layer, start, duration) -- Update clip
aviqtl.clip.select(clipId)                  -- Select clip
aviqtl.clip.split(clipId, frame)            -- Split clip
aviqtl.clip.copy(clipId)                    -- Copy to clipboard
aviqtl.clip.cut(clipId)                     -- Cut to clipboard
aviqtl.clip.paste(clipId, layer)            -- Paste from clipboard
aviqtl.clip.list()                          -- List all clips (table)
```

### Effect Operations

```lua
aviqtl.effect.add(clipId, effectId)         -- Add effect
aviqtl.effect.remove(clipId, effectIndex)   -- Remove effect
aviqtl.effect.set_param(clipId, index, key, value) -- Set parameter
```

### Project Information

```lua
aviqtl.project.width()                      -- Get width (integer)
aviqtl.project.height()                     -- Get height (integer)
aviqtl.project.fps()                        -- Get FPS (number)
aviqtl.project.save(path)                   -- Save project (boolean)
aviqtl.project.load(path)                   -- Load project (boolean)
```

### Scene Operations

```lua
aviqtl.scene.create(name)                   -- Create scene
aviqtl.scene.remove(sceneId)                -- Remove scene
aviqtl.scene.switch(sceneId)                -- Switch scene
```

### Settings (Persistent)

```lua
aviqtl.settings.set(key, value)             -- Save setting
aviqtl.settings.get(key)                    -- Load setting (string)
```

### Undo/Redo

```lua
aviqtl.undo()                               -- Undo last action
aviqtl.redo()                               -- Redo last undone action
aviqtl.command.begin_group(text)            -- Begin grouped operation
aviqtl.command.end_group()                  -- End grouped operation
```

### Logging

```lua
aviqtl.log(message)                         -- Log to console
```

## Permission System

AviQtl implements a permission-based security model. Plugins must be granted specific permissions to access certain APIs.

### Available Permissions

| Permission | Description | Required APIs |
|------------|-------------|---------------|
| `transport.control` | Playback control | `transport.*` |
| `clip.read` | Read clip info | `clip.list`, `clip.select` |
| `clip.modify` | Modify clips | `clip.create`, `clip.delete`, `clip.update`, `clip.split` |
| `effect.read` | Read effects | (reserved) |
| `effect.modify` | Modify effects | `effect.*` |
| `project.read` | Read project | `project.width`, `project.height`, `project.fps` |
| `project.save` | Save projects | `project.save` |
| `project.load` | Load projects | `project.load` |
| `scene.manage` | Manage scenes | `scene.*` |
| `settings.read` | Read settings | `settings.get` |
| `settings.write` | Write settings | `settings.set` |
| `clipboard.access` | Clipboard ops | `clip.copy`, `clip.cut`, `clip.paste` |
| `log.output` | Console log | `log` |

### Behavior

- New plugins have **no permissions** by default
- Users grant permissions via **Package Manager → Permissions** button
- Calling API without permission raises Lua error: "Permission denied"
- Permissions persist in application settings

### Error Handling

```lua
local ok, err = pcall(function()
    aviqtl.transport.play()
end)
if not ok then
    aviqtl.log("Permission denied: " .. err)
end
```

## Hot Reload

When enabled in Settings → Plugins, AviQtl monitors the `plugins/` directory for changes and automatically reloads plugins.

- Recommended for development only
- Disable for production stability
- Changes to `manifest.lua` require restart

## Package Management

### Installation Methods

1. **Package Manager**: Sync repository, click Install
2. **Manual**: Copy files to `plugins/` directory

### Repository Format

Packages are hosted on GitHub/Codeberg with release feeds:

```json
{
    "repository_name": "My Repository",
    "packages": [
        {
            "id": "com.example.plugin",
            "display_name": "My Plugin",
            "type": "mod",
            "release_feed": "https://github.com/user/repo/releases.atom"
        }
    ]
}
```

### Package Types

| Type | Deploy Directory |
|------|-----------------|
| `mod` | `plugins/` |
| `effect` | `effects/` |
| `object` | `objects/` |

## Comparison with AviUtl

| Aspect | AviUtl | AviQtl |
|--------|--------|--------|
| Script Binding | Bound to objects, executes per-frame | Global plugins, API-based |
| UI Definition | Comment declarations (adopted) | Comment declarations (implemented) |
| Rendering | Direct `obj.draw()` | Effect system |
| Permissions | None | 13 granular permissions |
| Package Mgmt | Manual copy | Full package manager |
| Platform | Windows only | Linux/Windows/macOS |
| Encoding | AviUtl1: SJIS, AviUtl2: UTF-8 | UTF-8 |
| Lua Version | Lua 5.1 / LuaJIT | LuaJIT |

## Implementation Files

| File | Purpose |
|------|---------|
| `scripting/mod_engine.hpp/cpp` | Plugin loading, lifecycle, API registration |
| `scripting/script_params.hpp/cpp` | Declarative parameter parser |
| `core/include/permission_manager.hpp` | Permission system |
| `core/include/package_manager.hpp` | Package download/deploy |
| `ui/qml/PackageManagerWindow.qml` | Package manager UI |
| `ui/qml/PluginPermissionDialog.qml` | Permission management UI |
| `docs/plugins/` | Documentation |
| `plugins/example_*/` | Example plugins |
| `tests/test_permission_manager.cpp` | Permission tests |
| `tests/test_plugin_manifest.cpp` | Manifest parsing tests |
| `tests/test_package_deploy.cpp` | Package deploy tests |
