/* LeetCode 13: 罗马数字转整数 */

static int roman_value(char symbol)
{
    if (symbol == 'I') return 1;
    if (symbol == 'V') return 5;
    if (symbol == 'X') return 10;
    if (symbol == 'L') return 50;
    if (symbol == 'C') return 100;
    if (symbol == 'D') return 500;
    if (symbol == 'M') return 1000;
    return 0;
}

int roman_to_integer(char *text)
{
    int total;
    int index;
    int current;
    int next;

    total = 0;
    index = 0;
    while (text[index] != '\0') {
        current = roman_value(text[index]);
        next = roman_value(text[index + 1]);
        if (current < next) total = total - current;
        else total = total + current;
        index = index + 1;
    }
    return total;
}

int main(void)
{
    if (roman_to_integer("III") != 3) return 1;
    if (roman_to_integer("LVIII") != 58) return 2;
    if (roman_to_integer("MCMXCIV") != 1994) return 3;
    if (roman_to_integer("XLIX") != 49) return 4;
    return 42;
}
