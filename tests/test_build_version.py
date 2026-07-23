import sys
import unittest
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from BUILD import parse_semver


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


if __name__ == "__main__":
    unittest.main()
