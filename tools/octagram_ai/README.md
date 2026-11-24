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
- llama.cpp `llama-server` with CUDA support (optional for API/workspace smoke tests)
- A GGUF model file

Recommended first model for 4 GB VRAM: Qwen2.5-Coder-3B-Instruct GGUF Q4_K_M. Its Q4_K_M file is about 2.1 GB; actual runtime memory also includes CUDA buffers and KV cache. Start with a 4K context and reduce GPU layers if VRAM is exhausted.

Model: https://huggingface.co/Qwen/Qwen2.5-Coder-3B-Instruct-GGUF

## 1. Prepare a clean clone

From WSL Ubuntu or Linux:

```bash
git clone --branch feature/native-self-hosting-testing-2 --single-branch https://github.com/Andra-Fauzi/OCTAGRAM-OS.git
cd OCTAGRAM-OS
```

Keep your normal development checkout separate. The agent creates its own worktree under `.octagram-ai/worktree`.

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
export OCTAGRAM_WORKTREE="$PWD/.octagram-ai/worktree"
export OCTAGRAM_LLM_BASE_URL="http://127.0.0.1:8080/v1"
export OCTAGRAM_LLM_MODEL="qwen2.5-coder-3b-instruct"
```

Start the API:

```bash
uvicorn octagram_ai:app --app-dir tools/octagram_ai --host 127.0.0.1 --port 8787
```

OpenAPI docs: http://127.0.0.1:8787/docs

## 4. API workflow

- `GET /health`: check configuration and worktree status.
- `POST /workspace/search`: fixed-string search in tracked repository files.
- `POST /workspace/read`: read one repository-relative file.
- `POST /chat`: ask the local model; matching repository snippets are added as context.
- `POST /workspace/apply-patch`: validate and apply a unified diff to the isolated worktree.
- `POST /workspace/build`: run one predefined build/test recipe.

Example:

```bash
curl -s http://127.0.0.1:8787/health
curl -s http://127.0.0.1:8787/workspace/search \
  -H 'Content-Type: application/json' \
  -d '{"query":"rust_native_probe"}'
```

Patch requests must contain a standard unified diff. Inspect the diff and test results before copying changes back or committing.

## 5. Build recipes

The allowlist follows the repository's documented clean-build order:

1. `rust-user` — `make -C user/rust make`
2. `user` — `make -C user`
3. `user-dynamic` — `make -C user dynamic`
4. `fmt-check` — `cargo +nightly fmt --manifest-path kernel/Cargo.toml --check`
5. `kernel-tests` — `cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu`
6. `kernel-build` — `make -C kernel KARCH=x86_64`
7. `iso-build` — `make all`

Run the first three in order before kernel tests/build because the kernel embeds userspace artifacts. QEMU and hardware tests are not currently exposed as autonomous recipes; they need a later explicit, controlled runner.

## 6. Current limitations

This is the first usable foundation, not a fully autonomous software engineer yet:

- Chat retrieves matching text snippets; it does not yet maintain a persistent semantic embedding index.
- The assistant does not automatically decide and execute tools from model output. The user/API client invokes each bounded endpoint.
- Cloud fallback, IDE extension packaging, persistent task memory, and automatic test-fix loops are future increments.
- The API's local loopback binding is not an authentication system.
- A successful host build is not evidence of QEMU or hardware success. Preserve the repository's evidence levels: host-tested, QEMU-tested, hardware-tested.

See `docs/rust-port-guide.md`, `docs/rust-m4-3-backend-plan.md`, `docs/native-self-hosting-roadmap.md`, `docs/building.md`, and `docs/testing.md` for project-specific rules.
