import sys
import os
import subprocess
import shutil
import argparse
import re
import multiprocessing
import shlex
import threading
import queue
import urllib.request
import platform
import locale
import signal
import tempfile
import json
import stat
from pathlib import Path
from dataclasses import dataclass
from typing import Callable, List, Optional, Type


@dataclass
class BuildConfig:
    source_dir: Path
    temp_base: Path
    output_dir: Path
    target: str
    is_debug: bool
    use_container: bool
    is_offline: bool
    qt_dir: Optional[Path] = None

    # Versioning fields
    version_major: int = 0
    version_minor: int = 0
    version_patch: int = 0
    version_string: str = "0.0.0"

    @property
    def build_type(self) -> str:
        return "Debug" if self.is_debug else "Release"

    @property
    def work_dir(self) -> Path:
        return self.temp_base / self.target / self.build_type

    @property
    def dist_dir(self) -> Path:
        return self.source_dir / "dist"


class Logger:
    def __init__(self, log_cb: Callable[[str], None], progress_cb: Callable[[int, str], None]):
        self._log = log_cb
        self._progress = progress_cb

    def log(self, msg: str):
        self._log(msg)

    def section(self, title: str):
        self._log(f">>> {title}")

    def progress(self, val: int, msg: str):
        self._progress(val, msg)


class PlatformBuilder:
    CARLA_DLL_PATTERNS = (
        "carla*.dll",
        "libcarla*.dll",
        "CarlaVst*.dll",
    )
    CARLA_DLL_ALIASES = {
        "libcarla_native-plugin.dll": "CarlaNativePlugin.dll",
    }

    def __init__(self, config: BuildConfig, logger: Logger):
        self.config = config
        self.logger = logger
        self.env = os.environ.copy()
        self.env["GIT_TERMINAL_PROMPT"] = "0"
        self.env["HOMEBREW_NO_AUTO_UPDATE"] = "1"
        self.env["DEBIAN_FRONTEND"] = "noninteractive"
        self.container_name = ""
        self.use_container = False
        self.cancelled = False
        self.current_proc: Optional[subprocess.Popen] = None

    def build(self):
        self.check_cancelled()
        self.logger.progress(10, f"{self.config.build_type} build started")
        self.logger.section("Checking dependencies")
        self.install_dependencies()
        self.check_cancelled()
        self.logger.section("Configuring CMake")
        self.configure()
        self.check_cancelled()
        self.logger.section("Updating translations")
        self.update_translations()
        self.check_cancelled()
        self.logger.section("Compiling")
        self.compile()
        self.check_cancelled()
        self.logger.section("Packaging")
        self.package()
        self.check_cancelled()
        self.logger.section("Creating archive")
        self.archive()
        self.logger.progress(100, "Complete")

    def check_cancelled(self):
        if self.cancelled:
            raise RuntimeError("Build cancelled")

    def cancel(self):
        self.cancelled = True
        proc = self.current_proc
        if proc and proc.poll() is None:
            self.logger.log("Stopping running command...")
            try:
                if os.name != "nt":
                    os.killpg(proc.pid, signal.SIGTERM)
                else:
                    proc.terminate()
            except ProcessLookupError:
                pass
            except Exception:
                proc.terminate()

    def get_cmake_cmd(self) -> str:
        return "cmake"

    def install_dependencies(self):
        pass

    def configure(self):
        self.run_cmd(self.get_cmake_config_cmd())

    def update_translations(self):
        self.run_cmd([self.get_cmake_cmd(), "--build", str(self.config.work_dir), "--target", "AviQtl_lupdate"])

    def compile(self):
        j = multiprocessing.cpu_count()
        self.logger.log(f"Parallel jobs: {j}")
        self.run_cmd([self.get_cmake_cmd(), "--build", str(self.config.work_dir), "-j", str(j)])

    def package(self):
        pass

    def prepare_output_dir(self):
        if self.config.output_dir.exists():
            self.remove_tree(self.config.output_dir)
        self.config.output_dir.mkdir(parents=True, exist_ok=True)

    def archive(self):
        self.config.dist_dir.mkdir(parents=True, exist_ok=True)
        archive_name = self.get_archive_name()
        self.create_zip(archive_name)
        self.logger.log(f"Archive: {self.config.dist_dir / (archive_name + '.zip')}")

    def run_cmd(self, cmd: List[str], shell: bool = False, force_host: bool = False):
        self.check_cancelled()
        in_container = self.use_container and not force_host
        display_cmd = ' '.join(cmd) if isinstance(cmd, list) else cmd

        if self.use_container:
            tag = "[Container]" if in_container else "[Host]"
            self.logger.log(f"{tag} {display_cmd}")
        else:
            self.logger.log(display_cmd)

        actual_cmd = cmd
        if in_container:
            cmd_str = shlex.join(cmd)
            cwd = os.getcwd()
            inner = f"cd {shlex.quote(cwd)} && {cmd_str}"
            actual_cmd = f"distrobox enter {self.container_name} -- bash -lc {shlex.quote(inner)}"
            shell = True

        popen_kwargs = {}
        if os.name != "nt":
            popen_kwargs["start_new_session"] = True

        proc = subprocess.Popen(
            actual_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            shell=shell,
            env=self.env,
            **popen_kwargs,
        )
        self.current_proc = proc
        try:
            for line in proc.stdout:
                self.logger.log(line.rstrip())
            proc.wait()
        finally:
            self.current_proc = None
        self.check_cancelled()
        if proc.returncode != 0:
            raise subprocess.CalledProcessError(proc.returncode, actual_cmd)

    def remove_tree(self, path: Path):
        def make_writable_and_retry(function, target, excinfo):
            try:
                os.chmod(target, stat.S_IWRITE)
                function(target)
            except Exception:
                raise excinfo[1]

        try:
            shutil.rmtree(path, onexc=make_writable_and_retry)
        except TypeError:
            def onerror(function, target, excinfo):
                make_writable_and_retry(function, target, excinfo)
            shutil.rmtree(path, onerror=onerror)

    def get_cmake_config_cmd(self) -> List[str]:
        cmd = [
            "cmake", "-B", str(self.config.work_dir), "-G", "Ninja",
            f"-DCMAKE_BUILD_TYPE={self.config.build_type}",
            f"-DAVIQTL_VERSION_MAJOR={self.config.version_major}",
            f"-DAVIQTL_VERSION_MINOR={self.config.version_minor}",
            f"-DAVIQTL_VERSION_PATCH={self.config.version_patch}",
            f"-DAVIQTL_VERSION_STRING={self.config.version_string}",
        ]
        return cmd

    def get_archive_name(self) -> str:
        return "AviQtl-Archive"

    def create_zip(self, archive_name: str):
        zip_file = self.config.dist_dir / (archive_name + ".zip")
        if zip_file.exists():
            zip_file.unlink()
        shutil.make_archive(str(self.config.dist_dir / archive_name), "zip", root_dir=self.config.output_dir)

    def setup_carla_sdk(self, is_windows: bool = False):
        sdk_dir = self.config.source_dir / "vendor" / "carla"
        inc_dir = sdk_dir / "include"
        lib_dir = sdk_dir / "lib"

        # Determine whether CarlaHost.h (dependency of CarlaNativePlugin.h) is included
        carla_host_header = inc_dir / "CarlaHost.h"
        if not carla_host_header.exists():
            if self.config.is_offline:
                self.logger.log("Carla SDK not found; skipping fetch in offline mode")
                return
            self.logger.log("Fetching Carla headers...")
            temp_clone = self.config.source_dir / ".carla_tmp"
            if temp_clone.exists():
                self.remove_tree(temp_clone)
            self.run_cmd(["git", "clone", "--depth", "1", "https://github.com/falkTX/Carla.git", str(temp_clone)], force_host=True)
            inc_dir.mkdir(parents=True, exist_ok=True)
            # Merge source/includes if it already exists
            shutil.copytree(temp_clone / "source/includes", inc_dir, dirs_exist_ok=True)
            # CarlaHost.h / CarlaUtils.h / CarlaBackend.h (dependencies of CarlaNativePlugin.h)
            # reside in source/backend/, so flat-copy them into include/
            backend_src = temp_clone / "source" / "backend"
            if backend_src.exists():
                for _hdr in list(backend_src.glob("*.h")) + list(backend_src.glob("*.hpp")):
                    shutil.copy2(str(_hdr), str(inc_dir / _hdr.name))
            self.remove_tree(temp_clone)

        windows_dlls = [
            "libcarla_standalone2.dll",
            "libcarla_native-plugin.dll",
            "libcarla_host-plugin.dll",
            "libcarla_utils.dll",
        ]
        runtime_dir = sdk_dir / "runtime"
        has_runtime = runtime_dir.exists() and all((runtime_dir / d).exists() for d in windows_dlls)
        if is_windows and not has_runtime:
            if self.config.is_offline:
                raise RuntimeError("Carla Windows runtime is incomplete; cannot auto-fetch in offline mode: " + str(runtime_dir))
            self.logger.log("Downloading Carla Windows binaries...")
            version = "2.5.10"
            url = f"https://github.com/falkTX/Carla/releases/download/v{version}/Carla-{version}-win64.zip"
            tmp_extract = sdk_dir / "_carla_extract_tmp"
            if tmp_extract.exists():
                self.remove_tree(tmp_extract)
            tmp_extract.mkdir(parents=True, exist_ok=True)
            self.download_and_extract(url, tmp_extract)

            # Find top-level directory inside ZIP (Carla-2.5.10-win64/)
            top_dirs = [d for d in tmp_extract.iterdir() if d.is_dir()]
            if not top_dirs:
                raise RuntimeError("No directory found in extracted Carla ZIP")
            extracted_root = top_dirs[0]

            # Save Carla/ subdirectory to runtime/
            carla_subdir = extracted_root / "Carla"
            if not carla_subdir.exists():
                raise RuntimeError(f"Carla/ subdirectory not found in Carla ZIP: {extracted_root}")
            if runtime_dir.exists():
                self.remove_tree(runtime_dir)
            shutil.copytree(str(carla_subdir), str(runtime_dir))

            # Copy DLLs to lib/ (for CMake find_library and linking)
            lib_dir.mkdir(parents=True, exist_ok=True)
            for dll_name in windows_dlls:
                src = runtime_dir / dll_name
                if src.exists():
                    shutil.copy2(str(src), str(lib_dir / dll_name))
                else:
                    self.logger.log(f"  Warning: {dll_name} not found in runtime/")

            self.remove_tree(tmp_extract)
            self.logger.log(f"Carla runtime setup complete: {runtime_dir}")


    def download_and_extract(self, url: str, dest_dir: Path):
        tmp_file = dest_dir / "download.tmp"
        try:
            self.logger.log(f"  Download: {url}")
            req = urllib.request.Request(
                url,
                headers={"User-Agent": "AviQtl-BUILD/1.0"},
            )
            timeout_seconds = 120
            with urllib.request.urlopen(req, timeout=timeout_seconds) as resp:
                with open(tmp_file, "wb") as out:
                    while True:
                        chunk = resp.read(65536)
                        if not chunk:
                            break
                        out.write(chunk)
            self.logger.log(f"  Extracting to {dest_dir}...")

            # shutil.unpack_archive auto-detects .tgz (tar.gz), but
            # some environments may not recognize .tgz, so specify format explicitly
            fmt = None
            if url.endswith(".tgz") or url.endswith(".tar.gz"):
                fmt = "gztar"
            elif url.endswith(".zip"):
                fmt = "zip"

            shutil.unpack_archive(str(tmp_file), str(dest_dir), format=fmt)
        finally:
            if tmp_file.exists():
                tmp_file.unlink()

    def copy_assets(self, asset_dest: Path):
        for d in ["effects", "objects", "transitions", "common/shaders"]:
            src = self.config.source_dir / "ui/qml" / d
            dst = asset_dest / d
            if src.exists():
                shutil.copytree(src, dst, ignore=shutil.ignore_patterns("*.frag", "*.vert", "*.comp", "*.glsl"), dirs_exist_ok=True)
        for d in ["effects", "objects", "transitions", "common/shaders"]:
            qsb_src = self.config.work_dir / d
            qsb_dst = asset_dest / d
            if qsb_src.exists():
                for f in qsb_src.glob("*.qsb"):
                    shutil.copy2(f, qsb_dst / f.name)
        plugins_src = self.config.source_dir / "plugins"
        if plugins_src.exists() and any(plugins_src.iterdir()):
            shutil.copytree(plugins_src, self.config.output_dir / "plugins", dirs_exist_ok=True)
        i18n_dest = asset_dest / "i18n"
        i18n_dest.mkdir(parents=True, exist_ok=True)
        for qm in self.config.work_dir.rglob("*.qm"):
            if "CMakeFiles" not in qm.parts:
                shutil.copy2(qm, i18n_dest / qm.name)
        # Copy effect packages (source files + compiled .qsb)
        effect_packages_src = self.config.source_dir / "effect-packages"
        if effect_packages_src.exists():
            effect_packages_dest = asset_dest / "effect-packages"
            # Copy source files (exclude raw shaders)
            shutil.copytree(effect_packages_src, effect_packages_dest,
                          ignore=shutil.ignore_patterns("*.frag", "*.vert", "*.comp", "*.glsl"),
                          dirs_exist_ok=True)
            # Copy compiled .qsb files from build directory
            effect_packages_build = self.config.work_dir / "effect-packages"
            if effect_packages_build.exists():
                for qsb_file in effect_packages_build.rglob("*.qsb"):
                    rel_path = qsb_file.relative_to(effect_packages_build)
                    dest_file = effect_packages_dest / rel_path
                    dest_file.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(qsb_file, dest_file)


