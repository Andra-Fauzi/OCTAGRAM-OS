#!/usr/bin/env python3
"""Compile a workspace's real resolved OCTAGRAM dependency graph.

The stage-2 bootstrap runs `x.py build --stage 2 --host x86_64-unknown-octagram
compiler/rustc src/tools/cargo`. Every registry crate in those two workspaces
is compiled for OCTAGRAM, and each one can name a libc item that the guest libc
does not export yet. The hand-written probes in check_rust_stage2_deps.sh only
cover the crates that already failed once, so the next crate to break is found
hours later, after the LLVM build.

This tool asks Cargo itself for the answer instead of keeping a list:

  1. `cargo tree --locked --edges normal --target <json>` on the pinned
     checkout resolves the graph exactly as the stage-2 build resolves it: the
     locked versions, the manifest feature selectors and the cfg evaluation for
     target_os = "octagram", so windows/macos-only dependencies drop out.
  2. Every registry package in that graph is compiled for OCTAGRAM with
     `cargo check -p <name>@<version>`, sharing one target directory so the
     build-std artifacts are reused.

Crates that declare `links` build C code for the target and need the cross C
toolchain (`make -C user sysroot`), which does not exist yet at preflight time.
They are reported and skipped together with every crate that depends on them,
since compiling such a dependent pulls its C build in anyway. Proc-macro crates
are compiled for the build platform, so they are not in the OCTAGRAM graph.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

TREE_LINE = re.compile(r"^(?P<depth>\d+)(?P<name>\S+) v(?P<version>\S+)(?P<rest>.*)$")
# `cargo tree --prefix none` marks packages that live in the checkout itself
# with their path in parentheses, for example `cargo v0.103.0 (/src/cargo)`.
PATH_ANNOTATION = re.compile(r"\((?:[^()]*/[^()]*)\)")
LINKS_KEY = re.compile(r"(?m)^links\s*=")
FAILED_CRATE = re.compile(r"could not compile `([^`]+)`")
# Cargo's feature resolver panics instead of reporting a package it cannot
# select on its own ("did not find features for ..."). Such a package is still
# compiled as a dependency of the other selected crates, so the panic limits
# only the direct selection, not the graph coverage.
RESOLVER_PANIC = "panicked at"


def run(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True)


def registry_roots(path: Path) -> list[Path]:
    """Accept either a Cargo source root or one of its index directories."""
    if not path.is_dir():
        raise SystemExit(f"Cargo registry source root is missing: {path}")
    roots = [path]
    for entry in path.iterdir():
        if entry.is_dir() and not entry.name.startswith("."):
            roots.append(entry)
    return roots


def manifest_for(roots: list[Path], name: str, version: str) -> Path | None:
    for root in roots:
        candidate = root / f"{name}-{version}" / "Cargo.toml"
        if candidate.is_file():
            return candidate
    return None


class Package:
    def __init__(self, name: str, version: str) -> None:
        self.name = name
        self.version = version
        self.children: set[tuple[str, str]] = set()
        self.proc_macro = False
        self.local = False

    @property
    def key(self) -> tuple[str, str]:
        return (self.name, self.version)

    @property
    def spec(self) -> str:
        return f"{self.name}@{self.version}"


class Graph:
    def __init__(self) -> None:
        self.packages: dict[tuple[str, str], Package] = {}

    def parse(self, tree_output: str, label: str) -> None:
        stack: list[Package] = []
        for line in tree_output.splitlines():
            line = line.strip()
            if not line:
                continue
            match = TREE_LINE.match(line)
            if not match:
                raise SystemExit(f"{label}: unexpected cargo tree line: {line!r}")
            depth = int(match.group("depth"))
            key = (match.group("name"), match.group("version"))
            package = self.packages.get(key)
            if package is None:
                package = Package(*key)
                self.packages[key] = package
            rest = match.group("rest").strip()
            package.proc_macro = package.proc_macro or "proc-macro" in rest
            package.local = package.local or bool(PATH_ANNOTATION.search(rest))
            if depth == 0:
                stack = [package]
            else:
                stack[depth - 1].children.add(key)
                stack = stack[:depth] + [package]

    def ancestors_of(self, keys: set[tuple[str, str]]) -> set[tuple[str, str]]:
        """Return every package that pulls one of keys in transitively."""
        parents: dict[tuple[str, str], set[tuple[str, str]]] = {
            key: set() for key in self.packages
        }
        for key, package in self.packages.items():
            for child in package.children:
                parents[child].add(key)
        blocked: set[tuple[str, str]] = set()
        pending = list(keys)
        while pending:
            for parent in parents.get(pending.pop(), ()):
                if parent not in blocked:
                    blocked.add(parent)
                    pending.append(parent)
        return blocked


def plan(
    graph: Graph,
    roots: list[Path],
    label: str,
    skip_names: set[str],
) -> tuple[list[str], list[str], list[str], list[str]]:
    native: set[tuple[str, str]] = set()
    for key, package in graph.packages.items():
        if package.local:
            # A checkout package: x.py builds it, so there is no registry
            # source to inspect here.
            continue
        manifest = manifest_for(roots, *key)
        if manifest is None:
            raise SystemExit(
                f"{label}: {package.name} {package.version} is in the OCTAGRAM "
                "graph but not in Cargo's registry cache; fetch it first"
            )
        if LINKS_KEY.search(manifest.read_text(encoding="utf-8")):
            native.add(key)

    # A package from the checkout itself is built by x.py, and anything that
    # pulls a links crate would build C for the target as a side effect. A
    # crate whose OCTAGRAM port is still missing blocks its dependents too,
    # because compiling one of them would compile it as a dependency anyway.
    explicitly_skipped = {
        key for key in graph.packages if key[0] in skip_names
    }
    untouchable = native | explicitly_skipped
    blocked = graph.ancestors_of(untouchable) | untouchable
    compile_specs: list[str] = []
    native_specs: list[str] = []
    blocked_specs: list[str] = []
    skipped_specs: list[str] = []
    for key, package in sorted(graph.packages.items()):
        if package.local or package.proc_macro:
            continue
        if key in native:
            native_specs.append(package.spec)
        elif key in explicitly_skipped:
            skipped_specs.append(package.spec)
        elif key in blocked:
            blocked_specs.append(package.spec)
        else:
            compile_specs.append(package.spec)
    return compile_specs, native_specs, blocked_specs, skipped_specs


class GraphCheck:
    def __init__(self, command: list[str], cwd: Path, label: str) -> None:
        self.command = command
        self.cwd = cwd
        self.label = label

    def check(self, specs: list[str]) -> subprocess.CompletedProcess[str]:
        return run([*self.command, *[arg for spec in specs for arg in ("-p", spec)]], self.cwd)

    def compile(self, specs: list[str]) -> list[str]:
        """Compile specs; return the crates that failed to compile."""
        if not specs:
            return []
        result = self.check(specs)
        if result.returncode == 0:
            return []
        if RESOLVER_PANIC in result.stderr and len(specs) > 1:
            # Isolate the package Cargo refuses to select by halving the batch.
            middle = len(specs) // 2
            return self.compile(specs[:middle]) + self.compile(specs[middle:])
        if RESOLVER_PANIC in result.stderr:
            print(
                f"WARNING: {self.label}: cargo's feature resolver cannot select "
                f"{specs[0]} directly; it is still compiled as a dependency of "
                "the other crates in this graph",
                file=sys.stderr,
            )
            return []
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        failed = sorted(set(FAILED_CRATE.findall(result.stderr)))
        return failed or specs


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkout", required=True, type=Path)
    parser.add_argument("--package", required=True, help="root package of the graph")
    parser.add_argument("--label", required=True, help="name used in the report")
    parser.add_argument("--target", required=True, type=Path)
    parser.add_argument("--toolchain", required=True)
    parser.add_argument("--registry-root", required=True, type=Path)
    parser.add_argument(
        "--skip",
        action="append",
        default=[],
        metavar="CRATE",
        help="crate name whose OCTAGRAM port is still missing; reported, not "
        "compiled. Use it only with a comment naming the gap.",
    )
    parser.add_argument("--target-dir", required=True, type=Path)
    parser.add_argument(
        "--work-dir",
        type=Path,
        help="cargo working directory; must not be inside the checkout so "
        "-Zbuild-std uses the patched rust-src standard library",
    )
    args = parser.parse_args()

    manifest = args.checkout / "Cargo.toml"
    if not manifest.is_file():
        raise SystemExit(f"{args.label}: no Cargo workspace at {manifest}")
    work_dir = args.work_dir or args.checkout.parent
    if not work_dir.is_dir():
        raise SystemExit(f"{args.label}: working directory is missing: {work_dir}")

    # Flags after the subcommand: `--locked` is not a global Cargo option.
    base = ["cargo", f"+{args.toolchain}"]
    common = [
        "--locked",
        "--manifest-path",
        str(manifest),
        "--target",
        str(args.target),
        "-Zjson-target-spec",
    ]

    print(f"{args.label}: resolving the OCTAGRAM dependency graph", flush=True)
    tree = run(
        [
            *base,
            "tree",
            *common,
            "--edges",
            "normal",
            # Depth prefixes keep the parent/child edges, which the plan needs
            # to drop crates that pull a native C build in transitively.
            "--prefix",
            "depth",
            "-p",
            args.package,
        ],
        work_dir,
    )
    if tree.returncode != 0:
        sys.stderr.write(tree.stderr)
        raise SystemExit(f"{args.label}: cargo tree failed for {args.package}")

    graph = Graph()
    graph.parse(tree.stdout, args.label)
    roots = registry_roots(args.registry_root)
    compile_specs, native_specs, blocked_specs, skipped_specs = plan(
        graph, roots, args.label, {name.split("@")[0] for name in args.skip}
    )

    print(
        f"{args.label}: {len(graph.packages)} resolved packages, "
        f"{len(compile_specs)} registry crates to compile",
        flush=True,
    )
    if native_specs:
        print(
            f"{args.label}: skipped {len(native_specs)} crate(s) that build C for "
            f"the target (needs `make -C user sysroot`): {', '.join(native_specs)}",
            flush=True,
        )
    if blocked_specs:
        print(
            f"{args.label}: skipped {len(blocked_specs)} crate(s) that pull one of "
            f"those in: {', '.join(blocked_specs)}",
            flush=True,
        )
    if skipped_specs:
        print(
            f"{args.label}: {len(skipped_specs)} crate(s) excluded because their "
            f"OCTAGRAM port is still missing: {', '.join(skipped_specs)}",
            flush=True,
        )

    args.target_dir.mkdir(parents=True, exist_ok=True)
    check = GraphCheck(
        [
            *base,
            "check",
            *common,
            # cargo tree takes no --target-dir; only the compile step shares
            # the artifacts the probe projects already built.
            "--target-dir",
            str(args.target_dir),
            "-Zbuild-std=core,alloc,std",
        ],
        work_dir,
        args.label,
    )
    failed = sorted(set(check.compile(compile_specs)))
    if failed:
        print(
            f"OCTAGRAM-CARGO-GRAPH-FAIL: {args.label}: "
            f"{', '.join(failed)} does not compile for OCTAGRAM",
            file=sys.stderr,
        )
        return 1

    print(
        f"OCTAGRAM-CARGO-GRAPH-CHECK-PASS: {args.label}: compiled "
        f"{len(compile_specs)} registry crate(s) for OCTAGRAM"
    )
    return 0


if __name__ == "__main__":
    os.environ.setdefault("CARGO_TERM_COLOR", "never")
    raise SystemExit(main())
