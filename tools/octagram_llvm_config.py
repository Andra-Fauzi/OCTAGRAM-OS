#!/usr/bin/env python3
"""Generate a host-runnable llvm-config for the OCTAGRAM LLVM install.

`build_llvm_octagram.sh` cross-compiles LLVM, so `$prefix/bin/llvm-config` is an
x86_64-unknown-octagram ELF: it cannot run on the build machine, yet bootstrap
executes llvm-config (for --libs, --system-libs, --cxxflags, --version) while
building rustc. This tool writes `$prefix/bin/octagram-llvm-config`, a portable
script that reports the same answers for the same install.

The data comes from the LLVM build tree:

  tools/llvm-config/BuildVariables.inc   version, flags, system libraries
  tools/llvm-config/LibraryDependencies.inc  component to library mapping
  include/llvm/Config/llvm-config.h      version string, host triple

Library order follows llvm-config.cpp: visit components depth-first, appending
a library after its dependencies, then reverse the whole list so dependents
precede their dependencies, which is what a static link needs.

Usage:
  tools/octagram_llvm_config.py --build-dir DIR --prefix DIR --output FILE
"""

import argparse
import json
import re
import sys
from pathlib import Path

COMPONENT_RE = re.compile(
    r'\{\s*"(?P<name>[A-Za-z0-9_.+-]+)"\s*,\s*'
    r'(?P<library>nullptr|"[^"]*")\s*,\s*'
    r'(?P<installed>true|false)\s*,\s*'
    r'\{(?P<deps>[^}]*)\}\s*\}'
)
EXTENSION_RE = re.compile(
    r'\{\s*"(?P<name>[A-Za-z0-9_.+-]+)"\s*,\s*\{(?P<deps>[^}]*)\}\s*\}'
)
DEFINE_RE = re.compile(r'^#define\s+(?P<name>[A-Z0-9_]+)\s+"(?P<value>[^"]*)"', re.M)

