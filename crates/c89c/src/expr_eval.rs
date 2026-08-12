//! 预处理常量表达式求值器（#if 用，C89 整数常量表达式子集）

use crate::lexer::{TokKind, Token};

/// 解析整数 token 文本（支持 0x/八进制/十进制/后缀）
pub(crate) fn parse_int_value(text: &str) -> Result<i64, String> {
    let t = text.trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).map_err(|_| format!("非法十六进制: {}", text))
    } else if t.len() > 1 && t.starts_with('0') {
        // 八进制
        i64::from_str_radix(&t[1..], 8).map_err(|_| format!("非法八进制: {}", text))
    } else {
        t.parse::<i64>().map_err(|_| format!("非法整数: {}", text))
    }
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
                    value = value * 16 + digit as i64;
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
        let mut value: i64 = 0;
        for b in inner.bytes() {
            value = (value << 8) | b as i64;
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

    fn parse_conditional(&mut self) -> Result<i64, String> {
        let c = self.parse_logical_or()?;
        if self.eat("?") {
            if self.discard {
                self.parse_conditional()?;
                if !self.eat(":") {
                    return Err("条件表达式缺少 :".into());
                }
                self.parse_conditional()?;
                return Ok(0);
            }
            if c != 0 {
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

    fn parse_logical_or(&mut self) -> Result<i64, String> {
        let mut v = self.parse_logical_and()?;
        while self.is_punct("||") {
            self.pos += 1;
            if self.discard {
                self.parse_logical_and()?;
            } else if v != 0 {
                // 短路：右操作数不求值
                self.with_discard(|p| p.parse_logical_and())?;
            } else {
                let r = self.parse_logical_and()?;
                v = (r != 0) as i64;
            }
        }
        Ok(v)
    }

    fn parse_logical_and(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_or()?;
        while self.is_punct("&&") {
            self.pos += 1;
            if self.discard {
                self.parse_bitwise_or()?;
            } else if v == 0 {
                self.with_discard(|p| p.parse_bitwise_or())?;
            } else {
                let r = self.parse_bitwise_or()?;
                v = (r != 0) as i64;
            }
        }
        Ok(v)
    }

    fn parse_bitwise_or(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_xor()?;
        while self.is_punct("|") {
            self.pos += 1;
            let r = self.parse_bitwise_xor()?;
            if !self.discard {
                v |= r;
            }
        }
        Ok(v)
    }
    fn parse_bitwise_xor(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_and()?;
        while self.is_punct("^") {
            self.pos += 1;
            let r = self.parse_bitwise_and()?;
            if !self.discard {
                v ^= r;
            }
        }
        Ok(v)
    }
    fn parse_bitwise_and(&mut self) -> Result<i64, String> {
        let mut v = self.parse_equality()?;
        while self.is_punct("&") {
            self.pos += 1;
            let r = self.parse_equality()?;
            if !self.discard {
                v &= r;
            }
        }
        Ok(v)
    }
    fn parse_equality(&mut self) -> Result<i64, String> {
        let mut v = self.parse_relational()?;
        loop {
            if self.eat("==") {
                let r = self.parse_relational()?;
                if !self.discard {
                    v = (v == r) as i64;
                }
            } else if self.eat("!=") {
                let r = self.parse_relational()?;
                if !self.discard {
                    v = (v != r) as i64;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_relational(&mut self) -> Result<i64, String> {
        let mut v = self.parse_shift()?;
        loop {
            if self.eat("<") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = (v < r) as i64;
                }
            } else if self.eat(">") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = (v > r) as i64;
                }
            } else if self.eat("<=") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = (v <= r) as i64;
                }
            } else if self.eat(">=") {
                let r = self.parse_shift()?;
                if !self.discard {
                    v = (v >= r) as i64;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_shift(&mut self) -> Result<i64, String> {
        let mut v = self.parse_additive()?;
        loop {
            if self.eat("<<") {
                let r = self.parse_additive()?;
                if !self.discard {
                    if !(0..64).contains(&r) {
                        return Err(format!("移位量超出范围: {}", r));
                    }
                    v = v.wrapping_shl(r as u32);
                }
            } else if self.eat(">>") {
                let r = self.parse_additive()?;
                if !self.discard {
                    if !(0..64).contains(&r) {
                        return Err(format!("移位量超出范围: {}", r));
                    }
                    v = v.wrapping_shr(r as u32);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_additive(&mut self) -> Result<i64, String> {
        let mut v = self.parse_multiplicative()?;
        loop {
            if self.eat("+") {
                let r = self.parse_multiplicative()?;
                if !self.discard {
                    v = v.wrapping_add(r);
                }
            } else if self.eat("-") {
                let r = self.parse_multiplicative()?;
                if !self.discard {
                    v = v.wrapping_sub(r);
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_multiplicative(&mut self) -> Result<i64, String> {
        let mut v = self.parse_unary()?;
        loop {
            if self.eat("*") {
                let r = self.parse_unary()?;
                if !self.discard {
                    v = v.wrapping_mul(r);
                }
            } else if self.eat("/") {
                let d = self.parse_unary()?;
                if !self.discard {
                    if d == 0 {
                        return Err("除零".into());
                    }
                    v = v.checked_div(d).ok_or("除法溢出")?;
                }
            } else if self.eat("%") {
                let d = self.parse_unary()?;
                if !self.discard {
                    if d == 0 {
                        return Err("模零".into());
                    }
                    v = v.checked_rem(d).ok_or("取模溢出")?;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_unary(&mut self) -> Result<i64, String> {
        if self.eat("!") {
            let r = self.parse_unary()?;
            return Ok(if self.discard { 0 } else { (r == 0) as i64 });
        }
        if self.eat("~") {
            let r = self.parse_unary()?;
            return Ok(if self.discard { 0 } else { !r });
        }
        if self.eat("-") {
            let r = self.parse_unary()?;
            return Ok(if self.discard { 0 } else { r.wrapping_neg() });
        }
        if self.eat("+") {
            return self.parse_unary();
        }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> Result<i64, String> {
        let t = self.peek().cloned().ok_or("表达式意外结束")?;
        match t.kind {
            TokKind::Number => {
                self.pos += 1;
                parse_int_value(&t.text)
            }
            TokKind::Char => {
                self.pos += 1;
                parse_char_value(&t.text)
            }
            TokKind::Ident if t.text == "sizeof" => {
                self.pos += 1;
                self.parse_sizeof()
            }
            TokKind::Ident => {
                // 未定义标识符（宏已展开过）→ 0（标准行为）
                self.pos += 1;
                Ok(0)
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
        let t = self.peek().cloned().ok_or("sizeof 表达式意外结束")?;
        if t.kind == TokKind::Ident {
            let name = t.text.clone();
            if name == "unsigned" || name == "signed" {
                self.pos += 1;
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
                "void" => return Err("sizeof(void) 无定义".into()),
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

/// 求值 #if 常量表达式（token 序列）
pub fn eval_expr(toks: &[Token]) -> Result<i64, String> {
    let filtered: Vec<Token> = toks
        .iter()
        .filter(|t| t.kind != TokKind::Newline)
        .cloned()
        .collect();
    let mut p = ExprParser {
        toks: filtered,
        pos: 0,
        discard: false,
    };
    let v = p.parse_conditional()?;
    if p.pos != p.toks.len() {
        return Err(format!("表达式末尾有多余 token: {:?}", p.toks[p.pos].text));
    }
    Ok(v)
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
    fn test_eval_char() {
        assert_eq!(ev("'a'"), 97);
        assert_eq!(ev("'\\n'"), 10);
    }
}
