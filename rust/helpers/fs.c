// SPDX-License-Identifier: GPL-2.0

/*
 * Copyright (C) 2024 Google LLC.
 */

#include <linux/fs.h>

__rust_helper struct file *rust_helper_get_file(struct file *f)
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

void rust_helper_inode_lock_shared(struct inode *inode)
{
	inode_lock_shared(inode);
}

void rust_helper_inode_unlock_shared(struct inode *inode)
{
	inode_unlock_shared(inode);
}

void *rust_helper_alloc_inode_sb(struct super_block *sb,
				 struct kmem_cache *cache, gfp_t gfp)
{
	return alloc_inode_sb(sb, cache, gfp);
}

loff_t rust_helper_i_size_read(const struct inode *inode)
{
	return i_size_read(inode);
}

void rust_helper_mark_inode_dirty(struct inode *inode)
{
	mark_inode_dirty(inode);
}

void rust_helper_inode_inc_link_count(struct inode *inode)
{
	inode_inc_link_count(inode);
}

void rust_helper_inode_dec_link_count(struct inode *inode)
{
	inode_dec_link_count(inode);
}
