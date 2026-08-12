/* ctype：ASCII 字符分类与转换（按 C89 语义，接受 EOF 时返回 0） */

int isalpha(int c) {
    return ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'));
}

int isdigit(int c) {
    return (c >= '0' && c <= '9');
}

int isalnum(int c) {
    return isalpha(c) || isdigit(c);
}

int iscntrl(int c) {
    return (c >= 0 && c <= 31) || c == 127;
}

int isgraph(int c) {
    return c > 32 && c < 127;
}

int islower(int c) {
    return c >= 'a' && c <= 'z';
}

int isprint(int c) {
    return c >= 32 && c < 127;
}

int ispunct(int c) {
    return isgraph(c) && !isalnum(c);
}

int isspace(int c) {
    return c == ' ' || (c >= 9 && c <= 13);
}

int isupper(int c) {
    return c >= 'A' && c <= 'Z';
}

int isxdigit(int c) {
    return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

int tolower(int c) {
    return (c >= 'A' && c <= 'Z') ? c + 32 : c;
}

int toupper(int c) {
    return (c >= 'a' && c <= 'z') ? c - 32 : c;
}
