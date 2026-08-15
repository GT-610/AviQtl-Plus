import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from BUILD import BuildConfig, Logger, MsvcBuilder, PlatformBuilder, parse_semver


class RecordingBuilder(PlatformBuilder):
    def __init__(self, config, logger):
        super().__init__(config, logger)
        self.run_cmd_call_count = 0
        self.cache_existed_when_command_ran = None

    def run_cmd(self, cmd, shell=False, force_host=False):
        del cmd, shell, force_host
        self.run_cmd_call_count += 1
        self.cache_existed_when_command_ran = (self.config.work_dir / "CMakeCache.txt").exists()


class TestBuildVersion(unittest.TestCase):
    def test_accepts_valid_semver(self):
        cases = {
            "0.0.0": (0, 0, 0),
            "0.5.8": (0, 5, 8),
            "0.5.8-rc.1+build.2": (0, 5, 8),
            "1.0.0-0.3.7": (1, 0, 0),
            "1.0.0-x.7.z.92+001": (1, 0, 0),
        }
        for version, expected in cases.items():
            with self.subTest(version=version):
                self.assertEqual(parse_semver(version), expected)

    def test_rejects_invalid_semver(self):
        cases = (
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "0.5.8-.",
            "0.5.8-",
            "0.5.8+",
            "1.0.0-01",
            "1.0.0-alpha..1",
            "1.0.0-alpha_1",
            "1.0.0+build..2",
            "v1.2.3",
            "1.2",
            "１.2.3",
        )
        for version in cases:
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    parse_semver(version)

    def test_configure_refreshes_cmake_cache_but_keeps_build_artifacts(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            source_dir = Path(temp_dir)
            config = BuildConfig(
                source_dir=source_dir,
                temp_base=source_dir / ".build_tmp",
                output_dir=source_dir / "build",
                target="test",
                is_debug=False,
                use_container=False,
                is_offline=True,
            )
            config.work_dir.mkdir(parents=True)
            cache_path = config.work_dir / "CMakeCache.txt"
            artifact_path = config.work_dir / "build.ninja"
            cache_path.write_text("LUAJIT_INCLUDE_DIRS=/stale/path\n", encoding="utf-8")
            artifact_path.write_text("# keep incremental build metadata\n", encoding="utf-8")
            messages = []
            builder = RecordingBuilder(config, Logger(messages.append, lambda _value, _message: None))

            builder.configure()

            self.assertGreater(builder.run_cmd_call_count, 0)
            self.assertFalse(builder.cache_existed_when_command_ran)
            self.assertFalse(cache_path.exists())
            self.assertTrue(artifact_path.exists())
            self.assertTrue(any("Refreshing CMake cache" in message for message in messages))


@unittest.skipUnless(os.name == "nt", "MSVC command construction is Windows-only")
class TestMsvcBuildConfig(unittest.TestCase):
    def test_cmd_environment_prefers_vcvarsall_uppercase_path(self):
        parsed = MsvcBuilder.parse_cmd_environment(
            "PATH=C:\\MSVC\\bin;C:\\Windows\n"
            "Path=C:\\stale-user-path\n"
            "VCToolsInstallDir=C:\\MSVC\\\n"
        )

        self.assertEqual(parsed["PATH"], "C:\\MSVC\\bin;C:\\Windows")
        self.assertEqual(parsed["VCTOOLSINSTALLDIR"], "C:\\MSVC\\")

    def test_release_uses_committed_manifest_and_offline_guards(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            source_dir = Path(temp_dir)
            (source_dir / "triplets").mkdir()
            (source_dir / "vendor" / "carla").mkdir(parents=True)
            (source_dir / "vcpkg.json").write_text("{}\n", encoding="utf-8")
            (source_dir / "vcpkg-configuration.json").write_text("{}\n", encoding="utf-8")
            (source_dir / "triplets" / "x64-windows-release.cmake").write_text(
                "set(VCPKG_TARGET_ARCHITECTURE x64)\n", encoding="utf-8"
            )
            config = BuildConfig(
                source_dir=source_dir,
                temp_base=source_dir / ".build_tmp",
                output_dir=source_dir / "build",
                target="msvc",
                is_debug=False,
                use_container=False,
                is_offline=True,
            )
            with mock.patch.dict(
                os.environ,
                {
                    "VCPKG_DEFAULT_TRIPLET": "x64-windows-release",
                    "VCPKG_DEFAULT_HOST_TRIPLET": "x64-windows-release",
                },
            ):
                builder = MsvcBuilder(
                    config, Logger(lambda _message: None, lambda _value, _message: None)
                )
            builder.vcpkg_root = source_dir / "vcpkg"
            builder.qt_prefix = source_dir / "Qt"
            builder.cmake_path = str(source_dir / "cmake.exe")
            builder.ninja_path = source_dir / "ninja.exe"

            builder.configure_vcpkg_manifest()
            command = builder.get_cmake_config_cmd()

            self.assertEqual(command[0], builder.cmake_path)
            self.assertIn(f"-DVCPKG_MANIFEST_DIR={source_dir}", command)
            self.assertIn("-DVCPKG_MANIFEST_INSTALL=OFF", command)
            self.assertIn("-DAVIQTL_CARGO_OFFLINE=ON", command)
            self.assertIn("-DVCPKG_TARGET_TRIPLET=x64-windows-release", command)
            self.assertIn("-DVCPKG_HOST_TRIPLET=x64-windows-release", command)
            self.assertIn(
                f"-DCARLA_IMPORT_LIB_DIR={builder.carla_import_lib_dir.as_posix()}", command
            )


if __name__ == "__main__":
    unittest.main()
