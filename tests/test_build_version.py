import argparse
import tempfile
import unittest
from pathlib import Path

from BUILD import (
    BuildConfig,
    Logger,
    PlatformBuilder,
    XcodeBuilder,
    determine_target,
    normalize_macos_architecture,
    parse_semver,
    read_project_version,
)


def config_for(source_dir: Path, *, debug=False, offline=False, target="test") -> BuildConfig:
    return BuildConfig(
        source_dir=source_dir,
        temp_base=source_dir / ".build_tmp",
        output_dir=source_dir / "build",
        target=target,
        is_debug=debug,
        use_container=False,
        is_offline=offline,
        version_string="0.6.2",
    )


class TestBuildScript(unittest.TestCase):
    def test_accepts_valid_semver(self):
        cases = {
            "0.0.0": (0, 0, 0),
            "0.5.8": (0, 5, 8),
            "0.5.8-rc.1+build.2": (0, 5, 8),
            "1.0.0-0.3.7": (1, 0, 0),
        }
        for version, expected in cases.items():
            with self.subTest(version=version):
                self.assertEqual(parse_semver(version), expected)

    def test_rejects_invalid_semver(self):
        for version in ("01.2.3", "1.02.3", "1.2.03", "0.5.8-", "v1.2.3", "1.2"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                parse_semver(version)

    def test_reads_rust_workspace_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            (source / "rust").mkdir()
            (source / "rust" / "Cargo.toml").write_text(
                '[workspace]\n[workspace.package]\nversion = "1.2.3-rc.1"\n', encoding="utf-8"
            )
            self.assertEqual(read_project_version(source), "1.2.3-rc.1")

    def test_normalizes_macos_archive_architectures(self):
        for machine, expected in {
            "arm64": "arm64", "aarch64": "arm64", "x86_64": "x86_64", "AMD64": "x86_64"
        }.items():
            self.assertEqual(normalize_macos_architecture(machine), expected)
        with self.assertRaises(RuntimeError):
            normalize_macos_architecture("powerpc")

    def test_legacy_target_switches_remain_supported(self):
        defaults = {name: False for name in ("arch", "msys2", "msvc", "xcode")}
        for target in defaults:
            values = {**defaults, target: True}
            self.assertEqual(determine_target(argparse.Namespace(**values), "unknown"), target)
        self.assertEqual(determine_target(argparse.Namespace(**defaults), "Darwin"), "xcode")

    def test_cargo_command_builds_only_slint_frontend(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            builder = PlatformBuilder(
                config_for(source, offline=True), Logger(lambda _message: None, lambda *_args: None)
            )
            command = builder.get_cargo_build_cmd()
            self.assertEqual(command[:2], ["cargo", "build"])
            self.assertIn(str(source / "rust" / "Cargo.toml"), command)
            self.assertIn("aviqtl-slint", command)
            self.assertIn("--release", command)
            self.assertIn("--offline", command)
            self.assertNotIn("cmake", " ".join(command).lower())
            self.assertNotIn("windeployqt", " ".join(command).lower())

    def test_debug_command_and_binary_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            builder = PlatformBuilder(
                config_for(source, debug=True), Logger(lambda _message: None, lambda *_args: None)
            )
            self.assertNotIn("--release", builder.get_cargo_build_cmd())
            self.assertEqual(
                builder.cargo_binary(),
                source / ".build_tmp" / "test" / "Debug" / "cargo-target" / "debug" / "aviqtl-slint",
            )

    def test_resource_layout_matches_runtime_lookup(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            fixtures = {
                ("ui", "qml", "effects", "blur.json"): "effect",
                ("ui", "qml", "objects", "scene.json"): "object",
                ("plugins", "example", "main.lua"): "plugin",
                ("effect-packages", "weather", "manifest.json"): "package",
                ("repos", "catalog.json"): "repo",
            }
            for parts, contents in fixtures.items():
                path = source.joinpath(*parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents, encoding="utf-8")
            destination = source / "bundle"
            builder = PlatformBuilder(
                config_for(source), Logger(lambda _message: None, lambda *_args: None)
            )
            builder.copy_resources(destination)
            for parts, contents in fixtures.items():
                packaged = destination.joinpath(parts[-2], parts[-1]) if parts[0] == "ui" else destination.joinpath(*parts)
                self.assertEqual(packaged.read_text(encoding="utf-8"), contents)

    def test_macos_plist_keeps_app_name_and_slint_process_name(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            contents = source / "AviQtl.app" / "Contents"
            contents.mkdir(parents=True)
            builder = XcodeBuilder(
                config_for(source, target="xcode"), Logger(lambda _message: None, lambda *_args: None)
            )
            builder.write_info_plist(contents)
            plist = (contents / "Info.plist").read_text(encoding="utf-8")
            self.assertIn("<string>AviQtl</string>", plist)
            self.assertIn("<string>aviqtl-slint</string>", plist)
            self.assertIn("<string>org.aviqtl.AviQtl</string>", plist)
            self.assertNotIn("Qt6", plist)


if __name__ == "__main__":
    unittest.main()
