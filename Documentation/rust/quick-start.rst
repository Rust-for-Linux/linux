.. _rust_quick_start:

Quick Start
===========

This document describes how to get started with kernel development in Rust.


Requirements
------------

This section explains how to fetch the requirements to work with Rust.
If you have worked previously with Rust, this will only take a moment.

Some of these requirements might be available from your Linux distribution
under names like ``rustc``, ``rust-src``, ``rust-bindgen``, etc. However,
at the time of writing, they are likely to not be recent enough.


rustc
*****

A recent *nightly* Rust toolchain (with, at least, ``rustc``) is required,
e.g. ``nightly-2021-01-02``. Our goal is to use a stable toolchain as soon
as possible, but for the moment we depend on a handful of nightly features.

If you are using ``rustup``, run::

    rustup toolchain install nightly

Otherwise, fetch a standalone installer or install ``rustup`` from:

    https://www.rust-lang.org


Rust standard library source
****************************

The Rust standard library source (``core`` and ``alloc``, at least) is required
because the build system will cross-compile it.

If you are using ``rustup``, run::

    rustup component add rust-src

Otherwise, if you used a standalone installer, you can clone the Rust
repository into the installation folder of your nightly toolchain::

    git clone https://github.com/rust-lang/rust `rustc --print sysroot`/lib/rustlib/src/rust


compiler-builtins source
************************

The source for ``compiler-builtins`` (a Rust port of LLVM's ``compiler-rt``)
is required.

The build system expects the sources alongside the Rust ones we just installed,
so you can clone it into the installation folder of your nightly toolchain::

    git clone https://github.com/rust-lang/compiler-builtins `rustc --print sysroot`/lib/rustlib/src/compiler-builtins


bindgen
*******

The bindings to the C side of the kernel are generated at build time using
the ``bindgen`` tool. A recent version should work, e.g. ``0.57.0``.

Install it via::

    cargo install --locked --version 0.57.0 bindgen


rustfmt
*******

Optionally, if you install the ``rustfmt`` tool, then the generated C bindings
will be automatically formatted. It is also useful to have the tool to format
your own code, too.

If you are using ``rustup``, its ``default`` profile already installs the tool,
so you should be good to go. If you are using another profile, you can install
the component manually::

    rustup component add rustfmt

The standalone installers also come with ``rustfmt``.


Configuration
-------------

``Rust support`` (``CONFIG_RUST``) needs to be enabled in the ``General setup``
menu. The option is only shown if the build system can locate ``rustc``.
In turn, this will make visible the rest of options that depend on Rust.

Afterwards, go to ``Character devices`` under ``Device Drivers`` and enable
the example Rust driver ``Rust example`` (``CONFIG_RUST_EXAMPLE``).


Building
--------

Building a x86_64 or arm64 kernel with either GCC, Clang or a complete LLVM
toolchain should all work. However, please note that using GCC is more
experimental at the moment.


Hacking
-------

If you want to dive deeper, take a look at the source code of the example
driver at ``drivers/char/rust_example.rs``, the Rust support code under
``rust/`` and the ``Rust hacking`` menu under ``Kernel hacking``.

If you use GDB/Binutils and Rust symbols aren't getting demangled, the reason
is your toolchain doesn't support Rust's new v0 mangling scheme yet. There are
a few ways out:

  - If you don't mind building your own tools, we provide the following fork
    with the support cherry-picked from GCC on top of very recent releases:

        https://github.com/ojeda/binutils-gdb/releases/tag/gdb-10.1-release-rust
        https://github.com/ojeda/binutils-gdb/releases/tag/binutils-2_35_1-rust

  - If you only need GDB and can enable ``CONFIG_DEBUG_INFO``, do so:
    some versions of GDB (e.g. vanilla GDB 10.1) are able to use
    the pre-demangled names embedded in the debug info.

  - If you don't need loadable module support, you may compile without
    the ``-Z symbol-mangling-version=v0`` flag. However, we don't maintain
    support for that, so avoid it unless you are in a hurry.

