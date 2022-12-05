.. SPDX-License-Identifier: GPL-2.0

Testing
=======

Like in any other Rust project it is possible to write and run unit tests and
documentation tests in the kernel.

Running Unit Tests
------------------

Unit tests in the kernel are identical to user-space Rust tests:

.. code-block:: rust

	#[cfg(test)]
	mod tests {
	    #[test]
	    fn it_works() {
	        let result = 2 + 2;
	        assert_eq!(result, 4);
	    }
	}

And can be run using the ``rusttest`` Make target:

.. code-block:: bash

	$ make LLVM=1 rusttest

Running Documentation Tests
---------------------------

Like in user-space, it is possible to write documentation tests:

.. code-block:: rust

	/// ```
	/// let result = 2 + 2;
	/// assert_eq!(result, 4);
	/// ```

Documentation tests use KUnit and it is possible to run them either on boot or
using the ``kunit.py`` tool:

.. code-block:: bash

	$ ./tools/testing/kunit/kunit.py run --kunitconfig=rust \
	  --make_options LLVM=1 --arch=x86_64

For general information about KUnit and `kunit.py``, please refer to
Documentation/dev-tools/kunit/start.rst.
