# Code Quality Audit

This audit records the quality-hardening baseline established in July 2026.
It separates issues fixed in the hardening branch from larger work that needs
dedicated design, fixtures, or performance measurements.

## Resolved in the hardening pass

- Package archives are inspected before extraction. Absolute paths, parent
  traversal, symbolic links, excessive entry counts, and excessive expanded
  sizes are rejected.
- Package downloads require HTTPS, use bounded transfer time and size, and
  continue to verify catalog-provided SHA-256 hashes.
- Package IDs and types are validated before deployment. Unknown types no
  longer fall back to the plugin directory.
- Package deployment uses a staged directory and restores the previous package
  if the atomic installation-state commit fails.
- Package metadata and installed-package state use atomic file replacement.
- The obsolete release-asset selection path was removed; the UI now uses the
  metadata-driven, versioned installation flow directly.
- `check.py` is read-only by default, supports Python 3.9 and later, excludes
  generated and virtual-environment trees, and reports skipped tools and
  non-blocking QML warnings explicitly.
- Renderer fixtures cover animated Text movement and a representative fragment
  shader effect through the real QML composition path.
- A real QML composition is encoded to MP4 and decoded for stream and pixel
  verification, and the production `VideoDecoder` is exercised through
  `VideoFrameStore` and `QVideoSink` with generated media.

## Remaining priorities

1. **QML static-analysis baseline.** `qmllint` reports a large inherited set of
   unqualified-access and external-package import warnings. Establish module
   metadata for external effects first, then reduce application warnings in
   behavior-preserving batches before making warnings fatal in CI.
2. **Measured large-project performance.** A repeatable 5,000-clip model and
   controller fixture now covers QML snapshot materialization, distributed
   lookup, and move/undo/redo scaling from 1 to 1,000 selected clips. Add
   viewport scrolling and zooming, seek latency, frame-cache pressure,
   long-audio decoding, and plugin scanning before changing cache or
   concurrency policy.
3. **Private Qt API reduction.** ZIP handling, QRhi integration, and shader
   tooling currently require Qt private modules. Builds must use matching Qt
   patch versions until stable replacements are practical.

## Verification expectations

- Run configuration, builds, CTest, and quality tools from a fresh build
  directory after dependency upgrades.
- Treat skipped analysis tools as missing coverage, not as a clean result.
- Require a regression test for security, serialization, deployment, timeline,
  or export behavior changes.
- Keep performance changes only when a named fixture demonstrates a meaningful
  improvement without correctness regressions.
