/* c89lib sprintf 冒烟测试：%d %i %u %x %c %s %f %% 与返回值 */
#include <stdio.h>
#include <string.h>

int main(void) {
    char buf[128];
    int ok = 0;
    int n = sprintf(buf, "%d|%i|%u|%x|%c|%s|%%|%f", -123, 42, -1, -1, 'Z', "hi", 3.14159);
    if (strcmp(buf, "-123|42|4294967295|ffffffff|Z|hi|%|3.141590") == 0) {
        ok += 1;
    }
    if (n == 43) {
        ok += 1;
    }
    sprintf(buf, "%f", 2.999);
    if (strcmp(buf, "2.999000") == 0) {
        ok += 1;
    }
    sprintf(buf, "%f", -1.5);
    if (strcmp(buf, "-1.500000") == 0) {
        ok += 1;
    }
    sprintf(buf, "%d", 0);
    if (strcmp(buf, "0") == 0) {
        ok += 1;
    }
    sprintf(buf, "%u", 7);
    if (strcmp(buf, "7") == 0) {
        ok += 1;
    }
    sprintf(buf, "%x", 255);
    if (strcmp(buf, "ff") == 0) {
        ok += 1;
    }
    sprintf(buf, "%s", "abc");
    if (strcmp(buf, "abc") == 0) {
        ok += 1;
    }
    if (ok == 8) {
        printf("sprintf ok\n");
        return 0;
    }
    printf("sprintf fail %d\n", ok);
    return 1;
}
