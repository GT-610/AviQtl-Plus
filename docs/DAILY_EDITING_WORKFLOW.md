# Daily Editing Workflow

This document defines the first end-to-end workflow AviQtl-Plus uses to judge
whether the editor is practical for everyday AviUtl-style work.

The workflow is intentionally small. It should be easy to run manually, and the
model-level parts should stay covered by automated tests.

## Scenario

Create a short project, place media and text on the timeline, animate a
parameter, save the project, reopen it, and confirm the editing state survived.

## Project Setup

Use these settings for the baseline workflow:

- Width: `1280`
- Height: `720`
- Frame rate: `30`
- Audio sample rate: `48000`
- Scene duration: at least `150` frames

The editor should expose these values before editing begins and should preserve
them after saving and reopening the project.

## Editing Steps

1. Create a new project with the baseline settings.
2. Import an image or video file at frame `0`, layer `0`.
3. Add a text object at frame `30`, layer `2`.
4. Change the text content to a recognizable title such as `Daily Edit`.
5. Adjust transform parameters on the text object:
   - `x`: `120`
   - `opacity`: `0.75`
6. Add keyframes to the text transform:
   - `x = 0` at frame `0`, linear interpolation
   - `x = 120` at frame `30`, linear interpolation
7. Add one visual effect such as Blur, set `size` to `12`, and confirm the effect
   remains enabled in the stack.
8. Save the project.
9. Reopen the saved project in a fresh project state.
10. Confirm project settings, clip timing, layer placement, media path, text
    parameters, effect order, enabled state, and keyframes match the saved edit.

## Missing Media Recovery

A project with moved or deleted image, video, or audio files must still open.
The editor shows a non-blocking missing-media notice; use **File > Manage Missing
Media** (or its **Manage** button) to replace each item with a file of the same
media type. Save the project after relinking.

For manual acceptance, remove one imported media file after saving, reopen the
project, verify that the missing item is listed without blocking the project,
replace it, then use one undo and redo to confirm the old missing path and the
replacement path are both restored correctly.

## Manual Preview Acceptance

After the edit is created:

- Seeking to frame `0` shows the imported media and the text at its initial
  keyframe state.
- Seeking to frame `30` shows the text at the later keyframe state.
- Pressing Space toggles playback when text input is not focused.
- Timeline interaction remains responsive while the preview updates.

These checks currently remain manual because preview correctness depends on the
QML `CompositeView` rendering path.

## Manual Export Acceptance

Export is part of the daily workflow. Configuration and missing-preview failure
paths are automated, while rendered-frame correctness remains manual until there
is a renderer-level test harness that can capture frames without a live QML view.

Automated export acceptance covers:

- Empty output paths and invalid frame ranges fail as configuration errors.
- Export FPS must match the project FPS.
- Missing preview capture surfaces fail as frame-capture errors before creating
  encoder output files or image-sequence frames.
- Preview items that cannot produce a frame fail explicitly instead of silently
  substituting black frames, and partial video or image-sequence output is removed.
- Image-sequence export refuses to overwrite an existing frame file.

For manual acceptance:

- Open the export dialog with the baseline project.
- Export frames `0` through `60` to a video file using the default software or
  available hardware encoder.
- Confirm the export completes successfully.
- Confirm the output file is created and has non-zero size.
- If export fails, the error should name the failed stage clearly enough for the
  user to decide whether the issue is configuration, encoding, or frame capture.

## Automated Coverage

The `daily_editing_workflow` CTest covers the model-level path:

- Project settings are set explicitly.
- A media asset is imported through `TimelineController::importMediaFile`.
- A text object is created through the same controller/service model used by the
  UI.
- Parameters, effect stack state, and keyframes are changed through normal
  editing APIs.
- The project is saved through `saveProject`, reopened through `loadProject`,
  and checked for preserved project and timeline state.
- The `missing_media` CTest covers detection after a saved project's media files
  are removed, type-safe per-item relinking, and undo/redo of a relink.

This test intentionally does not automate QML preview capture or video export.
Those should be added as a later workflow layer rather than hidden inside a
model-level serializer test.

The `qml_composite_capture` CTest covers the first renderer-level layer: it
loads the real `CompositeView`, renders its `View3D` at an export-sized logical
resolution, and captures it through `grabToImage`. It intentionally uses the
empty scene while richer text, effect, keyframe, and encoded-output fixtures are
added as later layers.

The `export_workflow` CTest covers the service-level export path:

- Video and image-sequence export configuration is validated before export work
  starts.
- Missing QML capture surfaces fail before partial output is produced.
- Failure messages identify configuration and frame-capture stages.
