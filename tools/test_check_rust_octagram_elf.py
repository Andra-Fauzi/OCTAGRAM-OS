#!/usr/bin/env python3
"""Unit tests for the static OCTAGRAM Rust ELF verifier."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

MODULE_PATH = Path(__file__).with_name("check_rust_octagram_elf.py")
SPEC = importlib.util.spec_from_file_location("check_rust_octagram_elf", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
VERIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFIER)


class RustOctagramElfVerifierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.elf = Path(self.temp_dir.name) / "guest.elf"
        self.elf.write_bytes(b"\x7fELF-test-fixture")
        self.entry = 0x401000
        self.responses = {
            ("readelf", "-h"): (
                "Class: ELF64\n"
                "Type: EXEC (Executable file)\n"
                "Machine: Advanced Micro Devices X86-64\n"
                f"Entry point address:               0x{self.entry:x}\n"
            ),
            ("readelf", "-l"): "Program Headers:\n  LOAD 0x000000 0x400000 0x400000\n",
            ("nm", "-u"): "",
            ("nm", "-n"): f"{self.entry:016x} T _start\n",
        }

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def run_verifier(self) -> tuple[int, str]:
        def fake_output(*args: str) -> str:
            return self.responses[args[:2]]

        with patch.object(VERIFIER, "output", side_effect=fake_output):
            return VERIFIER.validate_elf(self.elf)

    def test_accepts_static_elf_with_entry_matching_start(self) -> None:
        size, digest = self.run_verifier()
        self.assertEqual(size, self.elf.stat().st_size)
        self.assertEqual(len(digest), 64)

    def test_rejects_undefined_symbols(self) -> None:
        self.responses[("nm", "-u")] = "                 U memcpy\n"
        with self.assertRaisesRegex(VERIFIER.ValidationError, "undefined symbols"):
            self.run_verifier()

    def test_rejects_dynamic_interpreter(self) -> None:
        self.responses[("readelf", "-l")] = (
            "Program Headers:\n  INTERP 0x000200 0x400200 0x400200\n  LOAD 0x000000\n"
        )
        with self.assertRaisesRegex(VERIFIER.ValidationError, "dynamic interpreter"):
            self.run_verifier()

    def test_rejects_missing_start_symbol(self) -> None:
        self.responses[("nm", "-n")] = f"{self.entry:016x} T main\n"
        with self.assertRaisesRegex(VERIFIER.ValidationError, "_start"):
            self.run_verifier()

    def test_rejects_entry_point_not_equal_to_start(self) -> None:
        self.responses[("nm", "-n")] = "0000000000402000 T _start\n"
        with self.assertRaisesRegex(VERIFIER.ValidationError, "does not match"):
            self.run_verifier()

    def test_rejects_missing_load_segment(self) -> None:
        self.responses[("readelf", "-l")] = "Program Headers:\n  GNU_STACK 0x0\n"
        with self.assertRaisesRegex(VERIFIER.ValidationError, "PT_LOAD"):
            self.run_verifier()


if __name__ == "__main__":
    unittest.main()
