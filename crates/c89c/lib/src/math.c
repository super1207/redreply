/* c89lib math.c：浮点数学函数（C89 7.5）
 *
 * 教学实现：全部用 double 四则运算 / 比较 / 类型转换写成纯 C，
 * 不依赖任何特殊指令。误差约 1e-10 量级（级数 + 参数归约）：
 *   - sqrt：牛顿迭代（12 次收敛到双精度）
 *   - exp/log：归约到小参数后泰勒 / 反双曲正切级数
 *   - sin/cos：模 2π 归约 + 象限折叠 + 泰勒
 *   - atan：倍角恒等式归约 4 次后泰勒
 */

#include <errno.h>
#include <math.h>

#define C89_PI 3.14159265358979323846
#define C89_TWO_PI 6.28318530717958647693
#define C89_LN2 0.69314718055994530942
#define C89_LN10_INV 0.43429448190325182765

double fabs(double x) {
    return (x < 0.0) ? -x : x;
}

/* |x| < 2^31 时用 int 转换取整（CPU 的 fcvt 对 |x| >= 2^31 饱和为哨兵值，故大数不得走此路） */
static double c89_floor_small(double x) {
    double t = (double)(int)x;
    if (t > x) {
        t -= 1.0;
    }
    return t;
}

/* x >= 2^31 的 floor：按 2 的幂分块。每轮取 p 满足 p <= r < 2p，
   此时 r - p 精确（同量级相减）且指数严格下降，降到 r < 2^31 后走 int 路径。
   全程不依赖 (int) 转换大数，对任意 |x| 都正确。 */
static double c89_floor_big(double x) {
    double acc = 0.0;
    double r = x;
    double p;
    while (r >= 2147483648.0) {
        p = 1.0;
        while (p * 2.0 <= r) {
            p *= 2.0;
        }
        acc += p;
        r -= p;
    }
    return acc + c89_floor_small(r);
}

/* x <= -2^31 的 floor：floor(x) = -ceil(-x) */
static double c89_floor_big_neg(double x) {
    double acc = 0.0;
    double r = 0.0 - x;
    double p;
    double t;
    while (r >= 2147483648.0) {
        p = 1.0;
        while (p * 2.0 <= r) {
            p *= 2.0;
        }
        acc += p;
        r -= p;
    }
    t = (double)(int)r;
    if (t < r) {
        t += 1.0;
    }
    return 0.0 - (acc + t);
}

double floor(double x) {
    if (x != x) {
        return x; /* NaN */
    }
    if (x >= 2147483648.0) {
        return c89_floor_big(x);
    }
    if (x < -2147483648.0) {
        return c89_floor_big_neg(x);
    }
    return c89_floor_small(x);
}

double ceil(double x) {
    if (x != x) {
        return x; /* NaN */
    }
    return -floor(-x);
}

/* 向零取整 */
static double c89_trunc(double x) {
    if (x != x) {
        return x;
    }
    if (x < 0.0) {
        return -floor(-x);
    }
    return floor(x);
}

double fmod(double x, double y) {
    double q;
    if (y == 0.0) {
        errno = EDOM;
        return 0.0;
    }
    q = c89_trunc(x / y);
    return x - q * y;
}

double modf(double x, double *ip) {
    double i = c89_trunc(x);
    *ip = i;
    return x - i;
}

double frexp(double x, int *exp) {
    int e = 0;
    int neg = 0;
    if (x == 0.0) {
        *exp = 0;
        return 0.0;
    }
    if (x < 0.0) {
        neg = 1;
        x = -x;
    }
    while (x >= 1.0) {
        x /= 2.0;
        e += 1;
    }
    while (x < 0.5) {
        x *= 2.0;
        e -= 1;
    }
    *exp = e;
    if (neg) {
        return -x;
    }
    return x;
}

double ldexp(double x, int n) {
    while (n > 0) {
        x *= 2.0;
        n -= 1;
    }
    while (n < 0) {
        x /= 2.0;
        n += 1;
    }
    return x;
}

double sqrt(double x) {
    double y;
    int i;
    if (x < 0.0) {
        errno = EDOM;
        return 0.0;
    }
    if (x == 0.0) {
        return 0.0;
    }
    if (x > 1e308) {
        return x; /* +inf 防御：牛顿迭代在 inf 上产出 NaN */
    }
    y = x / 2.0 + 1.0;
    for (i = 0; i < 12; i += 1) {
        y = (y + x / y) / 2.0;
    }
    return y;
}

double exp(double x) {
    double r, term, t;
    int k, i;
    if (x > 709.0) {
        errno = ERANGE;
        return HUGE_VAL;
    }
    if (x < -745.0) {
        return 0.0;
    }
    k = (int)(x / C89_LN2 + 0.5);
    r = x - (double)k * C89_LN2;
    term = 1.0;
    t = 1.0;
    for (i = 1; i <= 16; i += 1) {
        term = term * r / (double)i;
        t += term;
    }
    return ldexp(t, k);
}

