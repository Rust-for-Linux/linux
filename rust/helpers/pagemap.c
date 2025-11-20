// SPDX-License-Identifier: GPL-2.0

#include <linux/pagemap.h>

void rust_helper_mapping_set_large_folios(struct address_space *mapping)
{
	mapping_set_large_folios(mapping);
}
