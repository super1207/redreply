/* LeetCode 4: 寻找两个正序数组的中位数 */

double find_median_sorted_arrays(int left[], int left_size,
                                 int right[], int right_size)
{
    int total;
    int first_target;
    int second_target;
    int i;
    int j;
    int position;
    int value;
    int first_value;
    int second_value;

    total = left_size + right_size;
    first_target = (total - 1) / 2;
    second_target = total / 2;
    i = 0;
    j = 0;
    first_value = 0;
    second_value = 0;

    for (position = 0; position <= second_target; position = position + 1) {
        if (i < left_size && (j >= right_size || left[i] <= right[j])) {
            value = left[i];
            i = i + 1;
        } else {
            value = right[j];
            j = j + 1;
        }
        if (position == first_target) {
            first_value = value;
        }
        if (position == second_target) {
            second_value = value;
        }
    }

    return ((double)first_value + (double)second_value) / 2.0;
}

int main(void)
{
    int a1[2] = {1, 3};
    int b1[1] = {2};
    int a2[2] = {1, 2};
    int b2[2] = {3, 4};
    int a3[2] = {0, 0};
    int b3[2] = {0, 0};
    int one[1] = {1};

    if (find_median_sorted_arrays(a1, 2, b1, 1) != 2.0) return 1;
    if (find_median_sorted_arrays(a2, 2, b2, 2) != 2.5) return 2;
    if (find_median_sorted_arrays(a3, 2, b3, 2) != 0.0) return 3;
    if (find_median_sorted_arrays(0, 0, one, 1) != 1.0) return 4;
    return 42;
}
