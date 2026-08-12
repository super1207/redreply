# Hello World 示例
.data
msg:    .asciiz "Hello, RISC-V!\n"

.text
main:
    la   a0, msg      # a0 = msg 地址
    li   a7, 4        # a7 = 4 → print string
    ecall

    li   a0, 0        # 退出码 0
    li   a7, 10       # a7 = 10 → exit
    ecall
