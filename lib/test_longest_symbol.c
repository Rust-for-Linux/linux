// SPDX-License-Identifier: GPL-2.0+

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/stringify.h>
#include <linux/kprobe.h>
#include <linux/kallsyms.h>
#include "../tools/testing/selftests/kselftest/module.h"

#define LONGEST_SYMBOL start_of_the_longest_symbol_possible__123456789_123456789_123456789_123456789_123456789_123__end_of_the_longest_symbol_possible

KSTM_MODULE_GLOBALS();

/*
 * Kernel module for testing the longest kernel symbol
 */

void LONGEST_SYMBOL(void) {}
EXPORT_SYMBOL(LONGEST_SYMBOL);

_Static_assert(KSYM_NAME_LEN == strlen(__stringify(LONGEST_SYMBOL)), \
		"LONGEST_SYMBOL not up to date with KSYM_NAME_LEN");

static int check_longest_symbol_exported(void)
{
	unsigned long (*kallsyms_lookup_name)(const char *name);
	struct kprobe kp = {
		.symbol_name = "kallsyms_lookup_name",
	};

	if (register_kprobe(&kp) < 0) {
		pr_info("test_longest_symbol: kprobe not registered\n");
		return 1;
	}

	kallsyms_lookup_name = (unsigned long (*)(const char *name))kp.addr;
	unregister_kprobe(&kp);

	if ((typeof(&LONGEST_SYMBOL)) \
		kallsyms_lookup_name(__stringify(LONGEST_SYMBOL))) {
		pr_info("test_longest_symbol: symbol found: " \
			 __stringify(LONGEST_SYMBOL) "\n");
		return 0;
	}

	pr_info("test_longest_symbol: longest_symbol not found\n");
	return 1;
}

static void __init selftest(void)
{
	pr_info("test_longest_symbol loaded\n");
#if defined(CONFIG_KPROBES)
	KSTM_CHECK_ZERO(check_longest_symbol_exported());
#else
	pr_info("To check the longest symbol exported it is needed to have " \
	        "defined CONFIG_KPROBES\n");
#endif
}

KSTM_MODULE_LOADERS(test_longest_symbol);
MODULE_LICENSE("GPL");
MODULE_INFO(test, "Y");
