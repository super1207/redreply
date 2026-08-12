/* 综合 demo：统计文本文件的行/词/字符数，用 sprintf 生成报告。
   覆盖：fopen/fgetc/fclose（FILE 流）、sprintf（变参）、printf、long 算术。
   用法（cwd = 仓库根目录）：
     c89c -S -l examples/lib_report_demo.c | rv-cli -r -
   （smoke 输入：scripts/.work/lib_demo_input.txt，见 scripts/check.ps1） */
#include <stdio.h>
#include <string.h>

int main(void) {
    FILE *f = fopen("scripts/.work/lib_demo_input.txt", "r");
    if (f == 0) {
        printf("open fail\n");
        return 1;
    }
    long lines = 0;
    long words = 0;
    long chars = 0;
    int in_word = 0;
    int c;
    while ((c = fgetc(f)) != EOF) {
        chars += 1;
        if (c == '\n') {
            lines += 1;
        }
        if (c == ' ' || c == '\n' || c == '\t') {
            in_word = 0;
        } else if (in_word == 0) {
            in_word = 1;
            words += 1;
        }
    }
    fclose(f);
    char buf[128];
    int n = sprintf(buf, "lines=%ld words=%ld chars=%ld", lines, words, chars);
    printf("%s\n", buf);
    printf("report bytes: %d\n", n);
    return 0;
}
