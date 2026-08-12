int valid_sudoku(char board[][10])
{
    int rows[9][9];
    int columns[9][9];
    int boxes[9][9];
    int r;
    int c;
    int d;
    int b;

    for (r = 0; r < 9; r = r + 1) {
        for (c = 0; c < 9; c = c + 1) {
            rows[r][c] = 0;
            columns[r][c] = 0;
            boxes[r][c] = 0;
        }
    }

    for (r = 0; r < 9; r = r + 1) {
        for (c = 0; c < 9; c = c + 1) {
            if (board[r][c] != '.') {
                d = board[r][c] - '1';
                b = (r / 3) * 3 + c / 3;
                if (rows[r][d] || columns[c][d] || boxes[b][d]) return 0;
                rows[r][d] = 1;
                columns[c][d] = 1;
                boxes[b][d] = 1;
            }
        }
    }
    return 1;
}

int main(void)
{
    char valid[9][10] = {
        "53..7....", "6..195...", ".98....6.", "8...6...3", "4..8.3..1",
        "7...2...6", ".6....28.", "...419..5", "....8..79"
    };
    char invalid[9][10] = {
        "83..7....", "6..195...", ".98....6.", "8...6...3", "4..8.3..1",
        "7...2...6", ".6....28.", "...419..5", "....8..79"
    };
    if (!valid_sudoku(valid)) return 1;
    if (valid_sudoku(invalid)) return 2;
    return 42;
}
