/* c89lib stdio.c：标准 I/O —— 控制台（putchar/puts/getchar）与 FILE 流文件读写 */
#include <stddef.h>
#include <errno.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <math.h>

/* 系统调用包装：crt0 提供 */
extern int sys_open(const char *path, int flags);
extern int sys_close(int fd);
extern int sys_read(int fd, void *buf, int count);
extern int sys_write(int fd, const void *buf, int count);
extern int sys_lseek(int fd, int offset, int whence);
extern int sys_remove(const char *path);
extern int sys_rename(const char *oldpath, const char *newpath);

/* stdin 伪流的回推缓冲区（ungetc 与 fscanf 失败回退共用；
   stdin 无 fd 可 lseek，只能把未读字符压回此栈） */
static int c89_stdin_pushback[256];
static int c89_stdin_push_count;

int putchar(int c) {
    return fputc(c, stdout);
}

int puts(const char *s) {
    while (*s != 0) {
        if (fputc(*s, stdout) == EOF) {
            return EOF;
        }
        s += 1;
    }
    return (fputc('\n', stdout) == EOF) ? EOF : 0;
}

int getchar(void) {
    return fgetc(stdin);
}

int getc(FILE *stream) {
    return fgetc(stream);
}

int putc(int c, FILE *stream) {
    return fputc(c, stream);
}

/* 控制台流（stdin/stdout/stderr 为伪 FILE 指针，不经过 FILE 结构） */
static int c89_stream_fd(FILE *stream) {
    if (stream == stdin) {
        return 0;
    }
    if (stream == stdout) {
        return 1;
    }
    if (stream == stderr) {
        return 2;
    }
    if (stream == 0) {
        return -1;
    }
    return stream->fd;
}

int fgetc(FILE *stream) {
    unsigned char c;
    int n;
    if (stream == stdin) {
        if (c89_stdin_push_count > 0) {
            c89_stdin_push_count -= 1;
            return c89_stdin_pushback[c89_stdin_push_count];
        }
        n = sys_read(0, &c, 1);
        return (n == 1) ? (c & 0xFF) : EOF;
    }
    if (stream == stdout || stream == stderr || stream == 0) {
        return EOF;
    }
    /* ungetc 回推的字符优先返回（按 has_pushback 标志判断，回推 '\0' 也有效） */
    if (stream->has_pushback) {
        int pushed = stream->pushback;
        stream->has_pushback = 0;
        return pushed;
    }
    n = sys_read(stream->fd, &c, 1);
    if (n == 1) {
        return c;
    }
    if (n == 0) {
        stream->eof = 1;
    } else {
        stream->error = 1;
    }
    return EOF;
}

int fputc(int c, FILE *stream) {
    int fd = c89_stream_fd(stream);
    char ch;
    if (fd < 0) {
        return EOF;
    }
    ch = (char)c;
    if (sys_write(fd, &ch, 1) == 1) {
        return (unsigned char)c; /* C89 4.9.7.3：返回写入字符按 unsigned char 转换 */
    }
    return EOF;
}

int fputs(const char *s, FILE *stream) {
    while (*s != 0) {
        if (fputc(*s, stream) == EOF) {
            return EOF;
        }
        s += 1;
    }
    return 0;
}

void perror(const char *s) {
    if (s != 0 && *s != '\0') {
        fputs(s, stderr);
        fputs(": ", stderr);
    }
    fputs(strerror(errno), stderr);
    fputc('\n', stderr);
}

char *fgets(char *s, int n, FILE *stream) {
    int i;
    if (n <= 0) {
        return 0;
    }
    i = 0;
    while (i < n - 1) {
        int c = fgetc(stream);
        if (c == EOF) {
            if (i == 0) {
                return 0;
            }
            break;
        }
        s[i] = (char)c;
        i += 1;
        if (c == '\n') {
            break;
        }
    }
    s[i] = 0;
    return s;
}

size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream) {
    unsigned char *p;
    size_t total;
    size_t k;
    if (size == 0) {
        return 0;
    }
    p = (unsigned char *)ptr;
    total = size * nmemb;
    k = 0;
    while (k < total) {
        int c = fgetc(stream);
        if (c == EOF) {
            break;
        }
        p[k] = (unsigned char)c;
        k += 1;
    }
    return k / size;
}

size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream) {
    const unsigned char *p;
    size_t total;
    size_t k;
    if (size == 0) {
        return 0;
    }
    p = (const unsigned char *)ptr;
    total = size * nmemb;
    k = 0;
    while (k < total) {
        if (fputc(p[k], stream) == EOF) {
            break;
        }
        k += 1;
    }
    return k / size;
}

int fflush(FILE *stream) {
    /* 无缓冲实现：直接写透，无需冲刷 */
    return 0;
}

int fseek(FILE *stream, long offset, int whence) {
    int fd;
    int r;
    /* 三个伪标准流是哨兵指针：与 ftell 一致，直接失败，避免 stream->error 写坏内存 */
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return -1;
    }
    fd = c89_stream_fd(stream);
    if (fd < 0) {
        return -1;
    }
    r = sys_lseek(fd, (int)offset, whence);
    if (r == -1) {
        stream->error = 1;
        return -1;
    }
    stream->eof = 0;
    return 0;
}

long ftell(FILE *stream) {
    int fd = c89_stream_fd(stream);
    if (fd < 0) {
        return -1;
    }
    return (long)sys_lseek(fd, 0, 1);
}

int feof(FILE *stream) {
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return 0;
    }
    return stream->eof;
}

int ferror(FILE *stream) {
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return 0;
    }
    return stream->error;
}

int fclose(FILE *stream) {
    int r;
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return 0;
    }
    if (stream->is_tmp) {
        remove(stream->path);
    }
    r = sys_close(stream->fd);
    free(stream);
    return (r == 0) ? 0 : EOF;
}

int remove(const char *path) {
    return sys_remove(path);
}

static int c89_fopen_flags(const char *mode) {
    int flags;
    int plus;
    int binary;
    const char *p;
    if (mode == 0) {
        return -1;
    }
    if (mode[0] == 'r') {
        flags = 1;
    } else if (mode[0] == 'w') {
        flags = 2 | 4 | 8;
    } else if (mode[0] == 'a') {
        flags = 2 | 4 | 16;
    } else {
        return -1;
    }
    plus = 0;
    binary = 0;
    p = mode + 1;
    while (*p != 0) {
        if (*p == '+' && !plus) {
            plus = 1;
            flags |= 1 | 2;
        } else if (*p == 'b' && !binary) {
            binary = 1;
        } else {
            return -1;
        }
        p += 1;
    }
    return flags;
}

