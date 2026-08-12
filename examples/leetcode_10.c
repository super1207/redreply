/* LeetCode 10: 正则表达式匹配（支持 . 和 *） */

int regular_expression_match(char *text, char *pattern)
{
    int first_matches;

    if (pattern[0] == '\0') return text[0] == '\0';
    first_matches = text[0] != '\0' &&
                    (pattern[0] == text[0] || pattern[0] == '.');

    if (pattern[1] == '*') {
        return regular_expression_match(text, pattern + 2) ||
               (first_matches && regular_expression_match(text + 1, pattern));
    }
    return first_matches && regular_expression_match(text + 1, pattern + 1);
}

int main(void)
{
    if (regular_expression_match("aa", "a")) return 1;
    if (!regular_expression_match("aa", "a*")) return 2;
    if (!regular_expression_match("ab", ".*")) return 3;
    if (!regular_expression_match("aab", "c*a*b")) return 4;
    if (regular_expression_match("mississippi", "mis*is*p*.")) return 5;
    if (!regular_expression_match("", "c*")) return 6;
    return 42;
}
