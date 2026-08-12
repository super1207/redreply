# D 扩展演示：双精度浮点运算（a7=3 print double 打印 f10）
.data
a:      .double 0.1
b:      .double 0.2
nl:     .byte 10

.text
main:
    # 0.1 + 0.2 = 0.30000000000000004（只有 double 能看到这个精度！）
    la   t0, a
    fld  f0, 0(t0)
    la   t0, b
    fld  f1, 0(t0)
    fadd.d f2, f0, f1
    fsgnj.d f10, f2, f2
    li   a7, 3              # print double
    ecall
    call print_nl

    # 整数 → double：42 → 42.0
    li   t0, 42
    fcvt.d.w f3, t0
    fsgnj.d f10, f3, f3
    li   a7, 3
    ecall
    call print_nl

    # double → int：3.99 → 3（向零截断）
    li   t0, 399
    fcvt.d.w f4, t0
    li   t1, 100
    fcvt.d.w f5, t1
    fdiv.d f6, f4, f5
    fcvt.w.d a0, f6
    li   a7, 1
    ecall
    call print_nl

    # double → float → double 精度损失演示
    fcvt.s.d f7, f0        # f7 = (float)0.1（损失精度）
    fcvt.d.s f10, f7       # 转回 double：0.10000000149011612
    li   a7, 3
    ecall
    call print_nl

    # 开方：sqrt(2.0)
    li   t0, 2
    fcvt.d.w f8, t0
    fsqrt.d f9, f8
    fsgnj.d f10, f9, f9
    li   a7, 3
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