FILE *fopen(const char *path, const char *mode) {
    int flags;
    int fd;
    FILE *f;
    flags = c89_fopen_flags(mode);
    if (flags < 0) {
        return 0;
    }
    fd = sys_open(path, flags);
    if (fd < 0) {
        return 0;
    }
    f = (FILE *)malloc(sizeof(FILE));
    if (f == 0) {
        sys_close(fd);
        return 0;
    }
    f->fd = fd;
    f->error = 0;
    f->eof = 0;
    f->pushback = 0;
    f->has_pushback = 0;
    f->is_tmp = 0;
    f->path[0] = 0;
    return f;
}

struct c89_format_target {
    char *buffer;
    FILE *stream;
    int count;
    int failed;
};

static int c89_emit_char(struct c89_format_target *target, int c) {
    if (target->failed) {
        return EOF;
    }
    if (target->buffer != 0) {
        target->buffer[target->count] = (char)c;
    } else if (fputc(c, target->stream) == EOF) {
        target->failed = 1;
        return EOF;
    }
    target->count += 1;
    return c;
}

/* 无符号整数 → 数字串（base 2..16，upper 大写），min 为最小位数 */
static int c89_fmt_uint(char *out, unsigned int u, int base, int upper, int min) {
    char tmp[70];
    char *digits;
    int i;
    int n;
    digits = "0123456789abcdef";
    if (upper) {
        digits = "0123456789ABCDEF";
    }
    i = 0;
    do {
        tmp[i] = digits[u % base];
        u = u / base;
        i += 1;
    } while (u != 0);
    while (i < min) {
        tmp[i] = '0';
        i += 1;
    }
    for (n = 0; n < i; n += 1) {
        out[n] = tmp[i - 1 - n];
    }
    return i;
}

struct c89_fmt_spec {
    int minus;
    int plus;
    int space;
    int zero;
    int hash;
    int width;
    int width_star;
    int precision;
    int precision_star;
    int conv;
};

/* 解析 % 之后的转换说明（不含 % 本身），pp 前进到转换符之后。
 * 星号宽度/精度只打标记，由调用方按参数顺序读取，保证 va_arg 顺序正确。 */
static void c89_parse_spec(const char **pp, struct c89_fmt_spec *spec) {
    const char *p;
    p = *pp;
    spec->minus = 0;
    spec->plus = 0;
    spec->space = 0;
    spec->zero = 0;
    spec->hash = 0;
    spec->width = 0;
    spec->width_star = 0;
    spec->precision = -1;
    spec->precision_star = 0;
    spec->conv = 0;
    for (;;) {
        if (*p == '-') {
            spec->minus = 1;
            p += 1;
        } else if (*p == '+') {
            spec->plus = 1;
            p += 1;
        } else if (*p == ' ') {
            spec->space = 1;
            p += 1;
        } else if (*p == '0') {
            spec->zero = 1;
            p += 1;
        } else if (*p == '#') {
            spec->hash = 1;
            p += 1;
        } else {
            break;
        }
    }
    if (*p == '*') {
        spec->width_star = 1;
        p += 1;
    } else {
        while (*p >= '0' && *p <= '9') {
            spec->width = spec->width * 10 + (*p - '0');
            p += 1;
        }
    }
    if (*p == '.') {
        p += 1;
        spec->precision = 0;
        if (*p == '*') {
            spec->precision_star = 1;
            p += 1;
        } else {
            while (*p >= '0' && *p <= '9') {
                spec->precision = spec->precision * 10 + (*p - '0');
                p += 1;
            }
        }
    }
    if (*p == 'h' || *p == 'l') {
        p += 1;
    }
    if (spec->width > 65535) {
        spec->width = 65535;
    }
    if (spec->precision > 60) {
        spec->precision = 60;
    }
    spec->conv = *p;
    *pp = p + 1;
}

/* 按宽度/标志输出转换结果（zero_pad 表示用 0 而不是空格填充） */
static void c89_emit_padded(struct c89_format_target *target, const char *core, int core_len,
                            char sign, const char *prefix, int prefix_len,
                            const struct c89_fmt_spec *spec, int zero_pad) {
    int total;
    int pad;
    int i;
    total = core_len + prefix_len;
    if (sign != 0) {
        total += 1;
    }
    pad = spec->width - total;
    if (pad < 0) {
        pad = 0;
    }
    if (zero_pad) {
        if (sign != 0) {
            c89_emit_char(target, sign);
        }
        for (i = 0; i < prefix_len; i += 1) {
            c89_emit_char(target, prefix[i]);
        }
        for (i = 0; i < pad; i += 1) {
            c89_emit_char(target, '0');
        }
        for (i = 0; i < core_len; i += 1) {
            c89_emit_char(target, core[i]);
        }
    } else {
        if (!spec->minus) {
            for (i = 0; i < pad; i += 1) {
                c89_emit_char(target, ' ');
            }
        }
        if (sign != 0) {
            c89_emit_char(target, sign);
        }
        for (i = 0; i < prefix_len; i += 1) {
            c89_emit_char(target, prefix[i]);
        }
        for (i = 0; i < core_len; i += 1) {
            c89_emit_char(target, core[i]);
        }
        if (spec->minus) {
            for (i = 0; i < pad; i += 1) {
                c89_emit_char(target, ' ');
            }
        }
    }
}

/* 非负整数 d（可为 >= 2^32 的大数）的十进制数字串：按 1e9 分块递归，
   pad 为最低组的最少位数（其余组固定补足 9 位）。
   仅对 d < 2^52 精确（该区间整数可精确表示，递归中的减法无消减误差）。 */
static int c89_fmt_int_part(char *out, double d, int pad) {
    double chunk = 1000000000.0;
    if (d < chunk) {
        return c89_fmt_uint(out, (unsigned int)d, 10, 0, pad);
    }
    {
        double high = floor(d / chunk);
        double low = d - high * chunk;
        int n;
        if (low < 0.0) {
            high -= 1.0;
            low += chunk;
        }
        if (low >= chunk) {
            high += 1.0;
            low -= chunk;
        }
        n = c89_fmt_int_part(out, high, 0);
        n += c89_fmt_uint(out + n, (unsigned int)low, 10, 0, 9);
        return n;
    }
}

/* 62 位值 (x1<<32 | x0) 除以 d：要求 x1 < d < 2^30（保证商落在 32 位内）。
   以 2 位为基（base 4）从高到低做长除法：每步 cur = r·4 + digit < 4·d < 2^32，
   全程 32 位算术，避免 64 位中间量。 */
static void c89_div62(unsigned long x1, unsigned long x0, unsigned long d,
                      unsigned long *q_out, unsigned long *r_out) {
    unsigned long q = 0;
    unsigned long r = 0;
    int i;
    for (i = 30; i >= 0; i -= 2) {
        unsigned long cur = (r << 2) + ((x1 >> i) & 3UL);
        unsigned long qd = cur / d;
        r = cur - qd * d;
        q = (q << 2) + qd;
    }
    for (i = 30; i >= 0; i -= 2) {
        unsigned long cur = (r << 2) + ((x0 >> i) & 3UL);
        unsigned long qd = cur / d;
        r = cur - qd * d;
        q = (q << 2) + qd;
    }
    *q_out = q;
    *r_out = r;
}

