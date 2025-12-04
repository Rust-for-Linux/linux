// SPDX-License-Identifier: GPL-2.0

#include <linux/pagemap.h>

void rust_helper_mapping_set_large_folios(struct address_space *mapping)
{
	mapping_set_large_folios(mapping);
}

struct folio *rust_helper_read_mapping_folio(struct address_space *mapping, pgoff_t index, struct file *file)
{
	return read_mapping_folio(mapping, index, file);
}

struct page *rust_helper_read_mapping_page(struct address_space *mapping, pgoff_t index, struct file *file)
{
	return read_mapping_page(mapping, index, file);
}
