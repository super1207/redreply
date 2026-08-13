//! 在内存中编译并运行 C89 源码。

use rv_core::assembler;
use rv_core::cpu::Cpu;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const MAX_STEPS: usize = 1000000000;
/// 累计输出上限：恶意/失控程序可在步数耗尽前输出撑爆宿主内存，
/// 超过上限即终止执行并给出结构化诊断。
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

/// 编译并运行 C89 源码，返回模拟器 fd 1/fd 2 的合并输出。
///
/// 编译、汇编或运行失败也以文本诊断返回。源码通过 c89lib 编译，
/// 因此可以直接 `#include <stdio.h>` 等标准库头文件。
pub fn run_c89(source: String) -> String {
    run_c89_with_cancel(source, Arc::new(AtomicBool::new(false)))
}

/// 编译并运行 C89 源码，可由调用方通过 `cancel` 请求停止模拟执行。
pub fn run_c89_with_cancel(source: String, cancel: Arc<AtomicBool>) -> String {
    if cancel.load(Ordering::Relaxed) {
        return "运行已取消\n".to_string();
    }
    let compilation = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || c89c::pipeline::compile_source_to_asm_with_lib(&source));
    let assembly = match compilation {
        Ok(handle) => match handle.join() {
            Ok(Ok(assembly)) => assembly,
            Ok(Err(error)) => return format!("{}\n", error),
            Err(_) => return "编译线程异常退出\n".to_string(),
        },
        Err(error) => return format!("无法创建编译线程: {}\n", error),
    };
    if cancel.load(Ordering::Relaxed) {
        return "运行已取消\n".to_string();
    }
    let program = match assembler::assemble(&assembly) {
        Ok(program) => program,
        Err(error) => return format!("汇编错误: {}\n", error),
    };
    let mut cpu = Cpu::new();
    if let Err(error) = cpu.load_program(&program) {
        return format!("程序加载错误: {}\n", error);
    }
    run_program(&mut cpu, &program, &cancel)
}

fn run_program(
    cpu: &mut Cpu,
    program: &rv_core::assembler::Program,
    cancel: &AtomicBool,
) -> String {
    let mut output = String::new();
    for _ in 0..MAX_STEPS {
        if cancel.load(Ordering::Relaxed) {
            output.push_str("运行已取消\n");
            return output;
        }
        match cpu.step(&program.instructions) {
            Ok(info) => {
                if output.len() + info.syscall_output.len() > MAX_OUTPUT {
                    output.push_str(&format!(
                        "运行错误: 输出超过上限 {} 字节\n",
                        MAX_OUTPUT
                    ));
                    return output;
                }
                output.push_str(&info.syscall_output);
                if cpu.exited || info.break_hit {
                    return output;
                }
            }
            Err(error) => {
                output.push_str(&format!("运行错误: {}\n", error));
                return output;
            }
        }
    }
    output.push_str(&format!("运行错误: 执行步数超过上限 {}\n", MAX_STEPS));
    output
}

#[cfg(test)]
mod tests {
    use super::run_c89;

    #[test]
    fn captures_stdout_and_stderr_together() {
        let output = run_c89(
            r#"#include <stdio.h>
int main(void) {
    printf("out");
    fprintf(stderr, "err");
    return 0;
}
"#
            .to_string(),
        );
        assert_eq!(output, "outerr");
    }

    #[test]
    fn returns_compile_diagnostics() {
        let output = run_c89("int main(void) { return missing; }".to_string());
        assert!(output.contains("error[semantic]"), "output: {}", output);
        assert!(output.contains("missing"), "output: {}", output);
    }

    #[test]
    fn supports_carriage_return_escape() {
        let output = run_c89(
            r#"#include <stdio.h>
int main(void) {
    printf("line\r\n");
    return 0;
}
"#
            .to_string(),
        );
        assert_eq!(output, "line\r\n");
    }

    #[test]
    fn preserves_utf8_string_output() {
        let output = run_c89(
            r#"#include <stdio.h>
int main(void) {
    printf("你好，C89！\n");
    return 0;
}
"#
            .to_string(),
        );
        assert_eq!(output, "你好，C89！\n");
    }
}
