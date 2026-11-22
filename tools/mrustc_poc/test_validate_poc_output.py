import unittest

from validate_poc_output import validate


class ValidatePocOutputTests(unittest.TestCase):
    def test_accepts_generated_main_definition(self):
        validate("/* generated */\\nint main(int argc, char **argv) { return argc; }\\n")

    def test_accepts_void_main_definition(self):
        validate("void main(void) {}")

    def test_rejects_empty_output(self):
        with self.assertRaisesRegex(ValueError, "empty"):
            validate("  \\n")

    def test_rejects_missing_main(self):
        with self.assertRaisesRegex(ValueError, "main definition"):
            validate("int poc_entry(void) { return 0; }")

    def test_rejects_main_mentioned_only_in_comment(self):
        with self.assertRaisesRegex(ValueError, "main definition"):
            validate("/* int main(void) */\\nint helper(void) { return 0; }")


if __name__ == "__main__":
    unittest.main()
