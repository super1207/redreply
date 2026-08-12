//! 汇编解析器：汇编文本 → 指令序列 + 数据段
//!
//! 支持 RARS 风格语法：
//!   - 段切换 .data / .text
//!   - 数据指令 .word .half .byte .asciiz .ascii .space .align
//!   - 标签 label:（可独立一行，也可与指令同行）
//!   - RV32I 全集指令 + 常用伪指令
//!   - 注释 #（字符串内的 # 不算注释）
//!   - 立即数：十进制 / 0x 十六进制
//!   - 访存偏移：imm(reg)

use crate::instr::{parse_reg, Instruction, Op};
use std::collections::HashMap;

/// 指令区基址（PC 从这里开始）
pub const TEXT_BASE: u32 = 0x0000_0000;
/// 数据区基址（.data 从这里开始）
pub const DATA_BASE: u32 = 0x0010_0000;
/// 汇编器允许引用的数据地址上界（与 CPU 的 8 MiB 内存一致）。
const ADDRESS_SPACE_SIZE: u32 = 8 * 1024 * 1024;

fn checked_data_end(address: u32, size: usize) -> Result<u32, String> {
    let size = u32::try_from(size).map_err(|_| "数据项大小超出 32 位地址空间".to_string())?;
    let end = address
        .checked_add(size)
        .ok_or_else(|| format!("数据段地址溢出 @0x{:08x}", address))?;
    if end > ADDRESS_SPACE_SIZE {
        return Err(format!("数据段越界: 末尾 0x{:08x}", end));
    }
    Ok(end)
}

#[derive(Debug)]
pub struct Program {
    /// 展开后的指令序列（地址 = TEXT_BASE + i * 4）
    pub instructions: Vec<Instruction>,
    /// 与指令一一对应的原始汇编文本（供显示）
    pub text_lines: Vec<String>,
    /// 数据段：(起始地址, 字节)
    pub data_segments: Vec<(u32, Vec<u8>)>,
    /// 数据段末尾（供 sbrk 堆从这里开始）
    pub data_end: u32,
    /// 入口地址（优先 main / _start，否则 .text 首条）
    pub entry: u32,
    /// 符号表（标签 → 地址）
    pub symbols: HashMap<String, u32>,
}

