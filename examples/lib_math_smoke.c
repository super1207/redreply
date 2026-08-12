/* c89lib math 鍐掔儫娴嬭瘯锛歴qrt/pow/fabs/floor/ceil/fmod/sin/log10/exp/atan2/ldexp/modf/errno */
#include <errno.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    char buf[64];
    double ip;
    int ok = 0;
    errno = 0;
    sprintf(buf, "%.2f", sqrt(2.0));
    if (strcmp(buf, "1.41") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", pow(2.0, 10.0));
    if (strcmp(buf, "1024.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", fabs(-3.5));
    if (strcmp(buf, "3.50") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", floor(3.7));
    if (strcmp(buf, "3.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", ceil(3.2));
    if (strcmp(buf, "4.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", fmod(7.5, 2.0));
    if (strcmp(buf, "1.50") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", sin(M_PI / 2.0));
    if (strcmp(buf, "1.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", log10(1000.0));
    if (strcmp(buf, "3.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", exp(1.0));
    if (strcmp(buf, "2.72") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", atan2(1.0, 1.0));
    if (strcmp(buf, "0.79") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", ldexp(1.0, 10));
    if (strcmp(buf, "1024.00") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", modf(3.75, &ip));
    if (strcmp(buf, "0.75") == 0) {
        ok += 1;
    }
    sprintf(buf, "%.2f", ip);
    if (strcmp(buf, "3.00") == 0) {
        ok += 1;
    }
    errno = 0;
    sqrt(-1.0);
    if (errno == EDOM) {
        ok += 1;
    }
    errno = 0;
    log(0.0);
    if (errno == ERANGE) {
        ok += 1;
    }
    if (ok == 15) {
        printf("math ok\n");
        return 0;
    }
    printf("math fail %d\n", ok);
    return 1;
}
