# Example Lua Plugin: Hello Mod

A minimal Lua plugin demonstrating the plugin lifecycle, API usage,
and persistent settings.

## Files

| File | Description |
|------|-------------|
| `manifest.lua` | Plugin metadata: id, name, version, author, description |
| `main.lua` | Plugin logic: lifecycle hooks and custom commands |

## How It Works

### Manifest (`manifest.lua`)

Returns a table with plugin metadata. All fields are required except
`min_app_version`.

```lua
return {
    id = "com.example.hello_mod",
    name = "Hello Mod",
    version = "1.0.0",
    author = "Example",
    description = "Minimal example",
    min_app_version = "0.3.0"
}
```

### Main (`main.lua`)

Defines lifecycle hooks and custom functions.

#### Lifecycle Hooks

| Hook | When Called | Use For |
|------|-----------|---------|
| `AviQtlOnLoad()` | Plugin loaded at startup | Initialization, loading state |
| `AviQtlUpdateHook(frame)` | Every frame during playback | Real-time monitoring (keep lightweight!) |
| `AviQtlOnProjectSave(path)` | User saves the project | Saving plugin state, logging |

#### APIs

All APIs are accessed through the `aviqtl` global table:

| API | Purpose |
|-----|---------|
| `aviqtl.log(msg)` | Write to the application log |
| `aviqtl.project.width/height/fps()` | Project properties |
| `aviqtl.clip.list()` | List all clips in the timeline |
| `aviqtl.settings.get/set(key, val)` | Persistent key-value storage |
| `aviqtl.scene.create(name)` | Create a new scene |
| `aviqtl.transport.play/pause/stop()` | Playback control |

See [API_REFERENCE.md](../../docs/plugins/API_REFERENCE.md) for the
complete API documentation.

### Permissions

New plugins have **zero permissions** by default. The user must grant
permissions via the Package Manager UI. This plugin needs:

- **project:read** — to read project properties
- **settings:read/write** — to save/load persistent settings
- **clip:read** — to list clips
- **log** — to write to the application log

Attempting to call an API without the required permission raises a
Lua error.

## Installation

1. Copy the `hello_mod/` directory to your AviQtl plugins directory:
   ```
   <AviQtl data dir>/plugins/hello_mod/
   ```

2. Restart AviQtl. The plugin loads automatically.

3. Grant permissions in the Package Manager if prompted.

4. Open the Lua console and type `hello()` to test.

## Creating Your Own Plugin

1. Copy this directory and rename it
2. Update `manifest.lua` with your plugin's metadata
3. Add lifecycle hooks and custom functions in `main.lua`
4. Set appropriate permissions for your plugin's needs

### Tips

- **Keep `AviQtlUpdateHook` lightweight** — it runs every frame
- **Use `aviqtl.settings`** for persistent state (survives restarts)
- **Prefix log messages** with your plugin name: `[My Plugin] ...`
- **Handle nil gracefully** — APIs may return nil if permissions are denied
- **Use `aviqtl.log`** for debugging — it writes to the app log

### Distribution

Plugins can be distributed via the package manager:

```json
{
    "id": "com.example.hello_mod",
    "display_name": "Hello Mod",
    "type": "mod",
    "release_feed": "https://github.com/you/hello-mod/releases.atom",
    "repository_url": "https://github.com/you/hello-mod"
}
```

See [DEVELOPER_GUIDE.md](../../docs/plugins/DEVELOPER_GUIDE.md) for
the complete plugin development guide.
