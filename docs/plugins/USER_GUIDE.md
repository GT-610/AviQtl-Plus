# AviQtl Plugin User Guide

This guide explains how to install, manage, and use plugins in AviQtl.

## What Are Plugins

Plugins are scripts that extend AviQtl's functionality. Written in the Lua programming language, they can:

- Automate timeline operations
- Retrieve and modify project information
- Create custom workflows
- Persist settings across sessions

## Installing Plugins

### Method 1: Using the Package Manager

1. Open **Menu** → **Tools** → **Package Manager**
2. Click **Sync Repository** to fetch the package list
3. Click the **Install** button for the plugin you want to install
4. Select the file to download if prompted

### Method 2: Manual Installation

1. Copy plugin files to AviQtl's `plugins/` directory
2. Restart AviQtl, or if hot reload is enabled, plugins load automatically

### Directory Structure

```text
plugins/
├── example_transport/      # Directory-based plugin
│   ├── manifest.lua       # Plugin metadata
│   └── main.lua           # Main code
├── simple_plugin.lua      # Single-file plugin
└── another_plugin/
    ├── manifest.lua
    ├── main.lua
    └── utils.lua          # Additional script files
```

## Managing Plugins

### Package Manager

The Package Manager allows you to:

- **Install**: Download and install plugins
- **Update**: Upgrade to newer versions when available
- **Remove**: Uninstall plugins
- **Permissions**: Configure which permissions plugins are granted

### Permission System

AviQtl implements a permission-based access control system for security. New plugins have no permissions by default.

#### Available Permissions

| Permission | Description |
|------------|-------------|
| Transport Control | Play, pause, seek |
| Clip Read | List and read clip information |
| Clip Modify | Create, delete, move clips |
| Effect Read | List effect information |
| Effect Modify | Add, remove, modify effects |
| Project Read | Get resolution, FPS, etc. |
| Project Save | Save project files |
| Project Load | Load project files |
| Scene Manage | Create, remove, switch scenes |
| Settings Read | Read plugin settings |
| Settings Write | Save plugin settings |
| Clipboard | Copy, cut, paste operations |
| Log Output | Write to console log |

#### Managing Permissions

1. Open the **Package Manager**
2. Find the installed plugin
3. Click the **Permissions** button
4. Check the permissions you want to grant
5. Click **OK** to save

## Hot Reload

When hot reload is enabled, AviQtl detects changes to plugin files and automatically reloads them.

### Enabling Hot Reload

1. Open **Settings** → **Plugins**
2. Check **Enable Hot Reload**

### Notes

- Hot reload is recommended for development only
- For production use, disable it for stability
- You can also restart AviQtl after making plugin changes

## Built-in Plugins

AviQtl includes the following example plugins:

### example_transport

Demonstrates the Transport Control API. Shows how to use playback controls.

```lua
-- Usage examples
aviqtl.transport.play()    -- Start playback
aviqtl.transport.pause()   -- Pause playback
aviqtl.transport.toggle()  -- Toggle play/pause
```

### example_clip_ops

Demonstrates the Clip Operations API. Shows how to manipulate clips.

```lua
-- Usage examples
local clips = aviqtl.clip.list()  -- List all clips
aviqtl.clip.create("text", 0, 0)  -- Create text clip
```

### example_project_info

Demonstrates the Project Info API. Shows how to access project information and settings.

```lua
-- Usage examples
local width = aviqtl.project.width()
local height = aviqtl.project.height()
aviqtl.settings.set("my_key", "my_value")
```

## Troubleshooting

### Plugin Not Loading

1. Verify files exist in the `plugins/` directory
2. Check for Lua syntax errors (errors appear in the console)
3. Try restarting AviQtl

### Permission Errors

1. Check plugin permissions in the Package Manager
2. Ensure all required permissions are granted
3. Reload the plugin after changing permissions

### Performance Issues

1. Reduce work in `AviQtlUpdateHook`
2. Disable unnecessary plugins
3. Disable hot reload and restart

## For Developers

See the [Developer Guide](DEVELOPER_GUIDE.md) for information on creating plugins.

## API Reference

See the [API Reference](API_REFERENCE.md) for detailed API documentation.
