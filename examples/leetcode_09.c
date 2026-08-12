/* LeetCode 9: 回文数 */

int is_palindrome_number(int value)
{
    int reversed_half;

    if (value < 0 || (value % 10 == 0 && value != 0)) return 0;
    reversed_half = 0;
    while (value > reversed_half) {
        reversed_half = reversed_half * 10 + value % 10;
        value = value / 10;
    }
    return value == reversed_half || value == reversed_half / 10;
}

int main(void)
{
    if (!is_palindrome_number(121)) return 1;
    if (is_palindrome_number(-121)) return 2;
    if (is_palindrome_number(10)) return 3;
    if (!is_palindrome_number(0)) return 4;
    if (!is_palindrome_number(12321)) return 5;
    return 42;
}
