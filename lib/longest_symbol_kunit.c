// SPDX-License-Identifier: GPL-2.0
/*
 * Test the longest symbol length. Execute with:
 *  ./tools/testing/kunit/kunit.py run longest-symbol
 *  --arch=x86_64 --kconfig_add CONFIG_KPROBES=y --kconfig_add CONFIG_MODULES=y
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <kunit/test.h>
#include <linux/stringify.h>
#include <linux/kprobes.h>
#include <linux/kallsyms.h>

#define DI(name) s##name##name
#define DDI(name) DI(n##name##name)
#define DDDI(name) DDI(n##name##name)
#define DDDDI(name) DDDI(n##name##name)
#define DDDDDI(name) DDDDI(n##name##name)

#define PLUS1(name) __PASTE(name, e)

/*Generate a symbol whose name length is 511 */
#define LONGEST_SYM_NAME  DDDDDI(g1h2i3j4k5l6m7n)

/*Generate a symbol whose name length is 512 */
#define LONGEST_SYM_NAME_PLUS1 PLUS1(LONGEST_SYM_NAME)

noinline int LONGEST_SYM_NAME(void)
{
	return 424242;
}

noinline int LONGEST_SYM_NAME_PLUS1(void)
{
	return 434343;
}

_Static_assert(sizeof(__stringify(LONGEST_SYM_NAME)) == KSYM_NAME_LEN,
"Incorrect symbol length found. Expected KSYM_NAME_LEN: "
__stringify(KSYM_NAME) ", but found: "
__stringify(sizeof(LONGEST_SYM_NAME)));

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
		pr_info("%s: kprobe not registered\n", __func__);
		kunit_warn(test, "test_longest_symbol kallsyms: kprobe not registered\n");
		KUNIT_ASSERT_TRUE(test, register_kprobe(&kp) < 0);
		KUNIT_FAIL(test, "test_longest_symbol kallsysms: kprobe not registered\n");
		return;
	}

	kunit_warn(test, "test_longest_symbol kallsyms: kprobe registered\n");
	kallsyms_lookup_name = (unsigned long (*)(const char *name))kp.addr;
	unregister_kprobe(&kp);

	longest_sym =
	    (void *) kallsyms_lookup_name(__stringify(LONGEST_SYM_NAME));
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
		pr_info("%s: kprobe not registered\n", __func__);
		KUNIT_ASSERT_TRUE(test, register_kprobe(&kp) < 0);
		KUNIT_FAIL(test, "test_longest_symbol kallsysms: kprobe not registered\n");
		return;
	}

	kunit_warn(test, "test_longest_symbol_plus1 kallsyms: kprobe registered\n");
	kallsyms_lookup_name = (unsigned long (*)(const char *name))kp.addr;
	unregister_kprobe(&kp);

	longest_sym_plus1 =
	    (void *) kallsyms_lookup_name(__stringify(LONGEST_SYM_NAME_PLUS1));
	KUNIT_EXPECT_EQ(test, NULL, longest_sym_plus1);
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
