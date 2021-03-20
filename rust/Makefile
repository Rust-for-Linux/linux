# SPDX-License-Identifier: GPL-2.0

obj-$(CONFIG_RUST) += helpers.o exports.o
obj-$(CONFIG_RUST) += core.o compiler_builtins.o alloc.o kernel.o
extra-$(CONFIG_RUST) += bindings_generated.rs libmodule.so
extra-$(CONFIG_RUST) += exports_core_generated.h exports_alloc_generated.h
extra-$(CONFIG_RUST) += exports_kernel_generated.h

# `$(rustc_flags)` is passed in case the user added `--sysroot`.
rustc_sysroot = $(shell $(RUSTC) $(rustc_flags) --print sysroot)
rustc_src = $(rustc_sysroot)/lib/rustlib/src/rust

RUSTDOC = rustdoc

quiet_cmd_rustdoc = RUSTDOC $<
      cmd_rustdoc = \
	RUST_BINDINGS_FILE=$(abspath $(objtree)/rust/bindings_generated.rs) \
	$(RUSTDOC) $(filter-out --emit=%, $(rustc_flags)) \
		$(rustdoc_target_flags) -L $(objtree)/rust/ \
		--output $(objtree)/rust/doc --crate-name $(subst rustdoc-,,$@) \
		@$(objtree)/include/generated/rustc_cfg $<

rustdoc: rustdoc-core rustdoc-module rustdoc-compiler_builtins rustdoc-alloc rustdoc-kernel

rustdoc-core: $(rustc_src)/library/core/src/lib.rs FORCE
	$(call if_changed,rustdoc)

rustdoc-module: private rustdoc_target_flags = --crate-type proc-macro \
    --extern proc_macro -Fmissing-docs
rustdoc-module: $(srctree)/rust/module.rs FORCE
	$(call if_changed,rustdoc)

rustdoc-compiler_builtins: private rustdoc_target_flags = -Fmissing-docs
rustdoc-compiler_builtins: $(srctree)/rust/compiler_builtins.rs FORCE
	$(call if_changed,rustdoc)

rustdoc-alloc: $(rustc_src)/library/alloc/src/lib.rs \
    $(objtree)/rust/compiler_builtins.o FORCE
	$(call if_changed,rustdoc)

rustdoc-kernel: private rustdoc_target_flags = --extern alloc \
    --extern module=$(objtree)/rust/libmodule.so -Fmissing-docs
rustdoc-kernel: $(srctree)/rust/kernel/lib.rs rustdoc-module \
    $(objtree)/rust/libmodule.so $(objtree)/rust/bindings_generated.rs FORCE
	$(call if_changed,rustdoc)

ifdef CONFIG_CC_IS_CLANG
bindgen_c_flags = $(c_flags)
else
# bindgen relies on libclang to parse C. Ideally, bindgen would support a GCC
# plugin backend and/or the Clang driver would be perfectly compatible with GCC.
#
# For the moment, here we are tweaking the flags on the fly. Some config
# options may not work (e.g. `GCC_PLUGIN_RANDSTRUCT` if we end up using one
# of those structs). We might want to redo how Clang flags are kept track of
# in the general `Makefile` even for GCC builds, similar to what we did with
# `TENTATIVE_CLANG_FLAGS`.
bindgen_skip_c_flags := -mno-fp-ret-in-387 -mpreferred-stack-boundary=% \
	-mskip-rax-setup -mgeneral-regs-only -msign-return-address=% \
	-mindirect-branch=thunk-extern -mindirect-branch-register -mrecord-mcount \
	-mabi=lp64 -mstack-protector-guard% -fconserve-stack -falign-jumps=% \
	-falign-loops=% -fno-ipa-cp-clone -fno-partial-inlining \
	-fno-reorder-blocks -fno-allow-store-data-races -fasan-shadow-offset=% \
	-Wno-packed-not-aligned -Wno-format-truncation -Wno-format-overflow \
	-Wno-stringop-truncation -Wno-unused-but-set-variable \
	-Wno-stringop-overflow -Wno-restrict -Wno-maybe-uninitialized \
	-Werror=designated-init -Wno-zero-length-bounds \
	--param=% --param asan-%
bindgen_extra_c_flags = $(TENTATIVE_CLANG_FLAGS) -Wno-address-of-packed-member
bindgen_c_flags = $(filter-out $(bindgen_skip_c_flags), $(c_flags)) \
	$(bindgen_extra_c_flags)
endif

bindgen_opaque_types := xregs_state desc_struct arch_lbr_state

