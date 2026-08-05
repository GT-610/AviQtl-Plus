import sys
import tempfile
import unittest
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from BUILD import BuildConfig, Logger, PlatformBuilder, parse_semver


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


if __name__ == "__main__":
    unittest.main()
