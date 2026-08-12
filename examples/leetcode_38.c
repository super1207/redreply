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

char *count_and_say(int n)
{
    static char result[256];
    char current[256];
    char next[256];
    int length;
    int step;
    int i;
    int j;
    int out;
    int count;

    current[0] = '1';
    current[1] = '\0';
    length = 1;

    for (step = 1; step < n; step = step + 1) {
        i = 0;
        out = 0;
        while (i < length) {
            j = i + 1;
            while (j < length && current[j] == current[i]) j = j + 1;
            count = j - i;
            next[out] = (char)('0' + count);
            next[out + 1] = current[i];
            out = out + 2;
            i = j;
        }
        next[out] = '\0';
        for (i = 0; i <= out; i = i + 1) current[i] = next[i];
        length = out;
    }

    for (i = 0; i <= length; i = i + 1) result[i] = current[i];
    return result;
}

int main(void)
{
    if (!same_text(count_and_say(1), "1")) return 1;
    if (!same_text(count_and_say(4), "1211")) return 2;
    if (!same_text(count_and_say(5), "111221")) return 3;
    if (!same_text(count_and_say(6), "312211")) return 4;
    return 42;
}
