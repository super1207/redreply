int search_insert(int nums[],int size,int target){int left;int right;int mid;left=0;right=size;while(left<right){mid=left+(right-left)/2;if(nums[mid]<target)left=mid+1;else right=mid;}return left;}
int main(void){int a[4]={1,3,5,6};if(search_insert(a,4,5)!=2)return 1;if(search_insert(a,4,2)!=1)return 2;if(search_insert(a,4,7)!=4)return 3;if(search_insert(a,4,0)!=0)return 4;return 42;}
