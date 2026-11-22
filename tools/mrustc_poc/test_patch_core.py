import unittest

from patch_core import (
    ATOMIC128,
    ATOMIC128_REPLACEMENT,
    OBSOLETE_XGETBV_STUB,
    XGETBV,
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


if __name__ == "__main__":
    unittest.main()
