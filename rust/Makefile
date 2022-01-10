# SPDX-License-Identifier: GPL-2.0

obj-$(CONFIG_RUST) += core.o compiler_builtins.o helpers.o
extra-$(CONFIG_RUST) += exports_core_generated.h

extra-$(CONFIG_RUST) += libmacros.so

extra-$(CONFIG_RUST) += bindings_generated.rs bindings_helpers_generated.rs
obj-$(CONFIG_RUST) += alloc.o kernel.o
extra-$(CONFIG_RUST) += exports_alloc_generated.h exports_kernel_generated.h

ifdef CONFIG_RUST_BUILD_ASSERT_DENY
extra-$(CONFIG_RUST) += build_error.o
else
obj-$(CONFIG_RUST) += build_error.o
endif

obj-$(CONFIG_RUST) += exports.o

ifeq ($(quiet),silent_)
cargo_quiet=-q
rust_test_quiet=-q
rustdoc_test_quiet=--test-args -q
else ifeq ($(quiet),quiet_)
rust_test_quiet=-q
rustdoc_test_quiet=--test-args -q
else
cargo_quiet=--verbose
endif

core-cfgs = \
    --cfg no_fp_fmt_parse

alloc-cfgs = \
    --cfg no_global_oom_handling \
    --cfg no_sync

quiet_cmd_rustdoc = RUSTDOC $(if $(rustdoc_host),H, ) $<
      cmd_rustdoc = \
	OBJTREE=$(abspath $(objtree)) \
	$(RUSTDOC) $(if $(rustdoc_host),,$(rust_cross_flags)) \
		$(filter-out -Cpanic=abort, $(filter-out --emit=%, $(rust_flags))) \
		$(rustc_target_flags) -L $(objtree)/rust \
		--output $(objtree)/rust/doc --crate-name $(subst rustdoc-,,$@) \
		@$(objtree)/include/generated/rustc_cfg $<

