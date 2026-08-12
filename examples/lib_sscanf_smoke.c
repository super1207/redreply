/* c89lib sscanf 冒烟测试 */
#include <stdio.h>
#include <string.h>

int main(void) {
    char word[16];
    int decimal;
    unsigned int hex;
    double real;
    int count;
    count = sscanf("-17 ff sample 3.25", "%d %x %s %lf",
                   &decimal, &hex, word, &real);
    if (count == 4 && decimal == -17 && hex == 255 &&
        strcmp(word, "sample") == 0 && real == 3.25) {
        printf("sscanf ok\n");
        return 0;
    }
    printf("sscanf fail %d\n", count);
    return 1;
}
