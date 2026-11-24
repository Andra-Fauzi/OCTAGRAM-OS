# OCTAGRAM AI Developer v1

A local-first coding assistant for OCTAGRAM-OS. It runs the LLM through a local OpenAI-compatible llama.cpp server and operates on a **detached Git worktree**, never on the developer's active checkout.

## Safety model

- The configured base ref is checked out into a separate detached worktree.
- Search and file reads are restricted to that worktree.
- File writes are accepted only as unified diffs, checked with `git apply --check`, then applied to the worktree.
- Build/test commands are selected from a fixed allowlist. No arbitrary shell command endpoint exists.
- No automatic commit, push, installer, disk formatting, or hardware operation.
- The API binds to loopback by default. Do not expose it to a LAN without authentication.

## Requirements

- Python 3.10+
- Git
- Docker Engine (required for sandboxed builds)
- llama.cpp `llama-server` with CUDA support (optional for API/workspace smoke tests)
- A GGUF model file

For the lightweight FastAPI assistant, use Qwen2.5-Coder-3B-Instruct GGUF Q4_K_M (about 2.1 GB) with a 4K context. The Hermes profile below instead defaults to Qwen2.5-Coder-7B-Instruct Q4_K_M with partial CPU/RAM offload for stronger coding quality; use 3B if the 7B model leaves too little system memory or is too slow.

Model: https://huggingface.co/Qwen/Qwen2.5-Coder-3B-Instruct-GGUF

## 1. Prepare a clean clone

From WSL Ubuntu or Linux:

```bash
git clone --branch feature/native-self-hosting-testing-2 --single-branch https://github.com/Andra-Fauzi/OCTAGRAM-OS.git
cd OCTAGRAM-OS
```

Keep your normal development checkout separate. The agent creates its own worktree in a sibling directory named `OCTAGRAM-OS-ai-worktree` by default, outside the main checkout.

## 2. Start llama.cpp

Example (adjust paths and flags to the installed llama.cpp build):

```bash
./llama-server -m /models/qwen2.5-coder-3b-instruct-q4_k_m.gguf \
  -ngl 99 -c 4096 -b 128 -ub 64 \
  --host 127.0.0.1 --port 8080
```

If CUDA reports out-of-memory, lower `-ngl`; leave some VRAM for Windows/desktop and KV cache. Verify the server at `http://127.0.0.1:8080/v1/models`.

llama.cpp server API reference: https://github.com/ggml-org/llama.cpp/tree/master/tools/server

## 3. Install and configure the assistant

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -r tools/octagram_ai/requirements.txt
cp tools/octagram_ai/.env.example .env
```

Set the variables in your shell (or export them from your local environment):

```bash
export OCTAGRAM_REPO="$PWD"
export OCTAGRAM_BASE_REF="feature/native-self-hosting-testing-2"
export OCTAGRAM_WORKTREE="$PWD/../OCTAGRAM-OS-ai-worktree"
export OCTAGRAM_LLM_BASE_URL="http://127.0.0.1:8080/v1"
export OCTAGRAM_LLM_MODEL="qwen2.5-coder-3b-instruct"
```

Start the API:

```bash
uvicorn octagram_ai:app --app-dir tools/octagram_ai --host 127.0.0.1 --port 8787
```

OpenAPI docs: http://127.0.0.1:8787/docs

## 4. Prepare the build sandbox

Build the toolchain image once (this step needs internet to install Ubuntu packages and Rust nightly):

```bash
docker build -f tools/octagram_ai/Dockerfile -t octagram-ai-build:latest .
```

Build recipes then run with networking disabled, a read-only container root, dropped Linux capabilities, resource limits, and only the isolated worktree mounted writable. The host Cargo registry cache may be mounted read-only. If dependencies are not cached, populate the cache before invoking the offline build runner. Docker daemon access is powerful; only run this service on a machine whose Docker installation you trust.

## 5. API workflow

- `GET /health`: check configuration and worktree status.
- `POST /workspace/search`: fixed-string search in tracked repository files.
- `POST /workspace/read`: read one repository-relative file.
- `POST /chat`: ask the local model; matching repository snippets are added as context.
- `POST /workspace/apply-patch`: validate and apply a unified diff to the isolated worktree.
- `POST /workspace/build`: run one predefined build/test recipe. Kernel test/build recipes first build the embedded Rust and C userspace artifacts.
- `POST /agent/task`: ask the local model for a unified diff, apply it to the detached worktree, run the selected allowlisted build recipe, and retry at most twice.

Example:

```bash
curl -s http://127.0.0.1:8787/health
curl -s http://127.0.0.1:8787/workspace/search \
  -H 'Content-Type: application/json' \
  -d '{"query":"rust_native_probe"}'
