//! 预处理阶段 1~3：行拼接 + 删注释 + 切分预处理记号
//!
//! 对照 C89 标准（c89-draft.html）：
//! - §2.1.1.2 阶段 2：反斜杠+换行 → 删除（行拼接）
//! - §2.1.1.2 阶段 3：源码分解为预处理记号，注释替换为空格
//! - §3.1 词法元素（pp-number 规则 §3.1.8）

/// 预处理记号类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    /// 标识符（含关键字，区分留给语法分析）
    Ident,
    /// 预处理数字（pp-number）
    Number,
    /// 字符串字面量
    String,
    /// 字符常量
    Char,
    /// 运算符/标点
    Punct,
    /// 换行（预处理器按行工作，保留给 # 指令识别）
    Newline,
    /// 文件结束
    Eof,
}

/// 一个预处理记号
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokKind,
    pub text: String,
    pub line: u32,
    /// Token 所属源文件；内存输入使用 <source>。
    pub file: String,
    /// Token 起始列号。旧 API 默认从第 1 列开始。
    pub column: u32,
    /// 该 token 之前是否有空白或注释（函数宏调用判定用）
    pub has_space: bool,
}

impl Token {
    pub(crate) fn new(kind: TokKind, text: String, line: u32, has_space: bool) -> Self {
        Token {
            kind,
            text,
            line,
            file: "<source>".into(),
            column: 1,
            has_space,
        }
    }

    pub(crate) fn new_at(
        kind: TokKind,
        text: String,
        line: u32,
        column: u32,
        has_space: bool,
    ) -> Self {
        let mut token = Self::new(kind, text, line, has_space);
        token.column = column.max(1);
        token
    }

    pub(crate) fn set_file(&mut self, file: impl Into<String>) {
        self.file = file.into();
    }
}

/// 阶段 2：行拼接 —— 删除"反斜杠+换行"
fn splice_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            // \ + \n 或 \ + \r\n
            if i + 1 < chars.len() && chars[i + 1] == '\n' {
                i += 2;
                continue;
            }
            if i + 2 < chars.len() && chars[i + 1] == '\r' && chars[i + 2] == '\n' {
                i += 3;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn peek(chars: &[char], i: usize, expect: char) -> bool {
    i + 1 < chars.len() && chars[i + 1] == expect
}
/// 阶段 3：切分预处理记号（注释 → 空格）
pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let input = input.strip_prefix("\u{feff}").unwrap_or(input);
    let src = splice_lines(input);
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut line_start = 0usize;
    let mut toks = Vec::new();
    let mut space = false; // 记录下一个 token 前是否有空白/注释

    while i < chars.len() {
        let c = chars[i];

        // 空白：跳过
        if c == ' ' || c == '\t' || c == '\r' {
            space = true;
            i += 1;
            continue;
        }
        // 换行：保留（预处理器按行工作）。换行同样是空白——
        // 其后 token 的 has_space 置位（F\n(1) 不是函数宏调用）。
        if c == '\n' {
            space = true;
            let column = (i - line_start + 1) as u32;
            toks.push(Token::new_at(
                TokKind::Newline,
                "\n".into(),
                line,
                column,
                true,
            ));
            line += 1;
            i += 1;
            line_start = i;
            continue;
        }
        // /* 块注释 */
        if c == '/' && peek(&chars, i, '*') {
            let start = line;
            i += 2;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '*' && peek(&chars, i, '/') {
                    i += 2;
                    closed = true;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                    line_start = i + 1;
                }
                i += 1;
            }
            if !closed {
                return Err(format!("第 {} 行: 注释未闭合", start));
            }
            space = true; // 注释 → 一个空格
            continue;
        }
        // 字符串字面量
        if c == '"' {
            let start = line;
            let start_column = (i - line_start + 1) as u32;
            let mut s = String::new();
            s.push(c);
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                s.push(ch);
                if ch == '\\' {
                    // 转义字符：吃掉下一个字符
                    if i + 1 < chars.len() {
                        s.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                }
                i += 1;
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    break;
                } // 跨行 → 未闭合
            }
            if !closed {
                return Err(format!("第 {} 行: 字符串未闭合", start));
            }
            toks.push(Token::new_at(
                TokKind::String,
                s,
                start,
                start_column,
                space,
            ));
            space = false;
            continue;
        }
        // 字符常量
        if c == '\'' {
            let start = line;
            let start_column = (i - line_start + 1) as u32;
            let mut s = String::new();
            s.push(c);
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                s.push(ch);
                if ch == '\\' && i + 1 < chars.len() {
                    s.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if ch == '\'' {
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    break;
                }
            }
            if !closed {
                return Err(format!("第 {} 行: 字符常量未闭合", start));
            }
            toks.push(Token::new_at(TokKind::Char, s, start, start_column, space));
            space = false;
            continue;
        }
        // 标识符：字母或 _ 开头
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            toks.push(Token::new_at(
                TokKind::Ident,
                text,
                line,
                (start - line_start + 1) as u32,
                space,
            ));
            space = false;
            continue;
        }
        // 预处理数字（pp-number）：digit 开头（C89 §3.1.8）
        if c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch.is_ascii_alphanumeric()
                    || ch == '.'
                    || ch == '_'
                    || ((ch == '+' || ch == '-') && (chars[i - 1] == 'e' || chars[i - 1] == 'E'))
                {
                    i += 1;
                } else {
                    break;
                }
            }
            let text: String = chars[start..i].iter().collect();
            toks.push(Token::new_at(
                TokKind::Number,
                text,
                line,
                (start - line_start + 1) as u32,
                space,
            ));
            space = false;
            continue;
        }
        // 标点/运算符（多字符优先）
        if let Some(p) = read_punct(&chars, i) {
            let n = p.chars().count();
            toks.push(Token::new_at(
                TokKind::Punct,
                p,
                line,
                (i - line_start + 1) as u32,
                space,
            ));
            space = false;
            i += n;
            continue;
        }
        return Err(format!("第 {} 行: 无法识别的字符 '{}'", line, c));
    }

    toks.push(Token::new_at(
        TokKind::Eof,
        String::new(),
        line,
        (i - line_start + 1) as u32,
        false,
    ));
    Ok(toks)
}
/// 读运算符/标点（C89 §3.1.6，多字符优先）
pub fn tokenize_with_file(input: &str, file: &str) -> Result<Vec<Token>, String> {
    let mut tokens = tokenize(input)?;
    for token in &mut tokens {
        token.set_file(file);
    }
    Ok(tokens)
}