/* d >= 2^52 的整数部分：double 已是整数，但 1e9 分块递归会因
   d - high*1e9 的灾难性消减失真。精确分解 d = (hi·2^32 + lo)·2^e，
   用 32 位大整数按 1e9 长除法提取十进制分块（双精度可精确表示
   hi·2^32+lo，左移为 2 的幂，全程精确）。 */
static int c89_print_big_int(char *out, double d) {
    double m;
    int e;
    unsigned long hi, lo;
    unsigned long words[36];
    int nwords;
    int q32, rbits;
    int i;
    unsigned long chunks[40];
    int nchunks;
    int len;
    m = frexp(d, &e);      /* d = m·2^e, m ∈ [0.5,1) */
    m = ldexp(m, 53);      /* 整数，∈ [2^52, 2^53) */
    e = e - 53;
    hi = (unsigned long)(m / 4294967296.0);
    lo = (unsigned long)fmod(m, 4294967296.0);
    words[0] = lo;
    words[1] = hi;
    nwords = hi != 0 ? 2 : 1;
    /* 左移 e 位：先整字移位，再按位进位 */
    q32 = e / 32;
    rbits = e % 32;
    if (q32 > 0) {
        for (i = nwords - 1; i >= 0; i -= 1) {
            words[i + q32] = words[i];
        }
        for (i = 0; i < q32; i += 1) {
            words[i] = 0;
        }
        nwords += q32;
    }
    if (rbits > 0) {
        unsigned long carry = 0;
        for (i = 0; i < nwords; i += 1) {
            unsigned long cur = words[i];
            words[i] = (cur << rbits) | carry;
            carry = cur >> (32 - rbits);
        }
        if (carry != 0) {
            words[nwords] = carry;
            nwords += 1;
        }
    }
    /* 逐次除以 1e9，余数即低位分块 */
    nchunks = 0;
    while (nwords > 0 && nchunks < 40) {
        unsigned long rem = 0;
        for (i = nwords - 1; i >= 0; i -= 1) {
            unsigned long q, r;
            c89_div62(rem, words[i], 1000000000UL, &q, &r);
            words[i] = q;
            rem = r;
        }
        chunks[nchunks] = rem;
        nchunks += 1;
        while (nwords > 0 && words[nwords - 1] == 0) {
            nwords -= 1;
        }
    }
    /* 输出：最高分块不补零，其余 9 位补零 */
    len = 0;
    if (nchunks == 0) {
        out[len] = '0';
        len += 1;
    } else {
        len += c89_fmt_uint(out, (unsigned int)chunks[nchunks - 1], 10, 0, 1);
        for (i = nchunks - 2; i >= 0; i -= 1) {
            len += c89_fmt_uint(out + len, (unsigned int)chunks[i], 10, 0, 9);
        }
    }
    return len;
}

/* 把 frac（∈ [0,1)，d 的小数部分）精确展开为 precision 位十进制数字，
   返回进位指示（残余 >= 0.5 时返回 1，即四舍五入的进位）。
   逐位 ×10 提取会在 frac·10 恰舍入为整数时吞掉残余（如 0.1：
   1.0000000000000000555…×10 舍入为 1.0，残余 5.55e-17 丢失）。
   精确算法：frac = m·2^e（m 为 53 位整数，e <= -1），
   frac·10^precision = m·5^precision·2^(e+precision) = w / 2^k（k = -(e+precision)）。
   w 用 32 位字大整数精确计算，右移 k 位即向下取整，逐次除 10 取数字；
   残余 >= 0.5 ⟺ w 的第 k-1 位为 1。仅用于 precision 18..60（上限同
   c89_parse_spec 的 60 位精度截断；更低精度保持原逐位提取，输出不变）。 */
static int c89_frac_digits_exact(char *digits, double frac, int precision) {
    double m;
    int e;
    unsigned long words[12];
    int nwords;
    int i;
    int j;
    int k;
    int q32;
    int rbits;
    int roundup;
    m = frexp(frac, &e);   /* frac = m·2^e，m ∈ [0.5,1) */
    m = ldexp(m, 53);      /* 整数，∈ [2^52, 2^53)（frac==0 时为 0） */
    e = e - 53;
    words[0] = (unsigned long)fmod(m, 4294967296.0);
    words[1] = (unsigned long)(m / 4294967296.0);
    nwords = 2;
    /* w = m·5^precision：×5 拆 16 位半字，避免 32 位中间量溢出 */
    for (j = 0; j < precision; j += 1) {
        unsigned long carry = 0;
        for (i = 0; i < nwords; i += 1) {
            unsigned long low = (words[i] & 0xFFFFUL) * 5UL + carry;
            unsigned long high = (words[i] >> 16) * 5UL + (low >> 16);
            words[i] = ((high & 0xFFFFUL) << 16) | (low & 0xFFFFUL);
            carry = high >> 16;
        }
        if (carry != 0) {
            words[nwords] = carry;
            nwords += 1;
        }
    }
    k = 0 - (e + precision); /* 右移位数；负时（e+precision>=0）无需移位 */
    if (k < 0) {
        k = 0;
    }
    roundup = 0;
    if (k > 0) {
        i = (k - 1) / 32;
        if (i < nwords) {
            roundup = (int)((words[i] >> ((k - 1) % 32)) & 1UL);
        }
    }
    q32 = k / 32;
    rbits = k % 32;
    if (q32 >= nwords) {
        /* w 整体移出：数字全 0（残余 < 0.5，roundup 已按位判出） */
        for (j = 0; j < precision; j += 1) {
            digits[j] = 0;
        }
        return roundup;
    }
    for (i = 0; i + q32 < nwords; i += 1) {
        words[i] = words[i + q32];
    }
    nwords -= q32;
    if (rbits > 0) {
        unsigned long carry = 0;
        for (i = nwords - 1; i >= 0; i -= 1) {
            unsigned long cur = words[i];
            words[i] = ((cur >> rbits) | (carry << (32 - rbits))) & 0xFFFFFFFFUL;
            carry = cur & ((1UL << rbits) - 1UL);
        }
    }
    /* 逐次除 10 取低位数字（c89_div62 要求余数 < 除数 10 < 2^30） */
    for (j = precision - 1; j >= 0; j -= 1) {
        unsigned long rem = 0;
        for (i = nwords - 1; i >= 0; i -= 1) {
            unsigned long q, r;
            c89_div62(rem, words[i], 10UL, &q, &r);
            words[i] = q;
            rem = r;
        }
        digits[j] = (char)rem;
        while (nwords > 0 && words[nwords - 1] == 0) {
            nwords -= 1;
        }
    }
    return roundup;
}

