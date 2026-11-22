import os
import tempfile
import unittest
from unittest.mock import patch

from qemu_test import (
    extract_compile_errors,
    format_compile_errors,
    qemu_log_tail,
    source_excerpt,
    wait_compile_success,
)


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


class CompileDiagnosticTests(unittest.TestCase):
    def test_extracts_file_line_diagnostics(self):
        text = (
            "tcc -bench -I /mnt/src /mnt/src/libcore.c -o /mnt/poc.elf\n"
            "libcore.c:1234: error: 'foo' undeclared\n"
            "libcore.c:12: warning: unused variable 'x'\n"
            "program exited with status 1\n"
        )
        errors = extract_compile_errors(text)
        self.assertEqual(len(errors), 2)
        self.assertEqual(errors[0], {
            "file": "libcore.c",
            "line": 1234,
            "kind": "error",
            "message": "'foo' undeclared",
        })
        self.assertEqual(errors[1]["kind"], "warning")

    def test_extracts_file_level_diagnostics(self):
        errors = extract_compile_errors("tcc: error: file 'shim.c' not found\n")
        self.assertEqual(len(errors), 1)
        self.assertIsNone(errors[0]["line"])
        self.assertEqual(errors[0]["file"], "tcc")

    def test_ignores_chatter_and_deduplicates(self):
        text = (
            "octagram:~$ tcc libcore.c -o poc.elf\n"
            "libcore.c:7: error: too many errors\n"
            "libcore.c:7: error: too many errors\n"
            "program exited with status 1\n"
            "octagram:~$\n"
        )
        self.assertEqual(len(extract_compile_errors(text)), 1)

    def test_respects_limit(self):
        text = "\n".join(
            "libcore.c:%d: error: failure %d" % (index, index)
            for index in range(1, 31)
        )
        self.assertEqual(len(extract_compile_errors(text, limit=5)), 5)

    def test_source_excerpt_reads_host_copy(self):
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".c", encoding="utf-8", delete=False
        ) as handle:
            handle.write("one\ntwo\nthree\n")
            path = handle.name
        try:
            directory = os.path.dirname(path)
            name = os.path.basename(path)
            self.assertEqual(
                source_excerpt(directory, name, 2, context=1),
                ["%s:1: one" % name, "%s:2: two" % name, "%s:3: three" % name],
            )
        finally:
            os.unlink(path)

    def test_source_excerpt_clamps_and_tolerates_missing_file(self):
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".c", encoding="utf-8", delete=False
        ) as handle:
            handle.write("only line\n")
            path = handle.name
        try:
            directory = os.path.dirname(path)
            name = os.path.basename(path)
            self.assertEqual(
                source_excerpt(directory, name, 99, context=2),
                ["%s:1: only line" % name],
            )
            self.assertEqual(source_excerpt(directory, "absent.c", 1), [])
            self.assertEqual(source_excerpt(None, name, 1), [])
        finally:
            os.unlink(path)

    def test_format_pairs_diagnostics_with_source_lines(self):
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".c", encoding="utf-8", delete=False
        ) as handle:
            handle.write("int main(void) { return 0; }\n")
            path = handle.name
        try:
            directory = os.path.dirname(path)
            name = os.path.basename(path)
            rendered = format_compile_errors(
                "%s:1: error: expected expression\n" % name, directory
            )
            self.assertEqual(rendered[0], "%s:1: error: expected expression" % name)
            self.assertEqual(rendered[1], "    %s:1: int main(void) { return 0; }" % name)
            self.assertEqual(format_compile_errors("all good\n", directory), [])
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
