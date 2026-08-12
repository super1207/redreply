#include <stdio.h>
#include <string.h>

int main(void) {
    FILE *f;
    char buf[64];
    int ok = 0;
    f = fopen("scripts/.work/c89lib_file_tmp.txt", "wb");
    ok += (f != 0);
    ok += (fputs("file smoke ok\n", f) >= 0);
    ok += (fclose(f) == 0);
    f = fopen("scripts/.work/c89lib_file_tmp.txt", "rb");
    ok += (f != 0);
    ok += (fgets(buf, sizeof(buf), f) != 0);
    ok += (strcmp(buf, "file smoke ok\n") == 0);
    ok += (feof(f) == 0);
    ok += (fseek(f, 6, 0) == 0);
    ok += (fgetc(f) == 's');
    ok += (fclose(f) == 0);
    if (ok == 9) {
        puts(buf);
        return 0;
    }
    return ok;
}
