#!/usr/bin/env python3
"""generate_rust_analyzer - Generates the `rust-project.json` file for `rust-analyzer`.
"""

import argparse
import json
import logging
import pathlib
import sys

def generate_crates(srctree, objtree, sysroot_src, bindings_file):
    # Generate the configuration list.
    cfg = []
    with open(objtree / "include" / "generated" / "rustc_cfg") as fd:
        for line in fd:
            line = line.replace("--cfg=", "")
            line = line.replace("\n", "")
            cfg.append(line)

    # Now fill the crates list -- dependencies need to come first.
    #
    # Avoid O(n^2) iterations by keeping a map of indexes.
    crates = []
    crates_indexes = {}

    def append_crate(display_name, root_module, is_workspace_member, deps, cfg):
        crates_indexes[display_name] = len(crates)
        crates.append({
            "display_name": display_name,
            "root_module": str(root_module),
            "is_workspace_member": is_workspace_member,
            "deps": [{"crate": crates_indexes[dep], "name": dep} for dep in deps],
            "cfg": cfg,
            "edition": "2018",
            "env": {
                "RUST_MODFILE": "This is only for rust-analyzer"
            }
        })

    # First, the ones in `rust/` since they are a bit special.
    append_crate(
        "core",
        sysroot_src / "core" / "src" / "lib.rs",
        False,
        [],
        [],
    )

    append_crate(
        "compiler_builtins",
        srctree / "rust" / "compiler_builtins.rs",
        True,
        [],
        [],
    )

    append_crate(
        "alloc",
        sysroot_src / "alloc" / "src" / "lib.rs",
        False,
        ["core", "compiler_builtins"],
        [],
    )

    append_crate(
        "macros",
        srctree / "rust" / "macros" / "lib.rs",
        True,
        [],
        [],
    )
    crates[-1]["proc_macro_dylib_path"] = "rust/libmacros.so"

    append_crate(
        "build_error",
        srctree / "rust" / "build_error.rs",
        True,
        ["core", "compiler_builtins"],
        [],
    )

    append_crate(
        "kernel",
        srctree / "rust" / "kernel" / "lib.rs",
        True,
        ["core", "alloc", "macros", "build_error"],
        cfg,
    )
    crates[-1]["env"]["RUST_BINDINGS_FILE"] = str(bindings_file.resolve(True))
    crates[-1]["source"] = {
        "include_dirs": [
            str(srctree / "rust" / "kernel"),
            str(objtree / "rust")
        ],
        "exclude_dirs": [],
    }

    # Then, the rest outside of `rust/`.
    #
    # We explicitly mention the top-level folders we want to cover.
    for folder in ("samples", "drivers"):
        for path in (srctree / folder).rglob("*.rs"):
            logging.info("Checking %s", path)
            name = path.name.replace(".rs", "")

            # Skip those that are not crate roots.
            if f"{name}.o" not in open(path.parent / "Makefile").read():
                continue

            logging.info("Adding %s", name)
            append_crate(
                name,
                path,
                True,
                ["core", "alloc", "kernel"],
                cfg,
            )

    return crates

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--verbose', '-v', action='store_true')
    parser.add_argument("srctree", type=pathlib.Path)
    parser.add_argument("objtree", type=pathlib.Path)
    parser.add_argument("sysroot_src", type=pathlib.Path)
    parser.add_argument("bindings_file", type=pathlib.Path)
    args = parser.parse_args()

    logging.basicConfig(
        format="[%(asctime)s] [%(levelname)s] %(message)s",
        level=logging.INFO if args.verbose else logging.WARNING
    )

    rust_project = {
        "crates": generate_crates(args.srctree, args.objtree, args.sysroot_src, args.bindings_file),
        "sysroot_src": str(args.sysroot_src),
    }

    json.dump(rust_project, sys.stdout, sort_keys=True, indent=4)

if __name__ == "__main__":
    main()
