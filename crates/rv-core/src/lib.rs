//! RV32IMFD 指令执行器核心库
//!
//! 三个模块：
//! - [instr]：指令定义与反汇编显示
//! - [assembler]：RARS 风格汇编解析（两遍扫描）
//! - [cpu]：CPU 执行（寄存器/内存/ecall/浮点）
//!
//! 供 rv-cli（交互调试）与 c89 编译器（测试后端）共同使用。

pub mod assembler;
pub mod cpu;
pub mod instr;
