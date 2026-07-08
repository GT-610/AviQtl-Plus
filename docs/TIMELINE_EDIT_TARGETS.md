# Timeline Edit Target Rules

This document defines where timeline commands act. AviQtl-Plus has to feel
familiar to AviUtl-style object editing while also supporting modern NLE and
After Effects-style workflows. The main rule is that selection answers "what is
edited", while edit targets answer "where the edit happens".

## Target Types

- **Pointer target**: right-click menus, drag-and-drop, and direct mouse tools
  use the explicit frame and layer under the pointer. These actions do not
  depend on the playhead, selected layer, or skimmer.
- **Clip edit target**: keyboard and menu commands that place or cut clips use
  the skimmer when it is visible in the active timeline window. Otherwise they
  use the playhead frame and the selected layer.
- **Animation edit target**: keyframes and parameter animation use the playhead
  and current selected object/property. They do not use the hover skimmer.
- **Selection target**: delete, copy, cut, property editing, and similar
  operations act on the selected clips. Context menu commands may act on the
  clicked clip when it is outside the selection.
- **Layer target**: pointer actions use the clicked or dropped layer. Keyboard
  and menu clip commands use the selected layer unless an active skimmer
  supplies a layer.

## Command Expectations

- Right-click add, paste, split, and duplicate commands use the right-clicked
  frame and layer.
- Dropping media uses the drop frame and layer, then advances the edit target to
  the end of the inserted media.
- Timeline shortcuts such as paste, duplicate, and split use the active
  skimmer when the timeline window is focused. If the timeline window is not
  active, they fall back to the playhead and selected layer.
- Insertions may be moved forward by collision avoidance. The returned actual
  frame becomes the basis for advancing the edit target.
- Keyframe creation and parameter changes use the playhead-relative frame of
  the selected clip. Hovering elsewhere in the timeline must not silently move
  the animation edit point.

## Manual Acceptance

- Right-click an empty layer and add an object: it appears at the clicked frame
  and layer.
- Right-click an object and split it: the split uses the clicked frame on that
  object.
- Drag media onto the timeline: it imports at the drop position and advances
  the target to the imported clip end.
- Hover the active timeline and press paste: the paste uses the skimmer frame
  and layer.
- Activate another window and trigger paste from a global menu: the paste uses
  the playhead and selected layer, not the stale skimmer.
- Add or edit a keyframe while the pointer hovers elsewhere: the keyframe uses
  the playhead-relative frame.
