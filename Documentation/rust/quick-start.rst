.. _rust_quick_start:

Quick Start
===========

This document describes how to get started with kernel development in Rust.
If you have worked previously with Rust, this will only take a moment.

Please note that, at the moment, a very restricted subset of architectures
is supported, see :doc:`/rust/arch-support`.


Requirements: Building
----------------------

This section explains how to fetch the tools needed for building.

Some of these requirements might be available from your Linux distribution
under names like ``rustc``, ``rust-src``, ``rust-bindgen``, etc. However,
at the time of writing, they are likely to not be recent enough.


rustc
*****

A recent *nightly* Rust toolchain (with, at least, ``rustc``) is required,
e.g. ``nightly-2021-02-20``. Our goal is to use a stable toolchain as soon
as possible, but for the moment we depend on a handful of nightly features.

If you are using ``rustup``, run::

    rustup default nightly-2021-02-20

Please avoid the very latest nightlies (>= nightly-2021-03-05) until
https://github.com/Rust-for-Linux/linux/issues/135 is resolved.

Otherwise, fetch a standalone installer or install ``rustup`` from:

    https://www.rust-lang.org


Rust standard library source
****************************

The Rust standard library source is required because the build system will
cross-compile ``core`` and ``alloc``.

If you are using ``rustup``, run::

    rustup component add rust-src

Otherwise, if you used a standalone installer, you can clone the Rust
repository into the installation folder of your nightly toolchain::

    git clone --recurse-submodules https://github.com/rust-lang/rust $(rustc --print sysroot)/lib/rustlib/src/rust


libclang
********

``libclang`` (part of LLVM) is used by ``bindgen`` to understand the C code
in the kernel, which means you will need a recent LLVM installed; like when
you compile the kernel with ``CC=clang`` or ``LLVM=1``.

Your Linux distribution is likely to have a suitable one available, so it is
best if you check that first.

There are also some binaries for several systems and architectures uploaded at:

    https://releases.llvm.org/download.html

For Debian-based distributions, you can also fetch them from:

    https://apt.llvm.org

Otherwise, building LLVM takes quite a while, but it is not a complex process:

    https://llvm.org/docs/GettingStarted.html#getting-the-source-code-and-building-llvm


bindgen
*******

The bindings to the C side of the kernel are generated at build time using
the ``bindgen`` tool. A recent version should work, e.g. ``0.56.0``.

Install it via (this will build the tool from source)::

    cargo install --locked --version 0.56.0 bindgen


Requirements: Developing
------------------------

This section explains how to fetch the tools needed for developing. That is,
if you only want to build the kernel, you do not need them.


rustfmt
*******

The ``rustfmt`` tool is used to automatically format all the Rust kernel code,
including the generated C bindings (for details, please see
:ref:`Documentation/rust/coding.rst <rust_coding>`).

If you are using ``rustup``, its ``default`` profile already installs the tool,
so you should be good to go. If you are using another profile, you can install
the component manually::

    rustup component add rustfmt

The standalone installers also come with ``rustfmt``.


clippy
******

``clippy`` is a Rust linter. Installing it allows you to get extra warnings
for Rust code passing ``CLIPPY=1`` to ``make`` (for details, please see
:ref:`Documentation/rust/coding.rst <rust_coding>`).

If you are using ``rustup``, its ``default`` profile already installs the tool,
so you should be good to go. If you are using another profile, you can install
the component manually::

    rustup component add clippy

The standalone installers also come with ``clippy``.


rustdoc
*******

If you install the ``rustdoc`` tool, then you will be able to generate pretty
HTML documentation for Rust code, including for the libraries (crates) inside
``rust/`` that are used by the rest of the kernel (for details, please see
:ref:`Documentation/rust/docs.rst <rust_docs>`).

If you are using ``rustup``, its ``default`` profile already installs the tool,
so you should be good to go. If you are using another profile, you can install
the component manually::

    rustup component add rustdoc

The standalone installers also come with ``rustdoc``.


Configuration
-------------

``Rust support`` (``CONFIG_RUST``) needs to be enabled in the ``General setup``
menu. The option is only shown if the build system can locate ``rustc``.
In turn, this will make visible the rest of options that depend on Rust.

Afterwards, go to::

    Kernel hacking
      -> Sample kernel code
           -> Rust samples

And enable some sample modules either as built-in or as loadable.


Building
--------

Building a kernel with Clang or a complete LLVM toolchain is the best supported
setup at the moment. That is::

    make ARCH=... CROSS_COMPILE=... CC=clang -j...

or::

    make ARCH=... CROSS_COMPILE=... LLVM=1 -j...

Using GCC also works for some configurations, but it is *very* experimental at
the moment.


Hacking
-------

If you want to dive deeper, take a look at the source code of the samples
at ``samples/rust/``, the Rust support code under ``rust/`` and
the ``Rust hacking`` menu under ``Kernel hacking``.

If you use GDB/Binutils and Rust symbols aren't getting demangled, the reason
is your toolchain doesn't support Rust's new v0 mangling scheme yet. There are
a few ways out:

- If you don't mind building your own tools, we provide the following fork
  with the support cherry-picked from GCC on top of very recent releases:

      https://github.com/Rust-for-Linux/binutils-gdb/releases/tag/gdb-10.1-release-rust
        https://github.com/Rust-for-Linux/binutils-gdb/releases/tag/binutils-2_35_1-rust

- If you only need GDB and can enable ``CONFIG_DEBUG_INFO``, do so:
  some versions of GDB (e.g. vanilla GDB 10.1) are able to use
  the pre-demangled names embedded in the debug info.

- If you don't need loadable module support, you may compile without
  the ``-Zsymbol-mangling-version=v0`` flag. However, we don't maintain
  support for that, so avoid it unless you are in a hurry.