class LinuxBuilderBase(PlatformBuilder):
    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        # Relax checks so it runs even in environments without containers (e.g. CI)
        self.use_container = config.use_container
        self.image_name = ""

    def warmup_container(self):
        self.logger.log(f"Waiting for container init (distrobox init)...")
        self.run_cmd(["true"])
        self.logger.log("Container init complete")

    def create_container(self):
        if not (shutil.which("distrobox") and shutil.which("podman")):
            raise RuntimeError("distrobox or podman not found")
        self.logger.log(f"Preparing container '{self.container_name}'...")
        try:
            self.run_cmd(
                ["distrobox", "create", "--name", self.container_name, "--image", self.image_name, "--yes"],
                force_host=True,
            )
        except subprocess.CalledProcessError:
            self.logger.log("Container already exists. Using as-is.")
        self.warmup_container()

    def install_dependencies(self):
        if not self.use_container:
            return
        if self.config.is_offline:
            self.logger.log("Skipping dependency installation (--offline)")
            self.warmup_container()
            return
        self.create_container()

    def get_cmake_config_cmd(self) -> List[str]:
        cmd = super().get_cmake_config_cmd()
        cmd.extend(["-DCMAKE_C_COMPILER=clang", "-DCMAKE_CXX_COMPILER=clang++"]) # clang/clang++ for ArchBuilder only
        if not self.config.is_debug:
            cmd.extend([
                "-DCMAKE_CXX_FLAGS=-O3 -flto -fno-semantic-interposition -funsafe-math-optimizations",
                "-DCMAKE_POLICY_DEFAULT_CMP0056=NEW",
                "-DCMAKE_SKIP_INSTALL_RPATH=ON",
            ])
        cmd.append(str(self.config.source_dir))
        return cmd

    def package(self):
        self.prepare_output_dir()
        dest_bin = self.config.output_dir / "AviQtl"
        # CMAKE_RUNTIME_OUTPUT_DIRECTORY = bin/, so binary is generated under bin/
        src_bin = self.config.work_dir / "bin" / "AviQtl"
        if dest_bin.exists():
            dest_bin.unlink()
        if not src_bin.exists():
            raise FileNotFoundError(f"Executable not found: {src_bin}")
        shutil.copy2(src_bin, dest_bin)
        self.copy_assets(self.config.output_dir)
        self.logger.log(f"Executable: {dest_bin}")