RUNTIME = r'''
import json
import sys
import os

DATA = json.loads(DATA_JSON)


def usage(exit_code):
    sys.stderr.write(
        "usage: octagram-llvm-config <OPTION>... [<COMPONENT>...]\n"
        "\n"
        "This is the host-runnable llvm-config for the cross-built OCTAGRAM\n"
        "LLVM install; the binary next to it only runs inside OCTAGRAM.\n"
        "\n"
        "Options: --assertion-mode --bindir --build-mode --build-system --cflags\n"
        "  --cmakedir --components --cppflags --cxxflags --has-rtti --help\n"
        "  --host-target --ignore-libllvm --includedir --ldflags --libdir --libfiles\n"
        "  --libnames --libs --obj-root --prefix --shared-mode --src-root\n"
        "  --system-libs --targets-built --version\n"
    )
    sys.exit(exit_code)


def library_path(library):
    return os.path.join(DATA["libdir"], "lib" + library + ".a")


def compute_libs(components):
    components_map = DATA["components"]
    library_of = {name: entry["library"] for name, entry in components_map.items()}
    installed = {name: entry["installed"] for name, entry in components_map.items()}
    required = []
    visited = set()

    def visit(name):
        if name in visited:
            return
        visited.add(name)
        if name not in components_map:
            sys.stderr.write(
                "octagram-llvm-config: unknown component name: %s\n" % name
            )
            sys.exit(1)
        if not installed[name]:
            return
        for dependency in components_map[name]["requires"]:
            visit(dependency)
        if name == "extensions":
            for extension in DATA["extensions"]:
                for dependency in extension["requires"]:
                    if dependency in components_map:
                        visit(dependency)
                    else:
                        required.append(dependency)
        if library_of[name]:
            required.append(library_of[name])

    for component in components or ["all"]:
        visit(component.lower())

    required.reverse()
    return required


def main(argv):
    print_libs = print_names = print_files = print_system_libs = False
    print_shared_mode = False
    link_shared = False
    components = []
    saw_option = False

    for argument in argv:
        if not argument.startswith("-"):
            components.append(argument)
            continue
        saw_option = True
        if argument == "--version":
            print(DATA["version"])
        elif argument == "--prefix":
            print(DATA["prefix"])
        elif argument == "--src-root":
            print(DATA["src_root"])
        elif argument == "--obj-root":
            print(DATA["prefix"])
        elif argument == "--bindir":
            print(DATA["bindir"])
        elif argument == "--includedir":
            print(DATA["includedir"])
        elif argument == "--libdir":
            print(DATA["libdir"])
        elif argument == "--cmakedir":
            print(DATA["cmakedir"])
        elif argument == "--cppflags":
            print("-I" + DATA["includedir"] + " " + DATA["cppflags"])
        elif argument == "--cflags":
            print("-I" + DATA["includedir"] + " " + DATA["cflags"])
        elif argument == "--cxxflags":
            print("-I" + DATA["includedir"] + " " + DATA["cxxflags"])
        elif argument == "--ldflags":
            print("-L" + DATA["libdir"] + " " + DATA["ldflags"])
        elif argument == "--targets-built":
            print(DATA["targets_built"])
        elif argument == "--host-target":
            print(DATA["host_target"])
        elif argument == "--build-mode":
            print(DATA["build_mode"])
        elif argument == "--assertion-mode":
            print(DATA["assertion_mode"])
        elif argument == "--build-system":
            print(DATA["build_system"])
        elif argument == "--has-rtti":
            print(DATA["has_rtti"])
        elif argument == "--components":
            print(" ".join(DATA["component_names"]))
        elif argument == "--libs":
            print_libs = True
        elif argument == "--libnames":
            print_names = True
        elif argument == "--libfiles":
            print_files = True
        elif argument == "--system-libs":
            print_system_libs = True
        elif argument == "--shared-mode":
            print_shared_mode = True
        elif argument == "--link-static":
            link_shared = False
        elif argument == "--link-shared":
            link_shared = True
        elif argument == "--ignore-libllvm":
            pass
        elif argument in ("--help", "-h"):
            usage(0)
        elif argument == "--quote-paths":
            pass
        else:
            sys.stderr.write("octagram-llvm-config: unknown argument: %s\n" % argument)
            usage(1)

    if not saw_option:
        usage(1)

    if link_shared:
        sys.stderr.write(
            "octagram-llvm-config: error: %s is missing\n" % DATA["dylib_name"]
        )
        return 1

    if print_libs or print_names or print_files or print_system_libs or print_shared_mode:
        libraries = compute_libs(components)
        missing = [library_path(lib) for lib in libraries if not os.path.exists(library_path(lib))]
        if missing:
            # A partial install (configure done, build still running) must not
            # claim flags it cannot back; llvm-config fails the same way.
            for path in missing:
                sys.stderr.write("octagram-llvm-config: error: missing: %s\n" % path)
            return 1

        if print_shared_mode:
            print("static")
            return 0

        if print_libs or print_names or print_files:
            if print_names:
                print(" ".join("lib" + lib + ".a" for lib in libraries))
            elif print_files:
                print(" ".join(library_path(lib) for lib in libraries))
            else:
                print(" ".join("-l" + lib for lib in libraries))
        if print_system_libs:
            print(DATA["system_libs"])
    elif components:
        sys.stderr.write("octagram-llvm-config: error: components given, but unused\n")
        usage(1)

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
'''


def parse_defines(text):
    return {match.group("name"): match.group("value") for match in DEFINE_RE.finditer(text)}


def parse_components(text):
    components = {}
    for match in COMPONENT_RE.finditer(text):
        library = match.group("library")
        components[match.group("name")] = {
            "library": None if library == "nullptr" else library.strip('"'),
            "installed": match.group("installed") == "true",
            "requires": re.findall(r'"([^"]+)"', match.group("deps")),
        }
    return components


