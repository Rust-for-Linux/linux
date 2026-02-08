// SPDX-License-Identifier: GPL-2.0

#include "linux/pagemap.h"
#include <linux/gfp.h>
#include <linux/highmem.h>
#include <linux/mm.h>
#include <linux/page-flags.h>

__rust_helper struct page *rust_helper_alloc_pages(gfp_t gfp_mask,
						   unsigned int order)
{
	return alloc_pages(gfp_mask, order);
}

__rust_helper void *rust_helper_kmap_local_page(struct page *page)
{
	return kmap_local_page(page);
}

__rust_helper void rust_helper_kunmap_local(const void *addr)
{
	kunmap_local(addr);
}

void *rust_helper_kmap(struct page *page)
{
	return kmap(page);
}

void rust_helper_kunmap(struct page *page)
{
	kunmap(page);
}

struct page *rust_helper_folio_page(struct folio *folio, size_t n)
{
	return folio_page(folio, n);
}

bool rust_helper_folio_test_highmem(struct folio *folio)
{
	return folio_test_highmem(folio);
}

void rust_helper_folio_lock(struct folio *folio)
{
	return folio_lock(folio);
}

void rust_helper_folio_unlock(struct folio *folio)
{
	return folio_unlock(folio);
}

#ifndef NODE_NOT_IN_PAGE_FLAGS
__rust_helper int rust_helper_page_to_nid(const struct page *page)
{
	return page_to_nid(page);
}
#endif
