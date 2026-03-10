// SPDX-License-Identifier: GPL-2.0

#include <linux/kdev_t.h>

unsigned int rust_helper_MKDEV(unsigned int major, unsigned int minor)
{
	return MKDEV(major, minor);
}
