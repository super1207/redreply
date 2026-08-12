#include <errno.h>
#include <stdlib.h>
#include <string.h>

static int cmp_int(const void *a, const void *b) {
    int x = *(const int *)a;
    int y = *(const int *)b;
    return (x > y) - (x < y);
}

int main(void) {
    char *end;
    char *p;
    double value;
    int ok = 0;

    p = (char *)malloc(100);
    ok += (p != 0);
    strcpy(p, "heap");
    ok += (strcmp(p, "heap") == 0);
    free(p);

    p = (char *)calloc(10, 4);
    ok += (p != 0);
    ok += (p[0] == 0 && p[39] == 0);
    free(p);

    p = (char *)malloc(8);
    ok += (p != 0);
    p = (char *)realloc(p, 64);
    ok += (p != 0);
    free(p);

    ok += (atoi("  -123x") == -123);
    ok += (atol("42") == 42);
    ok += (strtol("ff", 0, 16) == 255);
    ok += (strtol("0x1A", 0, 0) == 26);
    ok += (strtol("077", 0, 0) == 63);
    ok += (strtol("101", 0, 2) == 5);
    ok += (EXIT_SUCCESS == 0 && EXIT_FAILURE != 0);
    value = strtod(" -12.5e2x", &end);
    ok += (value == -1250.0 && *end == 'x');
    value = strtod(".125", &end);
    ok += (value == 0.125 && *end == '\0');
    ok += (atof("2.5") == 2.5);
    errno = 0;
    value = strtod("9e999", &end);
    ok += (errno == ERANGE && value > 1e307);
    errno = 0;
    value = strtod("1e-999", &end);
    ok += (errno == ERANGE && value == 0.0);
    ok += (abs(-7) == 7);
    ok += (labs(-900000L) == 900000L);

    srand(1);
    ok += (rand() == 16838);
    ok += (rand() >= 0);

    {
        div_t d = div(17, 5);
        ok += (d.quot == 3 && d.rem == 2);
    }

    {
        int arr[5];
        int i;
        arr[0] = 4; arr[1] = 1; arr[2] = 3; arr[3] = 5; arr[4] = 2;
        qsort(arr, 5, sizeof(int), cmp_int);
        ok += (arr[0] == 1 && arr[1] == 2 && arr[2] == 3 && arr[3] == 4 && arr[4] == 5);
        {
            int key = 3;
            int *found = (int *)bsearch(&key, arr, 5, sizeof(int), cmp_int);
            ok += (found != 0 && *found == 3);
        }
        {
            int key = 9;
            int *missing = (int *)bsearch(&key, arr, 5, sizeof(int), cmp_int);
            ok += (missing == 0);
        }
    }

    if (ok == 26) {
        return 0;
    }
    return ok;
}
