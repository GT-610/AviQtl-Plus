# Large-Project Performance Baseline

This document defines repeatable performance scenarios for timeline work. The
fixtures protect editing behavior and print measurements for comparison; they
do not use tight wall-clock pass/fail limits because developer and CI hardware
vary substantially.

## Large Timeline Fixture

The `large_timeline_performance` CTest creates a deterministic scene with:

- 5,000 clips across 50 layers.
- Ten-frame clips separated by ten-frame gaps on each layer.
- 10,000 deterministic clip-ID lookups spread across the scene.
- Selection sizes of 1, 10, 50, 100, 500, and 1,000 clips, each moved as one
  undoable operation against the same populated scene.

The test records elapsed time for:

- Materializing the complete `TimelineController::clips()` snapshot exposed to
  QML.
- Looking up clips distributed across the full timeline.
- Moving each selection size and applying undo and redo, exposing how operation
  cost changes from ordinary edits through stress-scale batch work.
- Creating delegates through the real `TimelineView.qml`, scrolling within and
  beyond its loaded range, and retaining an off-screen clip during a drag.

It also verifies the snapshot contents and the exact move, undo, and redo
results. Run it directly with verbose output when comparing a performance
change:

```text
ctest -R large_timeline_performance -V
```

Compare the same build type on the same machine, repeat the test several times,
and disregard the first run if process startup or cold filesystem caches make
it an outlier. Record the before and after medians in the pull request.

## First Finding

The initial fixture showed that redoing a 100-clip move repeatedly scanned the
selected-ID list while checking every candidate clip for collisions. Replacing
that nested list scan with set membership preserved the move and undo/redo
results while reducing the observed redo measurement from 709 ms to a repeated
63-108 ms range on the same Windows development build.

The complete QML snapshot and distributed ID lookup measurements did not
materially improve. They remain separate evidence for future snapshot caching,
and indexed lookup rather than justification for expanding the collision-check
optimization.

## Viewport Virtualization

The timeline now queries clips intersecting the visible frame and layer range,
plus a 160-pixel horizontal and one-layer vertical buffer. Scrolling inside the
buffer does not rebuild the delegate model. Selected clips are retained even
when their positions are outside the loaded range, so beginning a drag does not
replace the delegate holding the pointer grab.

On the same Windows development build, the 5,000-clip fixture changed from
creating all 5,000 `ClipItem` delegates in about 5.0 seconds to creating 696 in
about 1.9 seconds. The exact timings are informational, while the automated test
requires the delegate count to remain below 800 for its fixed viewport.

## Next Measurements

The fixture now covers the model/controller baseline and real QML delegate
virtualization. Separate measurements are still needed for continuous scroll
and zoom frame timing, decoder seek/cache pressure, and long-audio decoding.
