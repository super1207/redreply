# M 扩展示例：乘法、除法、取余
.data
nl:     .byte 10      # 换行符

.text
main:
    # 6 × 7 = 42
    li   t0, 6
    li   t1, 7
    mul  t2, t0, t1
    mv   a0, t2
    li   a7, 1
    ecall
    call print_nl

    # 10 / 3 = 3
    li   t0, 10
    li   t1, 3
    div  t2, t0, t1
    mv   a0, t2
    li   a7, 1
    ecall
    call print_nl

    # 10 % 3 = 1
    rem  t2, t0, t1
    mv   a0, t2
    li   a7, 1
    ecall
    call print_nl

    # -7 / 2 = -3（有符号除法向零取整）
    li   t0, -7
    li   t1, 2
    div  t2, t0, t1
    mv   a0, t2
    li   a7, 1
    ecall
    call print_nl

    # 溢出测试：2147483647 * 2 取低 32 位
    li   t0, 2147483647
    li   t1, 2
    mul  t2, t0, t1
    mv   a0, t2
    li   a7, 35      # print hex
    ecall
    call print_nl

    # 退出
    li   a0, 0
    li   a7, 10
    ecall

# 子函数：打印换行
print_nl:
    la   a0, nl
    lb   a0, 0(a0)
    li   a7, 11
    ecall
    ret
