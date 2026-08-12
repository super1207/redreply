#define DEBUG 1
#define VERSION 2

#if DEBUG
#define MSG "debug mode"
#else
#define MSG "release mode"
#endif

int main(void) {
#if VERSION == 1
    int v = 1;
#elif VERSION == 2
    int v = 2;
#else
    int v = 0;
#endif
#if defined(DEBUG) && DEBUG
    char *m = MSG;
#endif
    return v;
}
