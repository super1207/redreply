/* LeetCode 16: 最接近的三数之和 */

static void sort_numbers(int nums[], int size)
{
    int i;
    int j;
    int value;
    for (i = 1; i < size; i = i + 1) {
        value = nums[i];
        j = i - 1;
        while (j >= 0 && nums[j] > value) {
            nums[j + 1] = nums[j];
            j = j - 1;
        }
        nums[j + 1] = value;
    }
}

static int absolute(int value)
{
    if (value < 0) return -value;
    return value;
}

int three_sum_closest(int nums[], int size, int target)
{
    int best;
    int i;
    int left;
    int right;
    int sum;

    sort_numbers(nums, size);
    best = nums[0] + nums[1] + nums[2];
    for (i = 0; i < size - 2; i = i + 1) {
        left = i + 1;
        right = size - 1;
        while (left < right) {
            sum = nums[i] + nums[left] + nums[right];
            if (absolute(sum - target) < absolute(best - target)) best = sum;
            if (sum < target) left = left + 1;
            else if (sum > target) right = right - 1;
            else return sum;
        }
    }
    return best;
}

int main(void)
{
    int a[4] = {-1, 2, 1, -4};
    int b[3] = {0, 0, 0};
    int c[4] = {1, 1, 1, 0};
    if (three_sum_closest(a, 4, 1) != 2) return 1;
    if (three_sum_closest(b, 3, 1) != 0) return 2;
    if (three_sum_closest(c, 4, -100) != 2) return 3;
    return 42;
}
