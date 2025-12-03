#include <linux/highmem.h>

void rust_helper_copy_highpage(struct page *to, struct page *from)
{
	return copy_highpage(to, from);
}
