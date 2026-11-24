import unittest
from pathlib import Path
from unittest.mock import patch

import octagram_ai as ai


class SafetyTests(unittest.TestCase):
    def test_build_allowlist_has_only_named_recipes(self):
        self.assertIn("kernel-tests", ai.BUILD_RECIPES)
        self.assertNotIn("shell", ai.BUILD_RECIPES)
        self.assertNotIn("format-disk", ai.BUILD_RECIPES)

    def test_repo_path_rejects_parent_traversal(self):
        with patch.object(ai, "WORKTREE", Path("/tmp/octagram-ai-test")):
            with self.assertRaises(Exception):
                ai._safe_repo_path("../outside.txt")

    def test_repo_path_rejects_absolute_path(self):
        with patch.object(ai, "WORKTREE", Path("/tmp/octagram-ai-test")):
            with self.assertRaises(Exception):
                ai._safe_repo_path("/etc/passwd")


if __name__ == "__main__":
    unittest.main()