/// C89 关键字（§3.1.1）：在任何上下文中都不能用作标识符。
/// 与语法分析器共用（表达式标识符检查、宏名检查等）。
pub fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "auto"
            | "break"
            | "case"
            | "char"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extern"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "int"
            | "long"
            | "register"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "unsigned"
            | "void"
            | "volatile"
            | "while"
    )
}

pub fn tokenize_diagnostic(
    input: &str,
    file: &str,
) -> crate::diagnostics::CompileResult<Vec<Token>> {
    tokenize_with_file(input, file).map_err(|message| {
        let span = lex_error_span(input, file, &message);
        crate::diagnostics::CompileError::lex(span, message)
    })
}

pub(crate) fn lex_error_span(input: &str, file: &str, message: &str) -> crate::diagnostics::Span {
    let mut span = crate::diagnostics::legacy_span(file, message);
    if let Some(line) = input.lines().nth(span.line.saturating_sub(1) as usize) {
        let target = message
            .rsplit_once('\'')
            .and_then(|(before, _)| before.rsplit_once('\''))
            .and_then(|(_, value)| value.chars().next());
        if let Some(target) = target {
            if let Some(column) = line.chars().position(|ch| ch == target) {
                span.column = column as u32 + 1;
            }
        } else if message.contains("字符串未闭合") {
            span.column = line
                .chars()
                .position(|ch| ch == '"')
                .map_or(1, |v| v as u32 + 1);
        } else if message.contains("字符常量未闭合") {
            span.column = line
                .chars()
                .position(|ch| ch == '\'')
                .map_or(1, |v| v as u32 + 1);
        }
    }
    span
}

