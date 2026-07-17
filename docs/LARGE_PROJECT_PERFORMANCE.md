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
- A 100-clip selection moved as one undoable operation.

The test records elapsed time for:

- Materializing the complete `TimelineController::clips()` snapshot exposed to
  QML.
- Looking up clips distributed across the full timeline.
- Moving the selection and applying undo and redo.

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
indexed lookup, and viewport-model work rather than justification for expanding
this optimization.

## Next Measurements

The C++ fixture establishes the model/controller baseline. Separate fixtures
are still needed for viewport scrolling and zooming, QML delegate creation,
decoder seek/cache pressure, and long-audio decoding. Timeline rendering
changes should preserve the editing behavior covered here while reducing work
for clips outside the visible frame and layer range.
