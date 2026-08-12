/* LeetCode 14: 最长公共前缀 */

char *longest_common_prefix(char *words[], int count)
{
    static char result[128];
    int index;
    int word;

    if (count == 0) {
        result[0] = '\0';
        return result;
    }

    index = 0;
    while (words[0][index] != '\0') {
        for (word = 1; word < count; word = word + 1) {
            if (words[word][index] != words[0][index]) {
                result[index] = '\0';
                return result;
            }
        }
        result[index] = words[0][index];
        index = index + 1;
    }
    result[index] = '\0';
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
    char *a[3] = {"flower", "flow", "flight"};
    char *b[3] = {"dog", "racecar", "car"};
    char *c[1] = {"single"};
    if (!same_text(longest_common_prefix(a, 3), "fl")) return 1;
    if (!same_text(longest_common_prefix(b, 3), "")) return 2;
    if (!same_text(longest_common_prefix(c, 1), "single")) return 3;
    if (!same_text(longest_common_prefix(0, 0), "")) return 4;
    return 42;
}
