/* string：内存与字符串操作 */

#include <stddef.h>

void *memcpy(void *dest, const void *src, size_t n) {
    char *d = (char *)dest;
    const char *s = (const char *)src;
    size_t i;
    for (i = 0; i < n; i++) {
        d[i] = s[i];
    }
    return dest;
}

void *memmove(void *dest, const void *src, size_t n) {
    char *d = (char *)dest;
    const char *s = (const char *)src;
    size_t i;
    if (d < s) {
        for (i = 0; i < n; i++) {
            d[i] = s[i];
        }
    } else if (d > s) {
        for (i = n; i > 0; i--) {
            d[i - 1] = s[i - 1];
        }
    }
    return dest;
}

int memcmp(const void *s1, const void *s2, size_t n) {
    const unsigned char *a = (const unsigned char *)s1;
    const unsigned char *b = (const unsigned char *)s2;
    size_t i;
    for (i = 0; i < n; i++) {
        if (a[i] != b[i]) {
            return a[i] < b[i] ? -1 : 1;
        }
    }
    return 0;
}

void *memset(void *s, int c, size_t n) {
    char *p = (char *)s;
    size_t i;
    for (i = 0; i < n; i++) {
        p[i] = (char)c;
    }
    return s;
}

void *memchr(const void *s, int c, size_t n) {
    const unsigned char *p = (const unsigned char *)s;
    size_t i;
    for (i = 0; i < n; i++) {
        if (p[i] == (unsigned char)c) {
            return (void *)(p + i);
        }
    }
    return 0;
}

char *strcat(char *dest, const char *src) {
    char *d = dest;
    while (*d != '\0') {
        d++;
    }
    while (*src != '\0') {
        *d = *src;
        d++;
        src++;
    }
    *d = '\0';
    return dest;
}

char *strncat(char *dest, const char *src, size_t n) {
    char *d = dest;
    while (*d != '\0') {
        d++;
    }
    while (n > 0 && *src != '\0') {
        *d = *src;
        d++;
        src++;
        n--;
    }
    *d = '\0';
    return dest;
}

char *strchr(const char *s, int c) {
    while (*s != '\0') {
        if (*s == (char)c) {
            return (char *)s;
        }
        s++;
    }
    if (c == 0) {
        return (char *)s;
    }
    return 0;
}

char *strrchr(const char *s, int c) {
    const char *found = 0;
    while (*s != '\0') {
        if (*s == (char)c) {
            found = s;
        }
        s++;
    }
    if (c == 0) {
        return (char *)s;
    }
    return (char *)found;
}

int strcmp(const char *s1, const char *s2) {
    while (*s1 != '\0' && *s1 == *s2) {
        s1++;
        s2++;
    }
    if (*s1 == *s2) {
        return 0;
    }
    if ((unsigned char)*s1 < (unsigned char)*s2) {
        return -1;
    }
    return 1;
}

/* 当前运行时只有 C locale，因此排序规则就是无符号字节序。 */
int strcoll(const char *s1, const char *s2) {
    return strcmp(s1, s2);
}

int strncmp(const char *s1, const char *s2, size_t n) {
    size_t i;
    for (i = 0; i < n; i++) {
        if (s1[i] == '\0' || s1[i] != s2[i]) {
            if ((unsigned char)s1[i] < (unsigned char)s2[i]) {
                return -1;
            }
            if ((unsigned char)s1[i] > (unsigned char)s2[i]) {
                return 1;
            }
            return 0;
        }
    }
    return 0;
}

char *strcpy(char *dest, const char *src) {
    char *d = dest;
    while ((*d = *src) != '\0') {
        d++;
        src++;
    }
    return dest;
}

char *strncpy(char *dest, const char *src, size_t n) {
    size_t i = 0;
    while (i < n && src[i] != '\0') {
        dest[i] = src[i];
        i++;
    }
    while (i < n) {
        dest[i] = '\0';
        i++;
    }
    return dest;
}

size_t strcspn(const char *s, const char *reject) {
    const char *p = s;
    while (*p != '\0') {
        if (strchr(reject, *p) != 0) {
            break;
        }
        p++;
    }
    return (size_t)(p - s);
}

char *strpbrk(const char *s, const char *accept) {
    while (*s != '\0') {
        if (strchr(accept, *s) != 0) {
            return (char *)s;
        }
        s++;
    }
    return 0;
}

size_t strspn(const char *s, const char *accept) {
    const char *p = s;
    while (*p != '\0' && strchr(accept, *p) != 0) {
        p++;
    }
    return (size_t)(p - s);
}

char *strstr(const char *haystack, const char *needle) {
    const char *h;
    const char *n;
    if (*needle == '\0') {
        return (char *)haystack;
    }
    for (; *haystack != '\0'; haystack++) {
        h = haystack;
        n = needle;
        while (*n != '\0' && *h == *n) {
            h++;
            n++;
        }
        if (*n == '\0') {
            return (char *)haystack;
        }
    }
    return 0;
}

size_t strlen(const char *s) {
    const char *p = s;
    while (*p != '\0') {
        p++;
    }
    return (size_t)(p - s);
}

/* C locale 下转换结果等于原字符串；返回完整转换串长度。 */
size_t strxfrm(char *dest, const char *src, size_t n) {
    size_t length = strlen(src);
    size_t i;
    if (n == 0) {
        return length;
    }
    i = 0;
    while (i + 1 < n && src[i] != '\0') {
        dest[i] = src[i];
        i++;
    }
    dest[i] = '\0';
    return length;
}

/* strtok：跨调用的游标保存 */
static char *c89_strtok_next;

char *strtok(char *s, const char *delim) {
    char *start;
    if (s == 0) {
        s = c89_strtok_next;
    }
    if (s == 0) {
        return 0;
    }
    while (*s != '\0' && strchr(delim, *s) != 0) {
        s++;
    }
    if (*s == '\0') {
        c89_strtok_next = 0;
        return 0;
    }
    start = s;
    while (*s != '\0' && strchr(delim, *s) == 0) {
        s++;
    }
    if (*s != '\0') {
        *s = '\0';
        c89_strtok_next = s + 1;
    } else {
        c89_strtok_next = 0;
    }
    return start;
}

/* strerror：静态缓冲区构造 "Unknown error N" */
static char c89_strerror_buf[64];

char *strerror(int errnum) {
    char *p;
    unsigned int v;
    if (errnum == 33) {
        return "Domain error";
    }
    if (errnum == 34) {
        return "Range error";
    }
    p = c89_strerror_buf;
    p[0] = 'U';
    p[1] = 'n';
    p[2] = 'k';
    p[3] = 'n';
    p[4] = 'o';
    p[5] = 'w';
    p[6] = 'n';
    p[7] = ' ';
    p[8] = 'e';
    p[9] = 'r';
    p[10] = 'r';
    p[11] = 'o';
    p[12] = 'r';
    p[13] = ' ';
    p = c89_strerror_buf + 14;
    v = (unsigned int)errnum;
    if (v == 0) {
        *p = '0';
        p++;
    } else {
        {
            char c89_digits[12];
            int k = 0;
            while (v > 0) {
                c89_digits[k] = (char)('0' + (v % 10));
                v = v / 10;
                k++;
            }
            while (k > 0) {
                k--;
                *p = c89_digits[k];
                p++;
            }
        }
    }
    *p = '\0';
    return c89_strerror_buf;
}
