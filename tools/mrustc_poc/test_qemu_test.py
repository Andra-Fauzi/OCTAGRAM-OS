import tempfile
import unittest

from qemu_test import qemu_log_tail


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


if __name__ == "__main__":
    unittest.main()
