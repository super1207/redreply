/* LeetCode 30: 串联所有单词的子串
 *
 * 返回值是找到的起点数量，结果索引写入 result[]。
 * words 中的所有单词长度必须相同，允许重复单词。
 *
 * 这是不依赖 malloc/strcmp/strlen 的 C89 实现，适合 rv-core。
 * 为了避免固定大小的哈希表，它通过逐个统计词频来校验窗口。
 */

static int text_length(char *text)
{
    int length;
    length = 0;
    while (text[length] != '\0') {
        length = length + 1;
    }
    return length;
}

static int same_word(char *left, char *right, int length)
{
    int i;
    for (i = 0; i < length; i = i + 1) {
        if (left[i] != right[i]) {
            return 0;
        }
    }
    return 1;
}

/*
 * 找出所有串联子串的起点。
 *
 * 对每个可能起点，分别统计每个单词在 words 和候选窗口中的次数，
 * 因此 words 中存在重复单词时也能正确校验。
 */
int find_substring(char *s, char *words[], int word_count, int word_length,
                   int result[])
{
    int source_length;
    int total_length;
    int start;
    int word_index;
    int chunk_index;
    int expected;
    int actual;
    int result_count;
    int valid;
    char *required_word;
    char *chunk;

    if (word_count <= 0 || word_length <= 0) {
        return 0;
    }

    source_length = text_length(s);
    total_length = word_count * word_length;
    result_count = 0;

    for (start = 0; start + total_length <= source_length; start = start + 1) {
        valid = 1;

        for (word_index = 0; word_index < word_count; word_index = word_index + 1) {
            required_word = words[word_index];
            expected = 0;
            actual = 0;

            for (chunk_index = 0; chunk_index < word_count; chunk_index = chunk_index + 1) {
                if (same_word(required_word, words[chunk_index], word_length)) {
                    expected = expected + 1;
                }
            }

            for (chunk_index = 0; chunk_index < word_count; chunk_index = chunk_index + 1) {
                chunk = s + start + chunk_index * word_length;
                if (same_word(required_word, chunk, word_length)) {
                    actual = actual + 1;
                }
            }

            if (actual != expected) {
                valid = 0;
                break;
            }
        }

        if (valid) {
            result[result_count] = start;
            result_count = result_count + 1;
        }
    }

    return result_count;
}

static int contains(int values[], int count, int target)
{
    int i;
    for (i = 0; i < count; i = i + 1) {
        if (values[i] == target) {
            return 1;
        }
    }
    return 0;
}

static int check_case(char *s, char *words[], int word_count, int word_length,
                      int expected[], int expected_count)
{
    int result[32];
    int actual_count;
    int i;

    actual_count = find_substring(s, words, word_count, word_length, result);
    if (actual_count != expected_count) {
        return 0;
    }
    for (i = 0; i < expected_count; i = i + 1) {
        if (!contains(result, actual_count, expected[i])) {
            return 0;
        }
    }
    return 1;
}

int main(void)
{
    char *words1[2] = {"foo", "bar"};
    char *words2[4] = {"word", "good", "best", "word"};
    char *words3[3] = {"bar", "foo", "the"};
    char *words4[2] = {"aa", "aa"};
    int expected1[2] = {0, 9};
    int expected2[1] = {0};
    int expected3[3] = {6, 9, 12};
    int expected4[3] = {0, 1, 2};

    if (!check_case("barfoothefoobarman", words1, 2, 3, expected1, 2)) {
        return 1;
    }
    if (!check_case("wordgoodgoodgoodbestword", words2, 4, 4, expected2, 0)) {
        return 2;
    }
    if (!check_case("barfoofoobarthefoobarman", words3, 3, 3, expected3, 3)) {
        return 3;
    }
    if (!check_case("aaaaaa", words4, 2, 2, expected4, 3)) {
        return 4;
    }
    return 42;
}
