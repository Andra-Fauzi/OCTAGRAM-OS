#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd)"
WORKTREE="${OCTAGRAM_WORKTREE:-${REPO_ROOT%/*}/${REPO_ROOT##*/}-ai-worktree}"
BASE_REF="${OCTAGRAM_BASE_REF:-feature/native-self-hosting-testing-2}"

if ! command -v hermes >/dev/null 2>&1; then
  echo "Hermes Agent is not installed."
  echo "Install with: curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash"
  exit 1
fi
if ! python3 -c 'import mcp.server.fastmcp' >/dev/null 2>&1; then
  echo "MCP SDK is missing. Activate the OCTAGRAM AI venv and install requirements:"
  echo "python3 -m pip install -r ${REPO_ROOT}/tools/octagram_ai/requirements.txt"
  exit 1
fi

export OCTAGRAM_REPO="${REPO_ROOT}"
export OCTAGRAM_BASE_REF="${BASE_REF}"
export OCTAGRAM_WORKTREE="${WORKTREE}"
export PYTHONPATH="${REPO_ROOT}/tools/octagram_ai${PYTHONPATH:+:${PYTHONPATH}}"

if [[ ! -d "${WORKTREE}/.git" ]]; then
  git -C "${REPO_ROOT}" worktree add --detach "${WORKTREE}" "${BASE_REF}"
fi

cd "${WORKTREE}"
echo "OCTAGRAM repository: ${REPO_ROOT}"
echo "OCTAGRAM AI worktree: ${WORKTREE}"
echo "Before first use, review and merge tools/octagram_ai/hermes/config.yaml.example into ~/.hermes/config.yaml."
echo "Then run: hermes skills trust ${WORKTREE}"
exec hermes chat "$@"