/* 非负 d 按 f 风格 → out（precision 位小数，force 强制小数点），返回长度。
   |d| >= 2^32 时整数部分按 1e9 分块输出（(unsigned int)d 的 fcvt 会饱和）。 */
static int c89_fmt_f(char *out, double d, int precision, int force) {
    double ipart;
    double frac;
    char digits[64];
    int i;
    int len;
    int carry;
    int exact;
    ipart = floor(d);
    frac = d - ipart;
    exact = 0;
    carry = 0;
    if (precision >= 18 && precision <= 60) {
        /* 高位精度：逐位 ×10 会丢残余，改用大整数精确展开 */
        exact = 1;
        carry = c89_frac_digits_exact(digits, frac, precision);
    }
    if (!exact) {
        for (i = 0; i < precision; i += 1) {
            frac = frac * 10.0;
            digits[i] = (char)(int)frac;
            frac = frac - (double)(int)frac;
        }
        if (frac >= 0.5) {
            carry = 1;
        }
    }
    if (carry) {
        for (i = precision - 1; i >= 0; i -= 1) {
            digits[i] = (char)(digits[i] + 1);
            if (digits[i] < 10) {
                carry = 0;
                break;
            }
            digits[i] = 0;
        }
    }
    if (carry) {
        ipart += 1.0;
    }
    if (ipart < 4294967296.0) {
        len = c89_fmt_uint(out, (unsigned int)ipart, 10, 0, 1);
    } else if (ipart < 4503599627370496.0) {
        /* [2^32, 2^52)：整数可精确表示，1e9 分块递归精确 */
        len = c89_fmt_int_part(out, ipart, 1);
    } else {
        /* >= 2^52：递归分块有消减误差，走精确大整数路径 */
        len = c89_print_big_int(out, ipart);
    }
    if (precision > 0 || force) {
        out[len] = '.';
        len += 1;
        for (i = 0; i < precision; i += 1) {
            out[len] = (char)('0' + digits[i]);
            len += 1;
        }
    }
    return len;
}

/* 非负 d 按 e 风格 → out，返回长度；e_exp 输出十进制指数 */
static int c89_fmt_e(char *out, double d, int precision, int upper, int force, int *e_exp) {
    int exp;
    unsigned int ip;
    double frac;
    char digits[64];
    int i;
    int len;
    int carry;
    int a;
    int exact;
    exp = 0;
    if (d != 0.0) {
        while (d >= 10.0) {
            d = d / 10.0;
            exp += 1;
        }
        while (d < 1.0) {
            d = d * 10.0;
            exp -= 1;
        }
    }
    *e_exp = exp;
    ip = (unsigned int)d;
    frac = d - (double)ip;
    exact = 0;
    carry = 0;
    if (precision >= 18 && precision <= 60) {
        /* 高位精度：逐位 ×10 会丢残余，改用大整数精确展开 */
        exact = 1;
        carry = c89_frac_digits_exact(digits, frac, precision);
    }
    if (!exact) {
        for (i = 0; i < precision; i += 1) {
            frac = frac * 10.0;
            digits[i] = (char)(int)frac;
            frac = frac - (double)(int)frac;
        }
        if (frac >= 0.5) {
            carry = 1;
        }
    }
    if (carry) {
        for (i = precision - 1; i >= 0; i -= 1) {
            digits[i] = (char)(digits[i] + 1);
            if (digits[i] < 10) {
                carry = 0;
                break;
            }
            digits[i] = 0;
        }
        if (carry) {
            ip += 1;
        }
    }
    if (ip >= 10) {
        ip = 1;
        exp += 1;
        *e_exp = exp;
        for (i = 0; i < precision; i += 1) {
            digits[i] = 0;
        }
    }
    len = 0;
    out[len] = (char)('0' + ip);
    len += 1;
    if (precision > 0 || force) {
        out[len] = '.';
        len += 1;
        for (i = 0; i < precision; i += 1) {
            out[len] = (char)('0' + digits[i]);
            len += 1;
        }
    }
    out[len] = upper ? 'E' : 'e';
    len += 1;
    if (exp < 0) {
        out[len] = '-';
        a = 0 - exp;
    } else {
        out[len] = '+';
        a = exp;
    }
    len += 1;
    len += c89_fmt_uint(out + len, (unsigned int)a, 10, 0, 2);
    return len;
}

/* 非负 d 按 g 风格 → out，返回长度（precision<=0 视为 1，缺省由调用者传 6） */
static int c89_fmt_g(char *out, double d, int precision, int upper, int force) {
    int exp;
    int p;
    int len;
    int fd;
    double t;
    exp = 0;
    if (d != 0.0) {
        t = d;
        while (t >= 10.0) {
            t = t / 10.0;
            exp += 1;
        }
        while (t < 1.0) {
            t = t * 10.0;
            exp -= 1;
        }
    }
    p = precision > 0 ? precision : 1;
    if (exp < -4 || exp >= p) {
        len = c89_fmt_e(out, d, p - 1, upper, force, &exp);
    } else {
        fd = p - 1 - exp;
        if (fd < 0) {
            fd = 0;
        }
        len = c89_fmt_f(out, d, fd, force);
    }
    if (!force) {
        int i = 0;
        int j;
        int tail;
        int k;
        while (i < len && out[i] != 'e' && out[i] != 'E') {
            i += 1;
        }
        if (i < len) {
            /* e 风格：只剥离尾数尾部的 0 */
            tail = len - i;
            j = i - 1;
            while (j > 0 && out[j] == '0') {
                j -= 1;
            }
            if (out[j] == '.') {
                j -= 1;
            }
            for (k = 0; k < tail; k += 1) {
                out[j + 1 + k] = out[i + k];
            }
            len = j + 1 + tail;
        } else {
            /* f 风格：只剥离小数部分的尾零（C89 7.9.6.1）。
               无小数点（精度恰使小数位数为 0）时整数部分尾零必须保留：
               %g 打印 100000.0 应为 "100000" 而非 "1" */
            int dot = -1;
            for (i = 0; i < len; i += 1) {
                if (out[i] == '.') {
                    dot = i;
                    break;
                }
            }
            if (dot >= 0) {
                j = len - 1;
                while (j > dot && out[j] == '0') {
                    j -= 1;
                }
                if (j == dot) {
                    j -= 1; /* 孤立小数点一并删除 */
                }
                len = j + 1;
            }
        }
    }
    return len;
}

/* %s 转换：流式输出（不拷贝进 core，避免长字符串溢出），做精度/宽度填充。
   ap 通过 out_ap 返回推进后的值（va_arg 只能作用于普通标识符，须传值）。 */
