/* LeetCode 11: 盛最多水的容器 */

int max_area(int height[], int size)
{
    int left;
    int right;
    int low;
    int area;
    int best;

    left = 0;
    right = size - 1;
    best = 0;
    while (left < right) {
        if (height[left] < height[right]) low = height[left];
        else low = height[right];
        area = low * (right - left);
        if (area > best) best = area;
        if (height[left] < height[right]) left = left + 1;
        else right = right - 1;
    }
    return best;
}

int main(void)
{
    int a[9] = {1, 8, 6, 2, 5, 4, 8, 3, 7};
    int b[2] = {1, 1};
    int c[3] = {1, 2, 1};
    if (max_area(a, 9) != 49) return 1;
    if (max_area(b, 2) != 1) return 2;
    if (max_area(c, 3) != 2) return 3;
    return 42;
}
