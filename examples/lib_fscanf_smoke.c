/* c89lib fscanf 冒烟测试：仅普通文件输入，不依赖 stdin */
#include <stdio.h>
#include <string.h>

int main(void) {
    FILE *file;
    char word[16];
    int number;
    double real;
    int count;
    file = fopen("scripts/.work/fscanf_input.txt", "rb");
    if (file == 0) {
        return 1;
    }
    count = fscanf(file, "%d %s %lf", &number, word, &real);
    fclose(file);
    if (count == 3 && number == 42 && strcmp(word, "sample") == 0 && real == 1.5) {
        printf("fscanf ok\n");
        return 0;
    }
    printf("fscanf fail %d\n", count);
    return 1;
}