static void c89_fmt_s_case(struct c89_format_target *target, struct c89_fmt_spec *spec,
                           va_list ap, va_list *out_ap) {
    char *s;
    int max;
    int len;
    int pad;
    int i;
    s = (char *)va_arg(ap, int);
    max = spec->precision;
    if (max < 0) {
        max = 0x7fffffff;
    }
    len = 0;
    while (len < max && s[len] != 0) {
        len += 1;
    }
    pad = spec->width - len;
    if (pad < 0) {
        pad = 0;
    }
    if (!spec->minus) {
        for (i = 0; i < pad; i += 1) {
            c89_emit_char(target, ' ');
        }
    }
    for (i = 0; i < len; i += 1) {
        c89_emit_char(target, s[i]);
    }
    if (spec->minus) {
        for (i = 0; i < pad; i += 1) {
            c89_emit_char(target, ' ');
        }
    }
    *out_ap = ap;
}

/* 浮点转换（%f/%e/%E/%g/%G）公共：取 double 实参、处理符号前缀、生成 core。
   符号写入 *sign_out（0 = 无符号），返回 core_len；ap 通过 out_ap 返回。 */
static int c89_fmt_float_case(char *core, struct c89_fmt_spec *spec, int conv,
                              char *sign_out, va_list ap, va_list *out_ap) {
    double d;
    int core_len;
    d = va_arg(ap, double);
    if (d != d) {
        /* NaN：格式化循环（c89_fmt_int_part 递归 / %e 归一化）无法处理，
           直接输出，防止死循环与无限递归 */
        core[0] = 'n';
        core[1] = 'a';
        core[2] = 'n';
        *sign_out = 0;
        *out_ap = ap;
        return 3;
    }
    if (d < 0.0) {
        *sign_out = '-';
        d = 0.0 - d;
    } else if (spec->plus) {
        *sign_out = '+';
    } else if (spec->space) {
        *sign_out = ' ';
    } else {
        *sign_out = 0;
    }
    if (d > 1.7976931348623157e308) {
        /* +inf（含 -inf 取负后）：%e/%g 归一化循环在 inf 上死循环 */
        core[0] = 'i';
        core[1] = 'n';
        core[2] = 'f';
        *out_ap = ap;
        return 3;
    }
    if (conv == 'f') {
        core_len = c89_fmt_f(core, d, spec->precision < 0 ? 6 : spec->precision, spec->hash);
    } else if (conv == 'e' || conv == 'E') {
        int e_exp = 0;
        core_len = c89_fmt_e(core, d, spec->precision < 0 ? 6 : spec->precision,
                             conv == 'E', spec->hash, &e_exp);
    } else {
        core_len = c89_fmt_g(core, d, spec->precision < 0 ? 6 : spec->precision,
                             conv == 'G', spec->hash);
    }
    *out_ap = ap;
    return core_len;
}

/* 共享格式化核心：C89 转换说明（标志/宽度/精度/长度修饰符）。
   %s 与浮点转换下沉到 c89_fmt_s_case/c89_fmt_float_case，
   保持本函数体足够小（B 型分支 ±4KB 限制）。 */
static int c89_vformat(struct c89_format_target *target, const char *fmt, va_list ap) {
    const char *p;
    /* 大数 %f 的整数部分最多 309 位 + 小数点 + 60 位精度 */
    char core[384];
    int core_len;
    char sign;
    char prefix[2];
    int prefix_len;
    int numeric;
    int zero_pad;
    int is_int;
    int v;
    unsigned int u;
    p = fmt;
    while (*p != 0) {
        struct c89_fmt_spec spec;
        if (*p != '%') {
            c89_emit_char(target, *p);
            p += 1;
            continue;
        }
        p += 1;
        if (*p == 0) {
            c89_emit_char(target, '%');
            break;
        }
        c89_parse_spec(&p, &spec);
        if (spec.width_star) {
            spec.width = va_arg(ap, int);
            if (spec.width < 0) {
                spec.width = 0 - spec.width;
                spec.minus = 1;
            }
        }
        if (spec.precision_star) {
            spec.precision = va_arg(ap, int);
            if (spec.precision < 0) {
                spec.precision = -1;
            }
        }
        sign = 0;
        prefix_len = 0;
        numeric = 1;
        core_len = 0;
        switch (spec.conv) {
        case 'd':
        case 'i':
            v = va_arg(ap, int);
            if (v < 0) {
                sign = '-';
                u = (unsigned int)(-(v + 1)) + 1;
            } else {
                if (spec.plus) {
                    sign = '+';
                } else if (spec.space) {
                    sign = ' ';
                }
                u = (unsigned int)v;
            }
            if (spec.precision == 0 && u == 0) {
                core_len = 0;
            } else {
                core_len = c89_fmt_uint(core, u, 10, 0,
                                        spec.precision > 0 ? spec.precision : 0);
            }
            break;
        case 'u':
            u = (unsigned int)va_arg(ap, int);
            if (spec.precision == 0 && u == 0) {
                core_len = 0;
            } else {
                core_len = c89_fmt_uint(core, u, 10, 0,
                                        spec.precision > 0 ? spec.precision : 0);
            }
            break;
        case 'o':
            u = (unsigned int)va_arg(ap, int);
            if (spec.precision == 0 && u == 0) {
                core_len = 0;
            } else {
                core_len = c89_fmt_uint(core, u, 8, 0,
                                        spec.precision > 0 ? spec.precision : 0);
            }
            if (spec.hash) {
                if (u == 0) {
                    if (spec.precision <= 0) {
                        core[0] = '0';
                        core_len = 1;
                    }
                } else if (core_len == 0 || core[0] != '0') {
                    for (v = core_len; v > 0; v -= 1) {
                        core[v] = core[v - 1];
                    }
                    core[0] = '0';
                    core_len += 1;
                }
            }
            break;
        case 'x':
        case 'X':
            u = (unsigned int)va_arg(ap, int);
            if (spec.precision == 0 && u == 0) {
                core_len = 0;
            } else {
                core_len = c89_fmt_uint(core, u, 16, spec.conv == 'X',
                                        spec.precision > 0 ? spec.precision : 0);
            }
            if (spec.hash && u != 0) {
                prefix[0] = '0';
                prefix[1] = spec.conv == 'X' ? 'X' : 'x';
                prefix_len = 2;
            }
            break;
        case 'c':
            core[0] = (char)va_arg(ap, int);
            core_len = 1;
            numeric = 0;
            break;
        case 's':
            c89_fmt_s_case(target, &spec, ap, &ap);
            continue;
        case 'p':
            u = (unsigned int)va_arg(ap, int);
            core_len = c89_fmt_uint(core, u, 16, 0, 0);
            prefix[0] = '0';
            prefix[1] = 'x';
            prefix_len = 2;
            break;
        case 'n':
            {
                int *dest;
                dest = (int *)va_arg(ap, int);
                *dest = target->count;
            }
            continue;
        case 'f':
        case 'e':
        case 'E':
        case 'g':
        case 'G':
            core_len = c89_fmt_float_case(core, &spec, spec.conv, &sign, ap, &ap);
            break;
        case '%':
            c89_emit_char(target, '%');
            continue;
        default:
            c89_emit_char(target, '%');
            c89_emit_char(target, spec.conv);
            continue;
        }
        is_int = spec.conv == 'd' || spec.conv == 'i' || spec.conv == 'o' ||
                 spec.conv == 'u' || spec.conv == 'x' || spec.conv == 'X';
        zero_pad = spec.zero && !spec.minus && numeric && (!is_int || spec.precision < 0);
        c89_emit_padded(target, core, core_len, sign, prefix, prefix_len, &spec, zero_pad);
    }
    if (target->buffer != 0) {
        target->buffer[target->count] = 0;
    }
    return target->failed ? EOF : target->count;
}

