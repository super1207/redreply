#include <string.h>
#include <ctype.h>
#include <errno.h>

int main(void) {
    char buf[64];
    char *p;
    int total = 0;

    strcpy(buf, "hello");
    strcat(buf, " world");
    total += (strcmp(buf, "hello world") == 0);
    total += (strlen(buf) == 11);

    p = strchr(buf, 'w');
    total += (p != 0 && *p == 'w');
    p = strstr(buf, "lo wo");
    total += (p != 0);
    total += (strncmp(buf, "hello", 5) == 0);
    total += (strcmp(strerror(33), "Domain error") == 0);
    total += (isdigit('7') && isalpha('a') && isspace(' '));
    total += (toupper('q') == 'Q');
    total += (tolower('Z') == 'z');
    total += (memcmp("abc", "abd", 3) < 0);
    memset(buf, 'x', 3);
    total += (buf[0] == 'x' && buf[2] == 'x');

    strcpy(buf, "one,two;three");
    p = strtok(buf, ",;");
    total += (p != 0 && strcmp(p, "one") == 0);
    p = strtok(0, ",;");
    total += (p != 0 && strcmp(p, "two") == 0);
    p = strtok(0, ",;");
    total += (p != 0 && strcmp(p, "three") == 0);
    p = strtok(0, ",;");
    total += (p == 0);

    errno = EDOM;
    total += (errno == 33);

    if (total == 16) {
        return 0;
    }
    return total;
}
