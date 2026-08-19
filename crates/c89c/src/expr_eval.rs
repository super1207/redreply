//! 预处理常量表达式求值器（#if 用，C89 整数常量表达式子集）
//!
//! 值模型：ICVal 携带符号性（S=有符号 i64 / U=无符号 u32 语义），
//! 二元运算按 C89 §3.2.1.5 通常算术转换执行：任一操作数无符号时
//! 两边按无符号 int（模 2^32）运算与比较。

use crate::lexer::{TokKind, Token};

/// 常量值：S 为有符号 i64（位型），U 为无符号（模 2^32 语义）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ICVal {
    S(i64),
    U(u64),
}

impl ICVal {
    pub(crate) fn is_true(self) -> bool {
        match self {
            ICVal::S(v) => v != 0,
            ICVal::U(v) => v != 0,
        }
    }
    /// 转为无符号视图（S 值按 32 位位型取模）
    pub(crate) fn to_u64(self) -> u64 {
        match self {
            ICVal::S(v) => v as u32 as u64,
            ICVal::U(v) => v,
        }
    }
    /// 取位型（供 #if / 枚举值等按 i64 位模式消费；U 值按 32 位位型
    /// 符号扩展，0xFFFFFFFF 得 -1，与 int 上下文一致）
    pub(crate) fn bits(self) -> i64 {
        match self {
            ICVal::S(v) => v,
            ICVal::U(v) => v as u32 as i32 as i64,
        }
    }
}

pub(crate) fn ic_add(a: ICVal, b: ICVal) -> ICVal {
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => ICVal::S(x.wrapping_add(y)),
        _ => ICVal::U(a.to_u64().wrapping_add(b.to_u64()) & 0xFFFF_FFFF),
    }
}
pub(crate) fn ic_sub(a: ICVal, b: ICVal) -> ICVal {
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => ICVal::S(x.wrapping_sub(y)),
        _ => ICVal::U(a.to_u64().wrapping_sub(b.to_u64()) & 0xFFFF_FFFF),
    }
}
pub(crate) fn ic_mul(a: ICVal, b: ICVal) -> ICVal {
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => ICVal::S(x.wrapping_mul(y)),
        _ => ICVal::U(a.to_u64().wrapping_mul(b.to_u64()) & 0xFFFF_FFFF),
    }
}
pub(crate) fn ic_div(a: ICVal, b: ICVal) -> Result<ICVal, String> {
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => {
            if y == 0 {
                return Err("除零".into());
            }
            Ok(ICVal::S(x.checked_div(y).ok_or("除法溢出")?))
        }
        _ => {
            let d = b.to_u64();
            if d == 0 {
                return Err("除零".into());
            }
            Ok(ICVal::U(a.to_u64() / d))
        }
    }
}
pub(crate) fn ic_rem(a: ICVal, b: ICVal) -> Result<ICVal, String> {
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => {
            if y == 0 {
                return Err("模零".into());
            }
            Ok(ICVal::S(x.checked_rem(y).ok_or("取模溢出")?))
        }
        _ => {
            let d = b.to_u64();
            if d == 0 {
                return Err("模零".into());
            }
            Ok(ICVal::U(a.to_u64() % d))
        }
    }
}
pub(crate) fn ic_shl(a: ICVal, r: u32) -> ICVal {
    match a {
        ICVal::S(v) => ICVal::S(v.wrapping_shl(r)),
        ICVal::U(v) => ICVal::U((v << r) & 0xFFFF_FFFF),
    }
}
pub(crate) fn ic_shr(a: ICVal, r: u32) -> ICVal {
    match a {
        ICVal::S(v) => ICVal::S(v.wrapping_shr(r)),
        ICVal::U(v) => ICVal::U(v >> r),
    }
}
pub(crate) fn ic_cmp(a: ICVal, b: ICVal) -> (bool, bool, bool, bool, bool, bool) {
    // (lt, gt, le, ge, eq, ne)，按通常算术转换
    match (a, b) {
        (ICVal::S(x), ICVal::S(y)) => (x < y, x > y, x <= y, x >= y, x == y, x != y),
        _ => {
            let x = a.to_u64();
            let y = b.to_u64();
            (x < y, x > y, x <= y, x >= y, x == y, x != y)
        }
    }
}

