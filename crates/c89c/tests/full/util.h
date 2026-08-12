#ifndef UTIL_H
#define UTIL_H

#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define MAX(a, b) ((a) > (b) ? (a) : (b))
#define STR(x) #x

#if defined(__C89C__) && __C89C__ >= 1
#define C89C_READY 1
#else
#define C89C_READY 0
#endif

#endif /* UTIL_H */
