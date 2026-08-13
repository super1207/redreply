/* c89lib stdlib.c：内存分配、数值转换、随机数、排序/查找、进程退出 */
#include <stddef.h>
#include <errno.h>
#include <float.h>
#include <limits.h>
#include <stdlib.h>

/* ---- 堆内存分配（首次适应 + sbrk 扩展）---- */

/* 系统调用包装：crt0 提供 */
extern char *sys_sbrk(int size);
extern void sys_exit(int status);
extern int sys_system(const char *command);

/* 块头：16 字节（8 对齐，保证用户数据 8 字节对齐） */
struct c89_block {
    struct c89_block *next;
    size_t size; /* 用户数据大小（不含头） */
    int free;
    int pad;
};

static struct c89_block *c89_heap_head = 0;

static int c89_blocks_adjacent(struct c89_block *left, struct c89_block *right) {
    char *left_end = (char *)(left + 1) + left->size;
    return left_end == (char *)right;
}

static void c89_merge_next(struct c89_block *block) {
    struct c89_block *next;
    while (block->next != 0 && block->next->free &&
           c89_blocks_adjacent(block, block->next)) {
        next = block->next;
        block->size += sizeof(struct c89_block) + next->size;
        block->next = next->next;
    }
}

static struct c89_block *c89_split_block(struct c89_block *block, size_t size) {
    struct c89_block *split;
    char *payload;
    size_t remaining;
    if (block->size < size) {
        return 0;
    }
    remaining = block->size - size;
    if (remaining < sizeof(struct c89_block) + 8) {
        return 0;
    }
    payload = (char *)(block + 1);
    split = (struct c89_block *)(payload + size);
    split->next = block->next;
    split->size = remaining - sizeof(struct c89_block);
    split->free = 1;
    split->pad = 0;
    block->size = size;
    block->next = split;
    c89_merge_next(split);
    return split;
}

static struct c89_block *c89_heap_extend(size_t size) {
    size_t capacity = (size + 15) & ~(size_t)15;
    char *p = sys_sbrk((int)(capacity + sizeof(struct c89_block)));
    struct c89_block *b;
    if (p == (char *)-1) {
        return 0; /* sbrk 失败（宿主内存耗尽）：按 C89 malloc 约定返回 NULL */
    }
    b = (struct c89_block *)p;
    b->size = capacity;
    b->free = 1;
    b->pad = 0;
    b->next = 0;
    return b;
}

void *malloc(size_t size) {
    struct c89_block *b;
    struct c89_block *nb;
    if (size == 0) {
        size = 1;
    }
    size = (size + 7) & ~(size_t)7;
    if (c89_heap_head == 0) {
        c89_heap_head = c89_heap_extend(size);
        if (c89_heap_head == 0) {
            return 0;
        }
        c89_heap_head->free = 0;
        return (void *)(c89_heap_head + 1);
    }
    /* 首次适应：优先复用空闲块 */
    b = c89_heap_head;
    for (;;) {
        if (b->free && b->size >= size) {
            c89_split_block(b, size);
            b->free = 0;
            return (void *)(b + 1);
        }
        if (b->next == 0) {
            break;
        }
        b = b->next;
    }
    nb = c89_heap_extend(size);
    if (nb == 0) {
        return 0;
    }
    b->next = nb;
    nb->free = 0;
    return (void *)(nb + 1);
}

void free(void *ptr) {
    struct c89_block *b;
    struct c89_block *prev;
    if (ptr == 0) {
        return;
    }
    b = (struct c89_block *)ptr - 1;
    b->free = 1;
    c89_merge_next(b);
    prev = c89_heap_head;
    while (prev != 0 && prev->next != b) {
        prev = prev->next;
    }
    if (prev != 0 && prev->free && c89_blocks_adjacent(prev, b)) {
        c89_merge_next(prev);
    }
}

void *calloc(size_t count, size_t size) {
    size_t total;
    char *p;
    size_t i;
    /* 32 位 size_t：先检查乘法回绕（C89 7.10.3.2） */
    if (count != 0 && size > 0xFFFFFFFFu / count) {
        return 0;
    }
    total = count * size;
    p = (char *)malloc(total);
    if (p == 0) {
        return 0;
    }
    i = 0;
    while (i < total) {
        p[i] = 0;
        i += 1;
    }
    return p;
}