/// 拆分整数常量后缀（C89 §3.1.3.2 合法后缀集合：无/u/U/l/L/ul/uL/Ul/UL/lu/lU/Lu/LU），
/// 返回 (数字部分, 是否无符号, 是否 long)。非法后缀（ll/uu/lul 等）报错。
pub(crate) fn split_int_suffix(text: &str) -> Result<(&str, bool, bool), String> {
    let mut end = text.len();
    while end > 0 && matches!(text.as_bytes()[end - 1], b'u' | b'U' | b'l' | b'L') {
        end -= 1;
    }
    let (num, suffix) = (&text[..end], &text[end..]);
    match suffix {
        "" => Ok((num, false, false)),
        "u" | "U" => Ok((num, true, false)),
        "l" | "L" => Ok((num, false, true)),
        "ul" | "uL" | "Ul" | "UL" | "lu" | "lU" | "Lu" | "LU" => Ok((num, true, true)),
        _ => Err(format!("非法整数后缀: {}", text)),
    }
}

/// 解析整数 token 文本的数字部分（支持 0x/八进制/十进制；后缀须先由
/// split_int_suffix 剥离）。整数常量按 i64 位型返回。
pub(crate) fn parse_int_value(text: &str) -> Result<i64, String> {
    if let Some(h) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).map_err(|_| format!("非法十六进制: {}", text))
    } else if text.len() > 1 && text.starts_with('0') {
        // 八进制
        i64::from_str_radix(&text[1..], 8).map_err(|_| format!("非法八进制: {}", text))
    } else {
        text.parse::<i64>().map_err(|_| format!("非法整数: {}", text))
    }
}

/// 解析完整整数常量（数字 + 后缀），返回 (值, 无符号, long)
pub(crate) fn parse_int_literal(text: &str) -> Result<(i64, bool, bool), String> {
    let (num, unsigned, long) = split_int_suffix(text)?;
    let value = parse_int_value(num)?;
    Ok((value, unsigned, long))
}

/// 解码 C 字符串字面量转义（C89 3.1.3.4）。
/// 与汇编器共用同一实现，见 rv_core::assembler::decode_string_escapes。
pub(crate) fn decode_string_escapes(s: &str) -> Result<Vec<u8>, String> {
    rv_core::assembler::decode_string_escapes(s)
}

/// 解析字符常量 'a' / '\n' / '\x41' / '\101' 等
pub(crate) fn parse_char_value(text: &str) -> Result<i64, String> {
    let inner = text.trim_start_matches('\'').trim_end_matches('\'');
    if inner.starts_with('\\') {
        let bytes = inner.as_bytes();
        match bytes.get(1) {
            Some(b'n') => Ok(10),
            Some(b't') => Ok(9),
            Some(b'r') => Ok(13),
            Some(b'a') => Ok(7),
            Some(b'b') => Ok(8),
            Some(b'f') => Ok(12),
            Some(b'v') => Ok(11),
            Some(b'\\') => Ok(92),
            Some(b'\'') => Ok(39),
            Some(b'"') => Ok(34),
            Some(b'x') => {
                let mut value: i64 = 0;
                let mut any = false;
                for &b in &bytes[2..] {
                    let digit = match b {
                        b'0'..=b'9' => b - b'0',
                        b'a'..=b'f' => b - b'a' + 10,
                        b'A'..=b'F' => b - b'A' + 10,
                        _ => break,
                    };
                    // \x 消费全部十六进制位：i64 溢出报错而非 panic（debug 构建）
                    value = value
                        .checked_mul(16)
                        .and_then(|v| v.checked_add(digit as i64))
                        .ok_or_else(|| format!("字符常量转义值溢出: {}", text))?;
                    any = true;
                }
                if !any {
                    return Err(format!("非法字符常量: {}", text));
                }
                // 转义值不截断到 8 位（C89 §3.1.3.4：字符常量值是 int），
                // 赋给 char 时由 codegen 窄化。
                Ok(value)
            }
            Some(d @ b'0'..=b'7') => {
                let mut value: i64 = (d - b'0') as i64;
                let mut count = 1;
                for &b in &bytes[2..] {
                    if count >= 3 || !(b'0'..=b'7').contains(&b) {
                        break;
                    }
                    value = value * 8 + (b - b'0') as i64;
                    count += 1;
                }
                Ok(value)
            }
            _ => Err(format!("不支持的转义: {}", text)),
        }
    } else if !inner.is_empty() {
        // 多字符常量（实现定义值）：按 GCC 约定，各字节依次左移 8 位拼接。
        // 用 wrapping_shl 避免 8 字节以上常量移位溢出(debug panic)，与 GCC 截断行为一致
        let mut value: i64 = 0;
        for b in inner.bytes() {
            value = (value.wrapping_shl(8)) | b as i64;
        }
        Ok(value)
    } else {
        Err(format!("非法字符常量: {}", text))
    }
}

