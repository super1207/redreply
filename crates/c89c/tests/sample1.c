/* 测试：预处理阶段 1~3 */
#include <stdio.h>
#define PI 3.14

int main(void) {
    int x = 42;        /* 块注释 */
    double y = PI * \
                x;      /* 续行 */
    char *s = "/* not comment */";
    return (int)y;
}
