# Release Checklist

Use this checklist before creating an AviQtl-Plus release tag. A release is
green only when the same commit passes the checks below without relying on a
previous build directory.

## Version

- Update `project(AviQtl VERSION ...)` in `CMakeLists.txt`.
- Add the release section and date to `CHANGELOG.md`.
- Use a tag named `v<project-version>` exactly. The release workflow rejects a
  mismatched tag.
- Confirm the packaged About dialog reports the same version. `BUILD.py` uses
  the CMake project version by default; `--version` is only an explicit
  override.

## Clean Build and Tests

Run a Release build from a newly created build directory on each supported
platform, then run all CTest tests with failures printed:

```text
ctest --test-dir <clean-build-directory> --output-on-failure
```

Required platforms:

- Arch Linux container/runner
- macOS runner
- Windows MSYS2 UCRT64 runner

Do not treat skipped analysis tools or tests as passing coverage. Investigate
any test process that exits without Qt Test output as a startup/runtime failure.

## Quality and Packaging

- Run `python check.py --full` in an environment containing all configured
  analysis tools.
- Confirm the release archive contains the executable, Qt/QML runtime assets,
  translations, built-in effects/objects/transitions, effect packages, and
  required Carla/FFmpeg runtime libraries.
- Launch the packaged application, create a project, save and reopen it, render
  a short MP4, and verify the output outside the editor.
- Verify installation and startup on a machine or clean VM without the build
  toolchain installed.

## Publication

- Confirm all required GitHub Actions jobs are green for the release commit.
- Create the version tag only after the checks above pass.
- Download and smoke-test the artifacts produced by the tag workflow before
  announcing the release.
