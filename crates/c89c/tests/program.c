int add(int a, int b) {
    return a + b;
}

int main(void) {
    int x = 3;
    int y = 4;
    int sum = add(x, y);
    if (sum > 6) {
        return sum;
    }
    return 0;
}
