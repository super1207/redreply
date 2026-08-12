#include <stdio.h>

int main(void) {
    int ok = 0;
    ok += (putchar('a') == 'a');
    ok += (putchar('b') == 'b');
    ok += (puts("hello stdio") == 0);
    printf("value=%d\n", 42);
    if (ok == 3) {
        return 0;
    }
    return ok;
}
