// SPDX-License-Identifier: GPL-2.0

#include <linux/dcache.h>

struct dentry *rust_helper_dget(struct dentry *dentry)
{
	return dget(dentry);
}