class ArchBuilder(LinuxBuilderBase):
    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        self.container_name = "archlinux-aviqtl"
        self.image_name = "archlinux:latest"

    def install_dependencies(self):
        if not self.use_container:
            self.logger.log("No Container mode: skipping system package installation")
            return

        super().install_dependencies()
        if self.config.is_offline:
            return
        self.logger.log("Checking pacman lock file...")
        try:
            self.run_cmd(["sudo", "rm", "-f", "/var/lib/pacman/db.lck"])
        except Exception:
            pass
        self.logger.log("Running pacman -Syu --needed...")
        deps = [
            "base-devel", "git", "cmake", "ninja", "clang", "mold", "zip",
            "mesa", "vulkan-devel", "spirv-tools", "libxkbcommon", "wayland", "wayland-protocols",
            "libffi", "ffmpeg", "luajit", "fftw",
            "qt6-base", "qt6-wayland", "qt6-declarative", "qt6-quick3d", "qt6-multimedia",
            "qt6-shadertools", "qt6-svg", "qt6-5compat", "qt6-tools",
            "lilv", "ladspa", "carla",
            "openmp", "extra-cmake-modules",
            "fluidsynth",
        ]
        self.run_cmd(["sudo", "pacman", "-Syu", "--needed", "--noconfirm"] + deps)
        self.logger.log("Arch Linux dependency installation complete")

    def get_archive_name(self) -> str:
        return "AviQtl-Arch-Linux-x86_64"


class Msys2Builder(PlatformBuilder):
    MSYS2_RUNTIME_DLLS = (
        "libgcc_s_seh-1.dll",
        "libwinpthread-1.dll",
        "libstdc++-6.dll",
        "lua51.dll",
    )
    WINDOWS_SYSTEM_DLLS = {
        "advapi32.dll", "authz.dll", "avrt.dll", "bcrypt.dll", "bcryptprimitives.dll",
        "cfgmgr32.dll", "comctl32.dll", "comdlg32.dll", "crypt32.dll", "d3d9.dll",
        "d3d11.dll", "d3d12.dll", "d3dcompiler_47.dll", "dnsapi.dll", "dwmapi.dll",
        "dwrite.dll", "dxgi.dll", "dxva2.dll", "gdi32.dll", "gdiplus.dll", "imm32.dll", "iphlpapi.dll",
        "kernel32.dll", "mpr.dll", "msimg32.dll", "msvcp_win.dll", "msvcrt.dll",
        "mf.dll", "mfplat.dll", "mfreadwrite.dll", "ncrypt.dll", "netapi32.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
        "opengl32.dll", "powrprof.dll", "propsys.dll", "rpcrt4.dll", "secur32.dll",
        "setupapi.dll", "shcore.dll", "shell32.dll", "shlwapi.dll", "user32.dll", "userenv.dll",
        "usp10.dll", "uxtheme.dll", "version.dll", "winhttp.dll", "wininet.dll",
        "winmm.dll", "winspool.drv", "ws2_32.dll", "wsock32.dll", "wtsapi32.dll",
    }

    def install_dependencies(self):
        if self.config.is_offline:
            self.logger.log("Skipping dependency installation (--offline)")
            self.setup_carla_sdk(is_windows=True)
            return
        if "MSYSTEM" not in os.environ:
            self.logger.log("Warning: Outside MSYS2 environment. Skipping dependency installation.")
            self.setup_carla_sdk(is_windows=True)
            return
        if os.environ["MSYSTEM"] != "UCRT64":
            self.logger.log("Warning: Non-UCRT64 environment detected. UCRT64 is recommended.")
        self.logger.log("Running pacman -Syu --needed...")
        deps = [
            "mingw-w64-ucrt-x86_64-toolchain", "mingw-w64-ucrt-x86_64-cmake",
            "mingw-w64-ucrt-x86_64-ninja", "git",
            "mingw-w64-ucrt-x86_64-qt6",
            "mingw-w64-ucrt-x86_64-ffmpeg", "mingw-w64-ucrt-x86_64-luajit",
            "mingw-w64-ucrt-x86_64-vulkan-loader", "mingw-w64-ucrt-x86_64-vulkan-headers",
            "mingw-w64-ucrt-x86_64-pkg-config", "mingw-w64-ucrt-x86_64-mold",
            "mingw-w64-ucrt-x86_64-lilv", "mingw-w64-ucrt-x86_64-ladspa-sdk",
            "mingw-w64-ucrt-x86_64-curl", "mingw-w64-ucrt-x86_64-extra-cmake-modules",
            "zip", "mingw-w64-ucrt-x86_64-clang-tools-extra",
        ]
        self.run_cmd(["pacman", "-Syu", "--needed", "--noconfirm"] + deps)
        self.logger.log("MSYS2 dependency installation complete")
        self.setup_carla_sdk(is_windows=True)

    def get_cmake_config_cmd(self) -> List[str]:
        cmd = super().get_cmake_config_cmd()
        if (self.config.source_dir / "vendor" / "carla").exists():
            cmd.append(f"-DCARLA_SDK_DIR={Path(self.config.source_dir / 'vendor' / 'carla').as_posix()}")
        cmd.append(str(self.config.source_dir))
        return cmd

    def package(self):
        self.prepare_output_dir()
        dest_bin = self.config.output_dir / "AviQtl.exe"
        src_bin = self.config.work_dir / "bin" / "AviQtl.exe"
        if dest_bin.exists():
            dest_bin.unlink()
        if not src_bin.exists():
            raise FileNotFoundError(f"Executable not found: {src_bin}")
        shutil.copy2(src_bin, dest_bin)
        self.copy_assets(self.config.output_dir)
        self.copy_carla_link_dlls()
        self.copy_carla_discovery_tool()
        self.copy_msys2_runtime_dlls()


        deploy_tool = "windeployqt6" if shutil.which("windeployqt6") else "windeployqt"

        deploy_exe = shutil.which(deploy_tool)
        if deploy_exe:
            qt_bin_dir = str(Path(deploy_exe).resolve().parent)
            qt_share_bin_dir = str(Path(deploy_exe).resolve().parent.parent / "share" / "qt6" / "bin")
            current_path = self.env.get("PATH", os.environ.get("PATH", ""))
            extra_dirs = []
            if qt_bin_dir not in current_path.split(os.pathsep):
                extra_dirs.append(qt_bin_dir)
            if qt_share_bin_dir not in current_path.split(os.pathsep):
                extra_dirs.append(qt_share_bin_dir)
            if extra_dirs:
                self.env["PATH"] = os.pathsep.join(extra_dirs + [current_path])
                self.logger.log(f"Added Qt bin to PATH: {', '.join(extra_dirs)}")

        self.logger.log(f"Running {deploy_tool}...")
        self.run_cmd([
            deploy_tool,
            "--qmldir", str(self.config.source_dir / "ui" / "qml"),
            "--no-translations",
            "--release" if not self.config.is_debug else "--debug",
            str(dest_bin),
            "--dir", str(self.config.output_dir),
        ])
        self.copy_msys2_dependency_dlls()
        with open(self.config.output_dir / "qt.conf", "w", encoding="utf-8") as f:
            f.write("[Paths]\nPlugins = .\n")
        self.logger.log(f"Executable: {dest_bin}")

    def copy_carla_runtime(self):
        """Bundle the Carla runtime (runtime/ directory) into the package"""
        carla_runtime = self.config.source_dir / "vendor" / "carla" / "runtime"
        if not carla_runtime.exists():
            self.logger.log("Carla runtime/ not found. Not bundling Carla")
            return
        dest = self.config.output_dir / "Carla"
        if dest.exists():
            self.remove_tree(dest)
        shutil.copytree(str(carla_runtime), str(dest))
        self.logger.log(f"Bundled Carla runtime: {dest}")

    def get_msys2_bin_dirs(self) -> List[Path]:
        dirs: List[Path] = []
        for env_name in ("MINGW_PREFIX", "MSYSTEM_PREFIX"):
            prefix = os.environ.get(env_name) or self.env.get(env_name)
            if prefix:
                dirs.append(Path(prefix) / "bin")
        for part in self.env.get("PATH", os.environ.get("PATH", "")).split(os.pathsep):
            if part:
                dirs.append(Path(part))

        seen = set()
        result: List[Path] = []
        for directory in dirs:
            key = str(directory).lower()
            if key not in seen:
                seen.add(key)
                result.append(directory)
        return result

    def find_runtime_dll(self, dll_name: str) -> Path | None:
        for directory in self.get_msys2_bin_dirs():
            candidate = directory / dll_name
            if candidate.exists():
                return candidate
        found = shutil.which(dll_name, path=self.env.get("PATH"))
        if found:
            return Path(found)
        return None

    def find_objdump(self) -> Path:
        for directory in self.get_msys2_bin_dirs():
            for name in ("objdump.exe", "objdump"):
                candidate = directory / name
                if candidate.exists():
                    return candidate
        found = shutil.which("objdump", path=self.env.get("PATH"))
        if found:
            return Path(found)
        raise FileNotFoundError("objdump was not found. Run the MSYS2 package step from an environment with binutils in PATH.")

    def is_windows_system_dll(self, dll_name: str) -> bool:
        lower = dll_name.lower()
        if lower.startswith(("api-ms-win-", "ext-ms-win-")):
            return True
        if lower in self.WINDOWS_SYSTEM_DLLS:
            return True
        windir = os.environ.get("WINDIR") or os.environ.get("SystemRoot")
        if windir:
            for subdir in ("System32", "SysWOW64"):
                if (Path(windir) / subdir / dll_name).exists():
                    return True
        return False

    def get_packaged_binary_files(self) -> List[Path]:
        binaries: List[Path] = []
        for path in self.config.output_dir.rglob("*"):
            if not path.is_file():
                continue
            try:
                relative = path.relative_to(self.config.output_dir)
            except ValueError:
                continue
            if relative.parts and relative.parts[0].lower() == "carla":
                continue
            if path.suffix.lower() in (".exe", ".dll"):
                binaries.append(path)
        return binaries

    def get_imported_dlls(self, binary: Path, objdump: Path) -> List[str]:
        try:
            result = subprocess.run(
                [str(objdump), "-p", str(binary)],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                encoding="utf-8",
                errors="replace",
                env=self.env,
                check=False,
            )
        except OSError:
            return []
        if result.returncode != 0:
            return []
        imports = []
        for line in result.stdout.splitlines():
            match = re.search(r"DLL Name:\s*(\S+)", line)
            if match:
                imports.append(match.group(1))
        return imports

    def copy_msys2_dependency_dlls(self):
        """Recursively bundle MSYS2 DLL dependencies missed by windeployqt."""
        objdump = self.find_objdump()
        self.logger.log(f"MSYS2 dependency scan with objdump: {objdump}")

        scanned: set[str] = set()
        copied: set[str] = set()
        unresolved: set[str] = set()

        while True:
            copied_this_pass = False
            for binary in self.get_packaged_binary_files():
                key = str(binary.resolve()).lower()
                if key in scanned:
                    continue
                scanned.add(key)
                for dll_name in self.get_imported_dlls(binary, objdump):
                    if self.is_windows_system_dll(dll_name):
                        continue
                    if (self.config.output_dir / dll_name).exists():
                        continue
                    src = self.find_runtime_dll(dll_name)
                    if not src:
                        unresolved.add(dll_name)
                        continue
                    shutil.copy2(src, self.config.output_dir / dll_name)
                    copied.add(dll_name)
                    copied_this_pass = True
                    unresolved.discard(dll_name)
                    self.logger.log(f"MSYS2 dependency DLL bundled: {dll_name}")
            if not copied_this_pass:
                break

        if unresolved:
            raise FileNotFoundError(
                "MSYS2 dependency DLLs were not found: "
                + ", ".join(sorted(unresolved, key=str.lower))
                + ". Check that the package step is running from the MSYS2 UCRT64 environment."
            )
        self.logger.log(f"MSYS2 dependency DLL scan complete: {len(copied)} copied, {len(scanned)} binaries scanned")

    def copy_msys2_runtime_dlls(self):
        """Copy MinGW/LuaJIT runtime DLLs that windeployqt does not always deploy."""
        missing = []
        for dll_name in self.MSYS2_RUNTIME_DLLS:
            src = self.find_runtime_dll(dll_name)
            if not src:
                missing.append(dll_name)
                continue
            shutil.copy2(src, self.config.output_dir / dll_name)
            self.logger.log(f"Bundled MSYS2 runtime: {dll_name}")
        if missing:
            raise FileNotFoundError(
                "MSYS2 runtime DLLs were not found: "
                + ", ".join(missing)
                + ". Check that you are running in the MSYS2 UCRT64 environment."
            )

    def copy_carla_link_dlls(self):
        carla_lib = self.config.source_dir / "vendor" / "carla" / "lib"
        if not carla_lib.exists():
            self.logger.log("Carla lib/ not found. Not bundling Carla link DLLs")
            return
        copied = set()
        for pattern in self.CARLA_DLL_PATTERNS:
            for dll in carla_lib.glob(pattern):
                if dll.name in copied:
                    continue
                shutil.copy2(dll, self.config.output_dir / dll.name)
                copied.add(dll.name)
                self.logger.log(f"Bundled Carla link DLL: {dll.name}")
                if dll.name in self.CARLA_DLL_ALIASES:
                    alias = self.CARLA_DLL_ALIASES[dll.name]
                    shutil.copy2(dll, self.config.output_dir / alias)
                    copied.add(alias)
                    self.logger.log(f"Bundled Carla link DLL alias: {alias}")

    def copy_carla_discovery_tool(self):
        discovery = self.find_carla_discovery_tool()
        if not discovery:
            self.logger.log("carla-discovery-native.exe not found. Audio plugin discovery will rely on PATH.")
            return
        shutil.copy2(discovery, self.config.output_dir / "carla-discovery-native.exe")
        self.logger.log(f"Bundled Carla discovery tool: {discovery}")

    def find_carla_discovery_tool(self) -> Path | None:
        carla_root = self.config.source_dir / "vendor" / "carla"
        candidates = [
            carla_root / "carla-discovery-native.exe",
            carla_root / "runtime" / "carla-discovery-native.exe",
            carla_root / "Carla" / "carla-discovery-native.exe",
        ]
        candidates.extend(carla_root.glob("**/Carla/carla-discovery-native.exe"))
        candidates.extend(carla_root.glob("**/carla-discovery-native.exe"))
        for candidate in candidates:
            if candidate.exists():
                return candidate
        return None

    def get_archive_name(self) -> str:
        return "AviQtl-MSYS2-UCRT64-x86_64"


