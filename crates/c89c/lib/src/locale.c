/* c89lib locale.c：locale（C89 7.4）。
 * 本实现只支持 "C" locale（符合 C89 7.4.1 的约定）：
 * - setlocale(category, "C") / setlocale(category, "") → "C"；
 *   ""（原生环境）简化为 "C"，与 strcoll/printf 等现有 C locale 行为一致。
 * - 不支持的 locale 名返回 NULL。
 * - localeconv() 返回 C locale 的 lconv：小数点 "."，货币字段为空，
 *   各 char 成员为 CHAR_MAX（C89 7.4.2）。 */
#include <limits.h>
#include <locale.h>
#include <string.h>

char *setlocale(int category, const char *locale) {
    if (locale == NULL) {
        return "C";
    }
    if (locale[0] == '\0') {
        return "C";
    }
    if (strcmp(locale, "C") == 0) {
        return "C";
    }
    return NULL;
}

static struct lconv c89_c_lconv = {
    ".", "", "", "", "", "", "", "", "", "",
    CHAR_MAX, CHAR_MAX, CHAR_MAX, CHAR_MAX,
    CHAR_MAX, CHAR_MAX, CHAR_MAX, CHAR_MAX
};

struct lconv *localeconv(void) {
    return &c89_c_lconv;
}