int vfprintf(FILE *stream, const char *fmt, va_list ap) {
    struct c89_format_target target;
    target.buffer = 0;
    target.stream = stream;
    target.count = 0;
    target.failed = 0;
    return c89_vformat(&target, fmt, ap);
}

int vprintf(const char *fmt, va_list ap) {
    return vfprintf(stdout, fmt, ap);
}

int vsprintf(char *buf, const char *fmt, va_list ap) {
    struct c89_format_target target;
    target.buffer = buf;
    target.stream = 0;
    target.count = 0;
    target.failed = 0;
    return c89_vformat(&target, fmt, ap);
}

int fprintf(FILE *stream, const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vfprintf(stream, fmt, ap);
    va_end(ap);
    return result;
}

int printf(const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vfprintf(stdout, fmt, ap);
    va_end(ap);
    return result;
}

int sprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vsprintf(buf, fmt, ap);
    va_end(ap);
    return result;
}

static int c89_scan_space(int c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

struct c89_scan_source {
    const char *text;
    FILE *stream;
    int pushback[256];
    int push_count;
    int eof_seen;
    int chars_read; /* 本次调用已从输入读取的字符数（%n 用，不含回推再取） */
};

static int c89_scan_get(struct c89_scan_source *source) {
    int c;
    if (source->push_count > 0) {
        source->push_count -= 1;
        return source->pushback[source->push_count];
    }
    if (source->text != 0) {
        if (*source->text == 0) {
            source->eof_seen = 1;
            return EOF;
        }
        c = (unsigned char)*source->text;
        source->text += 1;
        source->chars_read += 1;
        return c;
    }
    c = fgetc(source->stream);
    if (c == EOF) {
        source->eof_seen = 1;
    } else {
        source->chars_read += 1;
    }
    return c;
}

static void c89_scan_putback(struct c89_scan_source *source, int c) {
    if (c != EOF && source->push_count < 256) {
        source->pushback[source->push_count] = c;
        source->push_count += 1;
    }
}

static int c89_scan_peek(struct c89_scan_source *source) {
    int c = c89_scan_get(source);
    c89_scan_putback(source, c);
    return c;
}

static void c89_scan_skip_space(struct c89_scan_source *source) {
    while (c89_scan_space(c89_scan_peek(source))) {
        c89_scan_get(source);
    }
}

static int c89_scan_token(struct c89_scan_source *source, char *token, int limit) {
    int c;
    int length = 0;
    while (length < 255 && (limit == 0 || length < limit)) {
        c = c89_scan_peek(source);
        if (c == EOF || c89_scan_space(c)) {
            break;
        }
        token[length] = (char)c89_scan_get(source);
        length += 1;
    }
    token[length] = 0;
    return length;
}

static void c89_scan_restore(struct c89_scan_source *source, const char *token,
                             int from, int length) {
    int i;
    i = length;
    while (i > from) {
        i -= 1;
        c89_scan_putback(source, (unsigned char)token[i]);
    }
}

/* 整数转换（%d/%i/%u/%o/%x/%X）公共实现：strtol/strtoul 解析 token，
   成功写回目标并返回 1；解析失败恢复整个 token 并返回 0。
   ap 按 va_arg 推进，通过 out_ap 返回（va_arg 只能作用于普通标识符，须传值）。 */
static int c89_scan_integer(struct c89_scan_source *source, char *token,
                            int token_length, int base, int is_unsigned,
                            int long_mode, int suppress, va_list ap, va_list *out_ap) {
    char *end;
    long integer;
    unsigned long unsigned_integer;
    int consumed;
    int pointer;
    if (is_unsigned) {
        unsigned_integer = strtoul(token, &end, base);
        if (end == token) {
            c89_scan_restore(source, token, 0, token_length);
            *out_ap = ap;
            return 0;
        }
        consumed = (int)(end - token);
        c89_scan_restore(source, token, consumed, token_length);
        if (!suppress) {
            pointer = va_arg(ap, int);
            if (long_mode) {
                unsigned long *ulong_dest = (unsigned long *)pointer;
                *ulong_dest = unsigned_integer;
            } else {
                unsigned int *uint_dest = (unsigned int *)pointer;
                *uint_dest = (unsigned int)unsigned_integer;
            }
        }
    } else {
        integer = strtol(token, &end, base);
        if (end == token) {
            c89_scan_restore(source, token, 0, token_length);
            *out_ap = ap;
            return 0;
        }
        consumed = (int)(end - token);
        c89_scan_restore(source, token, consumed, token_length);
        if (!suppress) {
            pointer = va_arg(ap, int);
            if (long_mode) {
                long *long_dest = (long *)pointer;
                *long_dest = integer;
            } else {
                int *int_dest = (int *)pointer;
                *int_dest = (int)integer;
            }
        }
    }
    *out_ap = ap;
    return 1;
}

/* 浮点转换（%f/%e/%g）公共实现：strtod 解析 token，成功写回并返回 1，失败恢复并返回 0。 */
static int c89_scan_float(struct c89_scan_source *source, char *token,
                          int token_length, int long_mode, int suppress,
                          va_list ap, va_list *out_ap) {
    char *end;
    double real;
    int consumed;
    int pointer;
    real = strtod(token, &end);
    if (end == token) {
        c89_scan_restore(source, token, 0, token_length);
        *out_ap = ap;
        return 0;
    }
    consumed = (int)(end - token);
    c89_scan_restore(source, token, consumed, token_length);
    if (!suppress) {
        pointer = va_arg(ap, int);
        if (long_mode) {
            double *double_dest = (double *)pointer;
            *double_dest = real;
        } else {
            float *float_dest = (float *)pointer;
            *float_dest = real;
        }
    }
    *out_ap = ap;
    return 1;
}

/* %[ 扫描集转换：解析格式并读取输入。返回 1=成功匹配、0=首字符不匹配（输入未消费）、
   -1=格式串畸形；成功与畸形时 *pp 均越过 ']'。ap 通过 out_ap 返回推进后的值。
   支持 a-z 范围与 ^ 取反；'-' 在首尾时按字面 '-' 处理。 */
static int c89_scan_scanset(struct c89_scan_source *source, const char **pp,
                            int width, int suppress, va_list ap, va_list *out_ap) {
    const char *p;
    char setmap[256];
    int k;
    int invert;
    int pointer;
    char *text;
    int i;
    p = *pp;
    p += 1; /* 跳过 '[' */
    invert = 0;
    for (k = 0; k < 256; k += 1) {
        setmap[k] = 0;
    }
    if (*p == '^') {
        invert = 1;
        p += 1;
    }
    if (*p == ']') {
        setmap[']'] = 1;
        p += 1;
    }
    while (*p != 0 && *p != ']') {
        if (p[1] == '-' && p[2] != 0 && p[2] != ']') {
            int lo = (unsigned char)*p;
            int hi = (unsigned char)p[2];
            int ch2;
            if (lo > hi) {
                int t = lo;
                lo = hi;
                hi = t;
            }
            for (ch2 = lo; ch2 <= hi; ch2 += 1) {
                setmap[ch2] = 1;
            }
            p += 3;
        } else {
            setmap[(unsigned char)*p] = 1;
            p += 1;
        }
    }
    if (*p != ']') {
        *out_ap = ap;
        return -1;
    }
    *pp = p + 1;
    if (c89_scan_peek(source) == EOF) {
        *out_ap = ap;
        return 0;
    }
    if (!suppress) {
        pointer = va_arg(ap, int);
        text = (char *)pointer;
    } else {
        text = 0;
    }
    i = 0;
    for (;;) {
        int ch = c89_scan_peek(source);
        int in_set;
        if (ch == EOF) {
            break;
        }
        if (width != 0 && i >= width) {
            break;
        }
        in_set = setmap[(unsigned char)ch];
        if (invert) {
            in_set = !in_set;
        }
        if (!in_set) {
            break;
        }
        if (text != 0) {
            *text = (char)c89_scan_get(source);
            text += 1;
        } else {
            c89_scan_get(source);
        }
        i += 1;
    }
    if (i == 0) {
        *out_ap = ap;
        return 0;
    }
    if (text != 0) {
        *text = 0;
    }
    *out_ap = ap;
    return 1;
}

/* 统一扫描核心：字符串和 FILE 输入只通过 source 区分。
   数值与扫描集转换下沉到 c89_scan_integer/c89_scan_float/c89_scan_scanset，
   保持本函数体足够小（B 型分支 ±4KB 限制）。 */
static int c89_vscan(struct c89_scan_source *source, const char *fmt, va_list ap) {
    const char *p;
    char token[256];
    char *text;
    int token_length;
    int matched;
    int long_mode;
    int suppress;
    int base;
    int pointer;
    int width;
    int i;
    int c;

    p = fmt;
    matched = 0;
    while (*p != 0) {
        if (c89_scan_space(*p)) {
            while (c89_scan_space(*p)) {
                p += 1;
            }
            c89_scan_skip_space(source);
            continue;
        }
        if (*p != '%') {
            c = c89_scan_get(source);
            if (c != (unsigned char)*p) {
                c89_scan_putback(source, c);
                break;
            }
            p += 1;
            continue;
        }

        p += 1;
        long_mode = 0;
        suppress = 0;
        width = 0;
        if (*p == '*') {
            /* 赋值抑制：完成转换但不赋给对象、不消耗参数 */
            suppress = 1;
            p += 1;
        }
        while (*p >= '0' && *p <= '9') {
            width = width * 10 + (*p - '0');
            p += 1;
        }
        if (*p == 'l') {
            long_mode = 1;
            p += 1;
        }
        if (*p == '%') {
            c = c89_scan_get(source);
            if (c != '%') {
                c89_scan_putback(source, c);
                break;
            }
            p += 1;
            continue;
        }
        if (*p == 'c') {
            int nchars = width > 0 ? width : 1;
            char *char_dest;
            if (suppress) {
                char_dest = 0;
            } else {
                pointer = va_arg(ap, int);
                char_dest = (char *)pointer;
            }
            for (i = 0; i < nchars; i += 1) {
                c = c89_scan_get(source);
                if (c == EOF) {
                    break;
                }
                if (char_dest != 0) {
                    char_dest[i] = (char)c;
                }
            }
            if (i == nchars && !suppress) {
                matched += 1;
            }
            p += 1;
            continue;
        }
        if (*p == 'n') {
            /* %n：不消耗输入、不增加匹配数；写入本次调用已净读字符数
               （已读字符减去已回推未读字符，C89 4.9.6.2）。%*n 不消耗参数。 */
            if (!suppress) {
                pointer = va_arg(ap, int);
                if (long_mode) {
                    long *long_dest = (long *)pointer;
                    *long_dest = (long)(source->chars_read - source->push_count);
                } else {
                    int *int_dest = (int *)pointer;
                    *int_dest = source->chars_read - source->push_count;
                }
            }
            p += 1;
            continue;
        }
        if (*p == '[') {
            int r;
            r = c89_scan_scanset(source, &p, width, suppress, ap, &ap);
            if (r == 1) {
                if (!suppress) {
                    matched += 1;
                }
            } else {
                break; /* 格式畸形或首字符不匹配 */
            }
            continue;
        }
        c89_scan_skip_space(source);
        if (*p == 'd' || *p == 'i') {
            base = (*p == 'i') ? 0 : 10;
            token_length = c89_scan_token(source, token, width);
            if (!c89_scan_integer(source, token, token_length, base, 0, long_mode,
                                  suppress, ap, &ap)) {
                break;
            }
            if (!suppress) {
                matched += 1;
            }
        } else if (*p == 'u' || *p == 'o' || *p == 'x' || *p == 'X') {
            base = (*p == 'x' || *p == 'X') ? 16 : (*p == 'o' ? 8 : 10);
            token_length = c89_scan_token(source, token, width);
            if (!c89_scan_integer(source, token, token_length, base, 1, long_mode,
                                  suppress, ap, &ap)) {
                break;
            }
            if (!suppress) {
                matched += 1;
            }
        } else if (*p == 'f' || *p == 'e' || *p == 'g') {
            token_length = c89_scan_token(source, token, width);
            if (!c89_scan_float(source, token, token_length, long_mode, suppress, ap, &ap)) {
                break;
            }
            if (!suppress) {
                matched += 1;
            }
        } else if (*p == 's') {
            if (c89_scan_peek(source) == EOF) {
                break;
            }
            if (suppress) {
                text = 0;
            } else {
                pointer = va_arg(ap, int);
                text = (char *)pointer;
            }
            i = 0;
            while (c89_scan_peek(source) != EOF &&
                   !c89_scan_space(c89_scan_peek(source)) &&
                   (width == 0 || i < width)) {
                c = c89_scan_get(source);
                if (text != 0) {
                    *text = (char)c;
                    text += 1;
                }
                i += 1;
            }
            if (text != 0) {
                *text = 0;
            }
            if (!suppress) {
                matched += 1;
            }
        } else {
            break;
        }
        p += 1;
    }
    return matched;
}

/* 字符串输入格式化：支持 %d/%i/%u/%o/%x/%X/%c/%s/%f/%e/%g/%n/%%、域宽与 %[扫描集]。 */
int sscanf(const char *input, const char *fmt, ...) {
    struct c89_scan_source source;
    va_list ap;
    int result;
    source.text = input;
    source.stream = 0;
    source.push_count = 0;
    source.eof_seen = 0;
    source.chars_read = 0;
    va_start(ap, fmt);
    result = c89_vscan(&source, fmt, ap);
    va_end(ap);
    return result;
}

int fscanf(FILE *stream, const char *fmt, ...) {
    struct c89_scan_source source;
    va_list ap;
    int result;
    int input_failure;
    int fd;
    int i;
    source.text = 0;
    source.stream = stream;
    source.push_count = 0;
    source.eof_seen = 0;
    source.chars_read = 0;
    va_start(ap, fmt);
    result = c89_vscan(&source, fmt, ap);
    va_end(ap);
    input_failure = (result == 0 && source.eof_seen && source.push_count == 0);
    if (source.push_count > 0 && stream != stdin && stream != stdout && stream != stderr) {
        fd = c89_stream_fd(stream);
        if (fd >= 0 && sys_lseek(fd, 0 - source.push_count, 1) >= 0) {
            source.push_count = 0;
            stream->eof = 0;
        }
    } else if (source.push_count > 0 && stream == stdin) {
        /* stdin 无 lseek：未读字符逐个 ungetc 回控制台流，保持字符未读 */
        for (i = 0; i < source.push_count; i += 1) {
            ungetc(source.pushback[i], stdin);
        }
    }
    if (input_failure) {
        return EOF;
    }
    return result;
}

int rename(const char *oldpath, const char *newpath) {
    return sys_rename(oldpath, newpath);
}

/* 临时文件计数器：tmpnam/tmpfile 生成不冲突文件名 */
static int c89_tmp_counter;

void clearerr(FILE *stream) {
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return;
    }
    stream->error = 0;
    stream->eof = 0;
}

