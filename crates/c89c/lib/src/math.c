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
    if (x == 0.0) {
        return 0.0; /* -0.0 → +0.0（|x| 定义要求；NaN 不会 == 0，仍走下方直通） */
    }
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

/* x >= 2^31 的 floor：O(1) 实现。
   2^31 <= x < 2^52 时加 2^52 再减（该区间浮点间距恰为 1，加法即向最近
   整数舍入），按 floor 方向修正；x >= 2^52 时双精度已无小数位，恒等。 */
static double c89_floor_big(double x) {
    double t;
    if (x >= 4503599627370496.0) {
        return x;
    }
    t = x + 4503599627370496.0;
    t = t - 4503599627370496.0;
    if (t > x) {
        t -= 1.0;
    }
    return t;
}

/* x <= -2^31 的 floor：floor(x) = -ceil(-x)，同样 O(1)。 */
static double c89_floor_big_neg(double x) {
    double u = 0.0 - x;
    double t;
    if (u >= 4503599627370496.0) {
        return x;
    }
    t = u + 4503599627370496.0;
    t = t - 4503599627370496.0;
    if (t < u) {
        t += 1.0;
    }
    return 0.0 - t;
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
    double r;
    double s;
    if (y == 0.0) {
        errno = EDOM;
        return 0.0;
    }
    if (x != x) {
        return x; /* NaN 直通 */
    }
    if (x > 1.7976931348623157e308 || x < -1.7976931348623157e308) {
        errno = EDOM; /* ±inf：不存在有限余数，防修正循环死循环 */
        return 0.0;
    }
    /* |x/y| >= 2^52 时 q = trunc(x/y) 舍入失真；x/y 溢出为 ±inf 时 q 非有穷，
       r = x - q*y = ∓inf，修正循环永不退出。预归约：s 取 |y| 的 2 的幂倍
       （2^52 起步；|y| 过小时 x/s 本身也溢出，逐次左移 s 使 x/s 有穷），
       每次 x -= trunc(x/s)*s 把 |x| 至少缩小 2^50 倍，直到 |x| < |y|·2^52。
       2 的幂乘法只移指数，s 仍是 |y| 的精确整数倍，不破坏模同余性。 */
    while (fabs(x) >= fabs(y) * 4503599627370496.0) {
        s = fabs(y) * 4503599627370496.0;
        while (fabs(x) / s > 1e308) {
            s = s * 4503599627370496.0;
        }
        q = c89_trunc(x / s);
        if (q == 0.0) {
            break; /* s > |x|：防御（正常不可达），防死循环 */
        }
        x -= q * s;
    }
    q = c89_trunc(x / y);
    r = x - q * y;
    /* x/y 的舍入使 q 可能偏离真值 ±1，r 随之偏差一个 |y|；按 |y| 修正
       （|x/y| < 2^52 时修正后即正确余数）。y < 0 时对称处理。 */
    if (y > 0.0) {
        while (r >= y) {
            r -= y;
        }
        while (r <= 0.0 - y) {
            r += y;
        }
    } else {
        while (r <= y) {
            r -= y;
        }
        while (r >= 0.0 - y) {
            r += y;
        }
    }
    return r;
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
    if (x != x) {
        *exp = 0;
        return x; /* NaN 直通 */
    }
    if (x > 1.7976931348623157e308 || x < -1.7976931348623157e308) {
        *exp = 0;
        return x; /* ±inf 防御：归一化循环在 inf 上死循环 */
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
    double m;
    int e;
    int i;
    double y;
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
    /* frexp 归一化：x = m·2^e，m ∈ [0.5, 1)。sqrt(m) ∈ [0.707, 1)，
       初值 0.7 的相对误差 <= 1.3%，12 次牛顿迭代收敛到双精度。
       此前直接用 x/2+1 作初值，x ≪ 1 时结果与真值无关。 */
    m = frexp(x, &e);
    y = 0.7;
    for (i = 0; i < 12; i += 1) {
        y = (y + m / y) / 2.0;
    }
    if (e % 2 != 0) {
        /* 奇指数：sqrt(m)·√2·2^((e-1)/2) */
        y = y * 1.41421356237309504880;
        e = (e - 1) / 2;
    } else {
        e = e / 2;
    }
    return ldexp(y, e);
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
    if (x > 1.7976931348623157e308) {
        return HUGE_VAL; /* +inf：log(+inf) = +inf */
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
