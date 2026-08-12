# F 扩展演示：浮点运算 + 打印（a7=2 print float 打印 f10）
.data
a:      .float 1.5
b:      .float 2.25
c:      .float 4.0
nl:     .byte 10

.text
main:
    # 1.5 + 2.25 = 3.75
    la   t0, a
    flw  f0, 0(t0)
    la   t0, b
    flw  f1, 0(t0)
    fadd.s f2, f0, f1
    fsgnj.s f10, f2, f2     # 拷贝 f2 → f10（fa0）
    li   a7, 2              # print float
    ecall
    call print_nl

    # 3 * 4.0 = 12.0（整数转浮点 + 乘法）
    li   t0, 3
    fcvt.s.w f3, t0
    la   t0, c
    flw  f4, 0(t0)
    fmul.s f5, f3, f4
    fsgnj.s f10, f5, f5
    li   a7, 2
    ecall
    call print_nl

    # 10.0 / 4.0 = 2.5
    li   t0, 10
    fcvt.s.w f6, t0
    fdiv.s f7, f6, f4
    fsgnj.s f10, f7, f7
    li   a7, 2
    ecall
    call print_nl

    # sqrt(16.0) = 4.0
    li   t0, 16
    fcvt.s.w f8, t0
    fsqrt.s f9, f8
    fsgnj.s f10, f9, f9
    li   a7, 2
    ecall
    call print_nl

    # 比较：1.5 < 2.25 → 1
    flt.s a0, f0, f1
    li   a7, 1
    ecall
    call print_nl

    # 退出
    li   a0, 0
    li   a7, 10
    ecall

print_nl:
    la   a0, nl
    lb   a0, 0(a0)
    li   a7, 11
    ecall
    ret