quiet_cmd_bindgen = BINDGEN $@
      cmd_bindgen = \
	$(BINDGEN) $< $(addprefix --opaque-type , $(bindgen_opaque_types)) \
		--use-core --with-derive-default --ctypes-prefix c_types \
		--size_t-is-usize -o $@ -- $(bindgen_c_flags) -DMODULE

$(objtree)/rust/bindings_generated.rs: $(srctree)/rust/kernel/bindings_helper.h FORCE
	$(call if_changed_dep,bindgen)

quiet_cmd_exports = EXPORTS $@
      cmd_exports = \
	$(NM) -p --defined-only $< \
		| grep -E ' (T|R) ' | cut -d ' ' -f 3 | grep -E '^(__rust_|_R)' \
		| xargs -n1 -Isymbol \
		echo 'EXPORT_SYMBOL$(exports_target_type)(symbol);' > $@

$(objtree)/rust/exports_core_generated.h: private exports_target_type := _RUST
$(objtree)/rust/exports_core_generated.h: $(objtree)/rust/core.o FORCE
	$(call if_changed,exports)

$(objtree)/rust/exports_alloc_generated.h: private exports_target_type := _RUST
$(objtree)/rust/exports_alloc_generated.h: $(objtree)/rust/alloc.o FORCE
	$(call if_changed,exports)

$(objtree)/rust/exports_kernel_generated.h: private exports_target_type := _RUST_GPL
$(objtree)/rust/exports_kernel_generated.h: $(objtree)/rust/kernel.o FORCE
	$(call if_changed,exports)

# `-Cpanic=unwind -Cforce-unwind-tables=y` overrides `rustc_flags` in order to
# avoid the https://github.com/rust-lang/rust/issues/82320 rustc crash.
quiet_cmd_rustc_procmacro = $(RUSTC_OR_CLIPPY_QUIET) P $@
      cmd_rustc_procmacro = \
	$(RUSTC_OR_CLIPPY) $(rustc_flags) \
		--emit=dep-info,link --extern proc_macro \
		-Cpanic=unwind -Cforce-unwind-tables=y \
		--crate-type proc-macro --out-dir $(objtree)/rust/ \
		--crate-name $(patsubst lib%.so,%,$(notdir $@)) $<; \
	mv $(objtree)/rust/$(patsubst lib%.so,%,$(notdir $@)).d $(depfile); \
	sed -i '/^\#/d' $(depfile)

$(objtree)/rust/libmodule.so: $(srctree)/rust/module.rs FORCE
	$(call if_changed_dep,rustc_procmacro)

quiet_cmd_rustc_library = $(if $(skip_clippy),RUSTC,$(RUSTC_OR_CLIPPY_QUIET)) L $@
      cmd_rustc_library = \
	RUST_BINDINGS_FILE=$(abspath $(objtree)/rust/bindings_generated.rs) \
	$(if $(skip_clippy),$(RUSTC),$(RUSTC_OR_CLIPPY)) \
		$(rustc_flags) $(rustc_cross_flags) $(rustc_target_flags) \
		--crate-type rlib --out-dir $(objtree)/rust/ -L $(objtree)/rust/ \
		--crate-name $(patsubst %.o,%,$(notdir $@)) $<; \
	mv $(objtree)/rust/$(patsubst %.o,%,$(notdir $@)).d $(depfile); \
	sed -i '/^\#/d' $(depfile) \
	$(if $(rustc_objcopy),;$(OBJCOPY) $(rustc_objcopy) $@)

.SECONDEXPANSION:
$(objtree)/rust/core.o: private skip_clippy = 1
$(objtree)/rust/core.o: $$(rustc_src)/library/core/src/lib.rs FORCE
	$(call if_changed_dep,rustc_library)

$(objtree)/rust/compiler_builtins.o: private rustc_objcopy = -w -W '__*'
$(objtree)/rust/compiler_builtins.o: $(srctree)/rust/compiler_builtins.rs \
    $(objtree)/rust/core.o FORCE
	$(call if_changed_dep,rustc_library)

$(objtree)/rust/alloc.o: private skip_clippy = 1
$(objtree)/rust/alloc.o: $$(rustc_src)/library/alloc/src/lib.rs \
    $(objtree)/rust/compiler_builtins.o FORCE
	$(call if_changed_dep,rustc_library)

# ICE on `--extern module`: https://github.com/rust-lang/rust/issues/56935
$(objtree)/rust/kernel.o: private rustc_target_flags = --extern alloc \
    --extern module=$(objtree)/rust/libmodule.so
$(objtree)/rust/kernel.o: $(srctree)/rust/kernel/lib.rs $(objtree)/rust/alloc.o \
    $(objtree)/rust/libmodule.so $(objtree)/rust/bindings_generated.rs FORCE
	$(call if_changed_dep,rustc_library)