rustdoc: rustdoc-core rustdoc-macros rustdoc-compiler_builtins rustdoc-alloc rustdoc-kernel
	$(Q)cp $(srctree)/Documentation/rust/assets/* $(objtree)/rust/doc

rustdoc-macros: private rustdoc_host = yes
rustdoc-macros: private rustc_target_flags = --crate-type proc-macro \
    --extern proc_macro
rustdoc-macros: $(srctree)/rust/macros/lib.rs FORCE
	$(call if_changed,rustdoc)

rustdoc-compiler_builtins: $(srctree)/rust/compiler_builtins.rs rustdoc-core FORCE
	$(call if_changed,rustdoc)

# We need to allow `rustdoc::broken_intra_doc_links` because some
# `no_global_oom_handling` functions refer to non-`no_global_oom_handling`
# functions. Ideally `rustdoc` would have a way to distinguish broken links
# due to things that are "configured out" vs. entirely non-existing ones.
rustdoc-alloc: private rustc_target_flags = $(alloc-cfgs) \
    -Arustdoc::broken_intra_doc_links
rustdoc-alloc: $(srctree)/rust/alloc/lib.rs rustdoc-core \
    rustdoc-compiler_builtins FORCE
	$(call if_changed,rustdoc)

rustdoc-kernel: private rustc_target_flags = --extern alloc \
    --extern build_error \
    --extern macros=$(objtree)/rust/libmacros.so
rustdoc-kernel: $(srctree)/rust/kernel/lib.rs rustdoc-core \
    rustdoc-macros rustdoc-compiler_builtins rustdoc-alloc \
    $(objtree)/rust/libmacros.so $(objtree)/rust/bindings_generated.rs \
    $(objtree)/rust/bindings_helpers_generated.rs FORCE
	$(call if_changed,rustdoc)

quiet_cmd_rustc_test_library = RUSTC TL $<
      cmd_rustc_test_library = \
	OBJTREE=$(abspath $(objtree)) \
	$(RUSTC) $(filter-out --sysroot=%, $(filter-out -Cpanic=abort, $(filter-out --emit=%, $(rust_flags)))) \
		$(rustc_target_flags) --crate-type $(if $(rustc_test_library_proc),proc-macro,rlib) \
		--out-dir $(objtree)/rust/test/ --cfg testlib \
		--sysroot $(objtree)/rust/test/sysroot \
		-L $(objtree)/rust/test/ --crate-name $(subst rusttest-,,$(subst rusttestlib-,,$@)) $<

rusttestlib-build_error: $(srctree)/rust/build_error.rs rusttest-prepare FORCE
	$(call if_changed,rustc_test_library)

rusttestlib-macros: private rustc_target_flags = --extern proc_macro
rusttestlib-macros: private rustc_test_library_proc = yes
rusttestlib-macros: $(srctree)/rust/macros/lib.rs rusttest-prepare FORCE
	$(call if_changed,rustc_test_library)

quiet_cmd_rustdoc_test = RUSTDOC T $<
      cmd_rustdoc_test = \
	OBJTREE=$(abspath $(objtree)) \
	$(RUSTDOC) --test $(filter-out --sysroot=%, $(filter-out -Cpanic=abort, $(filter-out --emit=%, $(rust_flags)))) \
		$(rustc_target_flags) $(rustdoc_test_target_flags) \
		--sysroot $(objtree)/rust/test/sysroot $(rustdoc_test_quiet) \
		-L $(objtree)/rust/test \
		--output $(objtree)/rust/doc --crate-name $(subst rusttest-,,$@) \
		@$(objtree)/include/generated/rustc_cfg $<

# We cannot use `-Zpanic-abort-tests` because some tests are dynamic,
# so for the moment we skip `-Cpanic=abort`.
quiet_cmd_rustc_test = RUSTC T  $<
      cmd_rustc_test = \
	OBJTREE=$(abspath $(objtree)) \
	$(RUSTC) --test $(filter-out --sysroot=%, $(filter-out -Cpanic=abort, $(filter-out --emit=%, $(rust_flags)))) \
		$(rustc_target_flags) --out-dir $(objtree)/rust/test \
		--sysroot $(objtree)/rust/test/sysroot \
		-L $(objtree)/rust/test/ --crate-name $(subst rusttest-,,$@) $<; \
	$(objtree)/rust/test/$(subst rusttest-,,$@) $(rust_test_quiet) \
		$(rustc_test_run_flags)

rusttest: rusttest-macros rusttest-kernel

# This prepares a custom sysroot with our custom `alloc` instead of
# the standard one.
#
# This requires several hacks:
#   - Unlike `core` and `alloc`, `std` depends on more than a dozen crates,
#     including third-party crates that need to be downloaded, plus custom
#     `build.rs` steps. Thus hardcoding things here is not maintainable.
#   - `cargo` knows how to build the standard library, but it is an unstable
#     feature so far (`-Zbuild-std`).
#   - `cargo` only considers the use case of building the standard library
#     to use it in a given package. Thus we need to create a dummy package
#     and pick the generated libraries from there.
#   - Since we only keep a subset of upstream `alloc` in-tree, we need
#     to recreate it on the fly by putting our sources on top.
#   - The usual ways of modifying the dependency graph in `cargo` do not seem
#     to apply for the `-Zbuild-std` steps, thus we have to mislead it
#     by modifying the sources in the sysroot.
#   - To avoid messing with the user's Rust installation, we create a clone
#     of the sysroot. However, `cargo` ignores `RUSTFLAGS` in the `-Zbuild-std`
#     steps, thus we use a wrapper binary passed via `RUSTC` to pass the flag.
#
# In the future, we hope to avoid the whole ordeal by either:
#   - Making the `test` crate not depend on `std` (either improving upstream
#     or having our own custom crate).
#   - Making the tests run in kernel space (requires the previous point).
#   - Making `std` and friends be more like a "normal" crate, so that
#     `-Zbuild-std` and related hacks are not needed.
quiet_cmd_rustsysroot = RUSTSYSROOT
      cmd_rustsysroot = \
	rm -rf $(objtree)/rust/test; \
	mkdir -p $(objtree)/rust/test; \
	cp -a $(rustc_sysroot) $(objtree)/rust/test/sysroot; \
	cp -r $(srctree)/rust/alloc/* \
		$(objtree)/rust/test/sysroot/lib/rustlib/src/rust/library/alloc/src; \
	echo '\#!/bin/sh' > $(objtree)/rust/test/rustc_sysroot; \
	echo "$(RUSTC) --sysroot=$(abspath $(objtree)/rust/test/sysroot) \"\$$@\"" \
		>> $(objtree)/rust/test/rustc_sysroot; \
	chmod u+x $(objtree)/rust/test/rustc_sysroot; \
	$(CARGO) -q new $(objtree)/rust/test/dummy; \
	RUSTC=$(objtree)/rust/test/rustc_sysroot $(CARGO) $(cargo_quiet) \
		test -Zbuild-std --target $(rustc_host_target) \
		--manifest-path $(objtree)/rust/test/dummy/Cargo.toml; \
	rm $(objtree)/rust/test/sysroot/lib/rustlib/$(rustc_host_target)/lib/*; \
	cp $(objtree)/rust/test/dummy/target/$(rustc_host_target)/debug/deps/* \
		$(objtree)/rust/test/sysroot/lib/rustlib/$(rustc_host_target)/lib

rusttest-prepare: FORCE
	$(call if_changed,rustsysroot)

rusttest-macros: private rustc_target_flags = --extern proc_macro
rusttest-macros: private rustdoc_test_target_flags = --crate-type proc-macro
rusttest-macros: $(srctree)/rust/macros/lib.rs rusttest-prepare FORCE
	$(call if_changed,rustc_test)
	$(call if_changed,rustdoc_test)

rusttest-kernel: private rustc_target_flags = --extern alloc \
    --extern build_error --extern macros
rusttest-kernel: private rustc_test_run_flags = \
    --skip bindgen_test_layout_
rusttest-kernel: $(srctree)/rust/kernel/lib.rs rusttest-prepare \
    rusttestlib-build_error rusttestlib-macros FORCE
	$(call if_changed,rustc_test)
	$(call if_changed,rustc_test_library)
	$(call if_changed,rustdoc_test)

ifdef CONFIG_CC_IS_CLANG
bindgen_c_flags = $(c_flags)
else
# bindgen relies on libclang to parse C. Ideally, bindgen would support a GCC
# plugin backend and/or the Clang driver would be perfectly compatible with GCC.
#
# For the moment, here we are tweaking the flags on the fly. Some config
# options may not work (e.g. `GCC_PLUGIN_RANDSTRUCT` if we end up using one
# of those structs).
bindgen_skip_c_flags := -mno-fp-ret-in-387 -mpreferred-stack-boundary=% \
	-mskip-rax-setup -mgeneral-regs-only -msign-return-address=% \
	-mindirect-branch=thunk-extern -mindirect-branch-register -mrecord-mcount \
	-mabi=lp64 -mstack-protector-guard% -fconserve-stack -falign-jumps=% \
	-falign-loops=% -femit-struct-debug-baseonly \
	-fno-ipa-cp-clone -fno-ipa-sra -fno-partial-inlining \
	-fplugin-arg-arm_ssp_per_task_plugin-% \
	-fno-reorder-blocks -fno-allow-store-data-races -fasan-shadow-offset=% \
	-Wno-packed-not-aligned -Wno-format-truncation -Wno-format-overflow \
	-Wno-stringop-truncation -Wno-unused-but-set-variable \
	-Wno-stringop-overflow -Wno-restrict -Wno-maybe-uninitialized \
	-Werror=designated-init -Wno-zero-length-bounds -Wimplicit-fallthrough=% \
	-Wno-alloc-size-larger-than --param=% --param asan-%

# PowerPC
bindgen_skip_c_flags += -mtraceback=no -mno-pointers-to-nested-functions \
	-mno-string -mno-strict-align

# Derived from `scripts/Makefile.clang`
BINDGEN_TARGET_arm	:= arm-linux-gnueabi
BINDGEN_TARGET_arm64	:= aarch64-linux-gnu
BINDGEN_TARGET_powerpc	:= powerpc64le-linux-gnu
BINDGEN_TARGET_riscv	:= riscv64-linux-gnu
BINDGEN_TARGET_x86	:= x86_64-linux-gnu
BINDGEN_TARGET		:= $(BINDGEN_TARGET_$(SRCARCH))

bindgen_extra_c_flags = --target=$(BINDGEN_TARGET) \
	-Wno-address-of-packed-member \
	-Wno-gnu-variable-sized-type-not-at-end
bindgen_c_flags = $(filter-out $(bindgen_skip_c_flags), $(c_flags)) \
	$(bindgen_extra_c_flags)
endif

ifdef CONFIG_LTO
bindgen_c_flags_lto = $(filter-out $(CC_FLAGS_LTO), $(bindgen_c_flags))
else
bindgen_c_flags_lto = $(bindgen_c_flags)
endif

# To avoid several recompilations in PowerPC, which inserts `-D_TASK_CPU`
bindgen_c_flags_final = $(filter-out -D_TASK_CPU=%, $(bindgen_c_flags_lto))

quiet_cmd_bindgen = BINDGEN $@
      cmd_bindgen = \
	$(BINDGEN) $< $(shell grep -v '^\#\|^$$' $(srctree)/rust/bindgen_parameters) \
		--use-core --with-derive-default --ctypes-prefix c_types \
		--no-debug '.*' \
		--size_t-is-usize -o $@ -- $(bindgen_c_flags_final) -DMODULE

$(objtree)/rust/bindings_generated.rs: $(srctree)/rust/kernel/bindings_helper.h \
	$(srctree)/rust/bindgen_parameters FORCE
	$(call if_changed_dep,bindgen)

quiet_cmd_bindgen_helper = BINDGEN $@
      cmd_bindgen_helper = \
	$(BINDGEN) $< --blacklist-type '.*' --whitelist-var '' \
		--whitelist-function 'rust_helper_.*' \
		--use-core --with-derive-default --ctypes-prefix c_types \
		--no-debug '.*' \
		--size_t-is-usize -o $@ -- $(bindgen_c_flags_final) \
		-I$(objtree)/rust/ -DMODULE; \
	sed -Ei 's/pub fn rust_helper_([a-zA-Z0-9_]*)/\#[link_name="rust_helper_\1"]\n    pub fn \1/g' $@

$(objtree)/rust/bindings_helpers_generated.rs: $(srctree)/rust/helpers.c FORCE
	$(call if_changed_dep,bindgen_helper)

quiet_cmd_exports = EXPORTS $@
      cmd_exports = \
	$(NM) -p --defined-only $< \
		| grep -E ' (T|R|D) ' | cut -d ' ' -f 3 \
		| xargs -Isymbol \
		echo 'EXPORT_SYMBOL_RUST_GPL(symbol);' > $@

$(objtree)/rust/exports_core_generated.h: $(objtree)/rust/core.o FORCE
	$(call if_changed,exports)

$(objtree)/rust/exports_alloc_generated.h: $(objtree)/rust/alloc.o FORCE
	$(call if_changed,exports)

$(objtree)/rust/exports_kernel_generated.h: $(objtree)/rust/kernel.o FORCE
	$(call if_changed,exports)

# `-Cpanic=unwind -Cforce-unwind-tables=y` overrides `rust_flags` in order to
# avoid the https://github.com/rust-lang/rust/issues/82320 rustc crash.
quiet_cmd_rustc_procmacro = $(RUSTC_OR_CLIPPY_QUIET) P $@
      cmd_rustc_procmacro = \
	$(RUSTC_OR_CLIPPY) $(rust_flags) \
		--emit=dep-info,link --extern proc_macro \
		-Cpanic=unwind -Cforce-unwind-tables=y \
		--crate-type proc-macro --out-dir $(objtree)/rust/ \
		--crate-name $(patsubst lib%.so,%,$(notdir $@)) $<; \
	mv $(objtree)/rust/$(patsubst lib%.so,%,$(notdir $@)).d $(depfile); \
	sed -i '/^\#/d' $(depfile)

# Procedural macros can only be used with the `rustc` that compiled it.
# Therefore, to get `libmacros.so` automatically recompiled when the compiler
# version changes, we add `core.o` as a dependency (even if it is not needed).
$(objtree)/rust/libmacros.so: $(srctree)/rust/macros/lib.rs \
	$(objtree)/rust/core.o FORCE
	$(call if_changed_dep,rustc_procmacro)

quiet_cmd_rustc_library = $(if $(skip_clippy),RUSTC,$(RUSTC_OR_CLIPPY_QUIET)) L $@
      cmd_rustc_library = \
	OBJTREE=$(abspath $(objtree)) \
	$(if $(skip_clippy),$(RUSTC),$(RUSTC_OR_CLIPPY)) \
		$(filter-out $(skip_flags),$(rust_flags) $(rust_cross_flags) $(rustc_target_flags)) \
		--crate-type rlib --out-dir $(objtree)/rust/ -L $(objtree)/rust/ \
		--crate-name $(patsubst %.o,%,$(notdir $@)) $<; \
	mv $(objtree)/rust/$(patsubst %.o,%,$(notdir $@)).d $(depfile); \
	sed -i '/^\#/d' $(depfile) \
	$(if $(rustc_objcopy),;$(OBJCOPY) $(rustc_objcopy) $@)

# `$(rust_flags)` is passed in case the user added `--sysroot`.
rustc_sysroot = $(shell $(RUSTC) $(rust_flags) --print sysroot)
rustc_host_target = $(shell $(RUSTC) --version --verbose | grep -F 'host: ' | cut -d' ' -f2)
RUST_LIB_SRC ?= $(rustc_sysroot)/lib/rustlib/src/rust/library

rust-analyzer:
	$(Q)$(srctree)/scripts/generate_rust_analyzer.py $(srctree) $(objtree) $(RUST_LIB_SRC) > $(objtree)/rust-project.json

$(objtree)/rust/compiler_builtins.o: private rustc_objcopy = -w -W '__*'
$(objtree)/rust/compiler_builtins.o: $(srctree)/rust/compiler_builtins.rs \
    $(objtree)/rust/core.o FORCE
	$(call if_changed_dep,rustc_library)

$(objtree)/rust/alloc.o: private skip_clippy = 1
$(objtree)/rust/alloc.o: private skip_flags = -Dunreachable_pub
$(objtree)/rust/alloc.o: private rustc_target_flags = $(alloc-cfgs)
$(objtree)/rust/alloc.o: $(srctree)/rust/alloc/lib.rs \
    $(objtree)/rust/compiler_builtins.o FORCE
	$(call if_changed_dep,rustc_library)

$(objtree)/rust/build_error.o: $(srctree)/rust/build_error.rs \
    $(objtree)/rust/compiler_builtins.o FORCE
	$(call if_changed_dep,rustc_library)

# ICE on `--extern macros`: https://github.com/rust-lang/rust/issues/56935
$(objtree)/rust/kernel.o: private rustc_target_flags = --extern alloc \
    --extern build_error \
    --extern macros=$(objtree)/rust/libmacros.so
$(objtree)/rust/kernel.o: $(srctree)/rust/kernel/lib.rs $(objtree)/rust/alloc.o \
    $(objtree)/rust/build_error.o \
    $(objtree)/rust/libmacros.so $(objtree)/rust/bindings_generated.rs \
    $(objtree)/rust/bindings_helpers_generated.rs FORCE
	$(call if_changed_dep,rustc_library)

# Targets that need to expand twice
.SECONDEXPANSION:
$(objtree)/rust/core.o: private skip_clippy = 1
$(objtree)/rust/core.o: private skip_flags = -Dunreachable_pub
$(objtree)/rust/core.o: private rustc_target_flags = $(core-cfgs)
$(objtree)/rust/core.o: $$(RUST_LIB_SRC)/core/src/lib.rs FORCE
	$(call if_changed_dep,rustc_library)

rustdoc-core: private rustc_target_flags = $(core-cfgs)
rustdoc-core: $$(RUST_LIB_SRC)/core/src/lib.rs FORCE
	$(call if_changed,rustdoc)