void *realloc(void *ptr, size_t size) {
    struct c89_block *b;
    void *np;
    char *dst;
    char *src;
    size_t i;
    if (ptr == 0) {
        return malloc(size);
    }
    if (size == 0) {
        free(ptr);
        return 0;
    }
    size = (size + 7) & ~(size_t)7;
    b = (struct c89_block *)ptr - 1;
    if (b->size >= size) {
        c89_split_block(b, size);
        return ptr;
    }
    if (b->next != 0 && b->next->free && c89_blocks_adjacent(b, b->next) &&
        b->size + sizeof(struct c89_block) + b->next->size >= size) {
        c89_merge_next(b);
        c89_split_block(b, size);
        b->free = 0;
        return ptr;
    }
    np = malloc(size);
    if (np == 0) {
        return 0;
    }
    dst = (char *)np;
    src = (char *)ptr;
    i = 0;
    while (i < b->size) {
        dst[i] = src[i];
        i += 1;
    }
    free(ptr);
    return np;
}

/* ---- 数值转换 ---- */

static int c89_isdigit_c(int c) {
    return c >= '0' && c <= '9';
}

double strtod(const char *s, char **end) {
    const char *start = s;
    const char *exponent_start;
    double value = 0.0;
    int sign = 1;
    int any = 0;
    int significant = 0;
    int decimal_exponent = 0;
    int exponent = 0;
    int exponent_sign = 1;
    int exponent_any = 0;
    int digit;

    while (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r' || *s == '\f' || *s == '\v') {
        s += 1;
    }
    if (*s == '-') {
        sign = -1;
        s += 1;
    } else if (*s == '+') {
        s += 1;
    }

    while (c89_isdigit_c(*s)) {
        digit = *s - '0';
        any = 1;
        if (significant != 0 || digit != 0) {
            if (significant < 18) {
                value = value * 10.0 + (double)digit;
                significant += 1;
            } else {
                decimal_exponent += 1;
            }
        }
        s += 1;
    }
    if (*s == '.') {
        s += 1;
        while (c89_isdigit_c(*s)) {
            digit = *s - '0';
            any = 1;
            if (significant == 0 && digit == 0) {
                decimal_exponent -= 1;
            } else if (significant < 18) {
                value = value * 10.0 + (double)digit;
                significant += 1;
                decimal_exponent -= 1;
            }
            s += 1;
        }
    }

    if (!any) {
        if (end != 0) {
            *end = (char *)start;
        }
        return 0.0;
    }

    exponent_start = s;
    if (*s == 'e' || *s == 'E') {
        s += 1;
        if (*s == '-') {
            exponent_sign = -1;
            s += 1;
        } else if (*s == '+') {
            s += 1;
        }
        while (c89_isdigit_c(*s)) {
            exponent_any = 1;
            if (exponent < 10000) {
                exponent = exponent * 10 + (*s - '0');
                if (exponent > 10000) {
                    exponent = 10000;
                }
            }
            s += 1;
        }
        if (!exponent_any) {
            s = exponent_start;
        } else {
            decimal_exponent += exponent_sign * exponent;
        }
    }

    if (end != 0) {
        *end = (char *)s;
    }
    if (value == 0.0) {
        /* 0 值 + 极端负指数（如 "0e-999"）：正确值下溢为 0，同样置 ERANGE
           （C89 7.10.1.4）。阈值与下方非 0 值下溢路径一致；"0"/"0.000"
           等普通零表示可表示值，不置。 */
        if (decimal_exponent < -400) {
            errno = ERANGE;
        }
        return sign < 0 ? -value : value;
    }
    if (decimal_exponent > 400) {
        errno = ERANGE;
        return sign < 0 ? -1e308 : 1e308;
    }
    if (decimal_exponent < -400) {
        errno = ERANGE;
        return sign < 0 ? -0.0 : 0.0;
    }
    while (decimal_exponent > 0) {
        if (value > 1e307) {
            errno = ERANGE;
            return sign < 0 ? -1e308 : 1e308;
        }
        value *= 10.0;
        decimal_exponent -= 1;
    }
    while (decimal_exponent < 0) {
        value /= 10.0;
        decimal_exponent += 1;
    }
    if (value == 0.0 || value < DBL_MIN) {
        errno = ERANGE;
    }
    return sign < 0 ? -value : value;
}

