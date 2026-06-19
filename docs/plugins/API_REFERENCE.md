# AviQtl Plugin API Reference

This document describes the Lua API available for AviQtl plugins.

## Plugin Structure

Each plugin is a directory under `plugins/` containing:
- `manifest.lua` - Plugin metadata (required for directory-based plugins)
- `main.lua` - Main plugin code (required for directory-based plugins)
- Other `.lua` files as needed

Alternatively, single `.lua` files can be placed directly in the `plugins/` directory.

## Manifest Format

```lua
return {
    id = "com.example.myplugin",        -- Unique plugin identifier
    name = "My Plugin",                 -- Display name
    version = "1.0.0",                  -- Semantic version
    author = "Your Name",              -- Author name
    description = "Plugin description", -- Short description
    min_app_version = "0.2.0"          -- Minimum AviQtl version required
}
```

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

### Example

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

## Permission System

AviQtl implements a permission-based security model. Plugins must be granted specific permissions to access certain APIs. Permissions can be managed through the Package Manager UI.

### Available Permissions

| Permission | Description | Required for APIs |
|------------|-------------|-------------------|
| `transport.control` | Playback control | `transport.*` |
| `clip.read` | Read clip information | `clip.list`, `clip.select` |
| `clip.modify` | Modify clips | `clip.create`, `clip.delete`, `clip.update`, `clip.split` |
| `effect.read` | Read effect information | (reserved) |
| `effect.modify` | Modify effects | `effect.add`, `effect.remove`, `effect.set_param` |
| `project.read` | Read project info | `project.width`, `project.height`, `project.fps` |
| `project.save` | Save projects | `project.save` |
| `project.load` | Load projects | `project.load` |
| `scene.manage` | Manage scenes | `scene.create`, `scene.remove`, `scene.switch` |
| `settings.read` | Read plugin settings | `settings.get` |
| `settings.write` | Write plugin settings | `settings.set` |
| `clipboard.access` | Clipboard operations | `clip.copy`, `clip.cut`, `clip.paste` |
| `log.output` | Console logging | `log` |

### Permission Behavior

- If a plugin lacks permission for an API call, a Lua error is raised with "Permission denied"
- New plugins have no permissions by default
- Users must explicitly grant permissions through the Package Manager
- Permissions are persisted in application settings

## Lifecycle Hooks

Plugins can define these global functions to respond to application events:

### AviQtlOnLoad()
Called once after the plugin is loaded and all APIs are initialized.

```lua
function AviQtlOnLoad()
    aviqtl.log("Plugin loaded!")
end
```

### AviQtlOnUnload()
Called when the plugin is being unloaded (currently only on application shutdown).

### AviQtlUpdateHook()
Called approximately every 16ms (60 times per second). Use for periodic tasks.

```lua
function AviQtlUpdateHook()
    local frame = aviqtl.transport.get_frame()
    -- Do something every frame
end
```

### AviQtlOnProjectOpen(path)
Called when a project file is opened.

- `path` (string) - Path to the opened project file

### AviQtlOnProjectSave(path)
Called when the project is saved.

- `path` (string) - Path where the project was saved

### AviQtlOnClipChange()
Called when clips are modified (creation, deletion, movement, etc.)

## API Reference

### Transport Control

#### aviqtl.transport.play()
Start playback.

#### aviqtl.transport.pause()
Pause playback.

#### aviqtl.transport.toggle()
Toggle between play and pause.

#### aviqtl.transport.seek(frame)
Seek to a specific frame.

- `frame` (integer) - Target frame number

#### aviqtl.transport.get_frame()
Returns the current frame number.

- Returns: integer

#### aviqtl.transport.is_playing()
Returns whether playback is active.

- Returns: boolean (0 or 1)

### Clip Operations

#### aviqtl.clip.create(type, startFrame, layer)
Create a new clip.

- `type` (string) - Clip type (e.g., "video", "audio", "image", "text", "rect")
- `startFrame` (integer) - Starting frame
- `layer` (integer) - Target layer

#### aviqtl.clip.delete(clipId)
Delete a clip by ID.

- `clipId` (integer) - ID of the clip to delete

#### aviqtl.clip.update(clipId, layer, startFrame, duration)
Update clip properties.

