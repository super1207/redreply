/* LeetCode 12: 整数转罗马数字 */

char *integer_to_roman(int number)
{
    static char result[32];
    static int values[13] = {1000, 900, 500, 400, 100, 90, 50,
                             40, 10, 9, 5, 4, 1};
    static char *symbols[13] = {"M", "CM", "D", "CD", "C", "XC", "L",
                                "XL", "X", "IX", "V", "IV", "I"};
    int index;
    int out;
    int j;

    index = 0;
    out = 0;
    while (number > 0) {
        while (number >= values[index]) {
            j = 0;
            while (symbols[index][j] != '\0') {
                result[out] = symbols[index][j];
                out = out + 1;
                j = j + 1;
            }
            number = number - values[index];
        }
        index = index + 1;
    }
    result[out] = '\0';
    return result;
}

static int same_text(char *left, char *right)
{
    int i;
    i = 0;
    while (left[i] != '\0' && right[i] != '\0') {
        if (left[i] != right[i]) return 0;
        i = i + 1;
    }
    return left[i] == right[i];
}

int main(void)
{
    if (!same_text(integer_to_roman(3), "III")) return 1;
    if (!same_text(integer_to_roman(58), "LVIII")) return 2;
    if (!same_text(integer_to_roman(1994), "MCMXCIV")) return 3;
    if (!same_text(integer_to_roman(3999), "MMMCMXCIX")) return 4;
    return 42;
}