double atof(const char *s) {
    return strtod(s, 0);
}

long strtol(const char *s, char **end, int base) {
    const char *start = s;
    int negative = 0;
    unsigned long limit;
    unsigned long val = 0;
    int any = 0;
    while (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r' || *s == '\f' || *s == '\v') {
        s += 1;
    }
    if (*s == '-') {
        negative = 1;
        s += 1;
    } else if (*s == '+') {
        s += 1;
    }
    /* base == 0：按前缀自动识别进制（C89 7.10.1.4） */
    if (base == 0) {
        if (*s == '0' && (s[1] == 'x' || s[1] == 'X')) {
            base = 16;
            s += 2;
        } else if (*s == '0') {
            base = 8;
        } else {
            base = 10;
        }
    } else if (base == 16 && *s == '0' && (s[1] == 'x' || s[1] == 'X')) {
        s += 2;
    }
    /* 无符号累加避免中间回绕：负向 limit 取 2^31（2147483648），正向取 LONG_MAX。
       每次进位前检查 val > (limit - digit) / base，溢出返回 LONG_MAX/LONG_MIN 并置 ERANGE。 */
    limit = negative ? 2147483648UL : (unsigned long)LONG_MAX;
    for (;;) {
        int c = *s;
        int digit;
        if (c >= '0' && c <= '9') {
            digit = c - '0';
        } else if (c >= 'a' && c <= 'z') {
            digit = c - 'a' + 10;
        } else if (c >= 'A' && c <= 'Z') {
            digit = c - 'A' + 10;
        } else {
            break;
        }
        if (digit >= base) {
            break;
        }
        if (val > (limit - (unsigned long)digit) / (unsigned long)base) {
            errno = ERANGE;
            val = limit;
        } else {
            val = val * (unsigned long)base + (unsigned long)digit;
        }
        any = 1;
        s += 1;
    }
    if (end != 0) {
        *end = any ? (char *)s : (char *)start;
    }
    if (!any) {
        return 0;
    }
    if (negative) {
        /* 0 - 2147483648 → 0x80000000，即 LONG_MIN */
        return (long)(0UL - val);
    }
    return (long)val;
}

unsigned long strtoul(const char *s, char **end, int base) {
    const char *start = s;
    int negative = 0;
    unsigned long limit = ULONG_MAX;
    unsigned long val = 0;
    int any = 0;
    while (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r' || *s == '\f' || *s == '\v') {
        s += 1;
    }
    if (*s == '-') {
        negative = 1;
        s += 1;
    } else if (*s == '+') {
        s += 1;
    }
    if (base == 0) {
        if (*s == '0' && (s[1] == 'x' || s[1] == 'X')) {
            base = 16;
            s += 2;
        } else if (*s == '0') {
            base = 8;
        } else {
            base = 10;
        }
    } else if (base == 16 && *s == '0' && (s[1] == 'x' || s[1] == 'X')) {
        s += 2;
    }
    for (;;) {
        int c = *s;
        int digit;
        if (c >= '0' && c <= '9') {
            digit = c - '0';
        } else if (c >= 'a' && c <= 'z') {
            digit = c - 'a' + 10;
        } else if (c >= 'A' && c <= 'Z') {
            digit = c - 'A' + 10;
        } else {
            break;
        }
        if (digit >= base) {
            break;
        }
        if (val > (limit - (unsigned long)digit) / (unsigned long)base) {
            errno = ERANGE;
            val = limit;
        } else {
            val = val * (unsigned long)base + (unsigned long)digit;
        }
        any = 1;
        s += 1;
    }
    if (end != 0) {
        *end = any ? (char *)s : (char *)start;
    }
    if (!any) {
        return 0;
    }
    return negative ? (0UL - val) : val;
}

int atoi(const char *s) {
    return (int)strtol(s, 0, 10);
}

