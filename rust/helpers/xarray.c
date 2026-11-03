// SPDX-License-Identifier: GPL-2.0

#include <linux/xarray.h>

__rust_helper int rust_helper_xa_err(void *entry)
{
	return xa_err(entry);
}

/*
 * Initialize `xa` with `flags` and register its internal spinlock with the
 * caller-supplied lockdep class `key` and name `name`, both of which must
 * live for the lifetime of `xa`.
 *
 * `xa_init_flags()` cannot be used instead: it expands `spin_lock_init()`,
 * which yields one static lockdep class per compilation unit, so every
 * caller reaching it through a Rust helper would share a single class.
 */
__rust_helper void rust_helper_xa_init_flags_with_key(struct xarray *xa,
						      gfp_t flags,
						      const char *name,
						      struct lock_class_key *key)
{
	xa_init_flags(xa, flags);
	lockdep_set_class_and_name(&xa->xa_lock, key, name);
}

__rust_helper int rust_helper_xa_trylock(struct xarray *xa)
{
	return xa_trylock(xa);
}

__rust_helper void rust_helper_xa_lock(struct xarray *xa)
{
	return xa_lock(xa);
}

__rust_helper void rust_helper_xa_unlock(struct xarray *xa)
{
	return xa_unlock(xa);
}
