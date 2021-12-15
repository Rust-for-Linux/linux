/* SPDX-License-Identifier: GPL-2.0 */
#ifndef __LINUX_RUST_H
#define __LINUX_RUST_H

#ifdef CONFIG_RUST
char *rust_fmt_argument(char* buf, char* end, void *ptr);
#else
static inline char *rust_fmt_argument(char* buf, char* end, void *ptr)
{
	return NULL;
}
#endif

#endif /* __LINUX_RUST_H */
