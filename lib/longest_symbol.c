#include "longest_symbol.h"
#include <linux/kallsyms.h>

int noinline LONGEST_SYM_NAME(void)
{
	return 424242;
}

int noinline LONGEST_SYM_NAME_PLUS1(void)
{
	return 434343;
}

_Static_assert(sizeof(__stringify(LONGEST_SYM_NAME)) == KSYM_NAME_LEN, \
"Incorrect symbol length found. Expected KSYM_NAME_LEN: " \
__stringify(KSYM_NAME) ", but found: " \
__stringify(sizeof(LONGEST_SYM_NAME)));

