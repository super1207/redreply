//! C89 编译器
//!
//! 流水线：
//! 预处理（行拼接/注释/token/宏）→ 语法分析 → 语义分析 → RISC-V 汇编生成
//!
//! 当前进度：
//! - [x] 预处理阶段 1~3：行拼接 + 删注释 + 切分预处理记号（[lexer]）
//! - [x] 预处理阶段 4：宏展开 / #include / 条件编译 / #line / #error / #pragma
//! - [ ] 语法分析（Parser）
//! - [ ] 代码生成（RISC-V 汇编）

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod expr_eval;
pub mod lexer;
pub mod parser;
pub mod pipeline;
pub mod preprocess;
pub mod semantic;
pub mod shadow;
pub mod stdlib;
pub mod types;