def parse_extensions(text):
    if text is None:
        return []
    return [
        {
            "name": match.group("name"),
            "requires": re.findall(r'"([^"]+)"', match.group("deps")),
        }
        for match in EXTENSION_RE.finditer(text)
    ]


def build_data(build_dir: Path, prefix: Path):
    build_variables = parse_defines(
        (build_dir / "tools/llvm-config/BuildVariables.inc").read_text()
    )
    config_header = (build_dir / "include/llvm/Config/llvm-config.h").read_text()
    components_inc = (build_dir / "tools/llvm-config/LibraryDependencies.inc").read_text()
    extension_path = build_dir / "tools/llvm-config/ExtensionDependencies.inc"
    extensions = parse_extensions(
        extension_path.read_text() if extension_path.exists() else None
    )

    components = parse_components(components_inc)
    if "all" not in components:
        raise SystemExit("octagram_llvm_config: LibraryDependencies.inc has no 'all' component")

    libdir_suffix = build_variables.get("LLVM_LIBDIR_SUFFIX", "")
    includedir = build_variables.get("LLVM_INSTALL_INCLUDEDIR", "include")
    version = re.search(
        r'#define LLVM_VERSION_STRING "([^"]+)"', config_header
    ).group(1)
    host_target = re.search(
        r'#define LLVM_DEFAULT_TARGET_TRIPLE "([^"]+)"', config_header
    ).group(1)

    return {
        "version": version,
        "prefix": str(prefix),
        "src_root": build_variables.get("LLVM_SRC_ROOT", str(prefix)),
        "bindir": str(prefix / build_variables.get("LLVM_TOOLS_INSTALL_DIR", "bin")),
        "includedir": str(prefix / includedir),
        "libdir": str(prefix / "lib" / libdir_suffix) if libdir_suffix else str(prefix / "lib"),
        "cmakedir": str(prefix / build_variables.get("LLVM_INSTALL_PACKAGE_DIR", "lib/cmake/llvm")),
        "cppflags": build_variables.get("LLVM_CPPFLAGS", ""),
        "cflags": build_variables.get("LLVM_CFLAGS", ""),
        "cxxflags": build_variables.get("LLVM_CXXFLAGS", ""),
        "ldflags": build_variables.get("LLVM_LDFLAGS", ""),
        "system_libs": build_variables.get("LLVM_SYSTEM_LIBS", ""),
        "targets_built": build_variables.get("LLVM_TARGETS_BUILT", ""),
        "host_target": host_target,
        "build_mode": build_variables.get("LLVM_BUILDMODE", "Release"),
        "assertion_mode": "OFF",
        "build_system": build_variables.get("LLVM_BUILD_SYSTEM", "cmake"),
        "has_rtti": "YES" if build_variables.get("LLVM_HAS_RTTI") == "1" else "NO",
        "dylib_name": "libLLVM-" + build_variables.get("LLVM_DYLIB_VERSION", "") + ".so",
        "component_names": [
            name for name, entry in components.items() if entry["installed"]
        ],
        "components": components,
        "extensions": extensions,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", required=True, type=Path)
    parser.add_argument("--prefix", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()

    data = build_data(arguments.build_dir, arguments.prefix)
    script = (
        "#!/usr/bin/env python3\n"
        "# Generated by tools/octagram_llvm_config.py; do not edit.\n"
        "# Host-runnable llvm-config for the cross-built OCTAGRAM LLVM install.\n"
        "DATA_JSON = r'''" + json.dumps(data) + "'''\n"
        + RUNTIME
    )
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(script)
    arguments.output.chmod(0o755)
    print(
        "wrote {} ({} components, {} libraries for 'all')".format(
            arguments.output,
            len(data["components"]),
            len(re.findall(r'"library"', json.dumps(data["components"]))),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
