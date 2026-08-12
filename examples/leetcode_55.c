int can_jump(int nums[],int size){int farthest;int i;farthest=0;for(i=0;i<size;i=i+1){if(i>farthest)return 0;if(i+nums[i]>farthest)farthest=i+nums[i];}return 1;}
int main(void){int a[5]={2,3,1,1,4};int b[5]={3,2,1,0,4};int c[1]={0};if(!can_jump(a,5))return 1;if(can_jump(b,5))return 2;if(!can_jump(c,1))return 3;return 42;}
