/* LeetCode 8: 字符串转换整数 atoi */

int string_to_integer(char *text)
{
    int index;
    int negative;
    int result;
    int digit;

    index = 0;
    while (text[index] == ' ') index = index + 1;
    negative = 0;
    if (text[index] == '-' || text[index] == '+') {
        negative = text[index] == '-';
        index = index + 1;
    }

    result = 0;
    while (text[index] >= '0' && text[index] <= '9') {
        digit = text[index] - '0';
        if (!negative) {
            if (result > 214748364 ||
                (result == 214748364 && digit > 7)) {
                return 2147483647;
            }
            result = result * 10 + digit;
        } else {
            if (result < -214748364 ||
                (result == -214748364 && digit > 8)) {
                return -2147483647 - 1;
            }
            result = result * 10 - digit;
        }
        index = index + 1;
    }
    return result;
}

int main(void)
{
    if (string_to_integer("42") != 42) return 1;
    if (string_to_integer("   -42") != -42) return 2;
    if (string_to_integer("4193 with words") != 4193) return 3;
    if (string_to_integer("words and 987") != 0) return 4;
    if (string_to_integer("-91283472332") != -2147483647 - 1) return 5;
    if (string_to_integer("2147483648") != 2147483647) return 6;
    if (string_to_integer("+-12") != 0) return 7;
    return 42;
}
