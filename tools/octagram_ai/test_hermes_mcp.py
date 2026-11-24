import unittest
from unittest.mock import patch

import hermes_mcp


class HermesMCPTests(unittest.TestCase):
    def test_search_tool_delegates_to_bounded_workspace(self):
        with patch.object(hermes_mcp.workspace, "ensure_worktree") as ensure, patch.object(
            hermes_mcp.workspace, "repo_search", return_value="kernel/src/main.rs:1:probe"
        ) as search:
            result = hermes_mcp.octagram_search("probe")
        ensure.assert_called_once_with()
        search.assert_called_once_with("probe")
        self.assertIn("kernel/src/main.rs", result)

    def test_empty_search_is_rejected_before_workspace_query(self):
        with patch.object(hermes_mcp.workspace, "repo_search") as search:
            result = hermes_mcp.octagram_search("")
        search.assert_not_called()
        self.assertIn("ERROR", result)

    def test_build_tool_delegates_only_named_recipe(self):
        expected = {"recipe": "kernel-tests", "passed": True, "output": "ok"}
        with patch.object(hermes_mcp.workspace, "ensure_worktree"), patch.object(
            hermes_mcp.workspace, "run_build", return_value=expected
        ) as build:
            result = hermes_mcp.octagram_run_build("kernel-tests")
        build.assert_called_once_with("kernel-tests")
        self.assertIn('"passed": true', result)


if __name__ == "__main__":
    unittest.main()
