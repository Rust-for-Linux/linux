// SPDX-License-Identifier: GPL-2.0
/*
 * Test the longest symbol length
 * execute with:
 * ./tools/testing/kunit/kunit.py run longest-symbol --arch=x86_64 --kconfig_add CONFIG_LONGEST_SYMBOL=y --kconfig_add CONFIG_KPROBES=y --kconfig_add CONFIG_MODULES=y
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "longest_symbol.h"
#include <kunit/test.h>
#include <linux/stringify.h>
#include <linux/kprobes.h>

static void test_longest_symbol(struct kunit *test)
{
	KUNIT_EXPECT_EQ(test, 424242, LONGEST_SYM_NAME());
};

static void test_longest_symbol_kallsyms(struct kunit *test)
{
	unsigned long (*kallsyms_lookup_name)(const char *name);
	static int (*longest_sym)(void);

	struct kprobe kp = {
		.symbol_name = "kallsyms_lookup_name",
	};

	if (register_kprobe(&kp) < 0) {
		pr_info("test_longest_symbol_kallsyms: kprobe not registered\n");
		kunit_warn(test, "test_longest_symbol kallsyms: kprobe not registered\n");
		KUNIT_ASSERT_TRUE(test, register_kprobe(&kp) < 0);
		KUNIT_FAIL(test, "test_longest_symbol kallsysms: kprobe not registered\n");
		return;
	}

	kunit_warn(test, "test_longest_symbol kallsyms: kprobe registered\n");
	kallsyms_lookup_name = (unsigned long (*)(const char *name))kp.addr;
	unregister_kprobe(&kp);

	longest_sym = \
	    (void*) kallsyms_lookup_name(__stringify(LONGEST_SYM_NAME));
	KUNIT_EXPECT_EQ(test, 424242, longest_sym());
};

static void test_longest_symbol_plus1(struct kunit *test)
{
	KUNIT_EXPECT_EQ(test, 434343, LONGEST_SYM_NAME_PLUS1());
};

static void test_longest_symbol_plus1_kallsyms(struct kunit *test)
{
	unsigned long (*kallsyms_lookup_name)(const char *name);
	static int (*longest_sym_plus1)(void);

	struct kprobe kp = {
		.symbol_name = "kallsyms_lookup_name",
	};

	if (register_kprobe(&kp) < 0) {
		pr_info("test_longest_symbol_plus1_kallsyms: "
				"kprobe not registered\n");
		KUNIT_ASSERT_TRUE(test, register_kprobe(&kp) < 0);
		KUNIT_FAIL(test, "test_longest_symbol kallsysms: kprobe not registered\n");
		return;
	}

	kallsyms_lookup_name = (unsigned long (*)(const char *name))kp.addr;
	unregister_kprobe(&kp);

	longest_sym_plus1 = \
	    (void*) kallsyms_lookup_name(__stringify(LONGEST_SYM_NAME_PLUS1));
	KUNIT_EXPECT_EQ(test, 434343, longest_sym_plus1());
};

static struct kunit_case longest_symbol_test_cases[] = {
	KUNIT_CASE(test_longest_symbol),
	KUNIT_CASE(test_longest_symbol_kallsyms),
	KUNIT_CASE(test_longest_symbol_plus1),
	KUNIT_CASE(test_longest_symbol_plus1_kallsyms),
	{}
};

static struct kunit_suite longest_symbol_test_suite = {
	.name = "longest-symbol",
	.test_cases = longest_symbol_test_cases,
};
kunit_test_suite(longest_symbol_test_suite);

MODULE_LICENSE("GPL");