int ungetc(int c, FILE *stream) {
    if (stream == stdout || stream == stderr) {
        return EOF;
    }
    /* stdin 是 (FILE *)0 哨兵：必须在本分支处理，不能落到下面的 NULL 检查 */
    if (stream == stdin || stream == 0) {
        if (c == EOF) {
            return EOF; /* 流不变 */
        }
        if (c89_stdin_push_count >= 256) {
            return EOF;
        }
        c89_stdin_pushback[c89_stdin_push_count] = (unsigned char)c;
        c89_stdin_push_count += 1;
        return (unsigned char)c;
    }
    if (c == EOF) {
        return EOF; /* 流不变 */
    }
    if (stream->has_pushback) {
        return EOF; /* 只允许一个字符回推 */
    }
    stream->pushback = (unsigned char)c;
    stream->has_pushback = 1;
    stream->eof = 0;
    return (unsigned char)c;
}

int setvbuf(FILE *stream, char *buf, int mode, size_t size) {
    if (stream == 0) {
        return -1;
    }
    if (mode != _IOFBF && mode != _IOLBF && mode != _IONBF) {
        return -1;
    }
    /* 标准流（stdin/stdout/stderr）也允许设置缓冲模式；
       模拟器按字符输出，无实际缓冲，仅记录模式 */
    if (stream == stdin || stream == stdout || stream == stderr) {
        return 0;
    }
    stream->buf_mode = mode;
    return 0;
}

