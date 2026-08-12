#define SQUARE(x) ((x)*(x))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define STR(x) #x
#define CAT(a,b) a##b
#define PI 3.14

int main(void) {
    int s = SQUARE(1+2);
    int m = MAX(3, 7);
    int xy = CAT(x, y);
    return s + m;
}
