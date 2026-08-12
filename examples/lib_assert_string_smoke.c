/* c89lib assert/stdarg/strcoll/strxfrm 冒烟测试 */
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

int sum(int count, ...) {
    va_list ap;
    int i;
    int total = 0;
    va_start(ap, count);
    for (i = 0; i < count; i = i + 1) {
        total += va_arg(ap, int);
    }
    va_end(ap);
    return total;
}

int main(void) {
    char transformed[16];
    int ok = 0;
    ok += (strcoll("abc", "abd") < 0);
    ok += (strcoll("same", "same") == 0);
    ok += (strxfrm(transformed, "hello", sizeof(transformed)) == 5);
    ok += (strcmp(transformed, "hello") == 0);
    ok += (sum(3, 10, 20, 30) == 60);
    assert(ok == 5);
    printf("assert/string ok\n");
    return 0;
}