```

The `/agent/task` endpoint can apply a model-generated patch and run the selected build automatically. It never commits or pushes. Inspect `/workspace/status` and the returned patch/build output before copying changes back or committing.

## 6. Build recipes

The allowlist follows the repository's documented clean-build order:

1. `rust-user` — `make -C user/rust make`
2. `user` — `make -C user`
3. `user-dynamic` — `make -C user dynamic`
4. `fmt-check` — `cargo +nightly fmt --manifest-path kernel/Cargo.toml --check`
5. `kernel-tests` — prepares `user/rust`, `user`, and `user dynamic`, then runs `cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu`
6. `kernel-build` — prepares embedded userspace artifacts, then runs `make -C kernel KARCH=x86_64`
7. `iso-build` — prepares userspace, builds the kernel, then runs `make all`

Run the first three in order before kernel tests/build because the kernel embeds userspace artifacts. QEMU and hardware tests are not currently exposed as autonomous recipes; they need a later explicit, controlled runner.

Run the unit tests with `python -m unittest discover -s tools/octagram_ai -p 'test_*.py'`.

## 7. Current limitations

This is the first usable foundation, not a fully autonomous software engineer yet:

- Chat retrieves matching text snippets; it does not yet maintain a persistent semantic embedding index.
- The legacy FastAPI /agent/task route remains a separate bounded patch/build loop. Hermes is the recommended agent interface and orchestrates the OCTAGRAM MCP tools directly.
- A cloud fallback route is not preconfigured; add a provider through Hermes when desired. IDE extension packaging and semantic repository indexing remain future increments.
- The API's local loopback binding is not an authentication system.
- A successful host build is not evidence of QEMU or hardware success. Preserve the repository's evidence levels: host-tested, QEMU-tested, hardware-tested.

See `docs/rust-port-guide.md`, `docs/rust-m4-3-backend-plan.md`, `docs/native-self-hosting-roadmap.md`, `docs/building.md`, and `docs/testing.md` for project-specific rules.

### Autonomous task example

Ask the local model to propose a small patch, apply it in the detached worktree, and run the ordered kernel host-test sequence:

```bash
curl -s http://127.0.0.1:8787/agent/task \
  -H 'Content-Type: application/json' \
  -d '{"task":"Inspect the Rust native bootstrap milestone and implement one small, evidence-backed missing test.","build_recipe":"kernel-tests","max_iterations":2}'
```

The agent stops after a successful build or two attempts. Inspect the returned patches and `GET /workspace/status`; nothing is committed or pushed automatically.

## 8. Hermes Agent integration

Hermes is the orchestration layer; llama.cpp remains the local inference server. Hermes connects to the local OpenAI-compatible endpoint and discovers the five bounded OCTAGRAM tools over MCP. The repository skill is project-local at `.hermes/skills/octagram-os-developer/SKILL.md`.

### Install Hermes and Python MCP dependency

Official Hermes installation for WSL2/Linux:

```bash
curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash
```

Install the OCTAGRAM MCP dependency in the Python environment used by Hermes' MCP subprocess:

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -r tools/octagram_ai/requirements.txt
```

Build the restricted build image:

```bash
docker build -f tools/octagram_ai/Dockerfile -t octagram-ai-build:latest .
```

### Configure Hermes

Copy `tools/octagram_ai/hermes/config.yaml.example` and merge its sections into `~/.hermes/config.yaml`. Replace every `/ABSOLUTE/PATH/TO/OCTAGRAM-OS` placeholder with the real Linux/WSL path. Do not overwrite unrelated existing Hermes configuration.

The configuration:
- Points Hermes to the local llama.cpp endpoint (default 7B Q4 model, 8K context).
- Starts the OCTAGRAM MCP server over stdio and exposes only status, search, read, patch, and allowlisted build tools.
- Marks the MCP server untrusted so write/build tool calls require the normal approval flow.
- Uses Hermes' Docker terminal backend with networking disabled, no extra capabilities, resource limits, and only the detached AI worktree mounted writable.
- Keeps unattended, cron, and one-shot dangerous-command approval set to deny.

Before first use, trust this repository's project skill:

```bash
hermes skills trust /absolute/path/to/OCTAGRAM-OS-ai-worktree
```

Start llama.cpp in another terminal. For a 4 GB RTX 2050, Qwen2.5-Coder-7B-Instruct Q4_K_M must use CPU/RAM offload; begin conservatively:

```bash
llama-server -m /models/qwen2.5-coder-7b-instruct-q4_k_m.gguf \
  --alias qwen2.5-coder-7b-instruct \
  -ngl 8 -c 8192 -b 64 -ub 32 \
  --host 127.0.0.1 --port 8080
```

If VRAM is exhausted, lower `-ngl` (for example 4); if host RAM becomes tight or generation is too slow, use Qwen2.5-Coder-3B Q4 instead. Keep the local model server and Hermes on loopback. Never expose the unauthenticated local inference endpoint to a public interface.

Launch Hermes inside the isolated worktree:

```bash
bash tools/octagram_ai/hermes/run_hermes.sh
```

The launcher prepares the detached worktree if it does not exist. Example task:

> Inspect the current Rust native self-hosting milestone. Read the port guide and roadmap, identify the next smallest missing implementation step, implement it, run the narrowest relevant build, and report evidence and remaining blockers. Do not change roadmap status.

### MCP tool surface

| Tool | Capability |
|---|---|
| `octagram_status` | Worktree base, changed files, diff summary |
| `octagram_search` | Fixed-string search of tracked files |
| `octagram_read_file` | Bounded repository-relative UTF-8 read |
| `octagram_apply_patch` | Validated unified diff into detached worktree |
| `octagram_run_build` | Allowlisted network-isolated Docker build/test |

Hermes' own skills, planning, and memory provide orchestration. The MCP server is the only supported OCTAGRAM-specific write/build interface. The Docker terminal backend is separately confined to the same detached worktree. No automatic commit, push, disk operation, firmware flash, or physical-hardware action is provided.

### Important limitations

- This integration does not make a 7B model equivalent to a frontier coding model. Expect slower CPU-offloaded inference and review larger changes carefully.
- Qwen2.5-Coder-7B tool-call quality may vary by GGUF chat template. Verify tool discovery with a small read-only task before allowing patch/build operations.
- Hermes' local model endpoint is offline-capable after the model and software dependencies have been downloaded. Optional web tools and cloud model providers require internet and separate provider credentials.
- A host build passing is not evidence of QEMU or physical-hardware success.
- The MCP server and Hermes Docker terminal configuration must be tested on the user's WSL/Docker installation; this repository change does not prove runtime compatibility.
