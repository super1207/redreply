/* c89lib time.c: C89 7.12 time.h implementation.
   Design notes:
   - time_t = unsigned long (matches ecall 30, 32-bit epoch seconds,
     valid until 2106-02-07, same limit as the underlying ecall).
   - time()/gmtime() are UTC; localtime()/mktime() apply the host UTC
     offset (sys_tz_offset), and DST uses the host's current DST state
     (no per-date DST rules).
   - mktime() normalizes and back-fills tm_wday/tm_yday per C89 4.12.2.3.
   - Times before 1970-01-01 (UTC) are not representable; mktime returns -1. */
#include <time.h>
#include <stdio.h>
#include <string.h>

extern unsigned long sys_time(void);
extern unsigned long sys_clock(void);
extern long sys_tz_offset(void);
extern int sys_tz_dst(void);
extern void sys_tz_name(char *buf);

/* floor division (C89 division truncates toward zero; we need floor) */
static long fdiv(long a, long b) {
    long q = a / b;
    if (a % b != 0 && a < 0) q = q - 1;
    return q;
}

/* days since 1970-01-01 -> civil date (y in [1970..], m 1..12) */
static void civil_from_days(long days, int *y, int *m, int *d) {
    long z = days + 719468;
    long era = z / 146097;
    long doe = z - era * 146097;
    long yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long yy = yoe + era * 400;
    long doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    long mp = (5 * doy + 2) / 153;
    long dd = doy - (153 * mp + 2) / 5 + 1;
    long mm = mp < 10 ? mp + 3 : mp - 9;
    *d = (int)dd;
    *m = (int)mm;
    *y = (int)((mm <= 2) ? yy + 1 : yy);
}

/* civil date -> days since 1970-01-01 (y >= 1970, m 1..12, d any) */
static long days_from_civil(long y, long m, long d) {
    y = y - (m <= 2);
    {
        long era = y / 400;
        long yoe = y - era * 400;
        long doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
        long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        return era * 146097 + doe - 719468;
    }
}

time_t mktime(struct tm *t) {
    long sec = t->tm_sec;
    long min = t->tm_min;
    long hour = t->tm_hour;
    long mday = t->tm_mday;
    long mon = t->tm_mon;
    long year = t->tm_year;
    long days;
    long q;
    long result;

    /* normalize sec/min/hour into mday */
    q = fdiv(sec, 60); min = min + q; sec = sec - q * 60;
    q = fdiv(min, 60); hour = hour + q; min = min - q * 60;
    q = fdiv(hour, 24); mday = mday + q; hour = hour - q * 24;
    /* normalize mon into year (mon can be negative or >= 12) */
    q = fdiv(mon, 12); year = year + q; mon = mon - q * 12;

    if (year + 1900 < 1970) return (time_t)-1;
    days = days_from_civil(year + 1900, mon + 1, mday);
    if (days < 0) return (time_t)-1;

    /* C89 4.12.2.3: correct invalid dates (e.g. 1900-02-29 -> 1900-03-01) */
    {
        int yy, mm, dd;
        civil_from_days(days, &yy, &mm, &dd);
        year = yy - 1900; mon = mm - 1; mday = dd;
    }

    /* tm 是本地时间：转 UTC 纪元秒需减去宿主时区偏移 */
    result = days * 86400L + hour * 3600 + min * 60 + sec;
    result = result - sys_tz_offset();
    if (result < 0) return (time_t)-1;

    t->tm_sec = (int)sec;
    t->tm_min = (int)min;
    t->tm_hour = (int)hour;
    t->tm_mday = (int)mday;
    t->tm_mon = (int)mon;
    t->tm_year = (int)year;
    t->tm_wday = (int)((days + 4) % 7);        /* 1970-01-01 = Thursday */
    t->tm_yday = (int)(days - days_from_civil(year + 1900, 1, 1));
    t->tm_isdst = sys_tz_dst();
    return (time_t)result;
}

/* epoch 秒 → 分解时间（t 可为负；days 用向下取整） */
static void c89_break_down(long t, struct tm *tmb, int isdst) {
    long days = fdiv(t, 86400L);
    long rem = t - days * 86400L;
    int y, m, d;
    civil_from_days(days, &y, &m, &d);
    tmb->tm_sec = (int)(rem % 60L); rem = rem / 60L;
    tmb->tm_min = (int)(rem % 60L); rem = rem / 60L;
    tmb->tm_hour = (int)rem;
    tmb->tm_mday = d;
    tmb->tm_mon = m - 1;
    tmb->tm_year = y - 1900;
    tmb->tm_wday = (int)(((days % 7L) + 11L) % 7L);   /* 1970-01-01 = Thursday(4) */
    tmb->tm_yday = (int)(days - days_from_civil(y, 1, 1));
    tmb->tm_isdst = isdst;
}

struct tm *gmtime(const time_t *timer) {
    static struct tm tmb;
    c89_break_down((long)*timer, &tmb, 0);
    return &tmb;
}