- `clipId` (integer) - Clip ID
- `layer` (integer) - New layer (-1 to keep current)
- `startFrame` (integer) - New start frame (-1 to keep current)
- `duration` (integer) - New duration (-1 to keep current)

#### aviqtl.clip.select(clipId)
Select a clip.

- `clipId` (integer) - ID of the clip to select

#### aviqtl.clip.split(clipId, frame)
Split a clip at a specific frame.

- `clipId` (integer) - Clip ID
- `frame` (integer) - Frame to split at

#### aviqtl.clip.copy(clipId)
Copy a clip to clipboard.

- `clipId` (integer) - Clip ID

#### aviqtl.clip.cut(clipId)
Cut a clip to clipboard.

- `clipId` (integer) - Clip ID

#### aviqtl.clip.paste(clipId, layer)
Paste clipboard content.

- `clipId` (integer) - Reference clip ID for positioning
- `layer` (integer) - Target layer

#### aviqtl.clip.list()
List all clips in the current scene.

- Returns: Table of clip objects with fields: `id`, `type`, `layer`, `startFrame`, `duration`

### Effect Operations

#### aviqtl.effect.add(clipId, effectId)
Add an effect to a clip.

- `clipId` (integer) - Target clip ID
- `effectId` (string) - Effect type ID

#### aviqtl.effect.remove(clipId, effectIndex)
Remove an effect from a clip.

- `clipId` (integer) - Target clip ID
- `effectIndex` (integer) - Effect index (0-based)

#### aviqtl.effect.set_param(clipId, effectIndex, key, value)
Set an effect parameter.

- `clipId` (integer) - Target clip ID
- `effectIndex` (integer) - Effect index (0-based)
- `key` (string) - Parameter name
- `value` (number, boolean, or string) - Parameter value

### Project Information

#### aviqtl.project.width()
Returns the project width in pixels.

- Returns: integer

#### aviqtl.project.height()
Returns the project height in pixels.

- Returns: integer

#### aviqtl.project.fps()
Returns the project frames per second.

- Returns: number

#### aviqtl.project.save(path)
Save the project to a file.

- `path` (string) - File path
- Returns: boolean (true on success)

#### aviqtl.project.load(path)
Load a project from a file.

- `path` (string) - File path
- Returns: boolean (true on success)

### Scene Operations

#### aviqtl.scene.create(name)
Create a new scene.

- `name` (string) - Scene name

#### aviqtl.scene.remove(sceneId)
Remove a scene.

- `sceneId` (integer) - Scene ID

#### aviqtl.scene.switch(sceneId)
Switch to a different scene.

- `sceneId` (integer) - Scene ID

### Settings

#### aviqtl.settings.set(key, value)
Save a persistent setting.

- `key` (string) - Setting key
- `value` (string) - Setting value

#### aviqtl.settings.get(key)
Retrieve a persistent setting.

- `key` (string) - Setting key
- Returns: string (empty if not found)

### Undo/Redo

#### aviqtl.undo()
Undo the last action.

#### aviqtl.redo()
Redo the last undone action.

### Command Grouping

#### aviqtl.command.begin_group(text)
Begin a grouped undo/redo operation.

- `text` (string) - Description for the undo history

#### aviqtl.command.end_group()
End a grouped undo/redo operation.

### Utility

#### aviqtl.log(message)
Log a message to the AviQtl console.

- `message` (string) - Message to log

## Best Practices

1. **Use unique plugin IDs**: Follow reverse domain notation (e.g., `com.yourname.plugin`)
2. **Keep hooks lightweight**: The `AviQtlUpdateHook` runs frequently; avoid heavy computations
3. **Save settings with namespaced keys**: Use your plugin ID as prefix (e.g., `com.example.myplugin.setting`)
4. **Handle errors gracefully**: Lua errors in hooks will be logged but won't crash the application
5. **Test with different project sizes**: Ensure your plugin works with various resolutions and frame rates

## Example Plugin

See the `plugins/` directory for complete examples:
- `example_transport/` - Transport control demonstration
- `example_clip_ops/` - Clip manipulation demonstration
- `example_project_info/` - Project info and settings demonstration
