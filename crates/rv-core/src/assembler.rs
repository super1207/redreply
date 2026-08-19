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
use std::collections::{HashMap, HashSet};

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

/// 数据地址向上对齐到 alignment（2 的幂）字节边界（RARS 数据指令自动对齐语义）
fn align_data_addr(address: u32, alignment: u32) -> Result<u32, String> {
    let aligned = address
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| "数据段对齐地址溢出".to_string())?;
    Ok(aligned)
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

/// 去掉注释：跳过 # 及之后内容（忽略字符串与字符字面量内的 #）
fn strip_comment(line: &str) -> String {
    let mut in_str = false;
    let mut in_char = false;
    let mut esc = false;
    let mut out = String::new();
    for c in line.chars() {
        if in_str || in_char {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' && in_str {
                in_str = false;
            } else if c == '\'' && in_char {
                in_char = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '\'' {
            in_char = true;
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

/// 解析字符串字面量列表（支持 \" \\ \n \r \t \0、八进制 \ddd、十六进制 \xHH）。
/// 相邻字面量拼接（GAS 惯用法）：.asciiz "a" "b" 与 .asciiz "a", "b" 都得 ab\0；
/// 段间的非法字符直接报错，不得静默编成数据字节。
fn parse_string_literal(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        // 多参数写法：跳过段间逗号与空白
        if rest.starts_with(',') {
            rest = rest[1..].trim_start();
            continue;
        }
        if !rest.starts_with('"') {
            return Err(format!("字符串字面量格式错误: {}", s));
        }
        // 找匹配的结束引号（跳过 \" 转义；引号/反斜杠均为 ASCII，逐字节扫描安全）
        let mut end = 1usize;
        while end < rest.len() {
            let b = rest.as_bytes()[end];
            if b == b'"' {
                break;
            }
            if b == b'\\' && end + 1 < rest.len() {
                end += 1;
            }
            end += 1;
        }
        if end >= rest.len() {
            return Err(format!("字符串字面量缺少结束引号: {}", s));
        }
        out.extend_from_slice(&decode_string_escapes(&rest[1..end])?);
        rest = rest[end + 1..].trim_start();
    }
    Ok(out)
}

/// 解析整数（十进制 / 十六进制 0x / 二进制 0b / 字符字面量 / 负数）
fn parse_int(s: &str) -> Result<i32, String> {
    let s = s.trim();
    // 字符字面量：'a' '\n' 等（解析为首个字符的字节值）
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        let inner = &s[1..s.len() - 1];
        let decoded = decode_string_escapes(inner)?;
        return match decoded.first() {
            Some(&b) => Ok(b as i32),
            None => Err(format!("空字符字面量: {}", s)),
        };
    }
    let (neg, body) = if let Some(b) = s.strip_prefix('-') {
        (true, b)
    } else {
        (false, s)
    };
    let v = if let Some(h) = body.strip_prefix("0x") {
        i64::from_str_radix(h, 16)
    } else if let Some(h) = body.strip_prefix("0X") {
        i64::from_str_radix(h, 16)
    } else if let Some(b) = body.strip_prefix("0b") {
        i64::from_str_radix(b, 2)
    } else if let Some(b) = body.strip_prefix("0B") {
        i64::from_str_radix(b, 2)
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

/// 解析 fcvt 系列指令的可选舍入模式第三操作数：
/// rne(0) rtz(1) rdn(2) rup(3) rmm(4) dyn(7)，缺省 rne
fn parse_rm(s: Option<&String>) -> Result<u8, String> {
    match s.map(|v| v.trim().to_lowercase()).as_deref() {
        None | Some("rne") => Ok(0),
        Some("rtz") => Ok(1),
        Some("rdn") => Ok(2),
        Some("rup") => Ok(3),
        Some("rmm") => Ok(4),
        Some("dyn") => Ok(7),
        Some(other) => Err(format!("未知舍入模式: {}（支持 rne/rtz/rdn/rup/rmm/dyn）", other)),
    }
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
        // 段切换（段名大小写不敏感，与指令助记符一致）
        if let Some(rest) = trimmed.strip_prefix('.') {
            let directive = rest.to_ascii_lowercase();
            if directive == "text" || directive == "data" {
                section = directive;
                continue;
            }
        }
        let mut label = None;
        let mut text = trimmed.to_string();
        // 行首标签：label: 或 label: 指令
        // 扫描时跟踪引号与反斜杠转义状态，字符串/字符字面量内的冒号
        // （如 .asciiz "http://example.com"、li a0, ':'）不算标签分隔符。
        let mut in_str = false;
        let mut in_char = false;
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
            } else if in_char {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '\'' {
                    in_char = false;
                }
            } else if c == '"' {
                in_str = true;
            } else if c == '\'' {
                in_char = true;
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

/// 数据指令的自然对齐（字节）：.word/.float=4、.double=8、.half=2，
/// 其余（.byte/.string/.space 等）为 1；.align 取其对齐参数（2 的幂）。
fn data_line_alignment(text: &str) -> u32 {
    let mut it = text.split_whitespace();
    let mnem = it.next().unwrap_or("").to_ascii_lowercase();
    match mnem.as_str() {
        ".word" | ".float" => 4,
        ".double" => 8,
        ".half" => 2,
        ".align" => it
            .next()
            .and_then(|arg| arg.trim().parse::<u32>().ok())
            .and_then(|power| 1u32.checked_shl(power))
            .filter(|align| *align <= 4096)
            .unwrap_or(1),
        _ => 1,
    }
}

/// 汇编主入口
pub fn assemble(source: &str) -> Result<Program, String> {
    let (lines, _order) = preprocess(source)?;

    // 超范围 B 型分支松弛为「反转分支 + jal」的行集合（按 lines 下标）。
    // 松弛插入 4 字节会移动后续地址，布局与生成整体迭代直到集合稳定
    // （集合单调增长，必然终止）。
    let mut relaxed: HashSet<usize> = HashSet::new();

    loop {
        // ===== 第一遍：布局 + 收集符号 =====
        let mut symbols: HashMap<String, u32> = HashMap::new();
        let mut text_addr = TEXT_BASE; // 指令按 4 字节递增
        let mut data_addr = DATA_BASE;
        let mut data_bytes: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut data_relocations: Vec<(usize, String, usize)> = Vec::new();

        for (line_index, l) in lines.iter().enumerate() {
            if let Some(lb) = &l.label {
                let addr = if l.section == "text" {
                    text_addr
                } else if l.text.is_empty() {
                    // 独立成行的标签绑定到下一条数据指令对齐后的地址（RARS/GAS 语义），
                    // 否则标签落在其后 .word/.double 等对齐填充字节之前，
                    // 经它寻址会读到填充字节或触发未对齐错误
                    let next_align = lines
                        .iter()
                        .skip(line_index + 1)
                        .find(|next| next.section == "data" && !next.text.is_empty())
                        .map(|next| data_line_alignment(&next.text))
                        .unwrap_or(1);
                    align_data_addr(data_addr, next_align)?
                } else {
                    // 同行数据指令会先按其自然大小对齐，标签应落在对齐后的地址
                    align_data_addr(data_addr, data_line_alignment(&l.text))?
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
                    // .align 在 .text 段为无害操作：指令流恒 4 字节对齐
                    if mnem == ".align" {
                        continue;
                    }
                    return Err(format!("数据指令 {} 不能出现在 .text 段", mnem));
                }
                let mut n = expansion_len(&mnem, &operands);
                if relaxed.contains(&line_index) {
                    n += 1; // 松弛为 2 条（反转分支 + jal）
                }
                let byte_len = n
                    .checked_mul(4)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| "文本段大小溢出".to_string())?;
                text_addr = text_addr
                    .checked_add(byte_len)
                    .ok_or_else(|| "文本段地址溢出".to_string())?;
            } else {
                // 数据段
                if l.text.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = l.text.splitn(2, char::is_whitespace).collect();
                let mnem = parts[0].to_ascii_lowercase();
                let args = if parts.len() > 1 { parts[1].trim() } else { "" };
                match mnem.as_str() {
                    ".globl" => {} // 符号导出指令段无关，忽略
                    ".float" => {
                        // 单精度浮点常量（4 字节，自动对齐到 4）
                        data_addr = align_data_addr(data_addr, 4)?;
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
                        // 双精度浮点常量（8 字节，自动对齐到 8）
                        data_addr = align_data_addr(data_addr, 8)?;
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
                        let size: usize = if mnem == ".word" {
                            4
                        } else if mnem == ".half" {
                            2
                        } else {
                            1
                        };
                        // 按自然大小对齐（RARS 语义），避免 lw/ld 运行时对齐错误
                        data_addr = align_data_addr(data_addr, size as u32)?;
                        for v in args.split(',') {
                            let v = v.trim();
                            if v.is_empty() {
                                continue;
                            }
                            let bytes = if let Ok(value) = parse_int(v) {
                                // 超范围静默截断会掩盖笔误（RARS 报错）
                                let ok = match size {
                                    2 => (-32768..=65535).contains(&value),
                                    1 => (-128..=255).contains(&value),
                                    _ => true,
                                };
                                if !ok {
                                    return Err(format!(
                                        "数据初值超出 {} 位范围: {}",
                                        size * 8,
                                        v
                                    ));
                                }
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

        // ===== 第二遍：生成指令（同时决定新增松弛）=====
        let mut instructions: Vec<Instruction> = Vec::new();
        let mut text_lines: Vec<String> = Vec::new();
        let mut cur_addr = TEXT_BASE;
        let mut changed = false;

        for (line_index, l) in lines.iter().enumerate() {
            // .globl 段无关且大小写不敏感（与第一遍一致）
            let first_word = l.text.split_whitespace().next().unwrap_or("");
            if l.section != "text"
                || l.text.is_empty()
                || first_word.eq_ignore_ascii_case(".globl")
                || first_word.eq_ignore_ascii_case(".align")
            {
                continue;
            }
            let parts: Vec<&str> = l.text.splitn(2, char::is_whitespace).collect();
            let mnem = parts[0].to_lowercase();
            let operands = if parts.len() > 1 {
                split_operands(parts[1])
            } else {
                vec![]
            };
            let raw_text = l.text.clone();
            if is_branch_mnem(&mnem) {
                let off = branch_target_offset(&mnem, &operands, cur_addr, &symbols)?;
                if !(-4096..=4095).contains(&off) || off % 2 != 0 {
                    if !relaxed.contains(&line_index) {
                        relaxed.insert(line_index);
                        changed = true;
                    }
                    let expanded = relaxed_branch(&mnem, &operands, cur_addr, &symbols)?;
                    for ins in expanded {
                        instructions.push(ins);
                        text_lines.push(raw_text.clone());
                        cur_addr = cur_addr.wrapping_add(4);
                    }
                    continue;
                }
            }
            let expanded = parse_instruction(&mnem, &operands, cur_addr, &symbols)?;
            for ins in expanded {
                instructions.push(ins);
                text_lines.push(raw_text.clone());
                cur_addr = cur_addr.wrapping_add(4);
            }
        }

        if !changed {
            let data_end = data_addr;
            // 入口：优先 _start（crt0 启动器），其次 main，否则 .text 首条
            let entry = symbols
                .get("_start")
                .copied()
                .or_else(|| symbols.get("main").copied())
                .unwrap_or(TEXT_BASE);

            return Ok(Program {
                instructions,
                text_lines,
                data_segments: data_bytes,
                data_end,
                entry,
                symbols,
            });
        }
    }
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

    // 操作数个数校验：多余操作数静默忽略会掩盖笔误（RARS 会报错）
    {
        let (min, max): (usize, usize) = match mnem {
            "add" | "sub" | "sll" | "slt" | "sltu" | "xor" | "srl" | "sra" | "or" | "and"
            | "mul" | "mulh" | "mulhsu" | "mulhu" | "div" | "divu" | "rem" | "remu"
            | "addi" | "slti" | "sltiu" | "xori" | "ori" | "andi" | "slli" | "srli" | "srai"
            | "beq" | "bne" | "blt" | "bge" | "bltu" | "bgeu" | "feq.s" | "flt.s" | "fle.s"
            | "feq.d" | "flt.d" | "fle.d" | "fadd.s" | "fsub.s" | "fmul.s" | "fdiv.s"
            | "fmin.s" | "fmax.s" | "fsgnj.s" | "fsgnjn.s" | "fsgnjx.s" | "fadd.d"
            | "fsub.d" | "fmul.d" | "fdiv.d" | "fmin.d" | "fmax.d" | "fsgnj.d" | "fsgnjn.d"
            | "fsgnjx.d" => (3, 3),
            "lb" | "lh" | "lw" | "lbu" | "lhu" | "sb" | "sh" | "sw" | "flw" | "fsw" | "fld"
            | "fsd" => (2, 2),
            "fmadd.s" | "fmsub.s" | "fnmadd.s" | "fnmsub.s" | "fmadd.d" | "fmsub.d"
            | "fnmadd.d" | "fnmsub.d" => (4, 4),
            "fsqrt.s" | "fsqrt.d" | "fclass.s" | "fclass.d" | "fmv.x.w" | "fmv.w.x"
            | "lui" | "auipc" | "fneg.d" | "neg" | "not" | "seqz" | "snez" | "sltz"
            | "sgtz" | "beqz" | "bnez" | "li" | "la"
            | "mv" => (2, 2),
            // bgt/ble/bgtu/bleu ≡ blt/bge（操作数交换）的三操作数形式
            "bgt" | "ble" | "bgtu" | "bleu" => (3, 3),
            "fcvt.w.s" | "fcvt.wu.s" | "fcvt.s.w" | "fcvt.s.wu" | "fcvt.d.s" | "fcvt.s.d"
            | "fcvt.w.d" | "fcvt.wu.d" | "fcvt.d.w" | "fcvt.d.wu" | "jalr" => (2, 3),
            "jal" => (1, 2),
            "j" | "call" | "jr" => (1, 1),
            "nop" | "ret" | "ecall" | "ebreak" | "fence" | "fence.i" => (0, 0),
            _ => (0, usize::MAX),
        };
        if ops.len() < min {
            return Err(format!("指令 {} 缺少操作数（需要 {} 个）", mnem, min));
        }
        if ops.len() > max {
            return Err(format!("指令 {} 操作数过多（最多 {} 个）", mnem, max));
        }
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
        "jr" => {
            // jr rs = jalr x0, rs, 0
            return Ok(vec![Instruction {
                op: Op::Jalr,
                rd: 0,
                rs1: parse_reg(op!(0))?,
                rs2: 0,
                rs3: 0,
                imm: 0,
            }]);
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
                resolve_branch_label(op!(1), addr, symbols)?,
            )])
        }
        "bnez" => {
            return Ok(vec![Instruction::r2(
                Op::Bne,
                parse_reg(op!(0))?,
                0,
                resolve_branch_label(op!(1), addr, symbols)?,
            )])
        }
        "bgt" => {
            return Ok(vec![Instruction::r2(
                Op::Blt,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_branch_label(op!(2), addr, symbols)?,
            )])
        }
        "ble" => {
            return Ok(vec![Instruction::r2(
                Op::Bge,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_branch_label(op!(2), addr, symbols)?,
            )])
        }
        "bgtu" => {
            return Ok(vec![Instruction::r2(
                Op::Bltu,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_branch_label(op!(2), addr, symbols)?,
            )])
        }
        "bleu" => {
            return Ok(vec![Instruction::r2(
                Op::Bgeu,
                parse_reg(op!(1))?,
                parse_reg(op!(0))?,
                resolve_branch_label(op!(2), addr, symbols)?,
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
        // I 型：12 位立即数（接受 -2048..=4095 的位型），按真机编码符号扩展
        //（0xFFF → -1），避免同一机器码在本模拟器与真机行为不一致。
        "addi" | "slti" | "sltiu" => {
            let imm = parse_int(op!(2))?;
            if !(-2048..=4095).contains(&imm) {
                return Err(format!("I 型立即数超出 12 位符号扩展范围: {}", imm));
            }
            let imm = (imm << 20) >> 20;
            let op = match mnem {
                "addi" => Op::Addi,
                "slti" => Op::Slti,
                _ => Op::Sltiu,
            };
            Ok(vec![Instruction::r2i(
                op,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                imm,
            )])
        }
        "xori" | "ori" | "andi" => {
            // 逻辑运算同样受 12 位位型约束：超出范围（如 ori t0, t1, 65536）
            // 在真机无法编码，这里静默执行会得到错误结果
            let imm = parse_int(op!(2))?;
            if !(-2048..=4095).contains(&imm) {
                return Err(format!("I 型立即数超出 12 位符号扩展范围: {}", imm));
            }
            let imm = (imm << 20) >> 20;
            let op = match mnem {
                "xori" => Op::Xori,
                "ori" => Op::Ori,
                _ => Op::Andi,
            };
            Ok(vec![Instruction::r2i(
                op,
                parse_reg(op!(0))?,
                parse_reg(op!(1))?,
                imm,
            )])
        }
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
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 I 型范围 ±2047: {}", imm));
            }
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
            let rm = parse_rm(ops.get(2))?;
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: 0,
                rs3: rm,
                imm: 0,
            }])
        }
        "fcvt.s.w" | "fcvt.s.wu" => {
            let op = if mnem == "fcvt.s.w" {
                Op::FCvtSW
            } else {
                Op::FCvtSWU
            };
            let rm = parse_rm(ops.get(2))?;
            Ok(vec![Instruction {
                op,
                rd: parse_freg(op!(0))?,
                rs1: parse_reg(op!(1))?,
                rs2: 0,
                rs3: rm,
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
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 I 型范围 ±2047: {}", imm));
            }
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
            rs3: parse_rm(ops.get(2))?,
            imm: 0,
        }]),
        "fcvt.s.d" => Ok(vec![Instruction {
            op: Op::FCvtSD,
            rd: parse_freg(op!(0))?,
            rs1: parse_freg(op!(1))?,
            rs2: 0,
            rs3: parse_rm(ops.get(2))?,
            imm: 0,
        }]),
        "fcvt.w.d" | "fcvt.wu.d" => {
            let op = if mnem == "fcvt.w.d" {
                Op::FCvtWD
            } else {
                Op::FCvtWUD
            };
            let rm = parse_rm(ops.get(2))?;
            Ok(vec![Instruction {
                op,
                rd: parse_reg(op!(0))?,
                rs1: parse_freg(op!(1))?,
                rs2: 0,
                rs3: rm,
                imm: 0,
            }])
        }
        "fcvt.d.w" | "fcvt.d.wu" => {
            let op = if mnem == "fcvt.d.w" {
                Op::FCvtDW
            } else {
                Op::FCvtDWU
            };
            let rm = parse_rm(ops.get(2))?;
            Ok(vec![Instruction {
                op,
                rd: parse_freg(op!(0))?,
                rs1: parse_reg(op!(1))?,
                rs2: 0,
                rs3: rm,
                imm: 0,
            }])
        }
        "fld" => {
            let rd = parse_freg(op!(0))?;
            let (imm, rs1) = parse_mem_operand(op!(1))?;
            if !(-2048..=2047).contains(&imm) {
                return Err(format!("访存偏移超出 I 型范围 ±2047: {}", imm));
            }
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
            // B 型编码：12 位 <<1 且要求偶数（-4096..=4094 的偶数）
            let off = resolve_branch_label(op!(2), addr, symbols)?;
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
            // 此时 ops 是 [rd, label]；J 型 ±1MB 范围（标签恒 4 对齐）
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
                } else if let Ok(rs1) = parse_reg(op!(1)) {
                    // jalr rd, rs1：GNU as/RARS 接受的标准写法，imm 缺省 0
                    (parse_reg(op!(0))?, rs1, 0)
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
        "lui" | "auipc" => {
            // U 型：20 位立即数（接受符号扩展位型 -0x80000..=0xFFFFF，
            // 如 lui t0, -1 表示 0xFFFFF；超范围静默回绕是真机不会有的行为）
            let imm = parse_int(op!(1))?;
            if !(-524288..=1048575).contains(&imm) {
                return Err(format!("U 型立即数超出 20 位范围: {}", imm));
            }
            let op = if mnem == "lui" { Op::Lui } else { Op::Auipc };
            Ok(vec![Instruction::r1i(op, parse_reg(op!(0))?, imm)])
        }
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
    // J 型立即数范围 -1048576..=1048575：下界必须包含 -1<<20，
    // 恰好 -1MiB 的向后跳转是可编码的
    if !(-(1 << 20)..=(1 << 20) - 1).contains(&off) {
        return Err(format!("跳转目标超出范围: {} (偏移 {})", label, off));
    }
    Ok(off)
}

/// 分支目标解析：B 型编码 12 位 <<1，偏移须为偶数且在 ±4KB 内
/// （伪分支 beqz/bgt/bleu 等与真分支同一编码约束）。
fn resolve_branch_label(
    label: &str,
    addr: u32,
    symbols: &HashMap<String, u32>,
) -> Result<i32, String> {
    let off = resolve_label(label, addr, symbols)?;
    if !(-4096..=4095).contains(&off) || off % 2 != 0 {
        return Err(format!(
            "分支偏移超出 B 型范围（12 位 <<1，须为偶数）: {}",
            off
        ));
    }
    Ok(off)
}

/// 分支指令判定（含伪分支）。
fn is_branch_mnem(mnem: &str) -> bool {
    matches!(
        mnem,
        "beq" | "bne"
            | "blt"
            | "bge"
            | "bltu"
            | "bgeu"
            | "beqz"
            | "bnez"
            | "bgt"
            | "ble"
            | "bgtu"
            | "bleu"
    )
}

/// 分支目标偏移（±1MB 校验，B 型范围由调用方/松弛处理）。
fn branch_target_offset(
    mnem: &str,
    ops: &[String],
    addr: u32,
    symbols: &HashMap<String, u32>,
) -> Result<i32, String> {
    let label_index = match mnem {
        "beqz" | "bnez" => 1,
        _ => 2,
    };
    let label = ops
        .get(label_index)
        .ok_or_else(|| format!("{} 缺少标签操作数", mnem))?;
    resolve_label(label, addr, symbols)
}

/// 超范围 B 型分支松弛：反转分支（跳到下一条指令）+ jal 直达目标。
/// 两条均可真机编码，语义与原分支等价（真分支路径多执行 1 条指令）。
fn relaxed_branch(
    mnem: &str,
    ops: &[String],
    addr: u32,
    symbols: &HashMap<String, u32>,
) -> Result<Vec<Instruction>, String> {
    // (反转后的操作码, rs1, rs2, 标签操作数下标)；伪分支已换算成等价真分支形式
    let (op, rs1, rs2, label_index): (Op, u8, u8, usize) = match mnem {
        "beq" => (Op::Bne, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        "bne" => (Op::Beq, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        "blt" => (Op::Bge, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        "bge" => (Op::Blt, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        "bltu" => (Op::Bgeu, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        "bgeu" => (Op::Bltu, parse_reg(&ops[0])?, parse_reg(&ops[1])?, 2),
        // beqz a, L ≡ beq a, x0, L → 反转 bne a, x0, +8
        "beqz" => (Op::Bne, parse_reg(&ops[0])?, 0, 1),
        "bnez" => (Op::Beq, parse_reg(&ops[0])?, 0, 1),
        // bgt a, b, L ≡ blt b, a, L → 反转 bge b, a, +8
        "bgt" => (Op::Bge, parse_reg(&ops[1])?, parse_reg(&ops[0])?, 2),
        "ble" => (Op::Blt, parse_reg(&ops[1])?, parse_reg(&ops[0])?, 2),
        "bgtu" => (Op::Bgeu, parse_reg(&ops[1])?, parse_reg(&ops[0])?, 2),
        "bleu" => (Op::Bltu, parse_reg(&ops[1])?, parse_reg(&ops[0])?, 2),
        _ => return Err(format!("{} 不是可松弛的分支指令", mnem)),
    };
    let label = ops
        .get(label_index)
        .ok_or_else(|| format!("{} 缺少标签操作数", mnem))?;
    let target = symbols
        .get(label)
        .copied()
        .ok_or_else(|| format!("未知标签: {}", label))?;
    let off = target as i32 - addr as i32;
    // 松弛生成的 jal 位于 addr+4，实际编码的立即数是 off-4，范围检查应针对它
    if !(-(1 << 20)..=(1 << 20) - 1).contains(&(off - 4)) {
        return Err(format!("跳转目标超出范围: {} (偏移 {})", label, off));
    }
    Ok(vec![
        Instruction {
            op,
            rd: 0,
            rs1,
            rs2,
            rs3: 0,
            imm: 8, // 跳过下一条 jal
        },
        Instruction {
            op: Op::Jal,
            rd: 0,
            rs1: 0,
            rs2: 0,
            rs3: 0,
            imm: off - 4, // jal 位于 addr+4
        },
    ])
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
            Err(error) => assert!(error.contains("缺少操作数") || error.contains("缺少第 2 个操作数")),
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

        // 条件分支偏移超出 ±4096（B 型 12 位 <<1）：自动松弛为
        // 「反转分支 + jal」两条，均真机可编码
        let mut far_src = String::from(".text\nstart: beq zero, zero, far\n");
        for _ in 0..1030 {
            far_src.push_str("nop\n");
        }
        far_src.push_str("far: nop\n");
        let p = assemble(&far_src).unwrap();
        assert_eq!(p.instructions.len(), 2 + 1030 + 1);
        assert_eq!(p.instructions[0].op, Op::Bne); // 反转分支跳过 jal
        assert_eq!(p.instructions[0].imm, 8);
        assert_eq!(p.instructions[1].op, Op::Jal);
        assert_eq!(p.instructions[1].imm, 4124); // 目标 4128 - 4

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

    #[test]
    fn test_bgt_ble_three_operand_pseudo() {
        // bgt a, b, L ≡ blt b, a, L；ble a, b, L ≡ bge b, a, L（操作数交换）
        let p = assemble(".text\nstart: bgt t0, t1, start\n").unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert_eq!(p.instructions[0].op, Op::Blt);
        assert_eq!(p.instructions[0].rs1, 6); // t1
        assert_eq!(p.instructions[0].rs2, 5); // t0
        assert_eq!(p.instructions[0].imm, 0);
        let p = assemble(".text\nstart: ble t0, t1, start\n").unwrap();
        assert_eq!(p.instructions[0].op, Op::Bge);
        assert_eq!(p.instructions[0].rs1, 6);
        assert_eq!(p.instructions[0].rs2, 5);
        let p = assemble(".text\nstart: bgtu t0, t1, start\n").unwrap();
        assert_eq!(p.instructions[0].op, Op::Bltu);
        let p = assemble(".text\nstart: bleu t0, t1, start\n").unwrap();
        assert_eq!(p.instructions[0].op, Op::Bgeu);
        // 远距离同样走松弛（操作数交换保持一致）
        let mut far = String::from(".text\nstart: bgt t0, t1, far\n");
        for _ in 0..1030 {
            far.push_str("nop\n");
        }
        far.push_str("far: nop\n");
        let p = assemble(&far).unwrap();
        assert_eq!(p.instructions.len(), 2 + 1030 + 1);
        assert_eq!(p.instructions[0].op, Op::Bge); // 反转：bge t1, t0, +8
        assert_eq!(p.instructions[0].rs1, 6);
        assert_eq!(p.instructions[0].rs2, 5);
        assert_eq!(p.instructions[1].op, Op::Jal);
    }

    #[test]
    fn test_uppercase_directives() {
        // 段切换、数据指令、.globl 大小写不敏感且两遍一致
        let src = ".TEXT\nmain: nop\n.GLOBL main\n.DATA\nval: .WORD 0x1234\nbuf: .ASCIIZ \"hi\"\n";
        let p = assemble(src).unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert!(p.symbols.contains_key("main"));
        assert_eq!(p.symbols["val"], DATA_BASE);
        let word = u32::from_le_bytes(p.data_segments[0].1[..4].try_into().unwrap());
        assert_eq!(word, 0x1234);
        assert_eq!(p.data_segments[1].1, b"hi\0");
    }

    #[test]
    fn test_char_literal_hash() {
        // '#' 在字符字面量内不是注释起始
        let p = assemble(".text\nmain:\n    li a0, '#'\n").unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert_eq!(p.instructions[0].op, Op::Addi);
        assert_eq!(p.instructions[0].imm, 35);
    }

    #[test]
    fn test_char_literal_colon_is_not_label() {
        // 字符字面量内的冒号不是标签分隔符
        let p = assemble(".text\nmain:\n    li a0, ':'\n").unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert_eq!(p.instructions[0].op, Op::Addi);
        assert_eq!(p.instructions[0].imm, 58);
        assert!(p.symbols.contains_key("main"));
    }

    #[test]
    fn test_align_directive_in_text_section() {
        // .align 在 .text 段为无害操作：两遍都必须跳过（第二遍曾报“未知指令”）
        let p = assemble(".text\nmain: nop\n.align 2\nnop\n").unwrap();
        assert_eq!(p.instructions.len(), 2);
    }

    #[test]
    fn test_jalr_two_register_form() {
        // jalr rd, rs1：GNU as/RARS 接受的标准写法（imm 缺省 0）
        let p = assemble(".text\nmain:\n    jalr ra, t0\n").unwrap();
        assert_eq!(p.instructions.len(), 1);
        assert_eq!(p.instructions[0].op, Op::Jalr);
        assert_eq!(p.instructions[0].rd, 1);
        assert_eq!(p.instructions[0].rs1, 5);
        assert_eq!(p.instructions[0].imm, 0);
    }

    #[test]
    fn standalone_data_label_binds_after_alignment() {
        // 独立成行的标签必须绑定到其后 .word 对齐后的地址，
        // 否则经它 lw 会读到对齐填充字节（或直接未对齐报错）
        let p = assemble(".data\nb: .byte 7\nlbl:\nw: .word 9\n.text\nmain: nop\n").unwrap();
        assert_eq!(p.symbols["b"], DATA_BASE);
        assert_eq!(p.symbols["lbl"], DATA_BASE + 4);
        assert_eq!(p.symbols["w"], DATA_BASE + 4);
        // 数据本身不受影响
        let seg = p
            .data_segments
            .iter()
            .find(|(addr, _)| *addr == DATA_BASE + 4)
            .unwrap();
        assert_eq!(seg.1, 9u32.to_le_bytes().to_vec());
    }
}
