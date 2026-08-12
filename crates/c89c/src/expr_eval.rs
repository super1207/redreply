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
                Ok(value & 0xFF)
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
                Ok(value & 0xFF)
            }
            _ => Err(format!("不支持的转义: {}", text)),
        }
    } else if inner.chars().count() == 1 {
        Ok(inner.chars().next().unwrap() as i64)
    } else {
        Err(format!("非法字符常量: {}", text))
    }
}

struct ExprParser {
    toks: Vec<Token>,
    pos: usize,
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

    fn parse_conditional(&mut self) -> Result<i64, String> {
        let c = self.parse_logical_or()?;
        if self.eat("?") {
            let a = self.parse_conditional()?;
            if !self.eat(":") {
                return Err("条件表达式缺少 :".into());
            }
            let b = self.parse_conditional()?;
            return Ok(if c != 0 { a } else { b });
        }
        Ok(c)
    }

    fn parse_logical_or(&mut self) -> Result<i64, String> {
        let mut v = self.parse_logical_and()?;
        while self.is_punct("||") {
            self.pos += 1;
            let r = self.parse_logical_and()?;
            v = if v != 0 || r != 0 { 1 } else { 0 };
        }
        Ok(v)
    }

    fn parse_logical_and(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_or()?;
        while self.is_punct("&&") {
            self.pos += 1;
            let r = self.parse_bitwise_or()?;
            v = if v != 0 && r != 0 { 1 } else { 0 };
        }
        Ok(v)
    }

    fn parse_bitwise_or(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_xor()?;
        while self.is_punct("|") {
            self.pos += 1;
            v |= self.parse_bitwise_xor()?;
        }
        Ok(v)
    }
    fn parse_bitwise_xor(&mut self) -> Result<i64, String> {
        let mut v = self.parse_bitwise_and()?;
        while self.is_punct("^") {
            self.pos += 1;
            v ^= self.parse_bitwise_and()?;
        }
        Ok(v)
    }
    fn parse_bitwise_and(&mut self) -> Result<i64, String> {
        let mut v = self.parse_equality()?;
        while self.is_punct("&") {
            self.pos += 1;
            v &= self.parse_equality()?;
        }
        Ok(v)
    }
    fn parse_equality(&mut self) -> Result<i64, String> {
        let mut v = self.parse_relational()?;
        loop {
            if self.eat("==") {
                v = (v == self.parse_relational()?) as i64;
            } else if self.eat("!=") {
                v = (v != self.parse_relational()?) as i64;
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
                v = (v < self.parse_shift()?) as i64;
            } else if self.eat(">") {
                v = (v > self.parse_shift()?) as i64;
            } else if self.eat("<=") {
                v = (v <= self.parse_shift()?) as i64;
            } else if self.eat(">=") {
                v = (v >= self.parse_shift()?) as i64;
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
                v <<= self.parse_additive()?;
            } else if self.eat(">>") {
                v >>= self.parse_additive()?;
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
                v = v.wrapping_add(self.parse_multiplicative()?);
            } else if self.eat("-") {
                v = v.wrapping_sub(self.parse_multiplicative()?);
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
                v *= self.parse_unary()?;
            } else if self.eat("/") {
                let d = self.parse_unary()?;
                if d == 0 {
                    return Err("除零".into());
                }
                v /= d;
            } else if self.eat("%") {
                let d = self.parse_unary()?;
                if d == 0 {
                    return Err("模零".into());
                }
                v %= d;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn parse_unary(&mut self) -> Result<i64, String> {
        if self.eat("!") {
            return Ok((self.parse_unary()? == 0) as i64);
        }
        if self.eat("~") {
            return Ok(!self.parse_unary()?);
        }
        if self.eat("-") {
            return Ok(-self.parse_unary()?);
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