class MsvcBuilder(PlatformBuilder):
    QT_ENV_VARS = ("QT_MSVC_DIR", "QT_DIR", "QTDIR")
    QT_VCPKG_PACKAGES = {
        "qtbase",
        "qtdeclarative",
        "qtquick3d",
        "qtmultimedia",
        "qtshadertools",
        "qtsvg",
        "qt5compat",
        "qttools",
    }

    def __init__(self, config: BuildConfig, logger: Logger):
        super().__init__(config, logger)
        if os.name != "nt":
            raise RuntimeError("MSVC build can only run on Windows")
        self.vcpkg_root: Path | None = None
        default_triplet = "x64-windows" if self.config.is_debug else "x64-windows-release"
        self.vcpkg_triplet = os.environ.get("VCPKG_DEFAULT_TRIPLET", default_triplet)
        default_host_triplet = "x64-windows" if self.config.is_debug else self.vcpkg_triplet
        self.vcpkg_host_triplet = os.environ.get("VCPKG_DEFAULT_HOST_TRIPLET", default_host_triplet)
        self.vs_install_dir: Path | None = None
        self.cmake_path: str | None = None
        self.ninja_path: str | None = None
        self.qt_prefix: Path | None = None
        self.vcpkg_manifest_dir: Path | None = None

    def install_dependencies(self):
        self.setup_msvc_environment()
        self.ensure_vcpkg()
        self.qt_prefix = self.find_qt_prefix()
        if not self.qt_prefix:
            raise RuntimeError(
                "Qt MSVC not found. Install official Qt and specify --qt-dir with the Qt root/kit, "
                "or set one of QT_MSVC_DIR, QT_DIR, QTDIR."
            )
        self.logger.log(f"Qt: {self.qt_prefix}")
        self.write_external_qt_manifest()
        self.ensure_vcpkg_triplets()
        self.prepare_vcpkg_installed_tree()
        self.setup_vcpkg_environment()
        self.cmake_path = self.find_msvc_tool("cmake")
        self.ninja_path = self.find_msvc_tool("ninja")
        if not self.cmake_path:
            raise RuntimeError("cmake not found. Add CMake to PATH")
        if not self.ninja_path:
            raise RuntimeError("ninja not found. Add Ninja to PATH")
        if not shutil.which("cl", path=self.env.get("PATH")):
            raise RuntimeError("cl.exe not found. vcvarsall.bat may not have loaded properly")
        self.setup_carla_sdk(is_windows=True)
        self.generate_carla_import_libs()

    def ensure_vcpkg(self):
        vcpkg_root_env = self.env.get("VCPKG_ROOT") or os.environ.get("VCPKG_ROOT")
        if vcpkg_root_env:
            env_root = Path(vcpkg_root_env)
            if not (env_root / "scripts").is_dir():
                raise RuntimeError(f"VCPKG_ROOT is invalid. vcpkg scripts directory not found: {env_root}")
            if not (env_root / "vcpkg.exe").exists() and not (env_root / "bootstrap-vcpkg.bat").exists():
                raise RuntimeError(f"VCPKG_ROOT is incomplete. vcpkg.exe or bootstrap-vcpkg.bat not found: {env_root}")

        self.vcpkg_root = self.find_vcpkg_root(need_executable=True)
        if self.vcpkg_root:
            self.logger.log(f"vcpkg found: {self.vcpkg_root}")
            self.env["VCPKG_ROOT"] = str(self.vcpkg_root)
            return

        incomplete = self.find_vcpkg_root(need_executable=False)
        if incomplete and (incomplete / "bootstrap-vcpkg.bat").exists():
            self.vcpkg_root = incomplete
            self.logger.log(f"vcpkg directory detected (no vcpkg.exe). Attempting bootstrap: {self.vcpkg_root}")
            self._bootstrap_vcpkg()
            self.env["VCPKG_ROOT"] = str(self.vcpkg_root)
            return

        self.vcpkg_root = self.config.source_dir / "vcpkg"
        if self.config.is_offline:
            raise RuntimeError("vcpkg not found. Cannot auto-fetch in offline mode; set VCPKG_ROOT.")
        if not shutil.which("git", path=self.env.get("PATH")):
            raise RuntimeError("git not found. Git is required for automatic vcpkg fetch. Install Git or set VCPKG_ROOT.")
        if self.vcpkg_root.exists() and not (self.vcpkg_root / "scripts").is_dir():
            self.logger.log(f"Removing incomplete vcpkg directory: {self.vcpkg_root}")
            self.remove_tree(self.vcpkg_root)
        self.logger.log(f"vcpkg not found. Cloning to {self.vcpkg_root}...")
        self.run_cmd(["git", "clone", "--depth", "1", "https://github.com/microsoft/vcpkg.git", str(self.vcpkg_root)], force_host=True)
        self._bootstrap_vcpkg()
        self.env["VCPKG_ROOT"] = str(self.vcpkg_root)

    def _bootstrap_vcpkg(self):
        bootstrap = self.vcpkg_root / "bootstrap-vcpkg.bat"
        if not bootstrap.exists():
            raise RuntimeError(f"vcpkg bootstrap script not found: {bootstrap}")
        self.logger.log("Bootstrapping vcpkg...")
        self.run_cmd([str(bootstrap)], force_host=True)
        if not (self.vcpkg_root / "vcpkg.exe").exists():
            raise RuntimeError("vcpkg bootstrap failed. vcpkg.exe was not generated. Check network connection.")
        self.logger.log("vcpkg ready")

    def prepare_vcpkg_installed_tree(self):
        installed_root = self.config.work_dir / "vcpkg_installed"
        marker = self.config.work_dir / ".vcpkg-triplets"
        expected = (
            f"target={self.vcpkg_triplet}\n"
            f"host={self.vcpkg_host_triplet}\n"
            f"qt={self.qt_prefix or 'vcpkg'}\n"
        )
        status_file = installed_root / "vcpkg" / "status"
        info_dir = installed_root / "vcpkg" / "info"
        has_installed_status = (
            status_file.exists()
            and "Status: install ok installed" in status_file.read_text(encoding="utf-8", errors="ignore")
        )
        has_package_lists = info_dir.exists() and any(info_dir.glob("*.list"))
        is_consistent = not has_installed_status or has_package_lists
        if marker.exists() and marker.read_text(encoding="utf-8") == expected and is_consistent:
            return

        if installed_root.exists():
            self.logger.log("Removing old or incomplete vcpkg_installed")
            self.remove_tree(installed_root)
        self.config.work_dir.mkdir(parents=True, exist_ok=True)
        marker.write_text(expected, encoding="utf-8")

    def write_external_qt_manifest(self):
        manifest_in = self.config.source_dir / "vcpkg.json"
        manifest_out_dir = self.config.work_dir / "vcpkg-manifest"
        manifest_out = manifest_out_dir / "vcpkg.json"
        if manifest_in.exists():
            data = json.loads(manifest_in.read_text(encoding="utf-8"))
        else:
            self.logger.log(f"Warning: {manifest_in} not found. Using fallback configuration.")
            data = {
                "name": "aviqtl",
                "dependencies": ["ffmpeg", "luajit", "vulkan", "ecm", "pkgconf"],
                "builtin-baseline": "99a97de2cb371449d4fb9dc970f2ac562d689ec2",
            }
        data["name"] = f"{data.get('name', 'aviqtl')}-msvc-external-qt"
        data["dependencies"] = [
            dep for dep in data.get("dependencies", [])
            if (dep if isinstance(dep, str) else dep.get("name")) not in self.QT_VCPKG_PACKAGES
        ]
        manifest_out_dir.mkdir(parents=True, exist_ok=True)
        manifest_out.write_text(json.dumps(data, indent=4) + "\n", encoding="utf-8")
        self.vcpkg_manifest_dir = manifest_out_dir

    def ensure_vcpkg_triplets(self):
        triplets_dir = self.config.source_dir / "triplets"
        triplets_content = "set(VCPKG_TARGET_ARCHITECTURE x64)\nset(VCPKG_CRT_LINKAGE dynamic)\nset(VCPKG_LIBRARY_LINKAGE dynamic)\nset(VCPKG_BUILD_TYPE release)\n"
        for name in ("x64-windows-release.cmake", "x64-windows.cmake"):
            path = triplets_dir / name
            if not path.exists():
                triplets_dir.mkdir(parents=True, exist_ok=True)
                path.write_text(triplets_content, encoding="utf-8")
                self.logger.log(f"Generated vcpkg triplet: {path}")

    def get_cmake_cmd(self) -> str:
        return self.cmake_path or "cmake"

    def generate_carla_import_libs(self):
        """MSVC cannot directly link MinGW-built Carla DLLs, so generate import libs (.lib + .def) via dumpbin /exports and lib.exe /DEF."""
        carla_lib_dir = self.config.source_dir / "vendor" / "carla" / "lib"
        carla_def_dir = self.config.source_dir / "vendor" / "carla" / "def"
        if not carla_lib_dir.exists():
            return
        dumpbin_exe = shutil.which("dumpbin", path=self.env.get("PATH"))
        lib_exe = shutil.which("lib", path=self.env.get("PATH"))
        if not dumpbin_exe or not lib_exe:
            self.logger.log("dumpbin.exe or lib.exe not found; skipping Carla import lib generation")
            self.logger.log(f"  dumpbin={dumpbin_exe}, lib={lib_exe}")
            return
        generated = 0
        for dll_name in [
            "libcarla_standalone2.dll",
            "libcarla_native-plugin.dll",
            "libcarla_host-plugin.dll",
            "libcarla_utils.dll",
        ]:
            dll_path = carla_lib_dir / dll_name
            if not dll_path.exists():
                continue
            lib_name = dll_name.replace(".dll", ".lib")
            def_name = dll_name.replace(".dll", ".def")
            lib_path = carla_lib_dir / lib_name
            def_path = carla_def_dir / def_name
            # Skip if .lib already exists and is newer than DLL
            if lib_path.exists() and lib_path.stat().st_mtime >= dll_path.stat().st_mtime:
                continue
            self.logger.log(f"  Generating import lib for {dll_name}")
            # 1. dumpbin /exports
            carla_def_dir.mkdir(parents=True, exist_ok=True)
            def_content = ["LIBRARY " + dll_name, "EXPORTS"]
            proc = subprocess.run(
                [dumpbin_exe, "/exports", str(dll_path)],
                capture_output=True, text=True, encoding="utf-8", errors="replace",
                env=self.env,
            )
            if proc.returncode != 0:
                self.logger.log(f"    dumpbin failed: {proc.stderr}")
                continue
            for line in proc.stdout.splitlines():
                # Format: [ordinal] [hint] [RVA] [name]
                parts = line.split()
                if len(parts) >= 4 and parts[0].isdigit():
                    symbol = parts[3]
                    # Skip forwarders (contain '=')
                    if "=" in symbol:
                        continue
                    # Heuristic: exported function names
                    if not symbol.startswith("_"):
                        def_content.append(f"    {symbol}")
            def_path.write_text("\n".join(def_content) + "\n", encoding="utf-8")
            # 2. lib /DEF:... /MACHINE:X64 /OUT:...
            proc2 = subprocess.run(
                [lib_exe, f"/DEF:{def_path}", "/MACHINE:X64", f"/OUT:{lib_path}", "/NOLOGO"],
                capture_output=True, text=True, encoding="utf-8", errors="replace",
            )
            if proc2.returncode != 0:
                self.logger.log(f"    lib.exe failed: {proc2.stderr}")
                continue
            generated += 1
        if generated:
            self.logger.log(f"Carla import lib generation complete: {generated} file(s)")

    def find_vcvarsall(self) -> Path | None:
        candidates = []
        for var in ("VCVARSALL", "VCVARSALL_BAT"):
            value = os.environ.get(var)
            if value:
                candidates.append(Path(value))

        vswhere_roots = [
            Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe",
            Path(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        ]
        for vswhere in vswhere_roots:
            if not vswhere.exists():
                continue
            for args in (
                ["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
                ["-latest", "-products", "*", "-property", "installationPath"],
            ):
                try:
                    result = subprocess.run([str(vswhere)] + args, capture_output=True, text=True, encoding=locale.getpreferredencoding(False), errors="replace")
                except OSError:
                    continue
                if result.returncode == 0 and result.stdout.strip():
                    candidates.append(Path(result.stdout.strip()) / "VC" / "Auxiliary" / "Build" / "vcvarsall.bat")

        for var in ("VSINSTALLDIR", "VCINSTALLDIR"):
            value = os.environ.get(var)
            if value:
                root = Path(value)
                candidates.append(root / "VC" / "Auxiliary" / "Build" / "vcvarsall.bat")
                candidates.append(root.parent / "Auxiliary" / "Build" / "vcvarsall.bat")

        candidates.extend([
            Path(r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"),
            Path(r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"),
            Path(r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvarsall.bat"),
            Path(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat"),
        ])

        for candidate in candidates:
            if candidate.exists():
                return candidate
        return None

    def setup_msvc_environment(self):
        vcvarsall = self.find_vcvarsall()
        if not vcvarsall:
            raise RuntimeError("vcvarsall.bat not found. Install the Visual Studio Build Tools with the C++ toolset")
        self.logger.log(f"vcvarsall: {vcvarsall}")
        self.vs_install_dir = vcvarsall.parents[3]
        wrapper_path = None
        try:
            with tempfile.NamedTemporaryFile("w", suffix=".bat", delete=False, encoding="utf-8") as wrapper:
                wrapper.write("@echo off\n")
                wrapper.write(f'call "{vcvarsall}" x64 > nul\n')
                wrapper.write("if errorlevel 1 exit /b %errorlevel%\n")
                wrapper.write("set\n")
                wrapper_path = wrapper.name
            proc = subprocess.run(
                ["cmd.exe", "/d", "/c", wrapper_path],
                capture_output=True,
                text=True,
                encoding=locale.getpreferredencoding(False),
                errors="replace",
            )
            if proc.returncode != 0:
                raise RuntimeError(f"vcvarsall.bat execution failed:\n{proc.stdout}\n{proc.stderr}")
            for line in proc.stdout.splitlines():
                if "=" not in line:
                    continue
                key, _, value = line.partition("=")
                self.env[key] = value
            if "Path" in self.env:
                self.env["PATH"] = self.env["Path"]
            elif "PATH" in self.env:
                self.env["Path"] = self.env["PATH"]
            self.sanitize_msvc_environment()
        finally:
            if wrapper_path:
                try:
                    Path(wrapper_path).unlink()
                except OSError:
                    pass

    def is_msys_path(self, path: str | None) -> bool:
        if not path:
            return False
        lowered = path.replace("/", "\\").lower()
        return "\\msys2\\" in lowered or "\\mingw" in lowered or "\\ucrt64\\" in lowered

    def sanitize_msvc_environment(self):
        for name in ("PATH", "Path", "PKG_CONFIG_PATH", "CMAKE_PREFIX_PATH"):
            value = self.env.get(name)
            if not value:
                continue
            filtered = [part for part in value.split(os.pathsep) if part and not self.is_msys_path(part)]
            if filtered:
                self.env[name] = os.pathsep.join(filtered)
            else:
                self.env.pop(name, None)
        if "PATH" in self.env:
            self.env["Path"] = self.env["PATH"]
        elif "Path" in self.env:
            self.env["PATH"] = self.env["Path"]

    def find_msvc_tool(self, name: str) -> str | None:
        exe = f"{name}.exe"
        env_name = f"{name.upper()}_EXE"
        if self.env.get(env_name) and Path(self.env[env_name]).exists():
            return self.env[env_name]

        candidates = []
        if self.vs_install_dir:
            if name == "cmake":
                candidates.append(self.vs_install_dir / "Common7" / "IDE" / "CommonExtensions" / "Microsoft" / "CMake" / "CMake" / "bin" / exe)
            elif name == "ninja":
                candidates.append(self.vs_install_dir / "Common7" / "IDE" / "CommonExtensions" / "Microsoft" / "CMake" / "Ninja" / exe)
        if self.vcpkg_root:
            candidates.append(self.vcpkg_root / "downloads" / "tools" / name / exe)

        for candidate in candidates:
            if candidate.exists():
                return str(candidate)

        found = shutil.which(exe, path=self.env.get("PATH"))
        if found and not self.is_msys_path(found):
            return found
        if found:
            self.logger.log(f"Warning: Detected MSYS2/MinGW {exe}, not using it for MSVC build: {found}")
        return None

    def find_vcpkg_root(self, *, need_executable: bool = True) -> Path | None:
        candidates = []
        vcpkg_root_env = self.env.get("VCPKG_ROOT") or os.environ.get("VCPKG_ROOT")
        if vcpkg_root_env:
            candidates.append(Path(vcpkg_root_env))
        candidates.extend([
            Path.home() / "vcpkg",
            Path(r"C:\vcpkg"),
            self.config.source_dir / "vcpkg",
        ])
        if self.vs_install_dir:
            candidates.append(self.vs_install_dir / "VC" / "vcpkg")
        for candidate in candidates:
            if not (candidate / "scripts").is_dir():
                continue
            if need_executable and not (candidate / "vcpkg.exe").exists():
                continue
            return candidate
        return None

    def vcpkg_installed_dir(self) -> Path:
        return self.config.work_dir / "vcpkg_installed" / self.vcpkg_triplet

    def setup_vcpkg_environment(self):
        if not self.vcpkg_root:
            return
        installed = self.vcpkg_installed_dir()
        self.env["VCPKG_DEFAULT_TRIPLET"] = self.vcpkg_triplet
        self.env["VCPKG_DEFAULT_HOST_TRIPLET"] = self.vcpkg_host_triplet
        paths = [
            installed / "bin",
            installed / "tools" / "pkgconf",
            installed / "tools" / "pkg-config",
        ]
        if self.qt_prefix:
            paths.insert(0, self.qt_prefix / "bin")
        else:
            paths.append(installed / "tools" / "Qt6" / "bin")
        self.env["PATH"] = os.pathsep.join([str(path) for path in paths if path.exists()] + [self.env.get("PATH", "")])
        self.env["Path"] = self.env["PATH"]
        pkg_paths = [installed / "lib" / "pkgconfig"]
        if self.config.is_debug:
            pkg_paths.append(installed / "debug" / "lib" / "pkgconfig")
        existing_pkg_path = self.env.get("PKG_CONFIG_PATH", "")
        self.env["PKG_CONFIG_PATH"] = os.pathsep.join([str(path) for path in pkg_paths if path.exists()] + ([existing_pkg_path] if existing_pkg_path else []))
        self.logger.log(f"vcpkg: {self.vcpkg_root} (target={self.vcpkg_triplet}, host={self.vcpkg_host_triplet}), installed: {installed}")

    def resolve_qt_prefix(self, path: Path) -> Path | None:
        if (path / "bin" / "windeployqt.exe").exists():
            return path
        candidates = sorted(path.glob("*/*msvc*_64/bin/windeployqt.exe"), reverse=True)
        if candidates:
            return candidates[0].parent.parent
        return None

    def find_qt_prefix(self) -> Path | None:
        if self.config.qt_dir:
            qt_prefix = self.resolve_qt_prefix(self.config.qt_dir)
            if qt_prefix:
                return qt_prefix
            raise RuntimeError(f"Could not detect MSVC Qt from the specified Qt directory: {self.config.qt_dir}")

        for name in self.QT_ENV_VARS:
            value = self.env.get(name) or os.environ.get(name)
            if value:
                qt_prefix = self.resolve_qt_prefix(Path(value))
                if qt_prefix:
                    return qt_prefix
        deployqt = shutil.which("windeployqt", path=self.env.get("PATH"))
        if deployqt and not self.is_msys_path(deployqt):
            return Path(deployqt).parent.parent
        if deployqt:
            self.logger.log(f"Warning: Detected MSYS2/MinGW windeployqt, not using it for MSVC build: {deployqt}")
        for qt_root in (Path(r"C:\Qt"),):
            if qt_root.exists():
                qt_prefix = self.resolve_qt_prefix(qt_root)
                if qt_prefix:
                    return qt_prefix
        return None

    def get_cmake_config_cmd(self) -> List[str]:
        cmd = super().get_cmake_config_cmd()
        if self.ninja_path:
            cmd.append(f"-DCMAKE_MAKE_PROGRAM={self.ninja_path}")

        cmd.extend(["-DCMAKE_C_COMPILER=cl", "-DCMAKE_CXX_COMPILER=cl"])
        if self.vcpkg_root:
            cmd.extend([
                f"-DCMAKE_TOOLCHAIN_FILE={self.vcpkg_root / 'scripts/buildsystems/vcpkg.cmake'}",
                f"-DVCPKG_TARGET_TRIPLET={self.vcpkg_triplet}",
                f"-DVCPKG_HOST_TRIPLET={self.vcpkg_host_triplet}",
                f"-DVCPKG_OVERLAY_TRIPLETS={self.config.source_dir / 'triplets'}",
            ])
            if self.vcpkg_manifest_dir:
                cmd.append(f"-DVCPKG_MANIFEST_DIR={self.vcpkg_manifest_dir}")
        if self.qt_prefix:
            cmd.append(f"-DCMAKE_PREFIX_PATH={self.qt_prefix}")
        if (self.config.source_dir / "vendor" / "carla").exists():
            cmd.append(f"-DCARLA_SDK_DIR={Path(self.config.source_dir / 'vendor' / 'carla').as_posix()}")
        cmd.append(str(self.config.source_dir))
        return cmd

    def find_windeployqt(self) -> str | None:
        if self.qt_prefix:
            deployqt = self.qt_prefix / "bin" / "windeployqt.exe"
            if deployqt.exists():
                return str(deployqt)
        deployqt = shutil.which("windeployqt", path=self.env.get("PATH"))
        if deployqt and not self.is_msys_path(deployqt):
            return deployqt
        if deployqt:
            self.logger.log(f"Warning: Detected MSYS2/MinGW windeployqt, not using it for MSVC build: {deployqt}")
        return None

    def package(self):
        self.prepare_output_dir()
        dest_bin = self.config.output_dir / "AviQtl.exe"
        src_bin = self.config.work_dir / "bin" / "AviQtl.exe"
        if dest_bin.exists():
            dest_bin.unlink()
        if not src_bin.exists():
            raise FileNotFoundError(f"Executable not found: {src_bin}")
        shutil.copy2(src_bin, dest_bin)
        self.copy_assets(self.config.output_dir)
        self.copy_carla_runtime()

        deployqt = self.find_windeployqt()
        if not deployqt:
            raise RuntimeError("windeployqt not found. Add the Qt MSVC bin directory to PATH or set QT_MSVC_DIR")
        self.logger.log("Running windeployqt...")
        self.run_cmd([
            deployqt,
            "--qmldir", str(self.config.source_dir / "ui/qml"),
            "--no-translations", "--no-compiler-runtime",
            "--release" if not self.config.is_debug else "--debug",
            str(dest_bin),
            "--dir", str(self.config.output_dir),
        ])
        with open(self.config.output_dir / "qt.conf", "w", encoding="utf-8") as f:
            f.write("[Paths]\nPlugins = .\n")
        self.logger.log(f"Executable: {dest_bin}")

    def get_archive_name(self) -> str:
        return "AviQtl-MSVC-x86_64"

    def copy_carla_runtime(self):
        carla_lib = self.config.source_dir / "vendor" / "carla" / "lib"
        if not carla_lib.exists():
            return
        copied = set()
        for pattern in self.CARLA_DLL_PATTERNS:
            for dll in carla_lib.glob(pattern):
                if dll.name in copied:
                    continue
                shutil.copy2(dll, self.config.output_dir / dll.name)
                copied.add(dll.name)
                if dll.name in self.CARLA_DLL_ALIASES:
                    alias = self.CARLA_DLL_ALIASES[dll.name]
                    shutil.copy2(dll, self.config.output_dir / alias)
                    copied.add(alias)
        discovery = self.find_carla_discovery_tool()
        if discovery:
            shutil.copy2(discovery, self.config.output_dir / "carla-discovery-native.exe")
            copied.add("carla-discovery-native.exe")
        if copied:
            self.logger.log(f"Bundled Carla runtime: {len(copied)} files")

    def find_carla_discovery_tool(self) -> Path | None:
        carla_root = self.config.source_dir / "vendor" / "carla"
        candidates = [
            carla_root / "carla-discovery-native.exe",
            carla_root / "Carla" / "carla-discovery-native.exe",
        ]
        candidates.extend(carla_root.glob("**/Carla/carla-discovery-native.exe"))
        candidates.extend(carla_root.glob("**/carla-discovery-native.exe"))
        for candidate in candidates:
            if candidate.exists():
                return candidate
        return None


class XcodeBuilder(PlatformBuilder):
    def install_dependencies(self):
        if self.config.is_offline:
            self.logger.log("Skipping dependency installation (--offline)")
            return
        if not shutil.which("brew"):
            raise RuntimeError("Homebrew not found")
        self.logger.log("Running brew install...")
        deps = [
            "cmake", "ninja", "qt6", "ffmpeg", "luajit",
            "vulkan-headers", "vulkan-loader", "spirv-tools", "pkg-config",
            "lilv", "extra-cmake-modules", "carla",
        ]
        self.run_cmd(["brew", "install"] + deps)
        self.logger.log("macOS dependency installation complete")

        # macOS uses Universal binaries

    def get_cmake_config_cmd(self) -> List[str]:
        cmd = super().get_cmake_config_cmd()
        try:
            brew_prefix = subprocess.check_output(["brew", "--prefix"], text=True).strip()
        except Exception:
            brew_prefix = "/opt/homebrew"
        cmd.append(f"-DCMAKE_PREFIX_PATH={brew_prefix}")
        cmd.append(str(self.config.source_dir))
        return cmd

    def package(self):
        self.prepare_output_dir()
        src_app = self.config.work_dir / "bin" / "AviQtl.app"
        dest_app = self.config.output_dir / "AviQtl.app"
        if dest_app.exists():
            self.remove_tree(dest_app)
        if not src_app.exists():
            raise FileNotFoundError(f"App bundle not found: {src_app}")
        shutil.copytree(src_app, dest_app)
        self.copy_assets(dest_app / "Contents/Resources")
        self.logger.log("Running macdeployqt...")
        qt_prefix = subprocess.check_output(["brew", "--prefix", "qt6"], text=True).strip()
        self.run_cmd([
            f"{qt_prefix}/bin/macdeployqt", str(dest_app),
            f"-qmldir={self.config.source_dir / 'ui/qml'}",
            "-verbose=1",
            "-no-codesign",
        ])

        self.logger.log("Fixing RPATH...")
        binary = dest_app / "Contents/MacOS/AviQtl"
        rpaths_to_remove = [
            "/opt/homebrew/lib",
        ]
        for rp in rpaths_to_remove:
            try:
                self.run_cmd(["install_name_tool", "-delete_rpath", rp, str(binary)])
                self.logger.log(f"  Removing: {rp}")
            except subprocess.CalledProcessError:
                self.logger.log(f"  Skipping: {rp}")

        self.logger.log("Cleaning up duplicate QML module copies...")
        duplicate_qml_dirs = [
            dest_app / "Contents/Resources/qml/AviQtl",
            dest_app / "Contents/PlugIns/qml/AviQtl",
        ]
        for d in duplicate_qml_dirs:
            if d.exists():
                self.remove_tree(d)
                self.logger.log(f"  Removing: {d}")

        # Bundle carla-discovery-native binary
        self.logger.log("Bundling carla-discovery-native...")
        try:
            carla_prefix = subprocess.check_output(["brew", "--prefix", "carla"], text=True).strip()
            # Search paths matching Homebrew structure
            carla_bin_src = Path(carla_prefix) / "lib/carla/carla-discovery-native"
            if not carla_bin_src.exists():
                 carla_bin_src = next(Path(carla_prefix).glob("**/carla-discovery-native"), None)

            carla_bin_dst = dest_app / "Contents/MacOS/carla-discovery-native"
            if carla_bin_src and carla_bin_src.exists():
                shutil.copy2(carla_bin_src, carla_bin_dst)
                self.logger.log(f"  Bundled: {carla_bin_src}")
                self.fix_bundled_binary_rpaths(carla_bin_dst, dest_app / "Contents/Frameworks")
        except Exception as e:
            self.logger.log(f"  Warning: could not locate carla-discovery-native: {e}")

        self.logger.log("Running codesign...")
        self.run_cmd(["codesign", "--deep", "--force", "--sign", "-", str(dest_app)])
        self.logger.log(f"App bundle: {dest_app}")

    def fix_bundled_binary_rpaths(self, binary: Path, frameworks_dir: Path):
        """Fix hardcoded Homebrew absolute paths in a copied binary to use @executable_path/../Frameworks/."""
        try:
            result = subprocess.run(
                ["otool", "-L", str(binary)],
                capture_output=True, text=True, encoding="utf-8", errors="replace",
            )
        except OSError:
            self.logger.log(f"  Warning: otool not found, skipping RPATH fix for {binary.name}")
            return

        bundled_names = {f.name for f in frameworks_dir.iterdir()} if frameworks_dir.exists() else set()

        for line in result.stdout.splitlines():
            line = line.strip()
            if not line or line.startswith(str(binary)):
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            lib_path = parts[0]
            # Skip system libraries and already-relative paths
            if lib_path.startswith(("@executable_path", "@rpath", "@loader_path", "/usr/lib", "/System")):
                continue
            # Only fix paths that point to Homebrew
            if "/opt/homebrew/" not in lib_path and "/usr/local/" not in lib_path:
                continue
            lib_name = Path(lib_path).name
            if lib_name not in bundled_names:
                self.logger.log(f"  Warning: {lib_name} not in Frameworks, cannot fix RPATH for {binary.name}")
                continue
            new_path = f"@executable_path/../Frameworks/{lib_name}"
            self.run_cmd(["install_name_tool", "-change", lib_path, new_path, str(binary)])
            self.logger.log(f"  Fixed: {lib_path} -> {new_path}")

    def get_archive_name(self) -> str:
        return "AviQtl-macOS-Xcode-Universal"

    def create_zip(self, archive_name: str):
        shutil.make_archive(
            str(self.config.dist_dir / archive_name), "zip",
            root_dir=self.config.output_dir, base_dir="AviQtl.app",
        )


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
            logger = Logger(self._enqueue_log, self._enqueue_progress)
            self.builder = BUILDERS[self.config.target](self.config, logger)
            if self.cancel_requested:
                self.builder.cancel()
            self.builder.build()
            self.success = True
            self.error_msg = "Build succeeded"
        except Exception as e:
            self.success = False
            self.error_msg = str(e)
        finally:
            self.log_queue.put(None)  # sentinel
            self.finished_event.set()

    def _enqueue_log(self, msg: str):
        self.log_queue.put(("log", msg))

    def _enqueue_progress(self, val: int, msg: str):
        self.log_queue.put(("progress", val, msg))

    def cancel(self):
        self.cancel_requested = True
        if self.builder:
            self.builder.cancel()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="BUILD.py",
        description="AviQtl build script",
        formatter_class=argparse.RawTextHelpFormatter,
        epilog=(
            "Examples:\n"
            "  python BUILD.py --arch\n"
            "  python BUILD.py --msys2 --debug\n"
            "  python BUILD.py --msvc --qt-dir C:\\Qt\n"
            "  python BUILD.py  # Windows defaults to MSYS2\n"
            "  python BUILD.py --xcode --offline\n"
        ),
    )
    target_group = parser.add_mutually_exclusive_group(required=False)
    target_group.add_argument(
        "--arch", action="store_true",
        help="Build for Linux (Arch)",
    )
    target_group.add_argument(
        "--msys2", action="store_true",
        help="Build for Windows (MSYS2)",
    )
    target_group.add_argument(
        "--msvc", action="store_true",
        help="Build for Windows (MSVC x64). Auto-detects vcvarsall.bat to load the MSVC environment.",
    )
    target_group.add_argument(
        "--xcode", action="store_true",
        help="Build for macOS (Xcode)",
    )
    parser.add_argument(
        "--offline", action="store_true",
        help="Skip downloading/installing dependencies. Use when the environment is already set up.",
    )
    parser.add_argument(
        "--debug", action="store_true",
        help="Perform a Debug build. Default is Release.",
    )
    parser.add_argument(
        "--no-container", action="store_true",
        help="Build for Linux target on the host without a container (not recommended).",
    )
    parser.add_argument(
        "--qt-dir", type=Path,
        help="Official Qt directory for MSVC build. Only used with --msvc. When unspecified, checks QT_MSVC_DIR, QT_DIR, QTDIR, PATH.",
    )
    parser.add_argument(
        "--version", type=str, default="0.0.0",
        help="Specify the application version (e.g. 0.1.0 or 0.1.0-Anon)."
    )
    return parser.parse_args()


def determine_target(args: argparse.Namespace, system_name: str | None = None) -> str | None:
    if args.arch:
        return "arch"
    if args.msys2:
        return "msys2"
    if args.msvc:
        return "msvc"
    if args.xcode:
        return "xcode"
    system_name = (system_name or platform.system()).lower()
    return {
        "linux": "arch",
        "windows": "msys2",
        "darwin": "xcode",
    }.get(system_name)


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")

    args = parse_args()

    # Determine target
    target = determine_target(args)

    if not target:
        print("Error: Could not determine build target. Specify one of --arch, --msys2, --msvc, --xcode.")
        sys.exit(1)

    source_dir = Path.cwd()
    config = BuildConfig(
        source_dir=source_dir,
        temp_base=source_dir / ".build_tmp",
        output_dir=source_dir / "build",
        target=target,
        is_debug=args.debug,
        # Only use container on Linux when --no-container is not set
        use_container=(target == "arch" and not args.no_container),
        is_offline=args.offline,
        qt_dir=args.qt_dir,
        # Assign version info from args to config
        version_string=args.version,
    )

    # Parse major, minor, patch from version_string
    match = re.match(r"(\d+)\.(\d+)\.(\d+)", args.version)
    if match:
        config.version_major = int(match.group(1))
        config.version_minor = int(match.group(2))
        config.version_patch = int(match.group(3))

    worker = BuildWorker(config)
    cancelled = False

    def cancel_build():
        nonlocal cancelled
        if cancelled:
            print("\nForce quitting.")
            os._exit(130)
        cancelled = True
        print("\nCancelling build...")
        worker.cancel()

    signal.signal(signal.SIGINT, lambda _signum, _frame: cancel_build())

    mode = "Container" if config.use_container else "No Container"
    print(f"Build started | target={target} | {config.build_type} | {mode} | offline={config.is_offline}")
    worker.start()

    def handle_worker_msg(msg):
        if msg is None:
            return False
        if msg[0] == "log":
            print(msg[1])
        elif msg[0] == "progress":
            print(f"[{msg[1]}%] {msg[2]}")
        return True

    # Drain the log queue until the worker finishes
    while not worker.finished_event.is_set():
        try:
            msg = worker.log_queue.get(timeout=0.2)
            if not handle_worker_msg(msg):
                break
        except queue.Empty:
            continue

    # Drain remaining messages
    while True:
        try:
            msg = worker.log_queue.get_nowait()
            if not handle_worker_msg(msg):
                break
        except queue.Empty:
            break

    worker.join(timeout=3)

    if cancelled:
        print("\nBuild cancelled.")
        sys.exit(130)
    if worker.success:
        sys.exit(0)
    else:
        print(f"\nBuild failed: {worker.error_msg}")
        sys.exit(1)


if __name__ == "__main__":
    main()
