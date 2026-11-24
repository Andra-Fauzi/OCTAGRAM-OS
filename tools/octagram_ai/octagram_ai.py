"""Local-first OCTAGRAM-OS coding assistant with an isolated Git worktree.

The API intentionally exposes a small set of bounded operations. It never
accepts arbitrary shell commands and never commits or pushes changes.
"""
from __future__ import annotations

import json
import os
import subprocess
import urllib.error
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Any

from dotenv import load_dotenv
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field


load_dotenv()

REPO = Path(os.environ.get("OCTAGRAM_REPO") or ".").expanduser().resolve()
BASE_REF = os.environ.get("OCTAGRAM_BASE_REF", "feature/native-self-hosting-testing-2")
WORKTREE = Path(os.environ.get("OCTAGRAM_WORKTREE") or str(REPO.parent / (REPO.name + "-ai-worktree"))).expanduser().resolve()
LLM_BASE_URL = os.environ.get("OCTAGRAM_LLM_BASE_URL", "http://127.0.0.1:8080/v1").rstrip("/")
LLM_MODEL = os.environ.get("OCTAGRAM_LLM_MODEL", "qwen2.5-coder-3b-instruct")
LLM_API_KEY = os.environ.get("OCTAGRAM_LLM_API_KEY", "local")
MAX_FILE_BYTES = int(os.environ.get("OCTAGRAM_MAX_FILE_BYTES", "200000"))
BUILD_TIMEOUT = int(os.environ.get("OCTAGRAM_BUILD_TIMEOUT_SECONDS", "900"))
DOCKER_BIN = os.environ.get("OCTAGRAM_DOCKER_BIN", "docker")
DOCKER_IMAGE = os.environ.get("OCTAGRAM_DOCKER_IMAGE", "octagram-ai-build:latest")

# Fixed commands only. No shell=True and no user-provided command arguments.
BUILD_RECIPES: dict[str, list[str]] = {
    "rust-user": ["make", "-C", "user/rust", "make"],
    "user": ["make", "-C", "user"],
    "user-dynamic": ["make", "-C", "user", "dynamic"],
    "fmt-check": ["cargo", "+nightly", "fmt", "--manifest-path", "kernel/Cargo.toml", "--check"],
    "kernel-tests": ["cargo", "+nightly", "test", "--manifest-path", "kernel/Cargo.toml", "--target", "x86_64-unknown-linux-gnu"],
    "kernel-build": ["make", "-C", "kernel", "KARCH=x86_64"],
    "iso-build": ["make", "all"],
}

SYSTEM_PROMPT = """You are OCTAGRAM AI Developer, a careful systems-programming assistant.
You work only on the OCTAGRAM-OS repository and must follow its checked-in docs.
Prioritize existing architecture and minimal patches over rewrites. Never invent
file contents, APIs, test results, or roadmap status. Distinguish host build/test,
QEMU guest test, and physical-hardware test. The OS syscall ABI is OCTAGRAM's
own int 0x80 ABI, not Linux. Respect the Rust port guide's host/target triples,
static ELF constraints, TLS limitations, and staged bootstrap plan.
When proposing code changes, return a unified diff and explain the exact build
recipe to run. Do not claim a test passed unless its output was observed."""


class SearchRequest(BaseModel):
    query: str = Field(min_length=1, max_length=200)


class ReadRequest(BaseModel):
    path: str = Field(min_length=1, max_length=500)


class ChatRequest(BaseModel):
    message: str = Field(min_length=1, max_length=12000)
    include_context: bool = True


class PatchRequest(BaseModel):
    patch: str = Field(min_length=1, max_length=100000)


class BuildRequest(BaseModel):
    recipe: str


def _git(args: list[str], *, cwd: Path, timeout: int = 30, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=cwd, text=True, encoding="utf-8", errors="replace",
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout, check=check,
    )


def ensure_worktree() -> None:
    if not (REPO / ".git").exists():
        raise RuntimeError(f"OCTAGRAM_REPO is not a Git checkout: {REPO}")
    _git(["rev-parse", "--verify", f"{BASE_REF}^{{commit}}"], cwd=REPO)
    if WORKTREE.exists():
        if not (WORKTREE / ".git").exists():
            raise RuntimeError(f"Worktree path exists but is not a Git worktree: {WORKTREE}")
        top = Path(_git(["rev-parse", "--show-toplevel"], cwd=WORKTREE).stdout.strip()).resolve()
        if top != WORKTREE:
            raise RuntimeError(f"Unexpected worktree root: {top}")
        return
    WORKTREE.parent.mkdir(parents=True, exist_ok=True)
    _git(["worktree", "add", "--detach", str(WORKTREE), BASE_REF], cwd=REPO, timeout=120)


