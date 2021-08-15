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

A particular version of the Rust compiler is required. Newer versions may or
may not work because, for the moment, we depend on some unstable Rust features.

If you are using ``rustup``, enter the checked out source code directory
and run::

    rustup override set 1.54.0

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
in the kernel, which means you will need an LLVM installed; like when
you compile the kernel with ``CC=clang`` or ``LLVM=1``.

Your Linux distribution is likely to have a suitable one available, so it is
best if you check that first.

There are also some binaries for several systems and architectures uploaded at:

    https://releases.llvm.org/download.html

Otherwise, building LLVM takes quite a while, but it is not a complex process:

    https://llvm.org/docs/GettingStarted.html#getting-the-source-code-and-building-llvm

See Documentation/kbuild/llvm.rst for more information and further ways
to fetch pre-built releases and distribution packages.


bindgen
*******

The bindings to the C side of the kernel are generated at build time using
the ``bindgen`` tool. The version we currently support is ``0.56.0``.

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


cargo
*****

``cargo`` is the Rust native build system. It is currently required to run
the tests (``rusttest`` target) since we use it to build a custom standard
library that contains the facilities provided by our custom ``alloc``.

If you are using ``rustup``, all the profiles already install the tool,
so you should be good to go. The standalone installers also include ``cargo``.


rustdoc
*******

``rustdoc`` is the documentation tool for Rust. It generates pretty HTML
documentation for Rust code (for details, please see
:ref:`Documentation/rust/docs.rst <rust_docs>`.

``rustdoc`` is also able to test the examples provided in documented Rust code
(called doctests or documentation tests). We use this feature, thus ``rustdoc``
is required to run the tests (``rusttest`` target).

If you are using ``rustup``, all the profiles already install the tool,
so you should be good to go. The standalone installers also include ``rustdoc``.


rust-analyzer
*************

The `rust-analyzer <https://rust-analyzer.github.io/>`_ language server can
be used with many editors to enable syntax highlighting, completion, go to
definition, and other features.

``rust-analyzer`` will need to be
`configured <https://rust-analyzer.github.io/manual.html#non-cargo-based-projects>`_
to work with the kernel by adding a ``rust-project.json`` file in the root folder.
A ``rust-project.json`` can be generated by building the Make target ``rust-analyzer``,
which will create a ``rust-project.json`` in the root of the output directory.


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

Note: Rust options are also dependent on (``CONFIG_MODVERSIONS``) being disabled.

Building
--------

Building a kernel with a complete LLVM toolchain is the best supported setup
at the moment. That is::

    make LLVM=1

For architectures that do not support a full LLVM toolchain, use::

    make CC=clang

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

  - Install a newer release (GDB >= 10.2, Binutils >= 2.36).

  - If you only need GDB and can enable ``CONFIG_DEBUG_INFO``, do so:
    some versions of GDB (e.g. vanilla GDB 10.1) are able to use
    the pre-demangled names embedded in the debug info.

  - If you don't need loadable module support, you may compile without
    the ``-Zsymbol-mangling-version=v0`` flag. However, we don't maintain
    support for that, so avoid it unless you are in a hurry.
