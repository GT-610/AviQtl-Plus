#!/usr/bin/env python3
"""Build and package the AviQtl desktop application (Rust + Slint by default)."""

import argparse
import locale
import multiprocessing
import os
import platform
import queue
import re
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, ClassVar, List, Optional, Type


_SEMVER_NUMERIC_IDENTIFIER = r"(?:0|[1-9][0-9]*)"
_SEMVER_NON_NUMERIC_IDENTIFIER = r"(?:[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
_SEMVER_PRERELEASE_IDENTIFIER = rf"(?:{_SEMVER_NUMERIC_IDENTIFIER}|{_SEMVER_NON_NUMERIC_IDENTIFIER})"
_SEMVER_PATTERN = re.compile(
    rf"^(?P<major>{_SEMVER_NUMERIC_IDENTIFIER})\."
    rf"(?P<minor>{_SEMVER_NUMERIC_IDENTIFIER})\."
    rf"(?P<patch>{_SEMVER_NUMERIC_IDENTIFIER})"
    rf"(?:-{_SEMVER_PRERELEASE_IDENTIFIER}(?:\.{_SEMVER_PRERELEASE_IDENTIFIER})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


def parse_semver(version_string: str) -> tuple[int, int, int]:
    match = _SEMVER_PATTERN.fullmatch(version_string)
    if not match:
        raise ValueError(f"Invalid semantic version: {version_string}")
    return tuple(int(match.group(name)) for name in ("major", "minor", "patch"))


def normalize_macos_architecture(machine: str) -> str:
    normalized = machine.strip().lower()
    normalized = {"aarch64": "arm64", "amd64": "x86_64", "x64": "x86_64"}.get(normalized, normalized)
    if normalized not in {"arm64", "x86_64"}:
        raise RuntimeError(f"Unsupported macOS architecture: {machine}")
    return normalized


def read_project_version(source_dir: Path) -> str:
    """Read the canonical Rust workspace version."""
    cargo_toml = source_dir / "rust" / "Cargo.toml"
    try:
        text = cargo_toml.read_text(encoding="utf-8")
    except OSError as exc:
        raise RuntimeError(f"Could not read project version from {cargo_toml}: {exc}") from exc
    match = re.search(r'^version\s*=\s*"([^"]+)"\s*$', text, flags=re.MULTILINE)
    if not match:
        raise RuntimeError(f"Could not find workspace package version in {cargo_toml}")
    return match.group(1)


@dataclass
class BuildConfig:
    source_dir: Path
    temp_base: Path
    output_dir: Path
    target: str
    is_debug: bool
    use_container: bool
    is_offline: bool
    frontend: str = "slint"  # "slint" (default) or "qt" (legacy C++/QML frontend).
    qt_dir: Optional[Path] = None  # Qt builds only: forwarded to CMAKE_PREFIX_PATH.
    version_major: int = 0
    version_minor: int = 0
    version_patch: int = 0
    version_string: str = "0.0.0"

    def __post_init__(self):
        if self.frontend not in ("slint", "qt"):
            raise ValueError(f"Unknown frontend: {self.frontend!r} (expected 'slint' or 'qt')")

    @property
    def build_type(self) -> str:
        return "Debug" if self.is_debug else "Release"

    @property
    def work_dir(self) -> Path:
        return self.temp_base / self.target / self.build_type

    @property
    def cargo_target_dir(self) -> Path:
        return self.work_dir / "cargo-target"

    @property
    def cargo_profile(self) -> str:
        return "debug" if self.is_debug else "release"

    @property
    def qt_build_dir(self) -> Path:
        return self.work_dir / "qt-build"

    @property
    def dist_dir(self) -> Path:
        return self.source_dir / "dist"


class Logger:
    def __init__(self, log_cb: Callable[[str], None], progress_cb: Callable[[int, str], None]):
        self._log = log_cb
        self._progress = progress_cb

    def log(self, message: str):
        self._log(message)

    def section(self, title: str):
        self._log(f">>> {title}")

    def progress(self, value: int, message: str):
        self._progress(value, message)


class PlatformBuilder:
    RESOURCE_DIRECTORIES: ClassVar[tuple[tuple[str, ...], ...]] = (
        ("ui", "qml", "effects"),
        ("ui", "qml", "objects"),
        ("plugins",),
        ("effect-packages",),
        ("repos",),
    )

    def __init__(self, config: BuildConfig, logger: Logger):
        self.config = config
        self.logger = logger
        self.env = os.environ.copy()
        self.env.update({
            "GIT_TERMINAL_PROMPT": "0",
            "HOMEBREW_NO_AUTO_UPDATE": "1",
            "DEBIAN_FRONTEND": "noninteractive",
            "CARGO_TARGET_DIR": str(config.cargo_target_dir),
        })
        self.container_name = ""
        self.use_container = False
        self.cancelled = False
        self.current_proc: Optional[subprocess.Popen] = None

    def build(self):
        frontend = "Rust + Slint" if self.config.frontend == "slint" else "Qt (legacy)"
        self.logger.progress(10, f"{self.config.build_type} {frontend} build started")
        self.logger.section("Checking dependencies")
        self.install_dependencies()
        self.check_cancelled()
        self.logger.section(f"Compiling {frontend} frontend")
        self.compile()
        self.logger.progress(70, "Compilation complete")
        self.check_cancelled()
        self.logger.section("Packaging")
        self.package()
        self.logger.progress(90, "Package assembled")
        self.check_cancelled()
        self.logger.section("Creating archive")
        self.archive()
        self.logger.progress(100, "Complete")

    def check_cancelled(self):
        if self.cancelled:
            raise RuntimeError("Build cancelled")

    def cancel(self):
        self.cancelled = True
        process = self.current_proc
        if process and process.poll() is None:
            self.logger.log("Stopping running command...")
            try:
                if os.name != "nt":
                    os.killpg(process.pid, signal.SIGTERM)
                else:
                    process.terminate()
            except ProcessLookupError:
                pass

    def install_dependencies(self):
        pass

    def get_cargo_build_cmd(self) -> List[str]:
        command = [
            "cargo", "build",
            "--manifest-path", str(self.config.source_dir / "rust" / "Cargo.toml"),
            "--package", "aviqtl-slint", "--locked",
        ]
        if not self.config.is_debug:
            command.append("--release")
        if self.config.is_offline:
            command.append("--offline")
        return command

    def compile(self):
        if self.config.frontend == "qt":
            self.compile_qt()
            return
        jobs = multiprocessing.cpu_count()
        self.env["CARGO_BUILD_JOBS"] = str(jobs)
        self.logger.log(f"Parallel jobs: {jobs}")
        self.run_cmd(self.get_cargo_build_cmd())

    def check_qt_prerequisites(self):
        if not shutil.which("cmake"):
            raise RuntimeError("cmake was not found; install CMake to build the Qt frontend")
        if not shutil.which("ninja"):
            raise RuntimeError("ninja was not found; install Ninja to build the Qt frontend")
        qt_dir = self.config.qt_dir
        if qt_dir is not None:
            if not (qt_dir / "lib" / "cmake" / "Qt6").is_dir():
                raise RuntimeError(
                    f"--qt-dir does not look like a Qt6 prefix: {qt_dir} "
                    "(expected lib/cmake/Qt6 underneath)"
                )
            return
        for tool in ("qmake6", "qmake"):
            found = shutil.which(tool, path=self.env.get("PATH"))
            if found:
                self.logger.log(f"Found Qt: {found}")
                return
        raise RuntimeError(
            "Qt6 was not found on PATH; pass --qt-dir <Qt6 prefix> "
            "or install Qt 6.5+ with Quick, QuickControls2, Quick3D, "
            "Multimedia, ShaderTools and LinguistTools"
        )

    def get_cmake_configure_cmd(self) -> List[str]:
        command = [
            "cmake", "-S", str(self.config.source_dir), "-B", str(self.config.qt_build_dir),
            "-G", "Ninja", f"-DCMAKE_BUILD_TYPE={self.config.build_type}",
        ]
        if self.config.qt_dir is not None:
            command.append(f"-DCMAKE_PREFIX_PATH={self.config.qt_dir}")
        if self.config.is_offline:
            command.append("-DAVIQTL_CARGO_OFFLINE=ON")
        return command

    def get_cmake_build_cmd(self) -> List[str]:
        return [
            "cmake", "--build", str(self.config.qt_build_dir),
            "--target", "AviQtl", "--", f"-j{multiprocessing.cpu_count()}",
        ]

    def compile_qt(self):
        self.check_qt_prerequisites()
        self.run_cmd(self.get_cmake_configure_cmd())
        self.check_cancelled()
        self.run_cmd(self.get_cmake_build_cmd())

    def qt_product(self) -> Path:
        """Locate the built Qt product: macOS bundle when present, else the binary."""
        bundle = self.config.qt_build_dir / "bin" / "AviQtl.app"
        if bundle.is_dir():
            return bundle
        suffix = ".exe" if os.name == "nt" else ""
        return self.config.qt_build_dir / "bin" / f"AviQtl{suffix}"

    def package_qt(self):
        """Stage a development Qt build: raw product plus shared resources.

        This intentionally skips platform deployment (macdeployqt/windeployqt
        bundling). Use it to keep working against the legacy frontend while
        Slint is under construction, not for release packaging.
        """
        self.prepare_output_dir()
        source = self.qt_product()
        if not source.exists():
            raise FileNotFoundError(f"Qt product not found: {source}")
        destination = self.config.output_dir / source.name
        if source.is_dir():
            shutil.copytree(source, destination, dirs_exist_ok=True)
        else:
            shutil.copy2(source, destination)
        self.copy_resources(self.config.output_dir)
        if discovery := self.find_carla_discovery_tool(windows=os.name == "nt"):
            shutil.copy2(discovery, self.config.output_dir / discovery.name)
        self.logger.log(f"Qt product (development build, undeployed): {destination}")
        self.logger.log("Note: run macdeployqt/windeployqt on the Qt binary before distributing it")

    def cargo_binary(self, windows: bool = False) -> Path:
        suffix = ".exe" if windows else ""
        return self.config.cargo_target_dir / self.config.cargo_profile / f"aviqtl-slint{suffix}"

    def package(self):
        raise NotImplementedError

    def prepare_output_dir(self):
        if self.config.output_dir.exists():
            self.remove_tree(self.config.output_dir)
        self.config.output_dir.mkdir(parents=True, exist_ok=True)

    def copy_resources(self, destination: Path):
        for parts in self.RESOURCE_DIRECTORIES:
            source = self.config.source_dir.joinpath(*parts)
            if source.exists():
                shutil.copytree(source, destination / parts[-1], dirs_exist_ok=True)
                self.logger.log(f"Bundled resources: {source.relative_to(self.config.source_dir)}")

    def find_carla_discovery_tool(self, windows: bool = False) -> Path | None:
        name = "carla-discovery-native.exe" if windows else "carla-discovery-native"
        found = shutil.which(name, path=self.env.get("PATH"))
        roots = [self.config.source_dir / "vendor" / "carla"]
        if found:
            roots.append(Path(found).parent)
        if platform.system() == "Darwin" and shutil.which("brew"):
            try:
                prefix = subprocess.check_output(
                    ["brew", "--prefix", "carla"], text=True, stderr=subprocess.DEVNULL
                ).strip()
                roots.append(Path(prefix))
            except subprocess.CalledProcessError:
                pass
        for root in roots:
            candidates = [root / name, root / "runtime" / name, *root.glob(f"**/{name}")]
            for candidate in candidates:
                if candidate.is_file():
                    return candidate
        return None

    def archive(self):
        self.config.dist_dir.mkdir(parents=True, exist_ok=True)
        archive_name = self.get_archive_name()
        if self.config.frontend == "qt":
            archive_name += "-Qt"
        self.create_zip(archive_name)
        self.logger.log(f"Archive: {self.config.dist_dir / (archive_name + '.zip')}")

    def get_archive_name(self) -> str:
        return "AviQtl-Archive"

    def create_zip(self, archive_name: str):
        archive_path = self.config.dist_dir / f"{archive_name}.zip"
        archive_path.unlink(missing_ok=True)
        shutil.make_archive(str(self.config.dist_dir / archive_name), "zip", root_dir=self.config.output_dir)

    def run_cmd(self, command: List[str], shell: bool = False, force_host: bool = False):
        self.check_cancelled()
        in_container = self.use_container and not force_host
        display_command = shlex.join(command) if isinstance(command, list) else command
        self.logger.log(f"{'[Container] ' if in_container else ''}{display_command}")
        actual_command = command
        if in_container:
            inner = f"cd {shlex.quote(str(self.config.source_dir))} && {display_command}"
            actual_command = f"distrobox enter {shlex.quote(self.container_name)} -- bash -lc {shlex.quote(inner)}"
            shell = True
        popen_options = {"start_new_session": True} if os.name != "nt" else {}
        process = subprocess.Popen(
            actual_command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, encoding="utf-8", errors="replace", shell=shell,
            env=self.env, cwd=self.config.source_dir, **popen_options,
        )
        self.current_proc = process
        try:
            assert process.stdout is not None
            for line in process.stdout:
                self.logger.log(line.rstrip())
            process.wait()
        finally:
            self.current_proc = None
        self.check_cancelled()
        if process.returncode != 0:
            raise subprocess.CalledProcessError(process.returncode, actual_command)

    def remove_tree(self, path: Path):
        def make_writable_and_retry(function, target, _exception):
            os.chmod(target, os.stat(target).st_mode | stat.S_IWUSR)
            function(target)
        try:
            shutil.rmtree(path, onexc=make_writable_and_retry)
        except TypeError:
            shutil.rmtree(path, onerror=make_writable_and_retry)


class LinuxBuilderBase(PlatformBuilder):
    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        self.use_container = config.use_container
        self.image_name = ""

    def create_container(self):
        if not (shutil.which("distrobox") and shutil.which("podman")):
            raise RuntimeError("distrobox or podman not found")
        try:
            self.run_cmd([
                "distrobox", "create", "--name", self.container_name,
                "--image", self.image_name, "--yes",
            ], force_host=True)
        except subprocess.CalledProcessError:
            self.logger.log("Container already exists. Using as-is.")
        self.run_cmd(["true"])

    def install_dependencies(self):
        if not self.use_container:
            self.logger.log("Host mode: skipping system package installation")
        elif self.config.is_offline:
            self.logger.log("Offline mode: using the existing container and dependencies")
            self.run_cmd(["true"])
        else:
            self.create_container()

    def package(self):
        if self.config.frontend == "qt":
            return self.package_qt()
        self.prepare_output_dir()
        source = self.cargo_binary()
        if not source.is_file():
            raise FileNotFoundError(f"Executable not found: {source}")
        destination = self.config.output_dir / "AviQtl"
        shutil.copy2(source, destination)
        self.copy_resources(self.config.output_dir)
        if discovery := self.find_carla_discovery_tool():
            shutil.copy2(discovery, self.config.output_dir / discovery.name)
        self.logger.log(f"Executable: {destination}")


class ArchBuilder(LinuxBuilderBase):
    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        self.container_name = "archlinux-aviqtl"
        self.image_name = "archlinux:latest"

    def install_dependencies(self):
        super().install_dependencies()
        if not self.use_container or self.config.is_offline:
            return
        dependencies = [
            "base-devel", "git", "rust", "clang", "pkgconf", "zip", "ffmpeg",
            "mesa", "vulkan-devel", "libx11", "libxcursor", "libxi", "libxrandr",
            "libxkbcommon", "wayland", "wayland-protocols",
        ]
        self.run_cmd(["sudo", "pacman", "-Syu", "--needed", "--noconfirm", *dependencies])

    def get_archive_name(self) -> str:
        return "AviQtl-Arch-Linux-x86_64"


class WindowsDependencyMixin:
    WINDOWS_SYSTEM_DLLS = {
        "advapi32.dll", "authz.dll", "avrt.dll", "bcrypt.dll", "bcryptprimitives.dll",
        "cfgmgr32.dll", "comctl32.dll",
        "comdlg32.dll", "crypt32.dll", "d3d9.dll", "d3d11.dll", "d3d12.dll",
        "d3dcompiler_47.dll", "dcomp.dll", "dnsapi.dll", "dwmapi.dll", "dwrite.dll",
        "dxgi.dll", "dxva2.dll", "gdi32.dll", "gdiplus.dll", "imm32.dll",
        "iphlpapi.dll", "kernel32.dll", "mpr.dll", "msimg32.dll", "msvcp140.dll",
        "msvcp_win.dll", "msvcrt.dll",
        "mf.dll", "mfplat.dll", "mfreadwrite.dll", "ncrypt.dll", "ntdll.dll",
        "netapi32.dll", "ole32.dll", "oleaut32.dll", "opengl32.dll", "powrprof.dll",
        "propsys.dll", "rpcrt4.dll", "secur32.dll", "setupapi.dll", "shcore.dll",
        "shell32.dll", "shlwapi.dll", "ucrtbase.dll", "user32.dll", "userenv.dll",
        "uxtheme.dll", "version.dll", "vcruntime140.dll", "vcruntime140_1.dll",
        "winhttp.dll", "wininet.dll", "winmm.dll", "winspool.drv", "ws2_32.dll",
        "wsock32.dll", "wtsapi32.dll",
    }

    def is_windows_system_dll(self, name: str) -> bool:
        lowered = name.lower()
        return lowered.startswith(("api-ms-win-", "ext-ms-win-")) or lowered in self.WINDOWS_SYSTEM_DLLS

    def packaged_binaries(self) -> list[Path]:
        return [path for path in self.config.output_dir.rglob("*")
                if path.is_file() and path.suffix.lower() in {".exe", ".dll"}]

    def copy_windows_dependencies(self, dependency_reader, dependency_finder):
        scanned: set[str] = set()
        unresolved: set[str] = set()
        while True:
            copied = False
            for binary in self.packaged_binaries():
                key = str(binary.resolve()).lower()
                if key in scanned:
                    continue
                scanned.add(key)
                for name in dependency_reader(binary):
                    if self.is_windows_system_dll(name) or (self.config.output_dir / name).exists():
                        continue
                    source = dependency_finder(name)
                    if source is None:
                        unresolved.add(name)
                        continue
                    shutil.copy2(source, self.config.output_dir / name)
                    unresolved.discard(name)
                    copied = True
                    self.logger.log(f"Bundled runtime DLL: {name}")
            if not copied:
                break
        if unresolved:
            raise FileNotFoundError("Runtime DLLs were not found: " + ", ".join(sorted(unresolved)))


class Msys2Builder(WindowsDependencyMixin, PlatformBuilder):
    def install_dependencies(self):
        if self.config.is_offline:
            self.logger.log("Skipping dependency installation (--offline)")
            return
        if os.environ.get("MSYSTEM") != "UCRT64":
            raise RuntimeError("The --msys2 build must run from an MSYS2 UCRT64 shell")
        dependencies = [
            "mingw-w64-ucrt-x86_64-toolchain", "mingw-w64-ucrt-x86_64-rust",
            "mingw-w64-ucrt-x86_64-ffmpeg", "mingw-w64-ucrt-x86_64-pkgconf",
            "mingw-w64-ucrt-x86_64-binutils", "zip",
        ]
        self.run_cmd(["pacman", "-Syu", "--needed", "--noconfirm", *dependencies])

    def msys2_bin_directories(self) -> list[Path]:
        directories = []
        for name in ("MINGW_PREFIX", "MSYSTEM_PREFIX"):
            if value := self.env.get(name):
                directories.append(Path(value) / "bin")
        directories.extend(Path(part) for part in self.env.get("PATH", "").split(os.pathsep) if part)
        return list(dict.fromkeys(directories))

    def find_runtime_dll(self, name: str) -> Path | None:
        return next((directory / name for directory in self.msys2_bin_directories()
                     if (directory / name).is_file()), None)

    def imported_dlls(self, binary: Path) -> list[str]:
        objdump = shutil.which("objdump", path=self.env.get("PATH"))
        if not objdump:
            raise FileNotFoundError("objdump was not found in the MSYS2 UCRT64 environment")
        result = subprocess.run([objdump, "-p", str(binary)], capture_output=True, text=True,
                                errors="replace", env=self.env)
        return re.findall(r"DLL Name:\s*(\S+)", result.stdout) if result.returncode == 0 else []

    def package(self):
        if self.config.frontend == "qt":
            return self.package_qt()
        self.prepare_output_dir()
        source = self.cargo_binary(windows=True)
        if not source.is_file():
            raise FileNotFoundError(f"Executable not found: {source}")
        destination = self.config.output_dir / "AviQtl.exe"
        shutil.copy2(source, destination)
        self.copy_resources(self.config.output_dir)
        if discovery := self.find_carla_discovery_tool(windows=True):
            shutil.copy2(discovery, self.config.output_dir / discovery.name)
        self.copy_windows_dependencies(self.imported_dlls, self.find_runtime_dll)
        self.logger.log(f"Executable: {destination}")

    def get_archive_name(self) -> str:
        return "AviQtl-MSYS2-UCRT64-x86_64"


class MsvcBuilder(WindowsDependencyMixin, PlatformBuilder):
    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        if os.name != "nt":
            raise RuntimeError("MSVC build can only run on Windows")
        self.vcpkg_root: Path | None = None
        self.vcpkg_triplet = "x64-windows"

    @staticmethod
    def parse_cmd_environment(output: str) -> dict[str, str]:
        parsed: dict[str, str] = {}
        uppercase_keys: set[str] = set()
        for line in output.splitlines():
            if "=" not in line:
                continue
            key, _, value = line.partition("=")
            normalized = key.upper()
            is_uppercase = key == normalized
            if normalized not in parsed or is_uppercase or normalized not in uppercase_keys:
                parsed[normalized] = value
            if is_uppercase:
                uppercase_keys.add(normalized)
        return parsed

    def find_vcvarsall(self) -> Path | None:
        candidates = [Path(value) for name in ("VCVARSALL", "VCVARSALL_BAT")
                      if (value := os.environ.get(name))]
        vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
        if vswhere.is_file():
            result = subprocess.run(
                [str(vswhere), "-latest", "-products", "*", "-property", "installationPath"],
                capture_output=True, text=True, encoding=locale.getpreferredencoding(False), errors="replace",
            )
            if result.returncode == 0 and result.stdout.strip():
                candidates.append(Path(result.stdout.strip()) / "VC/Auxiliary/Build/vcvarsall.bat")
        candidates.extend(Path(rf"C:\Program Files\Microsoft Visual Studio\2022\{edition}\VC\Auxiliary\Build\vcvarsall.bat")
                          for edition in ("BuildTools", "Community", "Professional", "Enterprise"))
        return next((candidate for candidate in candidates if candidate.is_file()), None)

    def setup_msvc_environment(self):
        vcvarsall = self.find_vcvarsall()
        if not vcvarsall:
            raise RuntimeError("vcvarsall.bat not found; install Visual Studio C++ Build Tools")
        wrapper_path = None
        try:
            with tempfile.NamedTemporaryFile("w", suffix=".bat", delete=False, encoding="utf-8") as wrapper:
                wrapper.write(f'@call "{vcvarsall}" x64 > nul\n@set\n')
                wrapper_path = wrapper.name
            result = subprocess.run(
                ["cmd.exe", "/d", "/c", wrapper_path], capture_output=True, text=True,
                encoding=locale.getpreferredencoding(False), errors="replace",
            )
            if result.returncode != 0:
                raise RuntimeError(f"vcvarsall.bat failed: {result.stderr}")
            self.env.update(self.parse_cmd_environment(result.stdout))
            self.env.pop("Path", None)
        finally:
            if wrapper_path:
                Path(wrapper_path).unlink(missing_ok=True)

    def find_vcpkg_root(self) -> Path | None:
        candidates = [Path(value) for value in
                      (self.env.get("VCPKG_ROOT"), os.environ.get("VCPKG_INSTALLATION_ROOT")) if value]
        candidates.extend((Path(r"C:\vcpkg"), self.config.source_dir / "vcpkg"))
        return next((root for root in candidates if (root / "vcpkg.exe").is_file()), None)

    def ffmpeg_release_log_paths(self, installed_root: Path) -> list[Path]:
        """Return the possible vcpkg log files for the FFmpeg Release build.

        vcpkg normally keeps buildtrees under its own checkout even when an
        alternate --x-install-root is used. Some versions/layouts instead put
        them below the install root, so support both locations.
        """
        roots = [
            installed_root / "vcpkg" / "buildtrees" / "ffmpeg",
            self.vcpkg_root / "buildtrees" / "ffmpeg" if self.vcpkg_root else None,
        ]
        paths: list[Path] = []
        for root in roots:
            if root is None:
                continue
            for suffix in ("out.log", "err.log"):
                path = root / f"build-{self.vcpkg_triplet}-rel-{suffix}"
                if path not in paths:
                    paths.append(path)
        return paths

    @staticmethod
    def read_log_tail(path: Path, max_bytes: int = 8192, max_lines: int = 1) -> str:
        try:
            with path.open("rb") as log_file:
                log_file.seek(0, os.SEEK_END)
                size = log_file.tell()
                log_file.seek(max(0, size - max_bytes))
                contents = log_file.read().decode(
                    locale.getpreferredencoding(False), errors="replace"
                )
        except OSError:
            return ""
        lines = [line.strip() for line in contents.splitlines() if line.strip()]
        return "\n".join(lines[-max_lines:]) if lines else ""

    def monitor_ffmpeg_release(self, installed_root: Path, stop_event: threading.Event):
        """Report vcpkg's otherwise captured FFmpeg Release build progress."""
        paths = self.ffmpeg_release_log_paths(installed_root)
        last_state = None
        last_report = 0.0
        while not stop_event.wait(10):
            existing = []
            for path in paths:
                try:
                    size = path.stat().st_size
                except OSError:
                    continue
                existing.append((path, size, self.read_log_tail(path)))

            if not existing:
                state = None
            else:
                state = tuple((str(path), size, tail) for path, size, tail in existing)

            now = time.monotonic()
            if state is not None and (state != last_state or now - last_report >= 30):
                details = "; ".join(
                    f"{path.name}: {size / 1024 / 1024:.1f} MiB"
                    + (f", latest: {tail}" if tail else "")
                    for path, size, tail in existing
                )
                self.logger.log(f"FFmpeg Release is still building ({details})")
                last_state = state
                last_report = now
            elif state is None and now - last_report >= 30:
                self.logger.log("FFmpeg Release is still building; waiting for vcpkg log files")
                last_state = None
                last_report = now

    def log_ffmpeg_failure(self, installed_root: Path):
        error_logs = [path for path in self.ffmpeg_release_log_paths(installed_root) if path.name.endswith("-err.log")]
        for path in error_logs:
            tail = self.read_log_tail(path, max_bytes=32768, max_lines=40)
            if tail:
                self.logger.log(f"FFmpeg Release error log: {path}")
                self.logger.log(tail)
                return

    def install_dependencies(self):
        self.setup_msvc_environment()
        self.vcpkg_root = self.find_vcpkg_root()
        if not self.vcpkg_root:
            raise RuntimeError("vcpkg.exe was not found; set VCPKG_ROOT")
        installed_root = self.config.work_dir / "vcpkg_installed"
        target_root = installed_root / self.vcpkg_triplet
        self.env.update({
            "VCPKG_ROOT": str(self.vcpkg_root),
            "VCPKGRS_TRIPLET": self.vcpkg_triplet,
            "FFMPEG_DIR": str(target_root),
        })
        if self.config.is_offline:
            if not (target_root / "include" / "libavcodec" / "avcodec.h").is_file():
                raise RuntimeError(f"Offline MSVC dependencies are incomplete: {target_root}")
            return
        stop_event = threading.Event()
        monitor = threading.Thread(
            target=self.monitor_ffmpeg_release,
            args=(installed_root, stop_event),
            name="ffmpeg-release-log-monitor",
            daemon=True,
        )
        self.logger.log("FFmpeg Release build output is captured by vcpkg; progress summaries will appear every 10 seconds")
        monitor.start()
        try:
            self.run_cmd([
                str(self.vcpkg_root / "vcpkg.exe"), "install", "--triplet", self.vcpkg_triplet,
                "--x-manifest-root", str(self.config.source_dir),
                "--x-install-root", str(installed_root),
            ])
        except subprocess.CalledProcessError:
            self.log_ffmpeg_failure(installed_root)
            raise
        finally:
            stop_event.set()
            monitor.join(timeout=2)

    def vcpkg_bin_directory(self) -> Path:
        return self.config.work_dir / "vcpkg_installed" / self.vcpkg_triplet / "bin"

    def find_runtime_dll(self, name: str) -> Path | None:
        candidate = self.vcpkg_bin_directory() / name
        return candidate if candidate.is_file() else None

    def imported_dlls(self, binary: Path) -> list[str]:
        dumpbin = shutil.which("dumpbin.exe", path=self.env.get("PATH"))
        if not dumpbin:
            raise FileNotFoundError("dumpbin.exe was not found in the MSVC environment")
        result = subprocess.run(
            [dumpbin, "/dependents", str(binary)], capture_output=True, text=True,
            encoding=locale.getpreferredencoding(False), errors="replace", env=self.env,
        )
        return re.findall(r"^\s+([^\s]+\.dll)\s*$", result.stdout, flags=re.IGNORECASE | re.MULTILINE)

    def package(self):
        if self.config.frontend == "qt":
            return self.package_qt()
        self.prepare_output_dir()
        source = self.cargo_binary(windows=True)
        if not source.is_file():
            raise FileNotFoundError(f"Executable not found: {source}")
        destination = self.config.output_dir / "AviQtl.exe"
        shutil.copy2(source, destination)
        self.copy_resources(self.config.output_dir)
        if discovery := self.find_carla_discovery_tool(windows=True):
            shutil.copy2(discovery, self.config.output_dir / discovery.name)
        self.copy_windows_dependencies(self.imported_dlls, self.find_runtime_dll)
        self.logger.log(f"Executable: {destination}")

    def get_archive_name(self) -> str:
        return "AviQtl-MSVC-x86_64"


class XcodeBuilder(PlatformBuilder):
    def install_dependencies(self):
        if self.config.is_offline:
            self.logger.log("Skipping dependency installation (--offline)")
            return
        if not shutil.which("brew"):
            raise RuntimeError("Homebrew not found")
        self.run_cmd(["brew", "install", "rust", "pkgconf", "ffmpeg"])

    def write_info_plist(self, contents: Path):
        plist = f'''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleExecutable</key><string>aviqtl-slint</string>
<key>CFBundleIdentifier</key><string>org.aviqtl.AviQtl</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleName</key><string>AviQtl</string>
<key>CFBundleDisplayName</key><string>AviQtl</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{self.config.version_string}</string>
<key>CFBundleVersion</key><string>{self.config.version_string}</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
'''
        (contents / "Info.plist").write_text(plist, encoding="utf-8")

    @staticmethod
    def macos_dependencies(binary: Path) -> list[str]:
        result = subprocess.run(["otool", "-L", str(binary)], capture_output=True,
                                text=True, errors="replace", check=True)
        return [line.strip().split(" (compatibility version", 1)[0]
                for line in result.stdout.splitlines()[1:] if line.strip()]

    @staticmethod
    def is_macos_system_dependency(path: str) -> bool:
        return path.startswith(("/System/", "/usr/lib/", "@executable_path/", "@loader_path/"))

    def bundle_macos_dependencies(self, executable: Path, frameworks: Path):
        frameworks.mkdir(parents=True, exist_ok=True)
        pending = [executable]
        scanned: set[Path] = set()
        while pending:
            binary = pending.pop()
            if binary in scanned:
                continue
            scanned.add(binary)
            for dependency in self.macos_dependencies(binary):
                if self.is_macos_system_dependency(dependency):
                    continue
                if dependency.startswith("@rpath/"):
                    if (frameworks / Path(dependency).name).exists():
                        continue
                    raise FileNotFoundError(f"Could not resolve macOS @rpath dependency: {dependency}")
                source = Path(dependency)
                if not source.exists():
                    raise FileNotFoundError(f"macOS dependency not found: {dependency}")
                destination = frameworks / source.name
                if not destination.exists():
                    shutil.copy2(source.resolve(), destination)
                    os.chmod(destination, destination.stat().st_mode | stat.S_IWUSR)
                    pending.append(destination)
                relative = f"@executable_path/../Frameworks/{destination.name}"
                self.run_cmd(["install_name_tool", "-change", dependency, relative, str(binary)])
        for dylib in frameworks.iterdir():
            self.run_cmd(["install_name_tool", "-id", f"@rpath/{dylib.name}", str(dylib)])

    def package(self):
        if self.config.frontend == "qt":
            return self.package_qt()
        self.prepare_output_dir()
        source = self.cargo_binary()
        if not source.is_file():
            raise FileNotFoundError(f"Executable not found: {source}")
        app = self.config.output_dir / "AviQtl.app"
        contents = app / "Contents"
        macos = contents / "MacOS"
        resources = contents / "Resources"
        frameworks = contents / "Frameworks"
        macos.mkdir(parents=True)
        resources.mkdir(parents=True)
        executable = macos / "aviqtl-slint"
        shutil.copy2(source, executable)
        self.write_info_plist(contents)
        self.copy_resources(resources)
        discovery = self.find_carla_discovery_tool()
        if discovery:
            shutil.copy2(discovery, macos / discovery.name)
        self.bundle_macos_dependencies(executable, frameworks)
        if discovery:
            self.bundle_macos_dependencies(macos / discovery.name, frameworks)
        self.run_cmd(["codesign", "--deep", "--force", "--sign", "-", str(app)])
        self.run_cmd(["codesign", "--verify", "--deep", "--strict", str(app)])
        self.logger.log(f"App bundle: {app}")

    def get_archive_name(self) -> str:
        return f"AviQtl-macOS-Xcode-{normalize_macos_architecture(platform.machine())}"

    def create_zip(self, archive_name: str):
        if self.config.frontend == "qt":
            return super().create_zip(archive_name)
        archive_path = self.config.dist_dir / f"{archive_name}.zip"
        archive_path.unlink(missing_ok=True)
        self.run_cmd([
            "ditto", "-c", "-k", "--sequesterRsrc", "--keepParent",
            str(self.config.output_dir / "AviQtl.app"), str(archive_path),
        ], force_host=True)


BUILDERS: dict[str, Type[PlatformBuilder]] = {
    "arch": ArchBuilder,
    "msys2": Msys2Builder,
    "msvc": MsvcBuilder,
    "xcode": XcodeBuilder,
}


class BuildWorker(threading.Thread):
    def __init__(self, config: BuildConfig):
        super().__init__(daemon=True)
        self.config = config
        self.builder: Optional[PlatformBuilder] = None
        self.cancel_requested = False
        self.log_queue: queue.Queue = queue.Queue()
        self.finished_event = threading.Event()
        self.success = False
        self.error_msg = ""

    def run(self):
        try:
            self.builder = BUILDERS[self.config.target](
                self.config, Logger(self._enqueue_log, self._enqueue_progress)
            )
            if self.cancel_requested:
                self.builder.cancel()
            self.builder.build()
            self.success = True
            self.error_msg = "Build succeeded"
        except Exception as exc:
            self.error_msg = str(exc)
        finally:
            self.log_queue.put(None)
            self.finished_event.set()

    def _enqueue_log(self, message: str):
        self.log_queue.put(("log", message))

    def _enqueue_progress(self, value: int, message: str):
        self.log_queue.put(("progress", value, message))

    def cancel(self):
        self.cancel_requested = True
        if self.builder:
            self.builder.cancel()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="BUILD.py", description="Build and package the AviQtl desktop application",
        formatter_class=argparse.RawTextHelpFormatter,
        epilog=("Examples:\n  python BUILD.py --arch\n  python BUILD.py --msys2 --debug\n"
                "  python BUILD.py --msvc\n  python BUILD.py --xcode --offline\n"
                "  python BUILD.py --xcode --frontend qt --debug\n"),
    )
    targets = parser.add_mutually_exclusive_group()
    targets.add_argument("--arch", action="store_true", help="Build for Arch Linux")
    targets.add_argument("--msys2", action="store_true", help="Build for Windows MSYS2 UCRT64")
    targets.add_argument("--msvc", action="store_true", help="Build for Windows MSVC x64")
    targets.add_argument("--xcode", action="store_true", help="Build for macOS")
    parser.add_argument("--frontend", choices=("slint", "qt"), default="slint",
                        help="Frontend to build (default: slint; qt selects the legacy C++/QML frontend)")
    parser.add_argument("--offline", action="store_true", help="Do not download dependencies")
    parser.add_argument("--debug", action="store_true", help="Build the debug profile")
    parser.add_argument("--no-container", action="store_true", help="Build Linux directly on the host")
    parser.add_argument("--qt-dir", type=Path,
                        help="Qt6 install prefix for --frontend qt (forwarded to CMAKE_PREFIX_PATH); "
                             "ignored by the Slint build")
    parser.add_argument("--version", type=str,
                        help="Validate the release version against rust/Cargo.toml")
    return parser.parse_args()


