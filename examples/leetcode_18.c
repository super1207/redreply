/* LeetCode 18: 四数之和 */

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

int four_sum(int nums[], int size, int target, int result[][4])
{
    int i;
    int j;
    int left;
    int right;
    int sum;
    int count;

    sort_numbers(nums, size);
    count = 0;
    for (i = 0; i < size - 3; i = i + 1) {
        if (i > 0 && nums[i] == nums[i - 1]) continue;
        for (j = i + 1; j < size - 2; j = j + 1) {
            if (j > i + 1 && nums[j] == nums[j - 1]) continue;
            left = j + 1;
            right = size - 1;
            while (left < right) {
                sum = nums[i] + nums[j] + nums[left] + nums[right];
                if (sum == target) {
                    result[count][0] = nums[i];
                    result[count][1] = nums[j];
                    result[count][2] = nums[left];
                    result[count][3] = nums[right];
                    count = count + 1;
                    left = left + 1;
                    right = right - 1;
                    while (left < right && nums[left] == nums[left - 1]) left = left + 1;
                    while (left < right && nums[right] == nums[right + 1]) right = right - 1;
                } else if (sum < target) left = left + 1;
                else right = right - 1;
            }
        }
    }
    return count;
}

int main(void)
{
    int a[6] = {1, 0, -1, 0, -2, 2};
    int b[5] = {2, 2, 2, 2, 2};
    int result[8][4];
    int count;
    count = four_sum(a, 6, 0, result);
    if (count != 3) return 1;
    if (result[0][0] != -2 || result[0][1] != -1 || result[0][2] != 1 || result[0][3] != 2) return 2;
    if (result[1][0] != -2 || result[1][1] != 0 || result[1][2] != 0 || result[1][3] != 2) return 3;
    if (result[2][0] != -1 || result[2][1] != 0 || result[2][2] != 0 || result[2][3] != 1) return 4;
    count = four_sum(b, 5, 8, result);
    if (count != 1 || result[0][0] != 2 || result[0][3] != 2) return 5;
    return 42;
}
