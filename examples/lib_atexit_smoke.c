/* c89lib atexit 冒烟测试：main 返回后按 LIFO 顺序执行 */
#include <stdio.h>
#include <stdlib.h>

static void first(void) {
    puts("atexit first");
}

static void second(void) {
    puts("atexit second");
}

int main(void) {
    if (atexit(first) != 0 || atexit(second) != 0) {
        return EXIT_FAILURE;
    }
    puts("main done");
    return EXIT_SUCCESS;
}