def _safe_repo_path(raw: str) -> Path:
    rel = PurePosixPath(raw)
    if rel.is_absolute() or not rel.parts or any(part in ("..", "") for part in rel.parts):
        raise HTTPException(status_code=400, detail="Path must be repository-relative and cannot contain '..'.")
    target = (WORKTREE / Path(*rel.parts)).resolve()
    try:
        target.relative_to(WORKTREE)
    except ValueError:
        raise HTTPException(status_code=400, detail="Path escapes the isolated worktree.")
    return target


def repo_search(query: str) -> str:
    result = _git(
        ["grep", "--line-number", "--no-heading", "--ignore-case",
         "--fixed-strings", "-e", query, "--", "."],
        cwd=WORKTREE, timeout=45, check=False,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stdout[-12000:])
    return result.stdout[:20000] or "No matching text found."


def repo_read(raw_path: str) -> str:
    target = _safe_repo_path(raw_path)
    if not target.is_file():
        raise HTTPException(status_code=404, detail="File not found.")
    size = target.stat().st_size
    if size > MAX_FILE_BYTES:
        raise HTTPException(status_code=413, detail=f"File is {size} bytes; limit is {MAX_FILE_BYTES}.")
    try:
        return target.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        raise HTTPException(status_code=415, detail="Only UTF-8 text files can be read.")


def apply_patch(patch: str) -> str:
    # Reject traversal and Git metadata paths before invoking git.
    for line in patch.splitlines():
        if line.startswith(("+++ ", "--- ")):
            raw_path = line[4:].split("\\t", 1)[0]
            if raw_path == "/dev/null":
                continue
            if raw_path.startswith(("a/", "b/")):
                raw_path = raw_path[2:]
            candidate = PurePosixPath(raw_path)
            if candidate.is_absolute() or ".." in candidate.parts or (candidate.parts and candidate.parts[0] == ".git"):
                raise HTTPException(status_code=400, detail="Patch contains an unsafe path.")
    # git apply rejects paths escaping the worktree unless --unsafe-paths is used.
    check = subprocess.run(
        ["git", "apply", "--check", "--whitespace=nowarn", "-"],
        cwd=WORKTREE, input=patch, text=True, encoding="utf-8",
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30, check=False,
    )
    if check.returncode != 0:
        raise HTTPException(status_code=400, detail=f"Patch rejected; no files changed:\n{check.stdout[-8000:]}")
    applied = subprocess.run(
        ["git", "apply", "--whitespace=nowarn", "-"],
        cwd=WORKTREE, input=patch, text=True, encoding="utf-8",
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30, check=False,
    )
    if applied.returncode != 0:
        raise HTTPException(status_code=400, detail=f"Patch application failed:\n{applied.stdout[-8000:]}")
    return "Patch applied to isolated worktree. No commit was created."


def _docker_build_command(recipe: str) -> list[str]:
    recipe_command = BUILD_RECIPES[recipe]
    command = [
        DOCKER_BIN, "run", "--rm",
        "--network=none", "--cap-drop=ALL", "--security-opt=no-new-privileges",
        "--pids-limit=256", "--memory=8g", "--cpus=4", "--read-only",
        "--tmpfs", "/tmp:rw,nosuid,nodev,size=2g",
        "--env", "CARGO_HOME=/tmp/cargo",
        "--env", "CARGO_NET_OFFLINE=true",
        "--mount", f"type=bind,src={WORKTREE},dst=/workspace,rw",
        "--workdir", "/workspace",
    ]
    cargo_registry = Path.home() / ".cargo" / "registry"
    cargo_git = Path.home() / ".cargo" / "git"
    if cargo_registry.is_dir():
        command += ["--mount", f"type=bind,src={cargo_registry.resolve()},dst=/opt/octagram-cargo-registry,readonly"]
    if cargo_git.is_dir():
        command += ["--mount", f"type=bind,src={cargo_git.resolve()},dst=/opt/octagram-cargo-git,readonly"]
    command += [DOCKER_IMAGE, *recipe_command]
    return command


