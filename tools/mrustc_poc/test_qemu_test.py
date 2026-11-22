import tempfile
import unittest
from unittest.mock import patch

from qemu_test import qemu_log_tail, wait_compile_success


class QemuLogTailTests(unittest.TestCase):
    def test_reads_entire_short_log(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as handle:
            handle.write("startup failed\n")
            handle.flush()
            self.assertEqual(qemu_log_tail(handle.name), "startup failed\n")

    def test_reads_only_tail_of_large_log(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as handle:
            handle.write("0123456789")
            handle.flush()
            self.assertEqual(qemu_log_tail(handle.name, limit=4), "6789")

    def test_replacement_decoding_handles_invalid_bytes(self):
        with tempfile.NamedTemporaryFile(mode="wb") as handle:
            handle.write(b"0123456789\xff")
            handle.flush()
            self.assertEqual(qemu_log_tail(handle.name, limit=1), "\ufffd")

    def test_compile_wait_requires_exit_status_and_shell_prompt(self):
        serial = "program exited with status 0" + chr(10) + "octagram:~$"
        with patch("qemu_test.serial_text", return_value=serial):
            self.assertTrue(wait_compile_success(0, timeout=1))

    def test_compile_wait_rejects_missing_shell_prompt(self):
        serial = "program exited with status 0"
        with patch("qemu_test.serial_text", return_value=serial):
            self.assertFalse(wait_compile_success(0, timeout=0))

    def test_compile_wait_rejects_missing_successful_exit(self):
        serial = "tcc: error: undefined symbol" + chr(10) + "octagram:~$"
        with patch("qemu_test.serial_text", return_value=serial):
            self.assertFalse(wait_compile_success(0, timeout=0))


if __name__ == "__main__":
    unittest.main()
