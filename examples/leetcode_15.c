/* LeetCode 15: 三数之和 */

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

int three_sum(int nums[], int size, int result[][3])
{
    int i;
    int left;
    int right;
    int sum;
    int count;

    sort_numbers(nums, size);
    count = 0;
    for (i = 0; i < size - 2; i = i + 1) {
        if (i > 0 && nums[i] == nums[i - 1]) continue;
        left = i + 1;
        right = size - 1;
        while (left < right) {
            sum = nums[i] + nums[left] + nums[right];
            if (sum == 0) {
                result[count][0] = nums[i];
                result[count][1] = nums[left];
                result[count][2] = nums[right];
                count = count + 1;
                left = left + 1;
                right = right - 1;
                while (left < right && nums[left] == nums[left - 1]) left = left + 1;
                while (left < right && nums[right] == nums[right + 1]) right = right - 1;
            } else if (sum < 0) {
                left = left + 1;
            } else {
                right = right - 1;
            }
        }
    }
    return count;
}

int main(void)
{
    int a[6] = {-1, 0, 1, 2, -1, -4};
    int b[4] = {0, 0, 0, 0};
    int out[4][3];
    int count;

    count = three_sum(a, 6, out);
    if (count != 2) return 1;
    if (out[0][0] != -1 || out[0][1] != -1 || out[0][2] != 2) return 2;
    if (out[1][0] != -1 || out[1][1] != 0 || out[1][2] != 1) return 3;
    count = three_sum(b, 4, out);
    if (count != 1 || out[0][0] != 0 || out[0][1] != 0 || out[0][2] != 0) return 4;
    return 42;
}
