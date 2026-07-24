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
  beyond its loaded range, retaining an off-screen clip during a drag, and
  repeatedly scrolling and zooming the viewport.

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

## Continuous Interaction

The QML fixture drives 120 incremental horizontal and vertical scroll updates,
then 120 zoom updates from 100% down to 25% and back. It records elapsed time,
viewport-model rebuilds, and the peak delegate count while verifying that the
final visible clip is present and delegate creation stays bounded.

The first zoom measurement rebuilt the viewport model for 118 of 120 scale
updates because every scale change forced a refresh. Clip geometry already
tracks the scale through QML bindings, so the model only needs rebuilding when
the visible range leaves the loaded overscan range. Reusing that range check
reduced the observed rebuild count from 118 to 6 and the zoom measurement from
about 33.9 seconds to 7.6 seconds on the same Windows development build.
Elapsed time remains informational; the deterministic rebuild-count assertion
protects the intended behavior across different machines.

## Decoder Seek and Cache

The `video_decoder` fixture generates 60 frames with a fixed 12-frame GOP size,
then seeks from the final GOP backward through all five GOPs. It verifies frame
content, rapid-seek latest-request behavior, cache occupancy, and the exact
cache path used for nearby and repeated frames.

The first instrumented run decoded all 60 frames but reported zero GOP blocks
and zero GOP hits. `VideoDecoder` built each `GopCacheBlock` while decoding but
never inserted the completed block into its three-entry LRU. Restoring the
missing insertion produced one nearby GOP hit, retained three blocks, evicted
the two oldest blocks, and served the final repeated frame from the larger
frame cache. These counts are deterministic assertions; the tiny generated
media is not used to claim representative seek latency.

## Frame Cache Eviction

The decoder fixture also generates 240 frames across 20 fixed GOPs and applies
a runtime-only 2 MiB frame-cache budget. Descending seeks fill every GOP while
forcing the frame cache and three-block GOP LRU beyond capacity. The test
verifies bounded frame-cache cost, a frame-cache hit for a recent frame no
longer present in the GOP LRU, and fresh decoding when an early evicted frame is
requested again.

Cold-seek timings and the two re-seek timings are printed for comparison but do
not use wall-clock pass/fail limits. The low-resolution generated video makes
cache lifecycle behavior deterministic; it is not representative evidence for
seek latency on production media.

## Representative Video Workload

The `video_decoder` test accepts an opt-in `AVIQTL_PERF_VIDEO` workload. Set it
to a local media path to measure a production file, or to `synthetic` for the
repeatable 60-second, 640x360, 30 fps H.264 fixture. The workload records index
startup time, forward and backward seek latency, decoded frames, cache hits,
cache entries, and estimated frame-cache memory.

On the Windows Release build used to introduce the fixture, the synthetic run
reported 103 ms startup, 28 ms median seek, 63 ms maximum seek, and 225,792,000
bytes of frame-cache cost after seven distributed seeks. The cache remained
inside its configured 512 MiB bound. The flat-color synthetic source exercises
the lifecycle repeatably but is deliberately not treated as evidence for
production codec complexity.

PowerShell:

```powershell
$env:AVIQTL_PERF_VIDEO = "D:\media\representative.mp4"
ctest --test-dir build -R "^video_decoder$" -V
```

POSIX shell:

```bash
AVIQTL_PERF_VIDEO=/media/representative.mp4 \
  ctest --test-dir build -R '^video_decoder$' -V
```

## Representative Audio Workload

The `audio_decoder` test similarly accepts `AVIQTL_PERF_AUDIO`. Its synthetic
mode generates two minutes of 48 kHz stereo PCM, then measures decoder startup,
distributed sample reads, full-duration waveform availability, chunk-cache
occupancy, evictions, and peak-pyramid size.

The initial Windows Release measurement reported 55 ms startup, 2 ms median
read, 3 ms maximum read, and 29 ms until the full peak pyramid was available.
The ten-chunk cache contained 3,840,000 float samples. Building the waveform
and the distributed reads decoded 37 chunks, evicted 26, and left the cache at
its ten-chunk limit.

```powershell
$env:AVIQTL_PERF_AUDIO = "synthetic" # or a local audio/media path
ctest --test-dir build -R "^audio_decoder$" -V
```

## Audio Plugin Scanning

The `audio_plugin_scan` fixture creates 40 application-relative VST2 targets,
including one target that emits malformed discovery output. It uses the real
parallel scan orchestration with a deterministic discovery subprocess, verifies
39 normalized results, and repeats the scan to expose rescan cost. This also
protects resolution of relative plugin directories.

The initial Windows Release measurement reported 4.4 seconds for both the first
and repeated scans with eight discovery workers. The fixture intentionally uses
separate processes and reports timings without a wall-clock assertion. A future
persistent discovery cache can use the repeated-scan number as its baseline.

```text
ctest --test-dir build -R audio_plugin_scan -V
```

## Next Measurements

The fixtures now cover the model/controller baseline, real QML delegate
virtualization, continuous viewport interaction, decoder cache lifecycle,
bounded video and audio caches, opt-in long-media workloads, and repeated plugin
scanning. The next evidence should come from a maintained corpus containing
representative H.264/H.265 camera files, compressed long-form audio, and real
VST3/LV2 installations. Record machine, codec, resolution, duration, plugin
count, and before/after medians when using those files.