def determine_target(args: argparse.Namespace, system_name: str | None = None) -> str | None:
    for name in ("arch", "msys2", "msvc", "xcode"):
        if getattr(args, name):
            return name
    return {"linux": "arch", "windows": "msys2", "darwin": "xcode"}.get(
        (system_name or platform.system()).lower()
    )


def main():
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    args = parse_args()
    target = determine_target(args)
    if not target:
        print("Error: specify one of --arch, --msys2, --msvc, --xcode")
        sys.exit(1)
    source_dir = Path.cwd()
    canonical_version = read_project_version(source_dir)
    version_string = args.version or canonical_version
    try:
        version_parts = parse_semver(version_string)
    except ValueError:
        print(f"Error: invalid semantic version '{version_string}'")
        sys.exit(1)
    if version_string != canonical_version:
        print(f"Error: --version {version_string} does not match Rust workspace version "
              f"{canonical_version}; update rust/Cargo.toml first")
        sys.exit(1)
    config = BuildConfig(
        source_dir=source_dir, temp_base=source_dir / ".build_tmp",
        output_dir=source_dir / "build", target=target, is_debug=args.debug,
        use_container=target == "arch" and not args.no_container,
        is_offline=args.offline, frontend=args.frontend, qt_dir=args.qt_dir,
        version_major=version_parts[0], version_minor=version_parts[1],
        version_patch=version_parts[2], version_string=version_string,
    )
    if args.qt_dir and args.frontend != "qt":
        print("Warning: --qt-dir only affects --frontend qt builds; ignored by the Slint build")
    worker = BuildWorker(config)
    cancelled = False

    def cancel_build():
        nonlocal cancelled
        if cancelled:
            os._exit(130)
        cancelled = True
        print("\nCancelling build...")
        worker.cancel()

    signal.signal(signal.SIGINT, lambda _signum, _frame: cancel_build())
    mode = "Container" if config.use_container else "Host"
    frontend_label = "Slint" if config.frontend == "slint" else "Qt (legacy)"
    print(f"Build started | frontend={frontend_label} | target={target} | {config.build_type} | "
          f"{mode} | offline={config.is_offline}")
    worker.start()
    while not worker.finished_event.is_set():
        try:
            message = worker.log_queue.get(timeout=0.2)
        except queue.Empty:
            continue
        if message is None:
            break
        print(message[1] if message[0] == "log" else f"[{message[1]}%] {message[2]}")
    while not worker.log_queue.empty():
        message = worker.log_queue.get_nowait()
        if message is not None:
            print(message[1] if message[0] == "log" else f"[{message[1]}%] {message[2]}")
    worker.join(timeout=3)
    if cancelled:
        print("\nBuild cancelled.")
        sys.exit(130)
    if not worker.success:
        print(f"\nBuild failed: {worker.error_msg}")
        sys.exit(1)


if __name__ == "__main__":
    main()
