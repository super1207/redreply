/* LeetCode 7: 整数反转 */

int reverse_integer(int value)
{
    int result;
    int digit;

    result = 0;
    while (value != 0) {
        digit = value % 10;
        value = value / 10;
        if (result > 214748364 ||
            (result == 214748364 && digit > 7)) {
            return 0;
        }
        if (result < -214748364 ||
            (result == -214748364 && digit < -8)) {
            return 0;
        }
        result = result * 10 + digit;
    }
    return result;
}

int main(void)
{
    if (reverse_integer(123) != 321) return 1;
    if (reverse_integer(-123) != -321) return 2;
    if (reverse_integer(120) != 21) return 3;
    if (reverse_integer(1534236469) != 0) return 4;
    if (reverse_integer(-2147483412) != -2143847412) return 5;
    return 42;
}