struct tm *localtime(const time_t *timer) {
    static struct tm tmb;
    c89_break_down((long)*timer + sys_tz_offset(), &tmb, sys_tz_dst());
    return &tmb;
}

time_t time(time_t *timer) {
    time_t t = (time_t)sys_time();
    if (timer != NULL) *timer = t;
    return t;
}

clock_t clock(void) { return (clock_t)sys_clock(); }

double difftime(time_t t1, time_t t0) { return (double)t1 - (double)t0; }

static const char *const wday_name[7] = {
    "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"
};
static const char *const wday_full[7] = {
    "Sunday", "Monday", "Tuesday", "Wednesday",
    "Thursday", "Friday", "Saturday"
};
static const char *const mon_name[12] = {
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
};
static const char *const mon_full[12] = {
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"
};

char *asctime(const struct tm *t) {
    static char buf[27];
    sprintf(buf, "%.3s %.3s%3d %.2d:%.2d:%.2d %d\n",
            wday_name[t->tm_wday], mon_name[t->tm_mon],
            t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec,
            t->tm_year + 1900);
    return buf;
}

char *ctime(const time_t *t) { return asctime(localtime(t)); }


/* strftime: C89 4.12.3.5 */
struct sf_buf {
    char *s;
    size_t max;
    size_t n;
    int ok;
};

static void sf_putc(struct sf_buf *b, char c) {
    if (!b->ok) return;
    if (b->n + 1 >= b->max) { b->ok = 0; return; }
    b->s[b->n++] = c;
}

static void sf_puts(struct sf_buf *b, const char *p) {
    while (*p) sf_putc(b, *p++);
}

static void sf_num(struct sf_buf *b, int v, int width) {
    char tmp[12];
    int i = 0;
    if (v < 0) { sf_putc(b, '-'); v = -v; }
    do { tmp[i++] = (char)('0' + v % 10); v = v / 10; } while (v > 0);
    { int pad; for (pad = i; pad < width; pad++) sf_putc(b, '0'); }
    while (i > 0) { i--; sf_putc(b, tmp[i]); }
}

size_t strftime(char *s, size_t max, const char *fmt, const struct tm *t) {
    struct sf_buf b;
    char tmp[32];
    int h;
    b.s = s;
    b.max = max;
    b.n = 0;
    b.ok = 1;
    while (*fmt && b.ok) {
        if (*fmt != '%') { sf_putc(&b, *fmt); fmt++; continue; }
        fmt++;
        switch (*fmt) {
        case 'a': sf_puts(&b, wday_name[t->tm_wday]); break;
        case 'A': sf_puts(&b, wday_full[t->tm_wday]); break;
        case 'b': sf_puts(&b, mon_name[t->tm_mon]); break;
        case 'B': sf_puts(&b, mon_full[t->tm_mon]); break;
        case 'c':
            sprintf(tmp, "%.3s %.3s%3d %.2d:%.2d:%.2d %d",
                    wday_name[t->tm_wday], mon_name[t->tm_mon],
                    t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec,
                    t->tm_year + 1900);
            sf_puts(&b, tmp);
            break;
        case 'd': sf_num(&b, t->tm_mday, 2); break;
        case 'H': sf_num(&b, t->tm_hour, 2); break;
        case 'I':
            h = t->tm_hour % 12;
            if (h == 0) h = 12;
            sf_num(&b, h, 2);
            break;
        case 'j': sf_num(&b, t->tm_yday + 1, 3); break;
        case 'm': sf_num(&b, t->tm_mon + 1, 2); break;
        case 'M': sf_num(&b, t->tm_min, 2); break;
        case 'p': sf_puts(&b, t->tm_hour < 12 ? "AM" : "PM"); break;
        case 'S': sf_num(&b, t->tm_sec, 2); break;
        case 'U': sf_num(&b, (t->tm_yday + 7 - t->tm_wday) / 7, 2); break;
        case 'w': sf_num(&b, t->tm_wday, 1); break;
        case 'W': sf_num(&b, (t->tm_yday + 7 - (t->tm_wday + 6) % 7) / 7, 2); break;
        case 'x':
            sprintf(tmp, "%.3s %3d %d", mon_name[t->tm_mon], t->tm_mday, t->tm_year + 1900);
            sf_puts(&b, tmp);
            break;
        case 'X':
            sprintf(tmp, "%.2d:%.2d:%.2d", t->tm_hour, t->tm_min, t->tm_sec);
            sf_puts(&b, tmp);
            break;
        case 'y': sf_num(&b, (t->tm_year + 1900) % 100, 2); break;
        case 'Y': sf_num(&b, t->tm_year + 1900, 4); break;
        case 'Z': { char zbuf[64]; sys_tz_name(zbuf); sf_puts(&b, zbuf); } break;
        case '%': sf_putc(&b, '%'); break;
        default: sf_putc(&b, '%'); sf_putc(&b, *fmt); break;
        }
        fmt++;
    }
    if (b.ok) { s[b.n] = '\0'; return b.n; }
    return 0;
}
