import unittest
from pathlib import Path
from unittest.mock import patch

import octagram_ai as ai


class SafetyTests(unittest.TestCase):
    def test_build_allowlist_has_only_named_recipes(self):
        self.assertIn("kernel-tests", ai.BUILD_RECIPES)
        self.assertNotIn("shell", ai.BUILD_RECIPES)
        self.assertNotIn("format-disk", ai.BUILD_RECIPES)
        self.assertEqual(ai.BUILD_SEQUENCES["kernel-tests"], ["rust-user", "user", "user-dynamic", "kernel-tests"])

    def test_repo_path_rejects_parent_traversal(self):
        with patch.object(ai, "WORKTREE", Path("/tmp/octagram-ai-test")):
            with self.assertRaises(Exception):
                ai._safe_repo_path("../outside.txt")

    def test_repo_path_rejects_absolute_path(self):
        with patch.object(ai, "WORKTREE", Path("/tmp/octagram-ai-test")):
            with self.assertRaises(Exception):
                ai._safe_repo_path("/etc/passwd")

    def test_docker_build_is_network_isolated(self):
        command = ai._docker_build_command("kernel-tests")
        self.assertIn("--network=none", command)
        self.assertIn("--cap-drop=ALL", command)
        self.assertIn("--read-only", command)
        self.assertIn("CARGO_NET_OFFLINE=true", command)
        self.assertNotIn("sh", command)

    def test_patch_rejects_git_metadata(self):
        patch = "\n".join([
            "--- a/.git/config",
            "+++ b/.git/config",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ])
        with self.assertRaises(Exception):
            ai.apply_patch(patch)

    def test_patch_rejects_parent_traversal(self):
        patch = "\n".join([
            "--- a/../outside",
            "+++ b/../outside",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ])
        with self.assertRaises(Exception):
            ai.apply_patch(patch)


if __name__ == "__main__":
    unittest.main()
