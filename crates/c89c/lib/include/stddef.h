#ifndef C89_STDDEF_H
#define C89_STDDEF_H

#define NULL ((void *)0)

typedef unsigned int size_t;
typedef int ptrdiff_t;

#define offsetof(type, member) ((size_t) & (((type *)0)->member))

#endif
