/* c89lib assert.h：断言宏（默认 C locale / 控制台诊断） */

#ifdef assert
#undef assert
#endif

#ifdef NDEBUG
#define assert(expression) ((void)0)
#else
#include <stdio.h>
#include <stdlib.h>
#define assert(expression) ((expression) ? (void)0 : (printf("Assertion failed: %s, file %s, line %d\n", #expression, __FILE__, __LINE__), abort()))
#endif
