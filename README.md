<p align="center">
  <img src="./assets/splash.svg" width="256">
</p>

<p align="center"><b>A next-generation video editor that inherits and surpasses AviUtl</b></p>

<p align="center">
  <a href="https://github.com/GT-610/AviQtl-Plus">GitHub</a>
  / <a href="https://github.com/GT-610/AviQtl-Plus/releases">Releases</a>
</p>

<p align="center">
  <b>English</b> / <a href="./i18n/README/README.ja.md">日本語</a> / <a href="./i18n/README/README.zh_CN.md">简体中文</a>
</p>

> [!IMPORTANT]
> This repository is a fork of [taisho-guy/AviQtl](https://codeberg.org/taisho-guy/AviQtl). In late May 2026, the original author decided to **reset development** — the `main` branch now hosts a new AviQtl built on **Qt Widgets + SDL3 + bgfx**, while the Qt Quick-based version was moved to the `legacy` branch (referred to as **AviQtl Legacy**). I ([GT610](https://github.com/GT-610)), an early core contributor to the original project, continue the Qt Quick path here as **AviQtl-Plus**.
>
> As a result, three AviQtl-related projects now exist in parallel:
> - **[AviQtl](https://codeberg.org/taisho-guy/AviQtl)** — the original project rebuilt with a new technology stack
> - **[AviQtl Legacy](https://codeberg.org/taisho-guy/AviQtl/src/branch/legacy)** — the original Qt Quick-based version, no longer updated
> - **AviQtl-Plus (this project)** — a fork continuing development of the Qt Quick + QRhi + ECS approach

### Why the original project paused

The original author identified these fundamental issues with Qt Quick:

- Qt Quick's resource management is incompatible with Compute Shaders, making implementation difficult
- Similarly, it conflicts with ECS architecture, hindering optimization
- Performance concerns for real-time processing

### How AviQtl-Plus addresses them

Building on the insights gained from the original project, AviQtl-Plus tackles these challenges:

1. **Compute Shader integration**: Circumventing Qt Quick's constraints to leverage GPU compute for high-performance effects processing
2. **ECS architecture optimization**: Maximizing the benefits of data-oriented design while resolving friction with Qt Quick
3. **Real-time performance**: Eliminating preview rendering bottlenecks to deliver a smooth editing experience

The vision remains unchanged: **a video editor that inherits and surpasses AviUtl**. We believe the Qt Quick + QRhi + ECS approach is a viable and valuable path — one we are committed to maturing into a practical, everyday editing tool.

### Original author's new direction

The original author has already acted on their plan — the new `main` branch is being rebuilt from scratch with **Qt Widgets + SDL3 + bgfx**, reflecting their conviction that Qt Quick is not the right foundation for the compute-shader-heavy future they envision. They have stated that PRs will not be accepted during the initial core implementation phase.

The old Qt Quick source code remains available on the [`legacy` branch](https://codeberg.org/taisho-guy/AviQtl/src/branch/legacy), and the original author [explicitly recommends](https://codeberg.org/taisho-guy/AviQtl) AviQtl-Plus to users who prefer the Qt Quick approach.

### Our relationship with the new AviQtl

The original author and I maintain a friendly relationship. If any of our contributions from AviQtl-Plus prove applicable to the new AviQtl, I will gladly submit them upstream. Similarly, I hope that innovations from both projects can cross-pollinate over time — ultimately benefiting all users who seek a modern, powerful, and intuitive AviUtl successor.

## What is [AviQtl-Plus](https://github.com/GT-610/AviQtl-Plus)?

<img src="./assets/screenshot.webp">

A project to develop a video editor that inherits the operability of **AviUtl 1.10** & **ExEdit 0.92** while delivering **performance that surpasses AviUtl**.

### Key Features

- UI closely resembling AviUtl
- **Fast and powerful GPU-accelerated effects**
- Support for **audio effects** such as VST3 and LV2
- Cross-platform: **Linux**, **Windows**, **macOS**

## Installation

1. On Linux, install the following dependencies:
   - Qt6, LuaJIT, Vulkan implementation (e.g. Mesa), FFmpeg, Carla, libc++
2. Download the optimal build for your system from the [Releases page](https://github.com/GT-610/AviQtl-Plus/releases).
3. Extract the archive, grant execute permission to `AviQtl`, and run it.

> [!NOTE]
> Linux users require an up-to-date environment equivalent to Arch Linux. Users of other distributions such as Ubuntu are strongly recommended to create an Arch Linux container with [Distrobox](https://distrobox.it/) and run AviQtl inside it.

## Building

`BUILD.py` automatically detects the build target from the current OS and installs all dependencies. Normally `python3 BUILD.py` alone suffices.

```bash
git clone https://github.com/GT-610/AviQtl-Plus.git
cd AviQtl-Plus
```

<details>
<summary>Linux</summary>

On Linux, the build environment is isolated using a distrobox/podman container by default.

1. **Install dependencies**
   - Pacman: `sudo pacman -S --needed distrobox podman python git`
   - APT: `sudo apt install distrobox podman python3 git`
   - DNF: `sudo dnf install distrobox podman python3 git`
2. **Build**
   - `python3 BUILD.py --arch`
3. **Run**
   - `./build/AviQtl`
</details>

<details>
<summary>macOS</summary>

On macOS, `BUILD.py` checks and installs dependencies via Homebrew (CMake, Ninja, Qt6, etc.), then runs `macdeployqt` and `codesign` to create a `.app` bundle.

1. **Install dependencies**
   - `brew install python git`
2. **Build**
   - `python3 BUILD.py --xcode`
3. **Run**
   - `open ./build/AviQtl.app`
</details>

<details>
<summary>Windows (MSYS2)</summary>

1. **Install dependencies**
   - `pacman -S git mingw-w64-ucrt-x86_64-python`
2. **Build**
   - `python3 BUILD.py --msys2`
3. **Run**
   - `./build/AviQtl.exe`
</details>

<details>
<summary>Windows (MSVC - Not recommended)</summary>

MSVC builds are not recommended due to the complexity of environment setup.

1. **Additional prerequisites**
   - Visual Studio 2022 Build Tools with C++ toolset
   - Official Qt MSVC x64 build (e.g. `msvc2022_64`)
   - vcpkg (can be specified via `VCPKG_ROOT` env var; `BUILD.py` will attempt to fetch it if not found)
2. **Build**
   - `python3 BUILD.py --msvc --qt-dir <Qt installation directory>`
   - If `--qt-dir` is omitted, automatic detection from `QT_MSVC_DIR` etc. is attempted.
3. **Run**
   - `.\build\AviQtl.exe`
</details>

## Q & A

> [!NOTE]
> The following Q&A reflects the original author's (taisho-guy) personal views and development history.

<details>
<summary>What motivated the development?</summary>

### The OS barrier
It started with AviUtl not running on Linux. **Maintaining a Windows environment solely for AviUtl** was unacceptable.

### Bloated ecosystem
Regardless of the reason, many users continue using AviUtl reluctantly. The ecosystem, bloated by years of extensions like a "Howl's Moving Castle", is hard to let go of despite the dissatisfaction.

### Project goals and mission
During a research project at [Kagoshima Prefectural Konan High School](https://edunet002.synapse-blog.jp/konan/), I decided to develop AviQtl independently to solve this problem.

- **Personal goal:** Produce music videos using only AviQtl on Linux, without juggling Domino, VocalShifter, REAPER, and AviUtl.
- **AviQtl's mission:** To be the optimal solution for those who use AviUtl reluctantly.
</details>

<details>
<summary>Why develop an AviUtl clone?</summary>

AviQtl-Plus is not a "reinvention of AviUtl". While it is strongly inspired by AviUtl, its internals are entirely different.

| Item | AviQtl-Plus | ExEdit0 | ExEdit2 |
| :--- | :--- | :--- | :--- |
| Core technology | Qt6 | Win32 API | Win32 API |
| Parallelism model | Data-driven (ECS) | Single-threaded | Multi-threaded |
| Memory space | 64-bit | 32-bit (max 4GB) | 64-bit |
| Preview rendering | Vulkan / Metal / DX12 | GDI | DX11 |
| Audio engine | Carla (VST3/LV2 etc.) | Built-in only | Built-in only |
| Plugin system | LuaJIT / C++ / QML / GLSL | Lua / C++ | LuaJIT / C++ |
| Supported OS | Linux, Windows, macOS | Windows | Windows |

AviQtl-Plus fundamentally resolves structural weaknesses:
1. **Data-oriented design with ECS (Entity Component System):** Maximizes CPU cache efficiency, accelerating processing of large numbers of objects.
2. **Modern memory management:** Adopts C++23 smart pointers, structurally minimizing unexplained crashes.
3. **Separation of UI and rendering:** Timeline operations are not blocked during heavy rendering, and the UI remains crisp in High-DPI environments.
</details>

<details>
<summary>Origin of the name and icon?</summary>

The name is a portmanteau of "AviUtl" and "Qt".
The icon is a design combining the Qt and AviUtl logos.

<p align="center">
  <img src="./assets/qt.svg" width="64" align="middle"> + <img src="./assets/aviutl.svg" width="64" align="middle"> = <img src="./assets/icon.svg" width="64" align="middle">
</p>
</details>

<details>
<summary>Can I use AviUtl plugins?</summary>

No. The mechanisms differ, so there is no compatibility. There are no plans to implement a compatibility layer either.
</details>

### AviQtl-Plus Q&A

> [!NOTE]
> The following Q&A reflects the perspective of AviQtl-Plus maintainer ([GT610](https://github.com/GT-610)).

<details>
<summary>Why continue AviQtl development?</summary>

The original project was suspended due to genuine technical difficulties with Qt Quick — but it also proved that **Qt + FFmpeg can rapidly produce a high-quality video editor prototype**. The architecture is well-designed, the foundation is solid, and I believe the Qt Quick route is still worth pursuing, especially now that QRhi provides a viable compute shader path that wasn't fully explored.

As an early core contributor, I've seen the project's potential firsthand. I'm picking up where the original author left off, not just to keep the code alive, but to see the original vision through — a video editor that inherits AviUtl's operability while surpassing its performance.
</details>

<details>
<summary>What is the roadmap for AviQtl-Plus?</summary>

**Next (0.2.1–0.3.x):**
- Complete effect and object plugin ecosystem
- Audio editing and mixing refinement
- Performance optimization and GPU compute shader improvements
- Initial public alpha releases

**Long-term:**
- Full-featured video editor suitable as an AviUtl replacement
- Potential rebranding (name and logo) to reflect the independent direction of the project
- Continue monitoring the new AviQtl's progress and explore cross-pollination opportunities between projects

The project is entirely driven by personal motivation — there are no deadlines or commercial pressures. Progress will be steady but paced.
</details>

<details>
<summary>How is AviQtl-Plus different from the original project?</summary>

The technical direction remains largely the same (Qt Quick + QRhi + ECS), but AviQtl-Plus places greater emphasis on:

- **Incremental deliverability**: getting a basic but usable editing workflow out the door, rather than pursuing architectural perfection upfront
- **Pragmatic problem-solving**: working within Qt Quick's constraints rather than treating them as blockers
- **Community transparency**: clear documentation of the fork relationship, plans, and long-term intentions

Once the project reaches a usable state, a rename and rebranding are likely to clearly distinguish it from the original project.
</details>

## Related Links

AviQtl-Plus stands on the shoulders of many wonderful projects.

| Project | License | Role |
| :--- | :--- | :--- |
| AviUtl | Non-free | Respected origin |
| AviQtl (Legacy) | AGPLv3 | Original Qt Quick project; `legacy` branch of the upstream |
| AviQtl | AGPLv3 | New Qt Widgets + bgfx version by the original author |
| AviQtl-Plus | AGPLv3 | This project — continued Qt Quick + QRhi + ECS development |
| Carla | GPLv2+ | Audio effect host (VST3/LV2 etc.) |
| FFmpeg | GPLv2+ | Video/audio decoding & encoding |
| LuaJIT | MIT | High-performance script engine |
| Qt | GPLv3 | UI/UX framework |
| Zrythm | AGPLv3 | Reference for audio plugin implementation |
| Remix Icon | Remix Icon License | Symbol icons |

## License

AviQtl-Plus is released under the [GNU Affero General Public License](https://www.gnu.org/licenses/agpl-3.0.txt).

[Remix Icon](https://remixicon.com/) used within AviQtl-Plus is provided under the [Remix Icon License](https://raw.githubusercontent.com/Remix-Design/RemixIcon/refs/heads/master/License).
