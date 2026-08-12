/* LeetCode 5: 最长回文子串 */

static int text_length(char *text)
{
    int length;
    length = 0;
    while (text[length] != '\0') length = length + 1;
    return length;
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

char *longest_palindrome(char *text)
{
    static char result[128];
    int length;
    int center;
    int left;
    int right;
    int best_start;
    int best_length;
    int current_length;
    int i;

    length = text_length(text);
    best_start = 0;
    best_length = 0;

    for (center = 0; center < length; center = center + 1) {
        left = center;
        right = center;
        while (left >= 0 && right < length && text[left] == text[right]) {
            current_length = right - left + 1;
            if (current_length > best_length) {
                best_start = left;
                best_length = current_length;
            }
            left = left - 1;
            right = right + 1;
        }

        left = center;
        right = center + 1;
        while (left >= 0 && right < length && text[left] == text[right]) {
            current_length = right - left + 1;
            if (current_length > best_length) {
                best_start = left;
                best_length = current_length;
            }
            left = left - 1;
            right = right + 1;
        }
    }

    for (i = 0; i < best_length; i = i + 1) {
        result[i] = text[best_start + i];
    }
    result[best_length] = '\0';
    return result;
}

int main(void)
{
    char *answer;

    answer = longest_palindrome("babad");
    if (!same_text(answer, "bab") && !same_text(answer, "aba")) return 1;
    if (!same_text(longest_palindrome("cbbd"), "bb")) return 2;
    if (!same_text(longest_palindrome("a"), "a")) return 3;
    if (!same_text(longest_palindrome("ac"), "a")) return 4;
    if (!same_text(longest_palindrome("forgeeksskeegfor"), "geeksskeeg")) return 5;
    if (!same_text(longest_palindrome(""), "")) return 6;
    return 42;
}
