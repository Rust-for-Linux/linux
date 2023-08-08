#ifndef _LONGEST_SYMBOL_H
#define _LONGEST_SYMBOL_H

#define DI(name) s##name##name
#define DDI(name) DI(n##name##name)
#define DDDI(name) DDI(n##name##name)
#define DDDDI(name) DDDI(n##name##name)
#define DDDDDI(name) DDDDI(n##name##name)

#define PLUS1(name) name##e

/*Generate a symbol whose name length is 511 */
#define LONGEST_SYM_NAME  DDDDDI(g1h2i3j4k5l6m7n)

/*Generate a symbol whose name length is 512 */
#define LONGEST_SYM_PLUS1 PLUS1(LONGEST_SYM_NAME)

int noinline LONGEST_SYM_NAME(void);

int noinline LONGEST_SYM_NAME_PLUS1(void);

#endif
