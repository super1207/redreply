/* c89lib vfprintf/vprintf/vsprintf/fprintf 冒烟测试 */
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

static int emit_vprintf(const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vprintf(fmt, ap);
    va_end(ap);
    return result;
}

static int emit_vsprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vsprintf(buf, fmt, ap);
    va_end(ap);
    return result;
}

int main(void) {
    char buf[64];
    int ok = 0;
    ok += (emit_vsprintf(buf, "%d/%f", 7, 2.5) == 9);
    ok += (strcmp(buf, "7/2.500000") == 0);
    ok += (fprintf(stdout, "fprintf %s\n", buf) == 18);
    ok += (emit_vprintf("vprintf %d\n", 42) == 11);
    if (ok == 4) {
        return 0;
    }
    return 1;
}
