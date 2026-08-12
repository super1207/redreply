# 循环求和示例：计算数组 1..10 的和，并打印
.data
arr:    .word 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
nl:     .byte 10      # 换行符

.text
main:
    la   t0, arr      # t0 = 数组首地址
    li   t1, 10       # t1 = 循环次数
    li   t2, 0        # t2 = 累加和

loop:
    lw   t3, 0(t0)    # t3 = *t0
    add  t2, t2, t3   # sum += t3
    addi t0, t0, 4    # 指针后移一个 word
    addi t1, t1, -1   # 计数减一
    bnez t1, loop     # 未到 0 则继续循环

    # 打印整数 sum
    mv   a0, t2
    li   a7, 1
    ecall

    # 打印换行
    la   a0, nl
    lb   a0, 0(a0)
    li   a7, 11
    ecall

    # 退出
    li   a0, 0
    li   a7, 10
    ecall
