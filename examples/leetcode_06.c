/* LeetCode 6: Z 字形变换 */

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

char *zigzag_convert(char *text, int rows)
{
    static char result[128];
    int length;
    int row;
    int index;
    int cycle;
    int diagonal;
    int out;

    length = 0;
    while (text[length] != '\0') length = length + 1;
    if (rows <= 1 || rows >= length) {
        for (index = 0; index <= length; index = index + 1) {
            result[index] = text[index];
        }
        return result;
    }

    cycle = 2 * rows - 2;
    out = 0;
    for (row = 0; row < rows; row = row + 1) {
        index = row;
        while (index < length) {
            result[out] = text[index];
            out = out + 1;
            diagonal = index + cycle - 2 * row;
            if (row != 0 && row != rows - 1 && diagonal < length) {
                result[out] = text[diagonal];
                out = out + 1;
            }
            index = index + cycle;
        }
    }
    result[out] = '\0';
    return result;
}

int main(void)
{
    if (!same_text(zigzag_convert("PAYPALISHIRING", 3), "PAHNAPLSIIGYIR")) return 1;
    if (!same_text(zigzag_convert("PAYPALISHIRING", 4), "PINALSIGYAHRPI")) return 2;
    if (!same_text(zigzag_convert("A", 1), "A")) return 3;
    if (!same_text(zigzag_convert("AB", 1), "AB")) return 4;
    return 42;
}