/// 去掉注释：跳过 # 及之后内容（忽略字符串内的 #）
fn strip_comment(line: &str) -> String {
    let mut in_str = false;
    let mut esc = false;
    let mut out = String::new();
    for c in line.chars() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '#' {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

/// 解码 C 风格字符串转义（\" \\ \n \r \t \a \b \f \v \'、
/// 八进制 \ddd（1~3 位）、十六进制 \xHH（任意位数，截断低 8 位）。
/// 入参为不带引号的字符串内部内容，返回解码后的字节序列。
pub fn decode_string_escapes(inner: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push(b'\n'),
                Some('r') => out.push(b'\r'),
                Some('t') => out.push(b'\t'),
                Some('a') => out.push(0x07),
                Some('b') => out.push(0x08),
                Some('f') => out.push(0x0c),
                Some('v') => out.push(0x0b),
                Some('\\') => out.push(b'\\'),
                Some('"') => out.push(b'"'),
                Some('\'') => out.push(b'\''),
                Some('x') => {
                    // \x 后跟一个或多个十六进制数字，值截断为低 8 位
                    let mut value: u32 = 0;
                    let mut any = false;
                    while let Some(&h) = chars.peek() {
                        if let Some(d) = h.to_digit(16) {
                            value = value.wrapping_mul(16).wrapping_add(d);
                            any = true;
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if !any {
                        return Err("\\x 后缺少十六进制数字".to_string());
                    }
                    out.push((value & 0xFF) as u8);
                }
                Some(d @ '0'..='7') => {
                    // 八进制转义：1~3 位八进制数字
                    let mut value: u32 = d.to_digit(8).unwrap();
                    for _ in 0..2 {
                        if let Some(&next) = chars.peek() {
                            if let Some(digit) = next.to_digit(8) {
                                value = value * 8 + digit;
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    out.push((value & 0xFF) as u8);
                }
                Some(other) => return Err(format!("不支持的转义: \\{}", other)),
                None => return Err("字符串以反斜杠结尾".to_string()),
            }
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Ok(out)
}

/// 解析字符串字面量（支持 \" \\ \n \r \t \0、八进制 \ddd、十六进制 \xHH）
fn parse_string_literal(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.starts_with('"') || !s.ends_with('"') {
        return Err(format!("字符串字面量格式错误: {}", s));
    }
    decode_string_escapes(&s[1..s.len() - 1])
}

/// 解析整数（十进制 / 十六进制 0x / 负数）
fn parse_int(s: &str) -> Result<i32, String> {
    let s = s.trim();
    let (neg, body) = if let Some(b) = s.strip_prefix('-') {
        (true, b)
    } else {
        (false, s)
    };
    let v = if let Some(h) = body.strip_prefix("0x") {
        i64::from_str_radix(h, 16)
    } else if let Some(h) = body.strip_prefix("0X") {
        i64::from_str_radix(h, 16)
    } else {
        body.parse::<i64>()
    };
    let v = v.map_err(|_| format!("立即数格式错误: {}", s))?;
    let v = if neg { -v } else { v };
    if v < i32::MIN as i64 || v > u32::MAX as i64 {
        return Err(format!("立即数超出 32 位范围: {}", s));
    }
    Ok(v as i32)
}

/// 解析浮点寄存器名 f0~f31
fn parse_freg(s: &str) -> Result<u8, String> {
    let s = s.trim().to_lowercase();
    if let Some(n) = s.strip_prefix('f') {
        if let Ok(v) = n.parse::<u8>() {
            if v <= 31 {
                return Ok(v);
            }
        }
    }
    Err(format!("未知浮点寄存器: {}", s))
}

/// 拆操作数：按逗号分割（trim）
fn split_operands(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}
// ---- 伪指令展开计数（第一遍布局用，必须与第二遍展开规则一致）----
fn expansion_len(mnem: &str, operands: &[String]) -> usize {
    match mnem {
        "li" => {
            // li 的立即数若不是合法数字字面量（标签符号，含前向引用），
            // 一律按 2 条指令估：第二遍对标签恒展开为 lui+addi，若这里
            // 低估成 1 条，其后所有 .text 标签地址会错 4 字节。
            let imm = if operands.len() >= 2 {
                match parse_int(&operands[1]) {
                    Ok(v) => v as i64,
                    Err(_) => return 2,
                }
            } else {
                0
            };
            if (-2048..=2047).contains(&imm) {
                1
            } else {
                2
            }
        }
        "la" => 2,
        _ => 1,
    }
}

/// 第一遍：按行解析出 (段, 行内标签, 指令文本)
struct RawLine {
    section: String,
    label: Option<String>,
    text: String,
}

fn preprocess(source: &str) -> Result<(Vec<RawLine>, Vec<String>), String> {
    let mut lines = Vec::new();
    let mut order = Vec::new(); // 记录出现顺序，供 REPL 用
    let mut section = "text".to_string();
    for raw in source.lines() {
        let line = strip_comment(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 段切换
        if let Some(rest) = trimmed.strip_prefix('.') {
            if rest == "text" || rest == "data" {
                section = rest.to_string();
                continue;
            }
        }
        let mut label = None;
        let mut text = trimmed.to_string();
        // 行首标签：label: 或 label: 指令
        // 扫描时跟踪引号与反斜杠转义状态，字符串字面量内的冒号
        // （如 .asciiz "http://example.com"）不算标签分隔符。
        let mut in_str = false;
        let mut esc = false;
        let mut label_end = None;
        for (i, c) in text.char_indices() {
            if in_str {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    in_str = false;
                }
            } else if c == '"' {
                in_str = true;
            } else if c == ':' {
                label_end = Some(i);
                break;
            }
        }
        if let Some(pos) = label_end {
            let head = text[..pos].trim();
            if !head.is_empty() && !head.chars().next().unwrap().is_ascii_digit() {
                label = Some(head.to_string());
                text = text[pos + 1..].trim().to_string();
            }
        }
        if text.is_empty() && label.is_none() {
            continue;
        }
        lines.push(RawLine {
            section: section.clone(),
            label,
            text,
        });
        order.push(lines.len() - 1);
    }
    Ok((lines, order.iter().map(|&i| format!("{}", i)).collect()))
}

/// 汇编主入口
pub fn assemble(source: &str) -> Result<Program, String> {
    let (lines, _order) = preprocess(source)?;

    // ===== 第一遍：布局 + 收集符号 =====
    let mut symbols: HashMap<String, u32> = HashMap::new();
    let mut text_addr = TEXT_BASE; // 指令按 4 字节递增
    let mut data_addr = DATA_BASE;
    let mut data_bytes: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut data_relocations: Vec<(usize, String, usize)> = Vec::new();
    let mut first_text_label: Option<String> = None;

    for l in &lines {
        if let Some(lb) = &l.label {
            let addr = if l.section == "text" {
                text_addr
            } else {
                data_addr
            };
            if symbols.insert(lb.clone(), addr).is_some() {
                return Err(format!("重复标签: {}", lb));
            }
        }
        if l.section == "text" {
            if l.text.is_empty() {
                continue;
            }
            // 数据指令不会出现在 text 段，这里按指令处理
            let parts: Vec<&str> = l.text.splitn(2, char::is_whitespace).collect();
            let mnem = parts[0].to_lowercase();
            if mnem == ".globl" {
                continue;
            } // 符号导出指令忽略
            let operands = if parts.len() > 1 {
                split_operands(parts[1])
            } else {
                vec![]
            };
            if mnem.starts_with('.') {
                return Err(format!("数据指令 {} 不能出现在 .text 段", mnem));
            }
            let n = expansion_len(&mnem, &operands);
            let byte_len = n
                .checked_mul(4)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| "文本段大小溢出".to_string())?;
            text_addr = text_addr
                .checked_add(byte_len)
                .ok_or_else(|| "文本段地址溢出".to_string())?;
            if first_text_label.is_none() {
                first_text_label = Some(l.text.clone());
            }
        } else {
            // 数据段
            if l.text.is_empty() {
                continue;
            }
            let parts: Vec<&str> = l.text.splitn(2, char::is_whitespace).collect();
            let mnem = parts[0];
            let args = if parts.len() > 1 { parts[1].trim() } else { "" };
            match mnem {
                ".float" => {
                    // 单精度浮点常量（4 字节）
                    for v in args.split(',') {
                        let v = v.trim();
                        if v.is_empty() {
                            continue;
                        }
                        let val = v
                            .parse::<f32>()
                            .map_err(|_| format!("浮点数格式错误: {}", v))?;
                        let bytes = val.to_bits().to_le_bytes().to_vec();
                        let end = checked_data_end(data_addr, bytes.len())?;
                        data_bytes.push((data_addr, bytes));
                        data_addr = end;
                    }
                }
                ".double" => {
                    // 双精度浮点常量（8 字节）
                    for v in args.split(',') {
                        let v = v.trim();
                        if v.is_empty() {
                            continue;
                        }
                        let val = v
                            .parse::<f64>()
                            .map_err(|_| format!("浮点数格式错误: {}", v))?;
                        let bytes = val.to_bits().to_le_bytes().to_vec();
                        let end = checked_data_end(data_addr, bytes.len())?;
                        data_bytes.push((data_addr, bytes));
                        data_addr = end;
                    }
                }
                ".word" | ".half" | ".byte" => {
                    let size = if mnem == ".word" {
                        4
                    } else if mnem == ".half" {
                        2
                    } else {
                        1
                    };
                    for v in args.split(',') {
                        let v = v.trim();
                        if v.is_empty() {
                            continue;
                        }
                        let bytes = if let Ok(value) = parse_int(v) {
                            (value as u32).to_le_bytes()[..size].to_vec()
                        } else if v
                            .chars()
                            .all(|c| c == '_' || c == '.' || c.is_ascii_alphanumeric())
                        {
                            let index = data_bytes.len();
                            data_relocations.push((index, v.to_owned(), size));
                            vec![0; size]
                        } else {
                            return Err(format!("数据初值格式错误: {}", v));
                        };
                        let end = checked_data_end(data_addr, bytes.len())?;
                        data_bytes.push((data_addr, bytes));
                        data_addr = end;
                    }
                }
                ".asciiz" | ".ascii" => {
                    let mut bytes = parse_string_literal(args)?;
                    if mnem == ".asciiz" {
                        bytes.push(0);
                    }
                    let end = checked_data_end(data_addr, bytes.len())?;
                    data_bytes.push((data_addr, bytes));
                    data_addr = end;
                }
                ".space" => {
                    let count = parse_int(args)?;
                    if count < 0 {
                        return Err(format!(".space 大小不能为负数: {}", count));
                    }
                    let count = count as usize;
                    let end = checked_data_end(data_addr, count)?;
                    data_bytes.push((data_addr, vec![0u8; count]));
                    data_addr = end;
                }
                ".align" => {
                    let exponent = parse_int(args)?;
                    if !(0..=16).contains(&exponent) {
                        return Err(format!(".align 指数必须在 0..=16: {}", exponent));
                    }
                    let alignment = 1u32 << exponent;
                    let aligned = data_addr
                        .checked_add(alignment - 1)
                        .map(|value| value & !(alignment - 1))
                        .ok_or_else(|| "数据段对齐地址溢出".to_string())?;
                    checked_data_end(aligned, 0)?;
                    data_addr = aligned;
                }
                _ => return Err(format!("不支持的数据指令: {}", mnem)),
            }
        }
    }

    for (index, symbol, size) in data_relocations {
        let value = symbols
            .get(&symbol)
            .copied()
            .ok_or_else(|| format!("数据初值引用了未知符号: {}", symbol))?;
        data_bytes[index]
            .1
            .copy_from_slice(&value.to_le_bytes()[..size]);
    }

    // 收集 .text 行（第二遍用）
    let text_lines_raw: Vec<&RawLine> = lines
        .iter()
        .filter(|l| l.section == "text" && !l.text.is_empty() && !l.text.starts_with(".globl"))
        .collect();

    // ===== 第二遍：生成指令 =====
    let mut instructions: Vec<Instruction> = Vec::new();
    let mut text_lines: Vec<String> = Vec::new();
    let mut cur_addr = TEXT_BASE;

    for l in &text_lines_raw {
        let parts: Vec<&str> = l.text.splitn(2, char::is_whitespace).collect();
        let mnem = parts[0].to_lowercase();
        let operands = if parts.len() > 1 {
            split_operands(parts[1])
        } else {
            vec![]
        };
        let raw_text = l.text.clone();
        let expanded = parse_instruction(&mnem, &operands, cur_addr, &symbols)?;
        for ins in expanded {
            instructions.push(ins);
            text_lines.push(raw_text.clone());
            cur_addr = cur_addr.wrapping_add(4);
        }
    }

    let data_end = data_addr;
    // 入口：优先 _start（crt0 启动器），其次 main，否则 .text 首条
    let entry = symbols
        .get("_start")
        .copied()
        .or_else(|| symbols.get("main").copied())
        .unwrap_or(TEXT_BASE);

    Ok(Program {
        instructions,
        text_lines,
        data_segments: data_bytes,
        data_end,
        entry,
        symbols,
    })
}
// ---- 指令解析与伪指令展开 ----

/// 伪指令 + 真指令 → 若干条 Instruction
fn parse_instruction(
    mnem: &str,
    ops: &[String],
    addr: u32,
    symbols: &HashMap<String, u32>,
) -> Result<Vec<Instruction>, String> {
    macro_rules! op {
        ($index:expr) => {
            match ops.get($index) {
                Some(value) => value,
                None => return Err(format!("指令 {} 缺少第 {} 个操作数", mnem, $index + 1)),
            }
        };
    }

    // ---- 伪指令 ----
    match mnem {
        "nop" => return Ok(vec![Instruction::r2i(Op::Addi, 0, 0, 0)]),
        "li" => {
            let rd = parse_reg(op!(0))?;
            let value = op!(1);
            if let Ok(imm) = parse_int(value) {
                return Ok(expand_li(rd, imm));
            }
            // 标签符号：恒展开为 lui+addi 两条（与第一遍 expansion_len 一致，
            // 避免小地址标签导致两遍布局不一致）
            let addr = symbols
                .get(value.as_str())
                .copied()
                .ok_or_else(|| format!("未知标签: {}", value))?;
            let u = addr as u32;
            let hi = ((u.wrapping_add(0x800)) >> 12) & 0xFFFFF;
            let lo = ((u << 20) as i32) >> 20;
            return Ok(vec![
                Instruction::r1i(Op::Lui, rd, hi as i32),
                Instruction::r2i(Op::Addi, rd, rd, lo),
            ]);
        }
        "la" => {
            let rd = parse_reg(op!(0))?;
            let label = op!(1);
            let addr = symbols
                .get(label.as_str())
                .copied()
                .ok_or_else(|| format!("未知标签: {}", label))?;
            // la 恒为 lui + addi 两条（与第一遍 expansion_len 一致，
            // 避免小地址标签导致两遍布局不一致）
            let u = addr as u32;
            let hi = ((u.wrapping_add(0x800)) >> 12) & 0xFFFFF;
            let lo = ((u << 20) as i32) >> 20;
            return Ok(vec![
                Instruction::r1i(Op::Lui, rd, hi as i32),
                Instruction::r2i(Op::Addi, rd, rd, lo),
            ]);
        }
        "mv" => {
            return Ok(vec![Instruction::r2i(
                Op::Addi,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                0,
            )])
        }
        "ret" => {
            return Ok(vec![Instruction {
                op: Op::Jalr,
                rd: 0,
                rs1: 1,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }])
        }
        "j" => {
            let target = resolve_label(op!(0), addr, symbols)?;
            return Ok(vec![Instruction::r2(Op::Jal, 0, 0, target)]);
        }
        "jal" => {
            if ops.len() == 1 {
                let target = resolve_label(op!(0), addr, symbols)?;
                return Ok(vec![Instruction {
                    op: Op::Jal,
                    rd: 1,
                    rs1: 0,
                    rs2: 0,
                    rs3: 0,
                    imm: target,
                }]);
            }
            // jal rd, label
        }
        "call" => {
            let target = resolve_label(op!(0), addr, symbols)?;
            return Ok(vec![Instruction {
                op: Op::Jal,
                rd: 1,
                rs1: 0,
                rs2: 0,
                rs3: 0,
                imm: target,
            }]);
        }
        "beqz" => {
            return Ok(vec![Instruction::r2(
                Op::Beq,
                parse_reg(op!(0))?,
                0,
                resolve_label(op!(1), addr, symbols)?,
            )])
        }
        "bnez" => {
            return Ok(vec![Instruction::r2(
                Op::Bne,
                parse_reg(op!(0))?,
                0,
                resolve_label(op!(1), addr, symbols)?,
            )])
        }
        "bgt" => {
            return Ok(vec![Instruction::r2(
                Op::Blt,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_label(op!(2), addr, symbols)?,
            )])
        }
        "ble" => {
            return Ok(vec![Instruction::r2(
                Op::Bge,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_label(op!(2), addr, symbols)?,
            )])
        }
        "bgtu" => {
            return Ok(vec![Instruction::r2(
                Op::Bltu,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_label(op!(2), addr, symbols)?,
            )])
        }
        "bleu" => {
            return Ok(vec![Instruction::r2(
                Op::Bgeu,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_label(op!(2), addr, symbols)?,
            )])
        }
        "not" => {
            return Ok(vec![Instruction::r2i(
                Op::Xori,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                -1,
            )])
        }
        "neg" => {
            return Ok(vec![Instruction::r3(
                Op::Sub,
                parse_reg(op!(0))?,
                0,
                parse_reg(op!(1))?,
            )])
        }
        "seqz" => {
            return Ok(vec![Instruction::r2i(
                Op::Sltiu,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                1,
            )])
        }
        "snez" => {
            return Ok(vec![Instruction::r3(
                Op::Sltu,
                parse_reg(op!(0))?,
                0,
                parse_reg(op!(1))?,
            )])
        }
        "sgtz" => {
            return Ok(vec![Instruction::r3(
                Op::Slt,
                parse_reg(op!(0))?,
                0,
                parse_reg(op!(1))?,
            )])
        }
        "sltz" => {
            return Ok(vec![Instruction::r3(
                Op::Slt,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                0,
            )])
        }
        _ => {}
    }

    // ---- 真指令 ----
    let mk = |op: Op| Instruction::new(op);
    match mnem {
        // R 型
        "add" => Ok(vec![Instruction::r3(
            Op::Add,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "sub" => Ok(vec![Instruction::r3(
            Op::Sub,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "sll" => Ok(vec![Instruction::r3(
            Op::Sll,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "slt" => Ok(vec![Instruction::r3(
            Op::Slt,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "sltu" => Ok(vec![Instruction::r3(
            Op::Sltu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "xor" => Ok(vec![Instruction::r3(
            Op::Xor,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "srl" => Ok(vec![Instruction::r3(
            Op::Srl,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "sra" => Ok(vec![Instruction::r3(
            Op::Sra,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "or" => Ok(vec![Instruction::r3(
            Op::Or,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "and" => Ok(vec![Instruction::r3(
            Op::And,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        // M 扩展（乘除法）
        "mul" => Ok(vec![Instruction::r3(
            Op::Mul,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "mulh" => Ok(vec![Instruction::r3(
            Op::Mulh,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "mulhsu" => Ok(vec![Instruction::r3(
            Op::Mulhsu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "mulhu" => Ok(vec![Instruction::r3(
            Op::Mulhu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "div" => Ok(vec![Instruction::r3(
            Op::Div,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "divu" => Ok(vec![Instruction::r3(
            Op::Divu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "rem" => Ok(vec![Instruction::r3(
            Op::Rem,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        "remu" => Ok(vec![Instruction::r3(
            Op::Remu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_reg(op!(2))?,
        )]),
        // I 型
        "addi" => Ok(vec![Instruction::r2i(
            Op::Addi,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "slti" => Ok(vec![Instruction::r2i(
            Op::Slti,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "sltiu" => Ok(vec![Instruction::r2i(
            Op::Sltiu,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "xori" => Ok(vec![Instruction::r2i(
            Op::Xori,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "ori" => Ok(vec![Instruction::r2i(
            Op::Ori,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "andi" => Ok(vec![Instruction::r2i(
            Op::Andi,
            parse_reg(op!(0))?,
            parse_reg(op!(1))?,
            parse_int(op!(2))?,
        )]),
        "slli" | "srli" | "srai" => {
            let imm = parse_int(op!(2))?;
            // RV32I 移位量字段只有 5 位，立即数 ≥ 32（或为负）真机无法编码
            if !(0..=31).contains(&imm) {
                return Err(format!("移位立即数必须在 0..=31: {}", imm));
            }
            let op = match mnem {
                "slli" => Op::Slli,
                "srli" => Op::Srli,
                _ => Op::Srai,
            };
            Ok(vec![Instruction::r2i(
                op,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                imm,
            )])
        }
        // 访存
        "lb" | "lh" | "lw" | "lbu" | "lhu" => {
            let rd = parse_reg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            let op = match mnem {
                "lb" => Op::Lb,
                "lh" => Op::Lh,
                "lw" => Op::Lw,
                "lbu" => Op::Lbu,
                _ => Op::Lhu,
            };
            Ok(vec![Instruction::r2i(op, rd, rs1, imm)])
        }
        "sb" | "sh" | "sw" => {
            let rs2 = parse_reg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 S 型范围 ±2047: {}", imm));
            }
            let op = match mnem {
                "sb" => Op::Sb,
                "sh" => Op::Sh,
                _ => Op::Sw,
            };
            Ok(vec![Instruction {
                op,
                rd: 0,
                rs1,
                rs2,
                rs3: 0,
                imm,
            }])
        }
        // ---- F 扩展（单精度浮点）----
        "fadd.s" | "fsub.s" | "fmul.s" | "fdiv.s" | "fmin.s" | "fmax.s" | "fsgnj.s"
        | "fsgnjn.s" | "fsgnjx.s" => {
            let op = match mnem {
                "fadd.s" => Op::FAddS,
                "fsub.s" => Op::FSubS,
                "fmul.s" => Op::FMulS,
                "fdiv.s" => Op::FDivS,
                "fmin.s" => Op::FMinS,
                "fmax.s" => Op::FMaxS,
                "fsgnj.s" => Op::FSignJ,
                "fsgnjn.s" => Op::FSignJn,
                _ => Op::FSignJx,
            };
            Ok(vec![Instruction::r3(
                op,
                parse_freg(op!(0))?,
                parse_freg(op!(1))?,
                parse_freg(op!(2))?,
            )])
        }
        "fsqrt.s" => Ok(vec![Instruction::r3(
            Op::FSqrtS,
            parse_freg(op!(0))?,
            parse_freg(op!(1))?,
            0,
        )]),
        "fmadd.s" | "fmsub.s" | "fnmadd.s" | "fnmsub.s" => {
            let op = match mnem {
                "fmadd.s" => Op::FMAddS,
                "fmsub.s" => Op::FMSubS,
                "fnmadd.s" => Op::FNMAddS,
                _ => Op::FNMSUBS,
            };
            Ok(vec![Instruction::r4(
                op,
                parse_freg(op!(0))?,
                parse_freg(op!(1))?,
                parse_freg(op!(2))?,
                parse_freg(op!(3))?,
            )])
        }
        "feq.s" | "flt.s" | "fle.s" => {
            let op = match mnem {
                "feq.s" => Op::FEqS,
                "flt.s" => Op::FLtS,
                _ => Op::FLeS,
            };
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: parse_freg(op!(2))?,
                rs3: 0,
                imm: 0,
            }])
        }
        "fclass.s" => Ok(vec![Instruction {
            op: Op::FClassS,
            rd: parse_reg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "fcvt.w.s" | "fcvt.wu.s" => {
            let op = if mnem == "fcvt.w.s" {
                Op::FCvtWS
            } else {
                Op::FCvtWUS
            };
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }])
        }
        "fcvt.s.w" | "fcvt.s.wu" => {
            let op = if mnem == "fcvt.s.w" {
                Op::FCvtSW
            } else {
                Op::FCvtSWU
            };
            Ok(vec![Instruction {
                op,
                rd: parse_freg(op!(0))?,
                rs1: parse_reg(op!(1))?,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }])
        }
        "fmv.x.w" => Ok(vec![Instruction {
            op: Op::FMvXW,
            rd: parse_reg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "fmv.w.x" => Ok(vec![Instruction {
            op: Op::FMvWX,
            rd: parse_freg(op!(0))?,
            rs1: parse_reg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "flw" => {
            let rd = parse_freg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            Ok(vec![Instruction {
                op: Op::FLw,
                rd,
                rs1,
                rs2: 0,
                rs3: 0,
                imm,
            }])
        }
        "fsw" => {
            let rs2 = parse_freg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 S 型范围 ±2047: {}", imm));
            }
            Ok(vec![Instruction {
                op: Op::FSw,
                rd: 0,
                rs1,
                rs2,
                rs3: 0,
                imm,
            }])
        }
        // ---- D 扩展（双精度浮点）----
        "fadd.d" | "fsub.d" | "fmul.d" | "fdiv.d" | "fmin.d" | "fmax.d" | "fsgnj.d"
        | "fsgnjn.d" | "fsgnjx.d" => {
            let op = match mnem {
                "fadd.d" => Op::FAddD,
                "fsub.d" => Op::FSubD,
                "fmul.d" => Op::FMulD,
                "fdiv.d" => Op::FDivD,
                "fmin.d" => Op::FMinD,
                "fmax.d" => Op::FMaxD,
                "fsgnj.d" => Op::FSignJD,
                "fsgnjn.d" => Op::FSignJnD,
                _ => Op::FSignJxD,
            };
            Ok(vec![Instruction::r3(
                op,
                parse_freg(op!(0))?,
                parse_freg(op!(1))?,
                parse_freg(op!(2))?,
            )])
        }
        "fneg.d" => Ok(vec![Instruction::r3(
            Op::FSignJnD,
            parse_freg(op!(0))?,
            parse_freg(op!(1))?,
            parse_freg(op!(1))?,
        )]),
        "fsqrt.d" => Ok(vec![Instruction::r3(
            Op::FSqrtD,
            parse_freg(op!(0))?,
            parse_freg(op!(1))?,
            0,
        )]),
        "fmadd.d" | "fmsub.d" | "fnmadd.d" | "fnmsub.d" => {
            let op = match mnem {
                "fmadd.d" => Op::FMAddD,
                "fmsub.d" => Op::FMSubD,
                "fnmadd.d" => Op::FNMAddD,
                _ => Op::FNMSubD,
            };
            Ok(vec![Instruction::r4(
                op,
                parse_freg(op!(0))?,
                parse_freg(op!(1))?,
                parse_freg(op!(2))?,
                parse_freg(op!(3))?,
            )])
        }
        "feq.d" | "flt.d" | "fle.d" => {
            let op = match mnem {
                "feq.d" => Op::FEqD,
                "flt.d" => Op::FLtD,
                _ => Op::FLeD,
            };
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: parse_freg(op!(2))?,
                rs3: 0,
                imm: 0,
            }])
        }
        "fclass.d" => Ok(vec![Instruction {
            op: Op::FClassD,
            rd: parse_reg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "fcvt.d.s" => Ok(vec![Instruction {
            op: Op::FCvtDS,
            rd: parse_freg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "fcvt.s.d" => Ok(vec![Instruction {
            op: Op::FCvtSD,
            rd: parse_freg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }]),
        "fcvt.w.d" | "fcvt.wu.d" => {
            let op = if mnem == "fcvt.w.d" {
                Op::FCvtWD
            } else {
                Op::FCvtWUD
            };
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }])
        }
        "fcvt.d.w" | "fcvt.d.wu" => {
            let op = if mnem == "fcvt.d.w" {
                Op::FCvtDW
            } else {
                Op::FCvtDWU
            };
            Ok(vec![Instruction {
                op,
                rd: parse_freg(op!(0))?,
                rs1: parse_reg(op!(1))?,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }])
        }
        "fld" => {
            let rd = parse_freg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            Ok(vec![Instruction {
                op: Op::FLd,
                rd,
                rs1,
                rs2: 0,
                rs3: 0,
                imm,
            }])
        }
        "fsd" => {
            let rs2 = parse_freg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 S 型范围 ±2047: {}", imm));
            }
            Ok(vec![Instruction {
                op: Op::FSd,
                rd: 0,
                rs1,
                rs2,
                rs3: 0,
                imm,
            }])
        }
        // 分支
        "beq" | "bne" | "blt" | "bge" | "bltu" | "bgeu" => {
            let rs1 = parse_reg(op!(0))?;
            let rs2 = parse_reg(op!(1))?;
            let off = resolve_label(op!(2), addr, symbols)?;
            // B 型编码：12 位 <<1 且要求偶数（即 -4096..=4094 的偶数）
            if !(-4096..=4095).contains(&off) || off % 2 != 0 {
                return Err(format!(
                    "分支偏移超出 B 型范围（12 位 <<1，须为偶数）: {}",
                    off
                ));
            }
            let op = match mnem {
                "beq" => Op::Beq,
                "bne" => Op::Bne,
                "blt" => Op::Blt,
                "bge" => Op::Bge,
                "bltu" => Op::Bltu,
                _ => Op::Bgeu,
            };
            Ok(vec![Instruction::r2(op, rs1, rs2, off)])
        }
        // 跳转
        "jal" => {
            // 此时 ops 是 [rd, label]
            let rd = parse_reg(op!(0))?;
            let off = resolve_label(op!(1), addr, symbols)?;
            Ok(vec![Instruction {
                op: Op::Jal,
                rd,
                rs1: 0,
                rs2: 0,
                rs3: 0,
                imm: off,
            }])
        }
        "jalr" => {
            // 支持两种写法：jalr rd, rs1, imm / jalr rd, imm(rs1) / jalr rs1, imm
            let (rd, rs1, imm) = if ops.len() == 3 {
                (
                    parse_reg(op!(0))?,
                    parse_reg(op!(1))?,
                    parse_int(op!(2))?,
                )
            } else if ops.len() == 2 {
                if op!(1).contains('(') {
                    let rd = parse_reg(op!(0))?;
                    let (imm, rs1) = parse_mem_operand(op!(1))?;
                    (rd, rs1, imm)
                } else {
                    let rd = 1; // jalr rs, imm 隐含写 ra
                    let rs1 = parse_reg(op!(0))?;
                    let imm = parse_int(op!(1))?;
                    (rd, rs1, imm)
                }
            } else {
                return Err("jalr 操作数数量错误".to_string());
            };
            // I 型编码：立即数为 12 位有符号
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("jalr 立即数超出范围 ±2047: {}", imm));
            }
            Ok(vec![Instruction {
                op: Op::Jalr,
                rd,
                rs1,
                rs2: 0,
                rs3: 0,
                imm,
            }])
        }
        // U 型
        "lui" => Ok(vec![Instruction::r1i(
            Op::Lui,
            parse_reg(op!(0))?,
            parse_int(op!(1))?,
        )]),
        "auipc" => Ok(vec![Instruction::r1i(
            Op::Auipc,
            parse_reg(op!(0))?,
            parse_int(op!(1))?,
        )]),
        // 系统
        "ecall" => Ok(vec![mk(Op::Ecall)]),
        "ebreak" => Ok(vec![mk(Op::Ebreak)]),
        "fence" | "fence.i" => Ok(vec![mk(Op::Fence)]),
        _ => Err(format!("未知指令或伪指令: {}", mnem)),
    }
}

/// 解析访存操作数 imm(reg) 或 reg(imm)（宽松兼容）
fn parse_mem_operand(s: &str) -> Result<(i32, u8), String> {
    let s = s.trim();
    let open = s
        .find('(')
        .ok_or_else(|| format!("访存操作数缺少括号: {}", s))?;
    let close = s
        .rfind(')')
        .ok_or_else(|| format!("访存操作数缺少右括号: {}", s))?;
    let inside = s[open + 1..close].trim();
    let rs1 = parse_reg(inside)?;
    let imm_str = s[..open].trim();
    let imm = if imm_str.is_empty() {
        0
    } else {
        parse_int(imm_str)?
    };
    Ok((imm, rs1))
}

/// 解析标签 → 相对偏移（带范围检查）
fn resolve_label(label: &str, addr: u32, symbols: &HashMap<String, u32>) -> Result<i32, String> {
    let target = symbols
        .get(label)
        .copied()
        .ok_or_else(|| format!("未知标签: {}", label))?;
    let off = target as i32 - addr as i32;
    if !(-(1 << 20)..(1 << 20)).contains(&off) {
        return Err(format!("跳转目标超出范围: {} (偏移 {})", label, off));
    }
    Ok(off)
}

/// li / la 大立即数展开：lui + addi（处理低 12 位符号扩展的进位）
fn expand_li(rd: u8, imm: i32) -> Vec<Instruction> {
    if (-2048..=2047).contains(&imm) {
        return vec![Instruction::r2i(Op::Addi, rd, 0, imm)];
    }
    let u = imm as u32;
    let hi = ((u.wrapping_add(0x800)) >> 12) & 0xFFFFF;
    let lo = ((u << 20) as i32) >> 20; // 低 12 位符号扩展（0xFFF → -1）
    vec![
        Instruction::r1i(Op::Lui, rd, hi as i32),
        Instruction::r2i(Op::Addi, rd, rd, lo),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_layout() {
        let src = ".data\nmsg: .asciiz \"hi\"\n.text\nmain:\n    li a0, 1\n    addi a1, zero, 2\n";
        let p = assemble(src).unwrap();
        assert_eq!(p.instructions.len(), 2);
        assert_eq!(p.entry, 0x0000_0000);
        assert_eq!(p.symbols["main"], 0);
        assert_eq!(p.symbols["msg"], DATA_BASE);
    }

    #[test]
    fn test_data_relocation_accepts_dot_prefixed_label() {
        let source = r#".data
ptr: .word .Lstr0
.Lstr0: .asciiz "ok"
.text
main: nop
"#;
        let program = assemble(source).unwrap();
        let pointer = u32::from_le_bytes(program.data_segments[0].1[..4].try_into().unwrap());
        assert_eq!(pointer, program.symbols[".Lstr0"]);
    }

    #[test]
    fn test_string_literal_carriage_return_escape() {
        let program = assemble(".data\nmsg: .ascii \"a\\r\\n\"\n.text\nmain: nop\n").unwrap();
        assert_eq!(program.data_segments[0].1, b"a\r\n");
    }

    #[test]
    fn test_string_literal_hex_octal_escapes() {
        let program =
            assemble(".data\nmsg: .ascii \"\\x41\\101\\x42\"\n.text\nmain: nop\n").unwrap();
        assert_eq!(program.data_segments[0].1, b"AAB");
        let truncated = assemble(".data\nmsg: .ascii \"\\x4141\"\n.text\nmain: nop\n").unwrap();
        assert_eq!(truncated.data_segments[0].1, b"A");
        let program2 =
            assemble(".data\nmsg: .ascii \"\\x68\\x69\\x0a\"\n.text\nmain: nop\n").unwrap();
        assert_eq!(program2.data_segments[0].1, b"hi\n");
    }

    #[test]
    fn test_expand_li_large() {
        let p = assemble(".text\nli t0, 0x12345678\n").unwrap();
        assert_eq!(p.instructions.len(), 2);
        assert_eq!(p.instructions[0].op, Op::Lui);
        assert_eq!(p.instructions[1].op, Op::Addi);
    }

    #[test]
    fn test_pseudo_branch() {
        let p = assemble(".text\nstart:\n    beqz t0, start\n").unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert_eq!(p.instructions[0].op, Op::Beq);
        assert_eq!(p.instructions[0].imm, 0);
    }
    #[test]
    fn test_missing_operand_is_error() {
        let result = assemble(".text\nmain: li t0\n");
        match result {
            Err(error) => assert!(error.contains("缺少第 2 个操作数")),
            Ok(_) => panic!("缺少操作数时不应汇编成功"),
        }
    }

    #[test]
    fn test_invalid_data_directives_are_errors() {
        assert!(assemble(".data\n.space -1\n.text\nmain: nop\n")
            .unwrap_err()
            .contains(".space 大小不能为负数"));
        assert!(assemble(".data\n.align 17\n.text\nmain: nop\n")
            .unwrap_err()
            .contains(".align 指数必须在 0..=16"));
    }

    #[test]
    fn test_li_label_forward_reference_layout() {
        // li 引用前向数据标签须按 2 条指令布局，否则其后 .text 标签地址错 4 字节
        let src = ".text\nmain:\n    li t0, buf\n    j main\nafter_li:\n    nop\n.data\nbuf: .word 0\n";
        let p = assemble(src).unwrap();
        // li→lui+addi 两条 + j + nop
        assert_eq!(p.instructions.len(), 4);
        assert_eq!(p.instructions[0].op, Op::Lui);
        assert_eq!(p.instructions[1].op, Op::Addi);
        // after_li 应在 li 展开后的地址 12，而非被低估成 8
        assert_eq!(p.symbols["after_li"], 12);
        // j main 的偏移 = 0 - 8 = -8（若布局错误会是 -4，跳进 li 展开中间）
        let j_ins = &p.instructions[2];
        assert_eq!(j_ins.op, Op::Jal);
        assert_eq!(j_ins.imm, -8);
        // 执行验证：li(2 条) + j 之后 PC 回到 main
        let mut cpu = crate::cpu::Cpu::new();
        cpu.load_program(&p).unwrap();
        for _ in 0..3 {
            cpu.step(&p.instructions).unwrap();
        }
        assert_eq!(cpu.pc, 0);
    }

    #[test]
    fn test_li_small_text_label_layout_consistent() {
        // 小地址 .text 标签：第一遍按 2 条估，第二遍也必须展开 2 条，两遍一致
        let src = ".text\n    li t0, lbl\nlbl: nop\n";
        let p = assemble(src).unwrap();
        assert_eq!(p.instructions.len(), 3); // li→2 + nop
        assert_eq!(p.symbols["lbl"], 8);
        assert_eq!(p.instructions[0].op, Op::Lui);
        assert_eq!(p.instructions[1].op, Op::Addi);
    }

    #[test]
    fn test_colon_inside_string_literal_not_label() {
        // 字符串字面量内的冒号不应被当成标签分隔符
        let src = ".data\nurl: .asciiz \"http://example.com\"\nraw: .ascii \"a:b:c\"\n.text\nmain: nop\n";
        let p = assemble(src).unwrap();
        assert!(p.symbols.contains_key("url"));
        assert!(p.symbols.contains_key("raw"));
        assert!(!p.symbols.contains_key(".asciiz \"http"));
        assert_eq!(p.symbols["url"], DATA_BASE);
        // 内容含冒号且 .asciiz 带结尾 NUL
        assert_eq!(p.data_segments[0].1, b"http://example.com\0");
        assert_eq!(p.data_segments[1].1, b"a:b:c");
    }

    #[test]
    fn test_immediate_range_errors() {
        // S 型访存偏移 ±2047
        assert!(assemble(".text\nmain: sw t0, 2048(sp)\n")
            .unwrap_err()
            .contains("访存偏移超出 S 型范围"));
        assert!(assemble(".text\nmain: sb t0, -2049(sp)\n")
            .unwrap_err()
            .contains("访存偏移超出 S 型范围"));
        assert!(assemble(".text\nmain: fsw f0, 2048(sp)\n")
            .unwrap_err()
            .contains("访存偏移超出 S 型范围"));
        assert!(assemble(".text\nmain: fsd f0, -2049(sp)\n")
            .unwrap_err()
            .contains("访存偏移超出 S 型范围"));
        assert!(assemble(".text\nmain: sw t0, 2047(sp)\n").is_ok());
        assert!(assemble(".text\nmain: sw t0, -2048(sp)\n").is_ok());

        // jalr 立即数 ±2047
        assert!(assemble(".text\nmain: jalr t0, t1, 2048\n")
            .unwrap_err()
            .contains("jalr 立即数超出范围"));
        assert!(assemble(".text\nmain: jalr t0, t1, -2049\n")
            .unwrap_err()
            .contains("jalr 立即数超出范围"));
        assert!(assemble(".text\nmain: jalr t0, 2048(t1)\n")
            .unwrap_err()
            .contains("jalr 立即数超出范围"));
        assert!(assemble(".text\nmain: jalr t0, t1, 2047\n").is_ok());
        assert!(assemble(".text\nmain: jalr t0, -2048(t1)\n").is_ok());

        // 条件分支偏移超出 ±4096（B 型 12 位 <<1）报错
        let mut far_src = String::from(".text\nstart: beq zero, zero, far\n");
        for _ in 0..1030 {
            far_src.push_str("nop\n");
        }
        far_src.push_str("far: nop\n");
        assert!(assemble(&far_src)
            .unwrap_err()
            .contains("分支偏移超出 B 型范围"));

        // 偏移 4004（≤ 4094 且偶数）应通过
        let mut near_src = String::from(".text\nstart: beq zero, zero, far\n");
        for _ in 0..1000 {
            near_src.push_str("nop\n");
        }
        near_src.push_str("far: nop\n");
        let p = assemble(&near_src).unwrap();
        assert_eq!(p.instructions[0].imm, 4004);
    }

    #[test]
    fn test_shift_immediate_range() {
        assert!(assemble(".text\nmain: slli t0, t1, 32\n")
            .unwrap_err()
            .contains("移位立即数必须在 0..=31"));
        assert!(assemble(".text\nmain: srli t0, t1, -1\n")
            .unwrap_err()
            .contains("移位立即数必须在 0..=31"));
        assert!(assemble(".text\nmain: srai t0, t1, 0x20\n")
            .unwrap_err()
            .contains("移位立即数必须在 0..=31"));
        assert!(assemble(".text\nmain: srai t0, t1, 31\n").is_ok());
        assert!(assemble(".text\nmain: slli t0, t1, 0\n").is_ok());
    }
}
