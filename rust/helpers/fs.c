// SPDX-License-Identifier: GPL-2.0

/*
 * Copyright (C) 2024 Google LLC.
 */

#include <linux/fs.h>

struct file *rust_helper_get_file(struct file *f)
{
	return get_file(f);
}

void rust_helper_i_uid_write(struct inode *inode, uid_t uid)
{
	i_uid_write(inode, uid);
}

void rust_helper_i_gid_write(struct inode *inode, gid_t gid)
{
	i_gid_write(inode, gid);
}
