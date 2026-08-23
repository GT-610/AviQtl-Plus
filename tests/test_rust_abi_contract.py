#!/usr/bin/env python3

import unittest
from pathlib import Path

import check_rust_abi


class RustAbiContractCheckerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(__file__).resolve().parents[1]
        cls.sources = check_rust_abi.load_rust_sources(root / "rust/aviqtl-core/src")
        cls.header = (root / "core/include/rust_core_abi.hpp").read_text(encoding="utf-8")

    def test_repository_contract_matches(self):
        self.assertEqual(check_rust_abi.compare_contract(self.sources, self.header), [])

    def test_missing_declaration_is_reported(self):
        sources = dict(self.sources)
        sources["abi.rs"] += '\n#[unsafe(no_mangle)]\npub extern "C" fn aviqtl_missing() -> u32 { 0 }\n'
        errors = check_rust_abi.compare_contract(sources, self.header)
        self.assertTrue(any("aviqtl_missing" in error for error in errors))

    def test_signature_drift_is_reported(self):
        header = self.header.replace(
            "std::int32_t aviqtl_project_current_version();",
            "std::uint32_t aviqtl_project_current_version();",
        )
        errors = check_rust_abi.compare_contract(self.sources, header)
        self.assertTrue(any("signature mismatch for aviqtl_project_current_version" in error for error in errors))

    def test_struct_drift_is_reported(self):
        header = self.header.replace("    double period;", "    float period;", 1)
        errors = check_rust_abi.compare_contract(self.sources, header)
        self.assertTrue(any("field mismatch for AviQtlEasingParameters" in error for error in errors))

    def test_missing_export_attribute_is_reported(self):
        sources = dict(self.sources)
        sources["abi.rs"] = sources["abi.rs"].replace(
            "#[unsafe(no_mangle)]\npub extern \"C\" fn aviqtl_core_abi_version",
            "pub extern \"C\" fn aviqtl_core_abi_version",
        )
        with self.assertRaisesRegex(ValueError, "missing.*no_mangle"):
            check_rust_abi.compare_contract(sources, self.header)


if __name__ == "__main__":
    unittest.main()
