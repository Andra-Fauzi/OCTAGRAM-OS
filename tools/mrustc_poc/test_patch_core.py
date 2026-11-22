import pathlib
import tempfile
import unittest

from patch_core import (
    ATOMIC128,
    ATOMIC128_REPLACEMENT,
    OBSOLETE_XGETBV_STUB,
    XGETBV,
    patch_file,
    rewrite,
)


class PatchCoreTests(unittest.TestCase):
    def test_preserves_xgetbv_and_rewrites_atomic128(self):
        source = "before\n" + XGETBV + "\n" + ATOMIC128 + "\nafter\n"
        result = rewrite(source)
        self.assertIn(XGETBV, result)
        self.assertIn(ATOMIC128_REPLACEMENT, result)
        self.assertNotIn(ATOMIC128, result)

    def test_rewrite_is_idempotent(self):
        source = XGETBV + "\n" + ATOMIC128
        once = rewrite(source)
        self.assertEqual(rewrite(once), once)

    def test_rejects_missing_xgetbv(self):
        with self.assertRaisesRegex(ValueError, "XGETBV fragment not found"):
            rewrite("/* changed mrustc output */\n" + ATOMIC128)

    def test_rejects_unknown_atomic_output(self):
        with self.assertRaisesRegex(ValueError, "AtomicU128 fragment not found"):
            rewrite(XGETBV + "\n/* changed atomic output */")

    def test_rejects_obsolete_xgetbv_noop(self):
        with self.assertRaisesRegex(ValueError, "obsolete XGETBV no-op stub"):
            rewrite(XGETBV + "\n" + OBSOLETE_XGETBV_STUB + "\n" + ATOMIC128)


    def test_rejects_duplicate_atomic128_fragments(self):
        with self.assertRaisesRegex(ValueError, "exactly one"):
            rewrite(XGETBV + "\n" + ATOMIC128 + "\n" + ATOMIC128)

    def test_rejects_mixed_patched_and_unpatched_atomic128(self):
        with self.assertRaisesRegex(ValueError, "exactly one"):
            rewrite(XGETBV + "\n" + ATOMIC128 + "\n" + ATOMIC128_REPLACEMENT)

    def test_rejects_duplicate_replacement_fragments(self):
        with self.assertRaisesRegex(ValueError, "exactly one"):
            rewrite(XGETBV + "\n" + ATOMIC128_REPLACEMENT + "\n" + ATOMIC128_REPLACEMENT)


    def test_patch_file_replaces_valid_input_atomically(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "libcore.c"
            path.write_text(XGETBV + "\n" + ATOMIC128)
            patch_file(path)
            result = path.read_text()
            self.assertIn(XGETBV, result)
            self.assertIn(ATOMIC128_REPLACEMENT, result)
            self.assertNotIn(ATOMIC128, result)

    def test_patch_file_preserves_original_on_validation_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "libcore.c"
            original = XGETBV + "\n" + ATOMIC128 + "\n" + ATOMIC128
            path.write_text(original)
            with self.assertRaisesRegex(ValueError, "exactly one"):
                patch_file(path)
            self.assertEqual(path.read_text(), original)

    def test_atomic_replacement_avoids_unsupported_int128_syntax(self):
        self.assertIn("poc_cxchg128((void *)arg0", ATOMIC128_REPLACEMENT)
        self.assertNotIn("__int128", ATOMIC128_REPLACEMENT)


if __name__ == "__main__":
    unittest.main()