void setbuf(FILE *stream, char *buf) {
    setvbuf(stream, buf, (buf != 0) ? _IOFBF : _IONBF, 0);
}

int fgetpos(FILE *stream, fpos_t *pos) {
    int fd = c89_stream_fd(stream);
    long p;
    if (fd < 0 || pos == 0) {
        return -1;
    }
    p = sys_lseek(fd, 0, 1);
    if (p < 0) {
        return -1;
    }
    *pos = (fpos_t)p;
    return 0;
}

int fsetpos(FILE *stream, const fpos_t *pos) {
    int fd = c89_stream_fd(stream);
    if (fd < 0 || pos == 0) {
        return -1;
    }
    if (sys_lseek(fd, (long)*pos, 0) < 0) {
        return -1;
    }
    stream->eof = 0;
    stream->error = 0;
    return 0;
}

void rewind(FILE *stream) {
    int fd = c89_stream_fd(stream);
    if (fd < 0) {
        return;
    }
    sys_lseek(fd, 0, 0);
    if (stream != stdin && stream != stdout && stream != stderr) {
        stream->eof = 0;
        stream->error = 0;
        stream->pushback = 0;
        stream->has_pushback = 0;
    }
}

/* 生成 "c89tmp<计数器>" 文件名到 target（容量 cap），计数器按十进制追加 */
static void c89_make_tmp_name(char *target, int cap) {
    static const char *prefix = "c89tmp";
    char digits[16];
    int n = 0;
    int value = 0;
    int p;
    int i;
    c89_tmp_counter += 1;
    value = c89_tmp_counter;
    do {
        digits[n] = (char)('0' + (value % 10));
        value = value / 10;
        n += 1;
    } while (value != 0 && n < (int)sizeof(digits));
    p = 0;
    for (i = 0; prefix[i] != 0 && p < cap - 1; i += 1) {
        target[p] = prefix[i];
        p += 1;
    }
    for (i = n - 1; i >= 0 && p < cap - 1; i -= 1) {
        target[p] = digits[i];
        p += 1;
    }
    target[p] = 0;
}

char *tmpnam(char *s) {
    static char buffer[L_tmpnam];
    char *target;
    if (s != 0) {
        target = s;
    } else {
        target = buffer;
    }
    c89_make_tmp_name(target, L_tmpnam);
    return target;
}

FILE *tmpfile(void) {
    char name[L_tmpnam];
    FILE *f;
    c89_make_tmp_name(name, (int)sizeof(name));
    f = fopen(name, "w+b");
    if (f != 0) {
        f->is_tmp = 1;
        strcpy(f->path, name);
    }
    return f;
}

FILE *freopen(const char *path, const char *mode, FILE *stream) {
    int flags;
    int fd;
    if (stream == stdin || stream == stdout || stream == stderr || stream == 0) {
        return 0;
    }
    flags = c89_fopen_flags(mode);
    if (flags < 0) {
        return 0;
    }
    fd = sys_open(path, flags);
    if (fd < 0) {
        return 0;
    }
    sys_close(stream->fd);
    stream->fd = fd;
    stream->error = 0;
    stream->eof = 0;
    stream->pushback = 0;
    stream->has_pushback = 0;
    stream->is_tmp = 0;
    stream->path[0] = 0;
    stream->buf_mode = _IOFBF;
    return stream;
}
