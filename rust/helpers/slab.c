// SPDX-License-Identifier: GPL-2.0

#include <linux/slab.h>

__rust_helper void *__must_check __realloc_size(2)
rust_helper_krealloc_node_align(const void *objp, size_t new_size, unsigned long align,
				gfp_t flags, int node)
{
	return krealloc_node_align(objp, new_size, align, flags, node);
}

__rust_helper void *__must_check __realloc_size(2)
rust_helper_kvrealloc_node_align(const void *p, size_t size, unsigned long align,
				 gfp_t flags, int node)
{
	return kvrealloc_node_align(p, size, align, flags, node);
}

// TODO: update to new API
struct kmem_cache *rust_helper_kmem_cache_create(
    const char *name,
    unsigned int size,
    unsigned int align,
    slab_flags_t flags,
    void (*ctor)(void *))
{
    return kmem_cache_create(name, size, align, flags, ctor);
}
