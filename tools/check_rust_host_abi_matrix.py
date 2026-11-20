#!/usr/bin/env python3
"""Validate the Rust host ABI matrix structure and non-overclaiming invariants."""

from __future__ import annotations

import json
from pathlib import Path

MATRIX = Path("tools/rust_host_abi_matrix.json")
ALLOWED_STATUSES = {
    "qemu_tested",
    "build_tested",
    "implemented_partial",
    "not_implemented",
}
REQUIRED_COMPONENTS = {
    "rust_target_spec",
    "rust_core",
    "rust_alloc",
    "threads_tls",
    "futex",
    "memory_mapping",
    "filesystem",
    "rust_std",
    "rustc_host",
    "cargo_host",
    "llvm_lld_host",
    "bootstrap_stage0",
}


def fail(message: str) -> None:
    raise SystemExit(f"RUST-HOST-ABI-MATRIX-FAIL: {message}")


def main() -> int:
    try:
        data = json.loads(MATRIX.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read valid JSON: {error}")

    if data.get("schema_version") != 1:
        fail("schema_version must be 1")
    if data.get("full_self_hosting_proven") is not False:
        fail("full_self_hosting_proven must remain false until compiler/kernel/image gates pass")

    triples = data.get("triples", {})
    expected_triples = {
        "build": "x86_64-unknown-linux-gnu",
        "host": "x86_64-unknown-octagram",
        "kernel_target": "x86_64-unknown-none",
    }
    if triples != expected_triples:
        fail(f"build/host/target triples do not match the documented bootstrap plan: {triples}")

    components = data.get("components")
    if not isinstance(components, list) or not components:
        fail("components must be a non-empty array")

    seen: set[str] = set()
    for component in components:
        if not isinstance(component, dict):
            fail("each component must be an object")
        component_id = component.get("id")
        if not isinstance(component_id, str) or not component_id:
            fail("every component needs a non-empty id")
        if component_id in seen:
            fail(f"duplicate component id: {component_id}")
        seen.add(component_id)
        if component.get("status") not in ALLOWED_STATUSES:
            fail(f"{component_id}: invalid status {component.get('status')!r}")
        for field in ("layer", "summary"):
            if not isinstance(component.get(field), str) or not component[field].strip():
                fail(f"{component_id}: {field} must be a non-empty string")
        for field in ("evidence", "limitations", "next_gate"):
            values = component.get(field)
            if not isinstance(values, list) or not values or not all(
                isinstance(value, str) and value.strip() for value in values
            ):
                fail(f"{component_id}: {field} must be a non-empty string array")

    missing = sorted(REQUIRED_COMPONENTS - seen)
    if missing:
        fail("missing required components: " + ", ".join(missing))

    for compiler_component in ("rust_std", "rustc_host", "cargo_host", "llvm_lld_host", "bootstrap_stage0"):
        found = next(item for item in components if item["id"] == compiler_component)
        if found["status"] != "not_implemented":
            fail(f"{compiler_component} cannot be promoted without new native execution evidence")

    print(
        "RUST-HOST-ABI-MATRIX-PASS: "
        f"{len(components)} components; build={triples['build']}; "
        f"host={triples['host']}; target={triples['kernel_target']}; "
        "full_self_hosting_proven=false"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