struct ExprParser {
    toks: Vec<Token>,
    pos: usize,
    /// discard 模式：当前解析的表达式不会被求值（?: 未选中分支、&&/|| 的
    /// 被短路右操作数）。语法错误仍须报错，但求值类错误（除零/溢出/移位越界）
    /// 不得产生诊断——C89 §3.8.1 规定这些操作数不求值。
    discard: bool,
    /// strict 模式：未定义标识符报错（枚举值/位域宽度/数组界等上下文）。
    /// #if 中保持宽松语义：未定义标识符按 0 求值（C89 §3.8.1）。
    strict: bool,
}
impl ExprParser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }
    fn is_punct(&self, s: &str) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokKind::Punct && t.text == s)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.is_punct(s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    /// 在 discard 模式下运行闭包：求值类错误被抑制，语法错误照常传播。
    fn with_discard<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let saved = self.discard;
        self.discard = true;
        let r = f(self);
        self.discard = saved;
        r
    }

    fn parse_conditional(&mut self) -> Result<ICVal, String> {
        let c = self.parse_logical_or()?;
        if self.eat("?") {
            if self.discard {
                self.parse_conditional()?;
                if !self.eat(":") {
                    return Err("条件表达式缺少 :".into());
                }
                self.parse_conditional()?;
                return Ok(ICVal::S(0));
            }
            if c.is_true() {
                // 取 a，b 仅做语法检查不求值
                let a = self.parse_conditional()?;
                if !self.eat(":") {
                    return Err("条件表达式缺少 :".into());
                }
                self.with_discard(|p| p.parse_conditional())?;
                return Ok(a);
            }
            // 取 b，a 仅做语法检查不求值
            self.with_discard(|p| p.parse_conditional())?;
            if !self.eat(":") {
                return Err("条件表达式缺少 :".into());
            }
            return self.parse_conditional();
        }
        Ok(c)
    }

    fn parse_logical_or(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_logical_and()?;
        while self.is_punct("||") {
            self.pos += 1;
            if self.discard {
                self.parse_logical_and()?;
            } else if v.is_true() {
                // 短路：右操作数不求值
                self.with_discard(|p| p.parse_logical_and())?;
            } else {
                let r = self.parse_logical_and()?;
                v = ICVal::S(r.is_true() as i64);
            }
        }
        Ok(v)
    }

    fn parse_logical_and(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_bitwise_or()?;
        while self.is_punct("&&") {
            self.pos += 1;
            if self.discard {
                self.parse_bitwise_or()?;
            } else if !v.is_true() {
                self.with_discard(|p| p.parse_bitwise_or())?;
            } else {
                let r = self.parse_bitwise_or()?;
                v = ICVal::S(r.is_true() as i64);
            }
        }
        Ok(v)
    }

    fn parse_bitwise_or(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_bitwise_xor()?;
        while self.is_punct("|") {
            self.pos += 1;
            let r = self.parse_bitwise_xor()?;
            if !self.discard {
                v = match (v, r) {
                    (ICVal::S(x), ICVal::S(y)) => ICVal::S(x | y),
                    _ => ICVal::U(v.to_u64() | r.to_u64()),
                };
            }
        }
        Ok(v)
    }
    fn parse_bitwise_xor(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_bitwise_and()?;
        while self.is_punct("^") {
            self.pos += 1;
            let r = self.parse_bitwise_and()?;
            if !self.discard {
                v = match (v, r) {
                    (ICVal::S(x), ICVal::S(y)) => ICVal::S(x ^ y),
                    _ => ICVal::U(v.to_u64() ^ r.to_u64()),
                };
            }
        }
        Ok(v)
    }
    fn parse_bitwise_and(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_equality()?;
        while self.is_punct("&") {
            self.pos += 1;
            let r = self.parse_equality()?;
            if !self.discard {
                v = match (v, r) {
                    (ICVal::S(x), ICVal::S(y)) => ICVal::S(x & y),
                    _ => ICVal::U(v.to_u64() & r.to_u64()),
                };
            }
        }
        Ok(v)
    }
    fn parse_equality(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_relational()?;
        loop {
            if self.eat("==") {
                let r = self.parse_relational()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).4 as i64);
                }
            } else if self.eat("!=") {
                let r = self.parse_relational()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).5 as i64);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_relational(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_shift()?;
        loop {
            if self.eat("<") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).0 as i64);
                }
            } else if self.eat(">") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).1 as i64);
                }
            } else if self.eat("<=") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).2 as i64);
                }
            } else if self.eat(">=") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = ICVal::S(ic_cmp(v, r).3 as i64);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_shift(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_additive()?;
        loop {
            if self.eat("<<") {
                let r = self.parse_additive()?;
                if !self.discard {
                    let amount = r.bits();
                    if !(0..64).contains(&amount) {
                        return Err(format!("移位量超出范围: {}", amount));
                    }
                    v = ic_shl(v, amount as u32);
                }
            } else if self.eat(">>") {
                let r = self.parse_additive()?;
                if !self.discard {
                    let amount = r.bits();
                    if !(0..64).contains(&amount) {
                        return Err(format!("移位量超出范围: {}", amount));
                    }
                    v = ic_shr(v, amount as u32);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_additive(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_multiplicative()?;
        loop {
            if self.eat("+") {
                let r = self.parse_multiplicative()?;
                if !self.discard {
                    v = ic_add(v, r);
                }
            } else if self.eat("-") {
                let r = self.parse_multiplicative()?;
                if !self.discard {
                    v = ic_sub(v, r);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_multiplicative(&mut self) -> Result<ICVal, String> {
        let mut v = self.parse_unary()?;
        loop {
            if self.eat("*") {
                let r = self.parse_unary()?;
                if !self.discard {
                    v = ic_mul(v, r);
                }
            } else if self.eat("/") {
                let d = self.parse_unary()?;
                if !self.discard {
                    v = ic_div(v, d)?;
                }
            } else if self.eat("%") {
                let d = self.parse_unary()?;
                if !self.discard {
                    v = ic_rem(v, d)?;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_unary(&mut self) -> Result<ICVal, String> {
        if self.eat("!") {
            let r = self.parse_unary()?;
            return Ok(if self.discard {
                ICVal::S(0)
            } else {
                ICVal::S((!r.is_true()) as i64)
            });
        }
        if self.eat("~") {
            let r = self.parse_unary()?;
            return Ok(if self.discard {
                ICVal::S(0)
            } else {
                match r {
                    ICVal::S(v) => ICVal::S(!v),
                    ICVal::U(v) => ICVal::U((!v) & 0xFFFF_FFFF),
                }
            });
        }
        if self.eat("-") {
            let r = self.parse_unary()?;
            return Ok(if self.discard {
                ICVal::S(0)
            } else {
                match r {
                    ICVal::S(v) => ICVal::S(v.wrapping_neg()),
                    ICVal::U(v) => ICVal::U(0u64.wrapping_sub(v) & 0xFFFF_FFFF),
                }
            });
        }
        if self.eat("+") {
            return self.parse_unary();
        }
        // 强转：(类型名) cast-expression —— 枚举值等常量上下文需要。
        // 无符号整型强转改变符号性；有符号强转保持值不变（教学模型不窄化）。
        if self.is_punct("(") {
            let type_like = matches!(
                self.toks.get(self.pos + 1),
                Some(t) if t.kind == TokKind::Ident
                    && matches!(
                        t.text.as_str(),
                        "char"
                            | "short"
                            | "int"
                            | "long"
                            | "float"
                            | "double"
                            | "signed"
                            | "unsigned"
                            | "const"
                            | "volatile"
                    )
            );
            if type_like {
                let save = self.pos;
                self.pos += 1; // (
                let mut is_unsigned = false;
                let mut is_char = false;
                let mut is_short = false;
                loop {
                    let done = !matches!(self.peek(), Some(t)
                        if t.kind == TokKind::Ident
                            && matches!(
                                t.text.as_str(),
                                "char"
                                    | "short"
                                    | "int"
                                    | "long"
                                    | "float"
                                    | "double"
                                    | "signed"
                                    | "unsigned"
                                    | "const"
                                    | "volatile"
                            ));
                    if done {
                        break;
                    }
                    match self.peek().map(|t| t.text.as_str()) {
                        Some("unsigned") => is_unsigned = true,
                        Some("char") => is_char = true,
                        Some("short") => is_short = true,
                        _ => {}
                    }
                    self.pos += 1;
                }
                while self.is_punct("*") {
                    self.pos += 1;
                }
                if !self.eat(")") {
                    // 不是类型转换 → 回退，按括号表达式解析
                    self.pos = save;
                    return self.parse_primary();
                }
                let v = self.parse_unary()?;
                return Ok(if self.discard {
                    ICVal::S(0)
                } else if is_unsigned && !is_char && !is_short {
                    // (unsigned) 与 (unsigned long)：本目标 long=32 位，
                    // 无符号强转改变符号性（(unsigned long)-1 == 4294967295）
                    ICVal::U(v.to_u64())
                } else {
                    // 窄化强转（C89 §3.3.4）；char 有符号（本实现约定）
                    let x = v.to_u64();
                    ICVal::S(if is_char {
                        if is_unsigned {
                            x as u8 as i64
                        } else {
                            x as u8 as i8 as i64
                        }
                    } else if is_short {
                        if is_unsigned {
                            x as u16 as i64
                        } else {
                            x as u16 as i16 as i64
                        }
                    } else {
                        x as u32 as i32 as i64
                    })
                });
            }
        }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> Result<ICVal, String> {
        let t = self.peek().cloned().ok_or("表达式意外结束")?;
        match t.kind {
            TokKind::Number => {
                self.pos += 1;
                let (value, unsigned, _long) = parse_int_literal(&t.text)?;
                // 无后缀常量超出 int 范围时按 C89 §3.1.3.2 自动升为无符号；
                // U 值按模 2^32 语义存储（与 ic_add 等运算保持一致，
                // 否则 #if 4294967296 与 #if 4294967296+0 结果不一致）
                Ok(if unsigned || value > i32::MAX as i64 {
                    ICVal::U(value as u64 & 0xFFFF_FFFF)
                } else {
                    ICVal::S(value)
                })
            }
            TokKind::Char => {
                self.pos += 1;
                Ok(ICVal::S(parse_char_value(&t.text)?))
            }
            TokKind::Ident if t.text == "sizeof" => {
                self.pos += 1;
                Ok(ICVal::S(self.parse_sizeof()?))
            }
            TokKind::Ident => {
                self.pos += 1;
                if self.strict {
                    return Err(format!("未定义的标识符: {}", t.text));
                }
                // 未定义标识符（宏已展开过）→ 0（标准行为）
                Ok(ICVal::S(0))
            }
            TokKind::Punct if t.text == "(" => {
                self.pos += 1;
                let v = self.parse_conditional()?;
                if !self.eat(")") {
                    return Err("缺少右括号 )".into());
                }
                Ok(v)
            }
            _ => Err(format!("无法解析的表达式 token: {}", t.text)),
        }
    }

    /// sizeof(类型名)：支持内建标量类型（#if 整数常量表达式子集）。
    /// 结构体/联合体大小依赖 TypeTable，预处理阶段不可知，明确报错。
    fn parse_sizeof(&mut self) -> Result<i64, String> {
        if !self.eat("(") {
            return Err("sizeof 后缺少 (".into());
        }
        // const/volatile 限定符可出现在类型名前（sizeof(const int)，C89 §3.5.3）
        loop {
            let done = !matches!(self.peek(), Some(t)
                if t.kind == TokKind::Ident
                    && matches!(t.text.as_str(), "const" | "volatile"));
            if done {
                break;
            }
            self.pos += 1;
        }
        let t = self.peek().cloned().ok_or("sizeof 表达式意外结束")?;
        if t.kind == TokKind::Ident {
            let name = t.text.clone();
            if name == "unsigned" || name == "signed" {
                self.pos += 1;
                // unsigned/signed 后也可跟限定符
                loop {
                    let done = !matches!(self.peek(), Some(t)
                        if t.kind == TokKind::Ident
                            && matches!(t.text.as_str(), "const" | "volatile"));
                    if done {
                        break;
                    }
                    self.pos += 1;
                }
                // 单独出现：sizeof(unsigned) / sizeof(signed) → int（4 字节）
                if self.is_punct(")") {
                    self.pos += 1;
                    return Ok(4);
                }
                let base = self.peek().cloned().ok_or("sizeof 表达式意外结束")?;
                if base.kind != TokKind::Ident {
                    return Err(format!("sizeof {} 后应为类型名", name));
                }
                self.pos += 1;
                let size = match base.text.as_str() {
                    "char" => 1i64,
                    "short" => 2,
                    "int" | "long" => 4,
                    _ => return Err(format!("sizeof 不支持的类型: {} {}", name, base.text)),
                };
                return self.finish_sizeof(size);
            }
            if name == "void" {
                self.pos += 1;
                // 仅指针合法：sizeof(void*) = 4；sizeof(void) 是约束违反
                if !self.is_punct("*") {
                    return Err("sizeof(void) 无定义".into());
                }
                return self.finish_sizeof(4);
            }
            match name.as_str() {
                "char" => {
                    self.pos += 1;
                    return self.finish_sizeof(1);
                }
                "short" => {
                    self.pos += 1;
                    return self.finish_sizeof(2);
                }
                "int" | "float" | "long" => {
                    self.pos += 1;
                    return self.finish_sizeof(4);
                }
                "double" => {
                    self.pos += 1;
                    return self.finish_sizeof(8);
                }
                "struct" | "union" | "enum" => {
                    return Err(format!("sizeof({}) 依赖布局，预处理阶段不支持", name));
                }
                _ => return Err(format!("sizeof 不支持的类型: {}", name)),
            }
        }
        Err("sizeof 表达式操作数暂不支持（#if 中仅支持类型名）".into())
    }

    /// 吸收尾部限定符/指针并消耗右括号；含 * 的指针类型一律 4 字节。
    fn finish_sizeof(&mut self, base: i64) -> Result<i64, String> {
        let mut extra = 0i64;
        loop {
            let done = !self.is_punct("*")
                && !matches!(self.peek(), Some(t) if t.kind == TokKind::Ident
                    && matches!(t.text.as_str(), "const" | "volatile"));
            if done {
                break;
            }
            if self.is_punct("*") {
                extra += 1;
            }
            self.pos += 1;
        }
        if !self.eat(")") {
            return Err("sizeof 缺少右括号 )".into());
        }
        Ok(if extra > 0 { 4 } else { base })
    }
}

/// 求值 #if 常量表达式（token 序列；未定义标识符按 0 处理——标准行为）
pub fn eval_expr(toks: &[Token]) -> Result<i64, String> {
    eval_expr_mode(toks, false)
}

/// 严格求值：未定义标识符报错。枚举值/位域宽度/数组界等上下文
/// 不允许前向引用与任意变量名（C89 整型常量表达式约束）。
pub fn eval_expr_strict(toks: &[Token]) -> Result<i64, String> {
    eval_expr_mode(toks, true)
}

fn eval_expr_mode(toks: &[Token], strict: bool) -> Result<i64, String> {
    let filtered: Vec<Token> = toks
        .iter()
        .filter(|t| t.kind != TokKind::Newline)
        .cloned()
        .collect();
    let mut p = ExprParser {
        toks: filtered,
        pos: 0,
        discard: false,
        strict,
    };
    let v = p.parse_conditional()?;
    if p.pos != p.toks.len() {
        return Err(format!("表达式末尾有多余 token: {:?}", p.toks[p.pos].text));
    }
    Ok(v.bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn ev(src: &str) -> i64 {
        let toks: Vec<Token> = tokenize(src)
            .unwrap()
            .into_iter()
            .filter(|t| t.kind != TokKind::Newline && t.kind != TokKind::Eof)
            .collect();
        eval_expr(&toks).unwrap()
    }

    #[test]
    fn test_eval_arithmetic() {
        assert_eq!(ev("1 + 2 * 3"), 7);
        assert_eq!(ev("(1 + 2) * 3"), 9);
        assert_eq!(ev("10 / 3"), 3);
        assert_eq!(ev("10 % 3"), 1);
        assert_eq!(ev("1 << 4"), 16);
        assert_eq!(ev("0xFF"), 255);
        assert_eq!(ev("010"), 8); // 八进制
    }

    #[test]
    fn test_eval_compare_logic() {
        assert_eq!(ev("1 < 2"), 1);
        assert_eq!(ev("3 >= 4"), 0);
        assert_eq!(ev("1 == 1 && 2 != 3"), 1);
        assert_eq!(ev("1 || 0"), 1);
        assert_eq!(ev("!0"), 1);
        assert_eq!(ev("~0"), -1);
        assert_eq!(ev("1 ? 10 : 20"), 10);
        assert_eq!(ev("0 ? 10 : 20"), 20);
    }

    #[test]
    fn test_eval_unsigned_semantics() {
        // C89 §3.2.1.5 通常算术转换：-1 与 0u 比较按无符号进行
        assert_eq!(ev("-1 < 0u"), 0);
        assert_eq!(ev("-1 < 0U"), 0);
        assert_eq!(ev("(unsigned)-1 < 0"), 0);
        assert_eq!(ev("0u - 1 > 0"), 1);
        assert_eq!(ev("4294967295u + 1 == 0"), 1);
        assert_eq!(ev("0xFFFFFFFFu == 4294967295"), 1);
        assert_eq!(ev("1u < 2"), 1);
        assert_eq!(ev("-1u"), -1); // 无符号取负按 32 位回绕，位型符号扩展
        assert_eq!(ev("~0u"), -1);
        assert_eq!(ev("(unsigned char)300"), 44); // 窄化
        assert_eq!(ev("(char)300"), 44);
    }

    #[test]
    fn test_eval_bad_suffix_rejected() {
        let toks: Vec<Token> = tokenize("42ll")
            .unwrap()
            .into_iter()
            .filter(|t| t.kind != TokKind::Newline && t.kind != TokKind::Eof)
            .collect();
        assert!(eval_expr(&toks).is_err());
    }

    #[test]
    fn test_eval_char() {
        assert_eq!(ev("'a'"), 97);
        assert_eq!(ev("'\\n'"), 10);
    }
}