double log(double x) {
    double m, t, t2, term, s;
    int e, i;
    if (x < 0.0) {
        errno = EDOM;
        return 0.0;
    }
    if (x == 0.0) {
        errno = ERANGE;
        return -HUGE_VAL;
    }
    m = frexp(x, &e);
    t = (m - 1.0) / (m + 1.0);
    t2 = t * t;
    term = t;
    s = t;
    for (i = 1; i <= 20; i += 1) {
        term = term * t2;
        s += term / (double)(2 * i + 1);
    }
    return 2.0 * s + (double)e * C89_LN2;
}

double log10(double x) {
    return log(x) * C89_LN10_INV;
}

double pow(double x, double y) {
    double r;
    if (y == 0.0) {
        return 1.0;
    }
    if (x == 1.0) {
        return 1.0;
    }
    if (x == 0.0) {
        if (y > 0.0) {
            return 0.0;
        }
        errno = ERANGE;
        return HUGE_VAL;
    }
    if (x < 0.0) {
        /* (int)y 对大指数饱和：改用 floor 判断整数性 */
        if (y != floor(y)) {
            errno = EDOM;
            return 0.0;
        }
        r = exp(y * log(-x));
        /* 奇偶用 fmod 判断：((int)y) 对大指数饱和后不可靠 */
        if (fmod(y, 2.0) != 0.0) {
            r = -r;
        }
        return r;
    }
    return exp(y * log(x));
}

/* 泰勒展开（|r| <= π/2 内 7 项，误差 < 1e-8；常用参数误差 1e-14） */
static double c89_sin_poly(double r) {
    double r2 = r * r, term = r, s = r;
    int i;
    for (i = 1; i <= 7; i += 1) {
        term = -term * r2 / ((double)(2 * i) * (double)(2 * i + 1));
        s += term;
    }
    return s;
}

static double c89_cos_poly(double r) {
    double r2 = r * r, term = 1.0, s = 1.0;
    int i;
    for (i = 1; i <= 7; i += 1) {
        term = -term * r2 / ((double)(2 * i - 1) * (double)(2 * i));
        s += term;
    }
    return s;
}

double sin(double x) {
    double r = fmod(x, C89_TWO_PI);
    if (r > C89_PI) {
        r -= C89_TWO_PI;
    }
    if (r < -C89_PI) {
        r += C89_TWO_PI;
    }
    if (r > C89_PI / 2.0) {
        return c89_sin_poly(C89_PI - r);
    }
    if (r < -C89_PI / 2.0) {
        return -c89_sin_poly(C89_PI + r);
    }
    return c89_sin_poly(r);
}

double cos(double x) {
    double r = fmod(x, C89_TWO_PI);
    if (r > C89_PI) {
        r -= C89_TWO_PI;
    }
    if (r < -C89_PI) {
        r += C89_TWO_PI;
    }
    if (r > C89_PI / 2.0) {
        return -c89_cos_poly(C89_PI - r);
    }
    if (r < -C89_PI / 2.0) {
        return -c89_cos_poly(C89_PI + r);
    }
    return c89_cos_poly(r);
}

double tan(double x) {
    return sin(x) / cos(x);
}

/* 恒等式 atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))) 归约 4 次 → |x| <= 0.1 */
double atan(double x) {
    double x2, term, s;
    int i, k;
    /* |x| 约大于 1.3e154 时 x*x 溢出 inf：直接返回 ±π/2 */
    if (x > 1e150) {
        return C89_PI / 2.0;
    }
    if (x < -1e150) {
        return -C89_PI / 2.0;
    }
    for (k = 0; k < 4; k += 1) {
        x = x / (1.0 + sqrt(1.0 + x * x));
    }
    x2 = x * x;
    term = x;
    s = x;
    for (i = 1; i <= 12; i += 1) {
        term = -term * x2;
        s += term / (double)(2 * i + 1);
    }
    return 16.0 * s;
}

double asin(double x) {
    if (x > 1.0 || x < -1.0) {
        errno = EDOM;
        return 0.0;
    }
    if (x == 1.0) {
        return C89_PI / 2.0;
    }
    if (x == -1.0) {
        return -C89_PI / 2.0;
    }
    return atan(x / sqrt(1.0 - x * x));
}

double acos(double x) {
    if (x > 1.0 || x < -1.0) {
        errno = EDOM;
        return 0.0;
    }
    return C89_PI / 2.0 - asin(x);
}

double atan2(double y, double x) {
    if (x > 0.0) {
        return atan(y / x);
    }
    if (x < 0.0) {
        if (y >= 0.0) {
            return atan(y / x) + C89_PI;
        }
        return atan(y / x) - C89_PI;
    }
    if (y > 0.0) {
        return C89_PI / 2.0;
    }
    if (y < 0.0) {
        return -C89_PI / 2.0;
    }
    return 0.0;
}

double sinh(double x) {
    return (exp(x) - exp(-x)) / 2.0;
}

double cosh(double x) {
    return (exp(x) + exp(-x)) / 2.0;
}

double tanh(double x) {
    double t = exp(2.0 * x);
    return (t - 1.0) / (t + 1.0);
}