long atol(const char *s) {
    return strtol(s, 0, 10);
}

/* ---- 随机数（线性同余）---- */

static unsigned int c89_seed = 1;

void srand(unsigned int seed) {
    c89_seed = seed;
}

int rand(void) {
    c89_seed = c89_seed * 1103515245u + 12345u;
    return (int)((c89_seed >> 16) & 0x7fffu);
}

/* ---- 绝对值 / 除法结构 ---- */

int abs(int n) {
    return n < 0 ? -n : n;
}

long labs(long n) {
    return n < 0 ? -n : n;
}

div_t div(int num, int denom) {
    div_t r;
    r.quot = num / denom;
    r.rem = num % denom;
    return r;
}

ldiv_t ldiv(long num, long denom) {
    ldiv_t r;
    r.quot = num / denom;
    r.rem = num % denom;
    return r;
}

/* ---- 退出 ---- */

#define C89_ATEXIT_MAX 32

static void (*c89_atexit_handlers[C89_ATEXIT_MAX])(void);
static int c89_atexit_count;

int atexit(void (*func)(void)) {
    if (c89_atexit_count >= C89_ATEXIT_MAX) {
        return -1;
    }
    c89_atexit_handlers[c89_atexit_count] = func;
    c89_atexit_count += 1;
    return 0;
}

void abort(void) {
    sys_exit(134);
    for (;;) {
    }
}

void exit(int status) {
    while (c89_atexit_count > 0) {
        c89_atexit_count -= 1;
        c89_atexit_handlers[c89_atexit_count]();
    }
    sys_exit(status);
    for (;;) {
    }
}

int system(const char *string) {
    if (string == 0) {
        return 1;
    }
    return sys_system(string);
}

/* getenv：读取宿主环境变量。
   先查询长度，短值用静态缓冲；长值动态分配精确大小（Windows 环境变量最长 32767 字符）。 */
extern int sys_getenv(const char *name, char *buf, int capacity);

static char *c89_getenv_big;

char *getenv(const char *name) {
    static char small[256];
    int n;
    if (c89_getenv_big != 0) {
        free(c89_getenv_big);
        c89_getenv_big = 0;
    }
    n = sys_getenv(name, 0, 0);
    if (n < 0) {
        return 0;
    }
    if (n + 1 <= (int)sizeof(small)) {
        if (sys_getenv(name, small, (int)sizeof(small)) != n) {
            return 0;
        }
        small[n] = 0;
        return small;
    }
    c89_getenv_big = (char *)malloc((size_t)n + 1);
    if (c89_getenv_big == 0) {
        return 0;
    }
    if (sys_getenv(name, c89_getenv_big, n + 1) != n) {
        return 0;
    }
    c89_getenv_big[n] = 0;
    return c89_getenv_big;
}

/* ---- 排序与查找 ---- */

static void c89_qsort_rec(char *base, size_t count, size_t size,
                          int (*compare)(const void *, const void *)) {
    size_t i;
    if (count <= 1) {
        return;
    }
    /* 简单选择排序（小规模够用，避免复杂分区） */
    i = 0;
    while (i + 1 < count) {
        size_t best = i;
        size_t j = i + 1;
        while (j < count) {
            if (compare(base + j * size, base + best * size) < 0) {
                best = j;
            }
            j += 1;
        }
        if (best != i) {
            /* 交换 base[i] 与 base[best] */
            size_t k = 0;
            while (k < size) {
                char t = base[i * size + k];
                base[i * size + k] = base[best * size + k];
                base[best * size + k] = t;
                k += 1;
            }
        }
        i += 1;
    }
}

void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *)) {
    c89_qsort_rec((char *)base, count, size, compare);
}

void *bsearch(const void *key, const void *base, size_t count, size_t size,
              int (*compare)(const void *, const void *)) {
    size_t lo = 0;
    size_t hi = count;
    while (lo < hi) {
        size_t mid = lo + (hi - lo) / 2;
        int c = compare((const char *)key, (const char *)base + mid * size);
        if (c == 0) {
            return (void *)((const char *)base + mid * size);
        }
        if (c < 0) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    return 0;
}
