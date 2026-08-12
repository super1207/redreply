/* LeetCode 2: 两数相加 */

struct ListNode {
    int val;
    struct ListNode *next;
};

struct ListNode *add_two_numbers(struct ListNode *left, struct ListNode *right,
                                 struct ListNode nodes[], int *result_size)
{
    struct ListNode dummy;
    struct ListNode *tail;
    int carry;
    int sum;
    int count;

    dummy.val = 0;
    dummy.next = 0;
    tail = &dummy;
    carry = 0;
    count = 0;

    while (left != 0 || right != 0 || carry != 0) {
        sum = carry;
        if (left != 0) {
            sum = sum + left->val;
            left = left->next;
        }
        if (right != 0) {
            sum = sum + right->val;
            right = right->next;
        }

        nodes[count].val = sum % 10;
        nodes[count].next = 0;
        tail->next = &nodes[count];
        tail = &nodes[count];
        count = count + 1;
        carry = sum / 10;
    }

    *result_size = count;
    return dummy.next;
}

static void link_nodes(struct ListNode nodes[], int values[], int count)
{
    int i;
    for (i = 0; i < count; i = i + 1) {
        nodes[i].val = values[i];
        if (i + 1 < count) {
            nodes[i].next = &nodes[i + 1];
        } else {
            nodes[i].next = 0;
        }
    }
}

static int check_list(struct ListNode *node, int expected[], int count)
{
    int i;
    for (i = 0; i < count; i = i + 1) {
        if (node == 0 || node->val != expected[i]) {
            return 0;
        }
        node = node->next;
    }
    return node == 0;
}

int main(void)
{
    struct ListNode left1[3];
    struct ListNode right1[3];
    struct ListNode out1[4];
    struct ListNode left2[7];
    struct ListNode right2[4];
    struct ListNode out2[8];
    int left_values1[3] = {2, 4, 3};
    int right_values1[3] = {5, 6, 4};
    int expected1[3] = {7, 0, 8};
    int left_values2[7] = {9, 9, 9, 9, 9, 9, 9};
    int right_values2[4] = {9, 9, 9, 9};
    int expected2[8] = {8, 9, 9, 9, 0, 0, 0, 1};
    int size;
    struct ListNode *result;

    link_nodes(left1, left_values1, 3);
    link_nodes(right1, right_values1, 3);
    result = add_two_numbers(left1, right1, out1, &size);
    if (size != 3 || !check_list(result, expected1, 3)) {
        return 1;
    }

    link_nodes(left2, left_values2, 7);
    link_nodes(right2, right_values2, 4);
    result = add_two_numbers(left2, right2, out2, &size);
    if (size != 8 || !check_list(result, expected2, 8)) {
        return 2;
    }

    return 42;
}
