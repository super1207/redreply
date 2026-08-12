//! CLI 黑盒诊断测试：直接运行 c89c 可执行文件，验证退出码与诊断输出。
//!
//! 覆盖：
//! - 参数错误（无参数 / 多余参数 / 缺文件）
//! - IO 错误（源文件不存在）
//! - 预处理错误（#error）
//! - 词法错误（无法识别的字符）
//! - 语法错误（缺右花括号）
//! - 语义错误（const 赋值 / 未声明标识符）
//! - codegen 错误（printf 参数不匹配 / 非字面量格式串 / 未定义外部函数）
//! - 成功路径（AST 列表输出、-S 汇编输出）

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// 在系统临时目录写入临时 .c 文件，返回路径。
fn write_case(name: &str, source: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("c89c_cli_diag_{}_{}.c", std::process::id(), name));
    fs::write(&path, source).expect("写入临时源文件失败");
    path
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_c89c"))
        .args(args)
        .output()
        .expect("启动 c89c 失败")
}

fn stdout_utf8(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_utf8(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("进程应正常退出")
}

/// 解析 stderr 中的结构化诊断：file:line:col: error[stage]: message
/// 返回 (文件, 行, 列, 阶段, 消息)。
fn parse_diagnostic(stderr: &str) -> Option<(String, u32, u32, String, String)> {
    let line = stderr.lines().next()?;
    let (loc, rest) = line.split_once(": error[")?;
    let (stage, message) = rest.split_once("]: ")?;
    let mut parts = loc.rsplitn(3, ':');
    let column: u32 = parts.next()?.parse().ok()?;
    let line_no: u32 = parts.next()?.parse().ok()?;
    let file = parts.next()?.to_string();
    Some((
        file,
        line_no,
        column,
        stage.to_string(),
        message.to_string(),
    ))
}

// ---------- 参数错误 ----------

#[test]
fn no_arguments_prints_usage_to_stderr_and_exits_1() {
    let output = run_cli(&[]);
    assert_eq!(exit_code(&output), 1);
    assert!(stdout_utf8(&output).is_empty(), "stdout 应为空");
    assert!(stderr_utf8(&output).contains("用法: c89c [-S] [-l] [-I <dir>] <file.c>"));
}

#[test]
fn extra_arguments_are_rejected() {
    let a = write_case("extra_a", "int x;");
    let b = write_case("extra_b", "int y;");
    let output = run_cli(&[a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    assert!(stderr_utf8(&output).contains("多余参数"));
}

#[test]
fn flag_without_file_is_rejected() {
    let output = run_cli(&["-S"]);
    assert_eq!(exit_code(&output), 1);
    assert!(stderr_utf8(&output).contains("缺少 C 源文件"));
}

// ---------- IO / 预处理 / 词法 / 语法 ----------

#[test]
fn missing_source_file_is_an_io_diagnostic() {
    let missing = std::env::temp_dir().join(format!(
        "c89c_cli_diag_does_not_exist_{}.c",
        std::process::id()
    ));
    let output = run_cli(&[missing.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    assert!(stdout_utf8(&output).is_empty(), "stdout 应为空");
    let (file, _line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "io");
    assert!(file.contains("does_not_exist"));
    assert!(message.contains("无法读取文件"), "消息: {}", message);
}

#[test]
fn preprocessor_error_reports_stage_and_location() {
    let path = write_case("pp_error", "#error custom boom\nint x;");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "preprocess");
    assert_eq!(file, path.to_str().unwrap());
    assert_eq!(line, 1);
    assert!(message.contains("custom boom"), "消息: {}", message);
}

#[test]
fn lexer_error_reports_stage_and_column() {
    let path = write_case("lex_error", "int x = @;");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, line, column, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "lex");
    assert_eq!(file, path.to_str().unwrap());
    assert_eq!(line, 1);
    assert_eq!(column, 9, "@ 应定位在第 9 列");
    assert!(message.contains("无法识别的字符"), "消息: {}", message);
}

#[test]
fn parse_error_reports_stage_and_line() {
    let path = write_case("parse_error", "int main(void) {\n    return 0;\n");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "parse");
    assert_eq!(file, path.to_str().unwrap());
    assert_eq!(line, 3, "缺少的右花括号应定位在第 3 行");
    assert!(message.contains("缺少右花括号"), "消息: {}", message);
}

// ---------- 语义 ----------

#[test]
fn const_assignment_is_a_semantic_diagnostic() {
    let path = write_case(
        "sem_const",
        "int main(void) {\n    const int x = 1;\n    x = 2;\n    return 0;\n}",
    );
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert_eq!(file, path.to_str().unwrap());
    assert_eq!(line, 3);
    assert!(message.contains("不能修改 const 对象"), "消息: {}", message);
}

#[test]
fn undeclared_identifier_is_a_semantic_diagnostic() {
    let path = write_case("sem_undecl", "int main(void) {\n    return missing;\n}");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert_eq!(file, path.to_str().unwrap());
    assert_eq!(line, 2);
    assert!(message.contains("未声明的标识符"), "消息: {}", message);
}

// ---------- codegen ----------

#[test]
fn undefined_external_function_is_a_codegen_diagnostic() {
    let path = write_case(
        "cg_undefined",
        "int helper(int value);\nint main(void) { return helper(1); }",
    );
    let output = run_cli(&["-S", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    assert!(stdout_utf8(&output).is_empty(), "失败时不应输出部分汇编");
    let (_file, _line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "codegen");
    assert!(message.contains("没有链接器"), "消息: {}", message);
}

// ---------- 成功路径 ----------

#[test]
fn valid_source_prints_ast_summary_without_asm_flag() {
    let path = write_case("ok_ast", "int main(void) { return 42; }");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
    let stdout = stdout_utf8(&output);
    assert!(stdout.contains("解析成功"), "stdout: {}", stdout);
    assert!(stdout.contains("顶层节点"), "stdout: {}", stdout);
    assert!(stderr_utf8(&output).is_empty(), "stderr 应为空");
}

#[test]
fn valid_source_with_dash_s_emits_assembly_to_stdout() {
    let path = write_case("ok_asm", "int main(void) { return 42; }");
    let output = run_cli(&["-S", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
    let stdout = stdout_utf8(&output);
    assert!(stdout.contains("main"), "汇编应包含 main 标签");
    assert!(stdout.contains("li"), "汇编应包含 li 指令");
    assert!(stderr_utf8(&output).is_empty(), "stderr 应为空");
}

#[test]
fn successful_diagnostics_do_not_leak_source_locations() {
    // 成功输出中不应出现 error[ 结构化诊断前缀
    let path = write_case("ok_clean", "int main(void) { return 0; }");
    let output = run_cli(&["-S", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
    assert!(
        !stdout_utf8(&output).contains("error["),
        "成功输出不应含诊断"
    );
    assert!(stderr_utf8(&output).is_empty());
}

// ---------- 缺陷修复回归测试 ----------

#[test]
fn switch_control_expression_must_be_integer() {
    let path = write_case(
        "sem_switch_float",
        "int main(void) { switch (2.5) { default: return 0; } }",
    );
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (file, _line, _col, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert_eq!(file, path.to_str().unwrap());
    assert!(message.contains("switch 控制表达式必须是整型"), "消息: {}", message);

    let path2 = write_case(
        "sem_switch_ptr",
        "int main(void) { char *p; switch (p) { default: return 0; } }",
    );
    let output = run_cli(&[path2.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, _l, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert!(message.contains("switch 控制表达式必须是整型"), "消息: {}", message);
}

#[test]
fn conditional_operator_requires_compatible_branches() {
    let path = write_case(
        "sem_cond_mixed",
        "int main(void) { int x = 1; char *p = x ? 1.0 : \"str\"; return 0; }",
    );
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, _l, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert!(message.contains("条件表达式两分支类型不兼容"), "消息: {}", message);

    // p ? p : 0（指针与空指针常量）合法
    let ok = write_case(
        "sem_cond_null",
        "int main(void) { char *p = 0; char *q = 1 ? p : 0; return q == 0; }",
    );
    let output = run_cli(&[ok.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
}

#[test]
fn string_literal_cannot_initialize_non_char_array() {
    let path = write_case("sem_str_array", "int a[4] = \"ab\";");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, _l, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert!(message.contains("字符串字面量只能初始化字符数组"), "消息: {}", message);

    // 字符数组仍可被字符串字面量初始化
    let ok = write_case("sem_str_array_ok", "char a[4] = \"ab\";");
    let output = run_cli(&[ok.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
}

#[test]
fn duplicate_local_declaration_in_same_scope_is_rejected() {
    let path = write_case(
        "sem_dup_decl",
        "int main(void) {\n    int x = 1;\n    int x = 2;\n    return x;\n}",
    );
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, line, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert_eq!(line, 3, "重复声明应定位在第二次声明行");
    assert!(message.contains("重复声明"), "消息: {}", message);
}

#[test]
fn same_name_in_nested_scope_is_allowed() {
    let ok = write_case(
        "sem_nested_ok",
        "int main(void) { int x = 1; { int x = 2; if (x != 2) return 1; } return x; }",
    );
    let output = run_cli(&[ok.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
}

#[test]
fn declaration_after_statement_in_block_is_rejected() {
    let path = write_case(
        "sem_decl_after_stmt",
        "int main(void) {\n    int x;\n    x = 5;\n    int y;\n    return y;\n}",
    );
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, line, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "semantic");
    assert_eq!(line, 4, "违规声明应定位在第 4 行");
    assert!(message.contains("块内声明必须位于所有语句之前"), "消息: {}", message);
}

#[test]
fn declaration_at_nested_block_start_is_allowed() {
    let ok = write_case(
        "sem_decl_block_start",
        "int main(void) { int x = 5; { int y = x + 1; x = y; } return x; }",
    );
    let output = run_cli(&[ok.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0);
}

#[test]
fn array_size_function_call_is_still_rejected() {
    let path = write_case("parse_array_call", "int f(void); int a[f()];");
    let output = run_cli(&[path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let (_f, _l, _c, stage, message) =
        parse_diagnostic(&stderr_utf8(&output)).expect("应输出结构化诊断");
    assert_eq!(stage, "parse");
    assert!(message.contains("数组大小错误"), "消息: {}", message);
}
