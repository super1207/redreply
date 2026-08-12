/* LeetCode 1: 两数之和 */

int two_sum(int nums[], int nums_size, int target, int result[])
{
    int i;
    int j;

    for (i = 0; i < nums_size; i = i + 1) {
        for (j = i + 1; j < nums_size; j = j + 1) {
            if (nums[i] + nums[j] == target) {
                result[0] = i;
                result[1] = j;
                return 2;
            }
        }
    }
    return 0;
}

static int check_case(int nums[], int nums_size, int target,
                      int expected0, int expected1)
{
    int result[2];
    int count;

    count = two_sum(nums, nums_size, target, result);
    return count == 2 && result[0] == expected0 && result[1] == expected1;
}

int main(void)
{
    int nums1[4] = {2, 7, 11, 15};
    int nums2[3] = {3, 2, 4};
    int nums3[2] = {3, 3};
    int nums4[5] = {-4, -1, 0, 3, 10};

    if (!check_case(nums1, 4, 9, 0, 1)) {
        return 1;
    }
    if (!check_case(nums2, 3, 6, 1, 2)) {
        return 2;
    }
    if (!check_case(nums3, 2, 6, 0, 1)) {
        return 3;
    }
    if (!check_case(nums4, 5, -5, 0, 1)) {
        return 4;
    }
    return 42;
}
