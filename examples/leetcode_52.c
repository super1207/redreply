static int search(int n,int row,int columns,int diag_left,int diag_right){int available;int bit;int count;if(row==n)return 1;available=((1<<n)-1)&~(columns|diag_left|diag_right);count=0;while(available!=0){bit=available&-available;available=available-bit;count=count+search(n,row+1,columns|bit,(diag_left|bit)<<1,(diag_right|bit)>>1);}return count;}
int total_n_queens(int n){return search(n,0,0,0,0);}
int main(void){if(total_n_queens(4)!=2)return 1;if(total_n_queens(1)!=1)return 2;if(total_n_queens(5)!=10)return 3;if(total_n_queens(8)!=92)return 4;return 42;}