fn read_punct(chars: &[char], i: usize) -> Option<String> {
    let rest: String = chars[i..].iter().take(3).collect();
    // 三字符
    if rest.starts_with("...") {
        return Some("...".into());
    }
    // 双字符
    const DOUBLE: [&str; 22] = [
        ">>=", "<<=", "->", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "*=", "/=",
        "%=", "+=", "-=", "&=", "^=", "|=", "##",
    ];
    for p in DOUBLE {
        if rest.starts_with(p) {
            return Some(p.to_string());
        }
    }
    // 单字符
    let c = chars[i];
    if "[](){}.,;:?~!%^&*+-=/<>|#".contains(c) {
        return Some(c.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 去掉换行/Eof，取 token 文本列表
    fn texts(src: &str) -> Vec<String> {
        tokenize(src)
            .unwrap()
            .iter()
            .filter(|t| t.kind != TokKind::Newline && t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        assert_eq!(
            texts("int main() { return 42; }"),
            ["int", "main", "(", ")", "{", "return", "42", ";", "}"]
        );
    }

    #[test]
    fn test_splice_lines() {
        // 反斜杠续行
        assert_eq!(
            texts(
                "#define X \\
42"
            ),
            ["#", "define", "X", "42"]
        );
    }

    #[test]
    fn test_block_comment() {
        assert_eq!(texts("int /* comment */ x;"), ["int", "x", ";"]);
        // 注释跨多行
        assert_eq!(texts("int /* a\nb */ x;"), ["int", "x", ";"]);
    }

    #[test]
    fn test_line_comment_is_not_supported() {
        // C89 只有 /* */ 注释；// 会被当作两个除号
        assert_eq!(
            texts("int x; // hi\nint y;"),
            ["int", "x", ";", "/", "/", "hi", "int", "y", ";"]
        );
    }

    #[test]
    fn test_string_not_comment() {
        // 字符串里的注释符号不是注释！
        assert_eq!(
            texts("char* s = \"/* not a comment */\";"),
            ["char", "*", "s", "=", "\"/* not a comment */\"", ";"]
        );
    }

    #[test]
    fn test_numbers() {
        assert_eq!(
            texts("1 0x1F 3.14 1e10 1e-5 42uL 0b101"),
            ["1", "0x1F", "3.14", "1e10", "1e-5", "42uL", "0b101"]
        );
    }

    #[test]
    fn test_pp_number_dot_start_is_c89_split() {
        // C89 §3.1.8：. 开头不是 pp-number，.5 拆为 . 和 5
        assert_eq!(texts(".5 .5e-2"), [".", "5", ".", "5e-2"]);
        assert_eq!(texts("a.b ..."), ["a", ".", "b", "..."]);
    }

    #[test]
    fn test_puncts() {
        assert_eq!(
            texts("a->b c<<=d e...f g##h"),
            ["a", "->", "b", "c", "<<=", "d", "e", "...", "f", "g", "##", "h"]
        );
    }

    #[test]
    fn test_char_literal() {
        assert_eq!(texts("char c = 'a';"), ["char", "c", "=", "'a'", ";"]);
        assert_eq!(texts("char n = '\\n';"), ["char", "n", "=", "'\\n'", ";"]);
    }

    #[test]
    fn test_unclosed_comment() {
        assert!(tokenize("/* oops").is_err());
    }

    #[test]
    fn test_unclosed_string() {
        assert!(tokenize("\"oops").is_err());
    }

    #[test]
    fn test_token_source_span() {
        let tokens = tokenize_with_file("  int value;", "span.c").unwrap();
        assert_eq!(tokens[0].file, "span.c");
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].column, 3);
    }

    #[test]
    fn test_diagnostic_column() {
        let error = tokenize_diagnostic("int x = @;", "bad.c").unwrap_err();
        assert_eq!(error.span.file, "bad.c");
        assert_eq!(error.span.line, 1);
        assert_eq!(error.span.column, 9);
        assert_eq!(error.stage, crate::diagnostics::DiagnosticStage::Lex);
    }

    #[test]
    fn test_utf8_bom() {
        assert_eq!(
            texts("\u{feff}int main(void) { return 0; }"),
            texts("int main(void) { return 0; }")
        );
    }

    #[test]
    fn test_line_numbers() {
        let toks = tokenize("int a;\nint b;").unwrap();
        let lines: Vec<u32> = toks.iter().map(|t| t.line).collect();
        assert!(lines.contains(&1));
        assert!(lines.contains(&2));
    }
}
