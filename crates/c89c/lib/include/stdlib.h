#ifndef C89_STD_LIB_H
#define C89_STD_LIB_H

#include <stddef.h>

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 32767

typedef struct {
    int quot;
    int rem;
} div_t;

typedef struct {
    long quot;
    long rem;
} ldiv_t;

void abort(void);
int atexit(void (*func)(void));
void exit(int status);
int system(const char *string);
char *getenv(const char *name);
int abs(int n);
long labs(long n);
div_t div(int num, int denom);
ldiv_t ldiv(long num, long denom);
int atoi(const char *s);
long atol(const char *s);
double atof(const char *s);
double strtod(const char *s, char **end);
long strtol(const char *s, char **end, int base);
unsigned long strtoul(const char *s, char **end, int base);
int rand(void);
void srand(unsigned int seed);
void *malloc(size_t size);
void *calloc(size_t count, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *));
void *bsearch(const void *key, const void *base, size_t count, size_t size,
              int (*compare)(const void *, const void *));

#endif /* C89_STD_LIB_H */
