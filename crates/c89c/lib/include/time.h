/* c89lib time.h: date and time (C89 7.12) */
#ifndef C89_TIME_H
#define C89_TIME_H

#include <stddef.h>

typedef unsigned long time_t;
typedef long clock_t;

struct tm {
    int tm_sec;   /* seconds 0-60 (60=leap second) */
    int tm_min;   /* minutes 0-59 */
    int tm_hour;  /* hours 0-23 */
    int tm_mday;  /* day of month 1-31 */
    int tm_mon;   /* month 0-11 */
    int tm_year;  /* year - 1900 */
    int tm_wday;  /* day of week 0=Sunday */
    int tm_yday;  /* day of year 0-365 */
    int tm_isdst; /* daylight saving flag */
};

#define CLOCKS_PER_SEC 1000

clock_t clock(void);
double difftime(time_t t1, time_t t0);
time_t mktime(struct tm *t);
time_t time(time_t *t);
char *asctime(const struct tm *t);
char *ctime(const time_t *t);
struct tm *gmtime(const time_t *t);
struct tm *localtime(const time_t *t);
size_t strftime(char *s, size_t max, const char *fmt, const struct tm *t);

#endif
