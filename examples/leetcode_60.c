/* LeetCode 60: 排列序列
 *
 * 使用阶乘数制直接选择第 k 个排列，不枚举前面的排列。
 * 题目约束为 1 <= n <= 9，因此结果缓冲区大小为 10（包含 '\0'）。
 */

static int same_text(char *left, char *right)
{
    int i;
    i = 0;
    while (left[i] != '\0' && right[i] != '\0') {
        if (left[i] != right[i]) {
            return 0;
        }
        i = i + 1;
    }
    return left[i] == right[i];
}

char *get_permutation(int n, int k)
{
    static char result[10];
    int digits[9];
    int factorial[10];
    int i;
    int j;
    int index;
    int block_size;
    int remaining;

    factorial[0] = 1;
    for (i = 1; i <= n; i = i + 1) {
        factorial[i] = factorial[i - 1] * i;
    }

    for (i = 0; i < n; i = i + 1) {
        digits[i] = i + 1;
    }

    /* 将从 1 开始的 k 转为从 0 开始的序号。 */
    k = k - 1;
    remaining = n;

    for (i = 0; i < n; i = i + 1) {
        block_size = factorial[remaining - 1];
        index = k / block_size;
        k = k % block_size;

        result[i] = (char)('0' + digits[index]);

        /* 删除已经选中的数字。 */
        for (j = index; j < remaining - 1; j = j + 1) {
            digits[j] = digits[j + 1];
        }
        remaining = remaining - 1;
    }

    result[n] = '\0';
    return result;
}

int main(void)
{
    if (!same_text(get_permutation(3, 3), "213")) {
        return 1;
    }
    if (!same_text(get_permutation(4, 9), "2314")) {
        return 2;
    }
    if (!same_text(get_permutation(3, 1), "123")) {
        return 3;
    }
    if (!same_text(get_permutation(4, 1), "1234")) {
        return 4;
    }
    if (!same_text(get_permutation(4, 24), "4321")) {
        return 5;
    }
    if (!same_text(get_permutation(1, 1), "1")) {
        return 6;
    }
    if (!same_text(get_permutation(9, 1), "123456789")) {
        return 7;
    }
    if (!same_text(get_permutation(9, 362880), "987654321")) {
        return 8;
    }
    return 42;
}
