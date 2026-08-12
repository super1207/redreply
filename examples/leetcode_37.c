/* LeetCode 37: 解数独（位掩码 + 最少候选优先） */

static int bit_count(int value)
{
    int count;
    count = 0;
    while (value != 0) {
        value = value & (value - 1);
        count = count + 1;
    }
    return count;
}

static int solve_next(char board[][10], int row_mask[], int column_mask[],
                      int box_mask[])
{
    int row;
    int column;
    int box;
    int candidates;
    int candidate_count;
    int best_count;
    int best_row;
    int best_column;
    int best_box;
    int best_candidates;
    int digit;
    int bit;

    best_count = 10;
    best_row = -1;
    best_column = -1;
    best_box = -1;
    best_candidates = 0;

    for (row = 0; row < 9; row = row + 1) {
        for (column = 0; column < 9; column = column + 1) {
            if (board[row][column] == '.') {
                box = (row / 3) * 3 + column / 3;
                candidates = (~(row_mask[row] | column_mask[column] |
                                box_mask[box])) & 511;
                candidate_count = bit_count(candidates);
                if (candidate_count == 0) return 0;
                if (candidate_count < best_count) {
                    best_count = candidate_count;
                    best_row = row;
                    best_column = column;
                    best_box = box;
                    best_candidates = candidates;
                    if (best_count == 1) break;
                }
            }
        }
        if (best_count == 1) break;
    }

    if (best_row < 0) return 1;

    for (digit = 0; digit < 9; digit = digit + 1) {
        bit = 1 << digit;
        if ((best_candidates & bit) != 0) {
            board[best_row][best_column] = (char)('1' + digit);
            row_mask[best_row] = row_mask[best_row] | bit;
            column_mask[best_column] = column_mask[best_column] | bit;
            box_mask[best_box] = box_mask[best_box] | bit;

            if (solve_next(board, row_mask, column_mask, box_mask)) return 1;

            row_mask[best_row] = row_mask[best_row] & ~bit;
            column_mask[best_column] = column_mask[best_column] & ~bit;
            box_mask[best_box] = box_mask[best_box] & ~bit;
            board[best_row][best_column] = '.';
        }
    }
    return 0;
}

void solve_sudoku(char board[][10])
{
    int row_mask[9];
    int column_mask[9];
    int box_mask[9];
    int row;
    int column;
    int box;
    int digit;
    int bit;

    for (row = 0; row < 9; row = row + 1) {
        row_mask[row] = 0;
        column_mask[row] = 0;
        box_mask[row] = 0;
    }

    for (row = 0; row < 9; row = row + 1) {
        for (column = 0; column < 9; column = column + 1) {
            if (board[row][column] != '.') {
                digit = board[row][column] - '1';
                bit = 1 << digit;
                box = (row / 3) * 3 + column / 3;
                row_mask[row] = row_mask[row] | bit;
                column_mask[column] = column_mask[column] | bit;
                box_mask[box] = box_mask[box] | bit;
            }
        }
    }
    solve_next(board, row_mask, column_mask, box_mask);
}

int main(void)
{
    char board[9][10] = {
        "53..7....", "6..195...", ".98....6.", "8...6...3", "4..8.3..1",
        "7...2...6", ".6....28.", "...419..5", "....8..79"
    };
    char *expected[9] = {
        "534678912", "672195348", "198342567", "859761423", "426853791",
        "713924856", "961537284", "287419635", "345286179"
    };
    int row;
    int column;

    solve_sudoku(board);
    for (row = 0; row < 9; row = row + 1) {
        for (column = 0; column < 9; column = column + 1) {
            if (board[row][column] != expected[row][column]) return 1;
        }
    }
    return 42;
}
