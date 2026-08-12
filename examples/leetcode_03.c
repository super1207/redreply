/* LeetCode 3: 无重复字符的最长子串 */

int length_of_longest_substring(char *text)
{
    int last[256];
    int i;
    int left;
    int right;
    int best;
    int code;

    for (i = 0; i < 256; i = i + 1) {
        last[i] = -1;
    }
    left = 0;
    right = 0;
    best = 0;

    while (text[right] != '\0') {
        code = (unsigned char)text[right];
        if (last[code] >= left) {
            left = last[code] + 1;
        }
        last[code] = right;
        if (right - left + 1 > best) {
            best = right - left + 1;
        }
        right = right + 1;
    }
    return best;
}

int main(void)
{
    if (length_of_longest_substring("abcabcbb") != 3) return 1;
    if (length_of_longest_substring("bbbbb") != 1) return 2;
    if (length_of_longest_substring("pwwkew") != 3) return 3;
    if (length_of_longest_substring("") != 0) return 4;
    if (length_of_longest_substring("dvdf") != 3) return 5;
    return 42;
}
