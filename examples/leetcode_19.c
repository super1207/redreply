struct ListNode { int val; struct ListNode *next; };
struct ListNode *remove_nth_from_end(struct ListNode *head, int n) {
    struct ListNode dummy; struct ListNode *fast; struct ListNode *slow; int i;
    dummy.val=0; dummy.next=head; fast=&dummy; slow=&dummy;
    for(i=0;i<n;i=i+1) fast=fast->next;
    while(fast->next!=0){fast=fast->next;slow=slow->next;}
    slow->next=slow->next->next; return dummy.next;
}
int main(void){struct ListNode a[5];struct ListNode *r;int i;for(i=0;i<5;i=i+1){a[i].val=i+1;a[i].next=i<4?&a[i+1]:0;}r=remove_nth_from_end(a,2);if(r->val!=1||r->next->next->next->val!=5)return 1;r=remove_nth_from_end(r,4);if(r->val!=2)return 2;return 42;}