def run_build(recipe: str) -> dict[str, Any]:
    if recipe not in BUILD_RECIPES:
        raise HTTPException(status_code=400, detail=f"Unknown recipe. Allowed: {', '.join(BUILD_RECIPES)}")
    command = _docker_build_command(recipe)
    try:
        result = subprocess.run(
            command, cwd=WORKTREE, text=True, encoding="utf-8", errors="replace",
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=BUILD_TIMEOUT, check=False,
        )
        return {
            "recipe": recipe, "command": command, "returncode": result.returncode,
            "passed": result.returncode == 0, "output": result.stdout[-30000:],
            "evidence_level": "host-build-in-container",
        }
    except FileNotFoundError:
        raise HTTPException(status_code=503, detail=f"Docker executable not found: {DOCKER_BIN}")
    except subprocess.TimeoutExpired as exc:
        output = exc.stdout or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        return {
            "recipe": recipe, "command": command, "returncode": None,
            "passed": False, "timed_out": True, "output": str(output)[-30000:],
            "evidence_level": "host-build-in-container",
        }


def repo_context(query: str) -> str:
    import re
    stop = {"the", "and", "for", "with", "from", "that", "this", "into", "how", "what",
            "yang", "dan", "untuk", "dari", "dengan", "pada", "buat", "tolong", "bisa"}
    terms = list(dict.fromkeys(
        word for word in re.findall(r"[A-Za-z_][A-Za-z0-9_./-]{2,}", query)
        if word.lower() not in stop
    ))[:8]
    excerpts: list[str] = []
    seen: set[str] = set()
    for term in terms:
        result = repo_search(term)
        if result == "No matching text found.":
            continue
        for line in result.splitlines():
            if line not in seen:
                seen.add(line)
                excerpts.append(line)
            if len(excerpts) >= 60:
                break
        if len(excerpts) >= 60:
            break
    return "\n".join(excerpts)[:12000] or "No matching repository snippets found."


def ask_local_model(message: str, context: str) -> str:
    body = {
        "model": LLM_MODEL,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT + ("\n\nRepository context:\n" + context if context else "")},
            {"role": "user", "content": message},
        ],
        "temperature": 0.2,
        "max_tokens": 1800,
        "stream": False,
    }
    request = urllib.request.Request(
        LLM_BASE_URL + "/chat/completions",
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {LLM_API_KEY}"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=240) as response:
            payload = json.loads(response.read().decode("utf-8"))
        return payload["choices"][0]["message"]["content"]
    except (urllib.error.URLError, TimeoutError, KeyError, IndexError, ValueError) as exc:
        raise HTTPException(status_code=502, detail=f"Local LLM request failed: {exc}")


app = FastAPI(title="OCTAGRAM AI Developer", version="0.1.0")


@app.on_event("startup")
def startup() -> None:
    ensure_worktree()


@app.get("/health")
def health() -> dict[str, Any]:
    ensure_worktree()
    head = _git(["rev-parse", "HEAD"], cwd=WORKTREE).stdout.strip()
    status = _git(["status", "--short"], cwd=WORKTREE, check=False).stdout.strip()
    return {
        "status": "ok", "repository": str(REPO), "base_ref": BASE_REF,
        "worktree": str(WORKTREE), "head": head, "dirty": bool(status),
        "llm_base_url": LLM_BASE_URL, "llm_model": LLM_MODEL,
        "build_recipes": sorted(BUILD_RECIPES),
    }


@app.post("/workspace/search")
def search_endpoint(request: SearchRequest) -> dict[str, str]:
    ensure_worktree()
    return {"results": repo_search(request.query)}


@app.post("/workspace/read")
def read_endpoint(request: ReadRequest) -> dict[str, str]:
    ensure_worktree()
    return {"path": request.path, "content": repo_read(request.path)}


@app.post("/workspace/apply-patch")
def patch_endpoint(request: PatchRequest) -> dict[str, str]:
    ensure_worktree()
    return {"result": apply_patch(request.patch)}


@app.post("/workspace/build")
def build_endpoint(request: BuildRequest) -> dict[str, Any]:
    ensure_worktree()
    return run_build(request.recipe)


@app.post("/chat")
def chat_endpoint(request: ChatRequest) -> dict[str, str]:
    ensure_worktree()
    context = ""
    if request.include_context:
        # Search by the user's complete request; results are bounded before prompt insertion.
        context = repo_context(request.message)
    return {"model": LLM_MODEL, "answer": ask_local_model(request.message, context)}
