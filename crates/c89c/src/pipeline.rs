//! High-level compiler pipeline used by the CLI and integration tests.

use crate::ast::Node;
use crate::codegen::generate_asm_checked_with_tokens_lib;
use crate::diagnostics::CompileResult;
use crate::lexer::Token;
use crate::parser::Parser;
use crate::preprocess::Preprocessor;
use crate::stdlib;
use crate::types::TypeTable;

pub struct ParsedProgram {
    pub items: Vec<Node>,
    pub types: TypeTable,
    pub tokens: Vec<Token>,
}

pub fn parse_file(path: &str) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    let tokens = preprocessor.run_file_diagnostic(path)?;
    parse_tokens(tokens)
}

pub fn parse_source(source: &str) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    let tokens = preprocessor.run_diagnostic(source)?;
    parse_tokens(tokens)
}

pub fn parse_source_with_lib(source: &str) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    preprocessor.set_embedded_headers(stdlib::embedded_lib_headers());
    let tokens = preprocessor.run_diagnostic(source)?;
    parse_tokens(tokens)
}

/// 解析内嵌的库源文件（虚拟文件名 + 编译期嵌入的内容）
pub fn parse_embedded_source(name: &str, source: &str) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    preprocessor.set_embedded_headers(stdlib::embedded_lib_headers());
    let tokens = preprocessor.run_named_diagnostic(name, source)?;
    parse_tokens(tokens)
}

/// 解析文件；`link_lib` 为 true 时启用内嵌库头文件。
pub fn parse_file_with_lib(path: &str, link_lib: bool) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    if link_lib {
        preprocessor.set_embedded_headers(stdlib::embedded_lib_headers());
    }
    let tokens = preprocessor.run_file_diagnostic(path)?;
    parse_tokens(tokens)
}

/// 解析文件并追加尖括号 include 搜索目录（dirs 在前，默认搜索在后）。
pub fn parse_file_with_dirs(
    path: &str,
    dirs: &[std::path::PathBuf],
) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    for dir in dirs {
        preprocessor.add_include_dir(&dir.to_string_lossy());
    }
    let tokens = preprocessor.run_file_diagnostic(path)?;
    parse_tokens(tokens)
}

/// 解析文件：用户 -I 目录 + 内嵌库头文件（CLI -l 用）
pub fn parse_file_with_dirs_and_lib(
    path: &str,
    dirs: &[std::path::PathBuf],
) -> CompileResult<ParsedProgram> {
    let mut preprocessor = Preprocessor::new();
    for dir in dirs {
        preprocessor.add_include_dir(&dir.to_string_lossy());
    }
    preprocessor.set_embedded_headers(stdlib::embedded_lib_headers());
    let tokens = preprocessor.run_file_diagnostic(path)?;
    parse_tokens(tokens)
}

fn parse_tokens(tokens: Vec<Token>) -> CompileResult<ParsedProgram> {
    // 保留 Eof token：文件末尾的错误（如缺少 '}'）需要定位到 Eof 所在行，
    // 而不是退化成 <unknown> 或最后一个真实 token。Parser 内部会跳过 Eof。
    let mut parser = Parser::new(tokens.clone());
    let mut items = parser.parse_program_diagnostic()?;
    // 把被遮蔽的局部变量重写为唯一名，使扁平符号表能区分不同作用域的同名变量。
    // 同时实施作用域约束检查（同作用域重复声明 / 块内声明位于语句之后）。
    crate::shadow::rename_shadowed_locals(&mut items)?;
    Ok(ParsedProgram {
        items,
        types: parser.tt,
        tokens,
    })
}

pub fn compile_source_to_asm(source: &str) -> CompileResult<String> {
    let program = parse_source(source)?;
    crate::semantic::validate_with_tokens(&program.items, "<source>", &program.tokens)?;
    generate_asm_checked_with_tokens_lib(
        &program.items,
        &program.types,
        "<source>",
        &program.tokens,
        None,
    )
}

pub fn compile_source_to_asm_with_lib(source: &str) -> CompileResult<String> {
    let program = parse_source_with_lib(source)?;
    compile_program_to_asm_with_lib(&program, "<source>")
}

pub fn compile_file_to_asm(path: &str) -> CompileResult<String> {
    let program = parse_file(path)?;
    crate::semantic::validate_with_tokens(&program.items, path, &program.tokens)?;
    generate_asm_checked_with_tokens_lib(
        &program.items,
        &program.types,
        path,
        &program.tokens,
        None,
    )
}

/// 带 c89lib 链接的编译：用户翻译单元 + 全部库源文件 + 库汇编片段。
///
/// 每个库源文件独立编译（自己的文件名与行号保留在诊断中），
/// 输出按顺序拼接：用户代码 → 库函数 → crt0/_start。
pub fn compile_file_to_asm_with_lib(path: &str) -> CompileResult<String> {
    let program = parse_file_with_lib(path, true)?;
    compile_program_to_asm_with_lib(&program, path)
}

fn compile_program_to_asm_with_lib(
    program: &ParsedProgram,
    source_name: &str,
) -> CompileResult<String> {
    crate::semantic::validate_with_tokens(&program.items, source_name, &program.tokens)?;
    let lib_symbols = stdlib::lib_symbol_set();
    let mut asm = generate_asm_checked_with_tokens_lib(
        &program.items,
        &program.types,
        source_name,
        &program.tokens,
        Some(&lib_symbols),
    )?;

    for (name, source) in stdlib::embedded_lib_sources() {
        let lib_program = parse_embedded_source(name, source)?;
        crate::semantic::validate_with_tokens(&lib_program.items, name, &lib_program.tokens)?;
        // 每个库翻译单元独立编译，局部标签从 1 开始；用文件名的前缀避免拼接后冲突
        let prefix = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>();
        let lib_asm = crate::codegen::generate_asm_checked_with_tokens_lib_prefix(
            &lib_program.items,
            &lib_program.types,
            name,
            &lib_program.tokens,
            Some(&lib_symbols),
            &prefix,
        )?;
        asm.push_str(&lib_asm);
    }

    for (_name, content) in stdlib::embedded_lib_asm() {
        asm.push_str(content);
    }
    Ok(asm)
}
