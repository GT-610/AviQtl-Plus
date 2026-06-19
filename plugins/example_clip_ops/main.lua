-- Clip Operations Example Plugin
-- This plugin demonstrates how to use the AviQtl clip API

-- Called when plugin is loaded
function AviQtlOnLoad()
    aviqtl.log("[Clip Ops Example] Plugin loaded!")
end

-- List all clips in the current scene
function list_all_clips()
    local clips = aviqtl.clip.list()
    aviqtl.log("[Clip Ops Example] Found " .. #clips .. " clips:")
    for i, clip in ipairs(clips) do
        aviqtl.log(string.format("  [%d] ID=%d Type=%s Layer=%d Start=%d Duration=%d",
            i, clip.id, clip.type, clip.layer, clip.startFrame, clip.duration))
    end
    return clips
end

-- Create a text clip at the beginning of layer 0
function create_text_clip()
    aviqtl.clip.create("text", 0, 0)
    aviqtl.log("[Clip Ops Example] Created text clip at frame 0, layer 0")
end

-- Delete a clip by ID
function delete_clip_by_id(clip_id)
    if clip_id == nil then
        aviqtl.log("[Clip Ops Example] Usage: delete_clip_by_id(clip_id)")
        return
    end
    aviqtl.clip.delete(clip_id)
    aviqtl.log("[Clip Ops Example] Deleted clip: " .. clip_id)
end

-- Move a clip to a different layer
function move_clip_to_layer(clip_id, new_layer)
    if clip_id == nil or new_layer == nil then
        aviqtl.log("[Clip Ops Example] Usage: move_clip_to_layer(clip_id, new_layer)")
        return
    end
    -- Get current clip info
    local clips = aviqtl.clip.list()
    for _, clip in ipairs(clips) do
        if clip.id == clip_id then
            aviqtl.clip.update(clip_id, new_layer, clip.startFrame, clip.duration)
            aviqtl.log(string.format("[Clip Ops Example] Moved clip %d to layer %d", clip_id, new_layer))
            return
        end
    end
    aviqtl.log("[Clip Ops Example] Clip not found: " .. clip_id)
end

-- Duplicate a clip
function duplicate_clip(clip_id)
    if clip_id == nil then
        aviqtl.log("[Clip Ops Example] Usage: duplicate_clip(clip_id)")
        return
    end
    aviqtl.clip.copy(clip_id)
    local clips = aviqtl.clip.list()
    for _, clip in ipairs(clips) do
        if clip.id == clip_id then
            aviqtl.clip.paste(clip_id, clip.layer + 1)
            aviqtl.log(string.format("[Clip Ops Example] Duplicated clip %d to layer %d", clip_id, clip.layer + 1))
            return
        end
    end
end

-- Called when project is opened
function AviQtlOnProjectOpen(path)
    aviqtl.log("[Clip Ops Example] Project opened, listing clips:")
    list_all_clips()
end

aviqtl.log("[Clip Ops Example] Commands available:")
aviqtl.log("  list_all_clips()                - List all clips")
aviqtl.log("  create_text_clip()              - Create a text clip")
aviqtl.log("  delete_clip_by_id(id)           - Delete a clip")
aviqtl.log("  move_clip_to_layer(id, layer)   - Move clip to layer")
aviqtl.log("  duplicate_clip(id)              - Duplicate a clip")
