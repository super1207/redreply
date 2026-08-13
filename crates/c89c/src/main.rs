//! c89c 命令行入口
//!
//! 用法：
//!   c89c <file.c>         预处理 + 解析 + 打印 AST
//!   c89c -S <file.c>      生成 RISC-V 汇编到 stdout
//!   c89c -S -l <file.c>   生成汇编并链接 c89lib（crt0 + 标准库）
//!   c89c -S -I <dir> <file.c> 追加尖括号 include 搜索目录

use c89c::codegen::generate_asm_checked_with_tokens_lib;
use c89c::pipeline::{parse_file, parse_file_with_dirs, parse_file_with_dirs_and_lib};
use c89c::semantic;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: c89c [-S] [-l] [-I <dir>] <file.c>");
        std::process::exit(1);
    }

    let mut to_asm = false;
    let mut link_lib = false;
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    let mut file = String::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-S" => to_asm = true,
            "-l" => link_lib = true,
            "-I" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("参数错误：-I 后缺少目录");
                    std::process::exit(1);
                }
                include_dirs.push(PathBuf::from(&args[i]));
            }
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("未知参数: {}", s);
                std::process::exit(1);
            }
            _ => {
                if file.is_empty() {
                    file = args[i].clone();
                } else {
                    eprintln!("参数错误：多余参数 '{}'", args[i]);
                    std::process::exit(1);
                }
            }
        }
        i += 1;
    }
    if file.is_empty() {
        eprintln!("参数错误：缺少 C 源文件");
        std::process::exit(1);
    }

    // 解析/生成全部在 64MB 栈的线程上进行。Windows 主线程默认栈只有 1MB，
    // 而 debug 构建的递归下降解析器栈帧很大，稍复杂的文件（如 -l 链接的
    // c89lib 源文件）会在主线程栈上溢出（0xC00000FD）。
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_cli(&file, &include_dirs, to_asm, link_lib))
        .expect("无法创建编译线程");
    handle.join().expect("编译线程崩溃");
}

fn run_cli(file: &str, include_dirs: &[PathBuf], to_asm: bool, link_lib: bool) {
    if link_lib {
        // -l 自动启用内嵌库头文件；用户 -I 目录仍按顺序搜索
        let program = match parse_file_with_dirs_and_lib(file, include_dirs) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("{}", error);
                std::process::exit(1);
            }
        };
        run(file, &program, to_asm, true);
    } else {
        let program = if include_dirs.is_empty() {
            match parse_file(file) {
                Ok(program) => program,
                Err(error) => {
                    eprintln!("{}", error);
                    std::process::exit(1);
                }
            }
        } else {
            match parse_file_with_dirs(file, include_dirs) {
                Ok(program) => program,
                Err(error) => {
                    eprintln!("{}", error);
                    std::process::exit(1);
                }
            }
        };
        run(file, &program, to_asm, false);
    }
}

fn run(file: &str, program: &c89c::pipeline::ParsedProgram, to_asm: bool, link_lib: bool) {
    if let Err(error) = semantic::validate_with_types(&program.items, file, &program.tokens, &program.types) {
        eprintln!("{}", error);
        std::process::exit(1);
    }
    if !to_asm {
        println!("解析成功，共 {} 个顶层节点:", program.items.len());
        for item in &program.items {
            println!("  {}", item);
        }
        return;
    }
    let result = if link_lib {
        c89c::pipeline::compile_file_to_asm_with_lib(file)
    } else {
        generate_asm_checked_with_tokens_lib(
            &program.items,
            &program.types,
            file,
            &program.tokens,
            None,
        )
    };
    match result {
        Ok(asm) => print!("{}", asm),
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    }
}
