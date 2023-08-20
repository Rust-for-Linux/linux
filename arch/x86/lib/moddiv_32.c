#include <linux/math64.h>

unsigned long long __udivdi3(unsigned long long a, unsigned long long b)
{
	return div64_u64(a, b);
}

unsigned long long __umoddi3(unsigned long long a, unsigned long long b)
{
	unsigned long long rem;
	div64_u64_rem(a, b, &rem);
	return rem;
}
