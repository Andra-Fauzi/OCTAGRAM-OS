import unittest

from patch_core import (
    ATOMIC128,
    ATOMIC128_REPLACEMENT,
    XGETBV,
    XGETBV_REPLACEMENT,
    rewrite,
)


class PatchCoreTests(unittest.TestCase):
    def test_rewrites_both_known_fragments(self):
        source = "before\n" + XGETBV + "\n" + ATOMIC128 + "\nafter\n"
        result = rewrite(source)
        self.assertIn(XGETBV_REPLACEMENT, result)
        self.assertIn(ATOMIC128_REPLACEMENT, result)
        self.assertNotIn(XGETBV, result)
        self.assertNotIn(ATOMIC128, result)

    def test_rewrite_is_idempotent(self):
        source = XGETBV + "\n" + ATOMIC128
        once = rewrite(source)
        self.assertEqual(rewrite(once), once)

    def test_rejects_unknown_xgetbv_output(self):
        with self.assertRaisesRegex(ValueError, "XGETBV fragment not found"):
            rewrite("/* changed mrustc output */\n" + ATOMIC128)

    def test_rejects_unknown_atomic_output(self):
        with self.assertRaisesRegex(ValueError, "AtomicU128 fragment not found"):
            rewrite(XGETBV + "\n/* changed atomic output */")


if __name__ == "__main__":
    unittest.main()
