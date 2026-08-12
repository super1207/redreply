# setjmp/longjmp（C89 7.6）
# jmp_buf 布局（int 槽）：[0]=sp [1]=s0（帧指针）[2]=ra
# 本代码生成器只使用 s0 作为被调用方保存寄存器，其余 s/f 保存寄存器未使用。

.globl setjmp
setjmp:
    sw sp, 0(a0)
    sw s0, 4(a0)
    sw ra, 8(a0)
    li a0, 0
    ret

.globl longjmp
longjmp:
    lw sp, 0(a0)
    lw s0, 4(a0)
    lw ra, 8(a0)
    mv a0, a1
    bnez a0, .Lsetjmp_val_ok
    li a0, 1
.Lsetjmp_val_ok:
    ret
