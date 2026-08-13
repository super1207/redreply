//! 语法分析器（递归下降）
//!
//! 对照 C89 标准 §3.3 表达式（P1）：
//! expr → assign(,assign)* → cond(?:) → || → && → | → ^ → &
//!      → ==!= → 关系 → 移位 → 加减 → 乘除模 → cast → unary → postfix → primary
//!
//! P2 语句 / P3 声明 后续添加。

use crate::ast::{AssignOp, BinaryOp, DeclSpec, Node, Param, Storage, Type, UnaryOp};
use crate::expr_eval::eval_expr_strict;
use crate::expr_eval::{parse_char_value, parse_int_literal};
use crate::lexer::{is_keyword, TokKind, Token};
use crate::types::TypeTable;
use std::collections::HashMap;

/// 判断数字字面量是否为浮点（含 . 或 e/E，排除 0x 十六进制）
fn is_float_literal(text: &str) -> bool {
    if text.starts_with("0x") || text.starts_with("0X") {
        return false;
    }
    text.contains('.') || text.contains('e') || text.contains('E')
}

/// 声明符语法树（§3.5.4 螺旋解析）
#[derive(Debug)]
enum DeclNode {
    Name(String),
    Pointer(Box<DeclNode>, bool, bool),
    Array(Box<DeclNode>, Option<usize>),
    Func(Box<DeclNode>, Vec<Param>, bool, bool), // 变参、是否有原型
}

/// 递归下降解析器
type FunctionParams = (Vec<Param>, bool, bool);
type DeclaratorResult = (Type, String, Option<FunctionParams>);

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// typedef 名 → 真实类型（解析 T *p 时区分类型名与变量名）
    typedefs: HashMap<String, Type>,
    /// 类型表（struct/enum 定义）
    pub tt: TypeTable,
    /// 枚举常量表（RED → 0）
    enum_constants: HashMap<String, i64>,
    /// 表达式/声明符递归深度（防御深度嵌套栈溢出）
    expr_depth: usize,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser {
            toks,
            pos: 0,
            typedefs: HashMap::new(),
            tt: TypeTable::new(),
            enum_constants: HashMap::new(),
            expr_depth: 0,
        }
    }

    /// 递归深度护栏：cast/一元链、括号声明符各自经此计数。
    fn enter_depth(&mut self) -> Result<(), String> {
        self.expr_depth += 1;
        if self.expr_depth > 1024 {
            self.expr_depth -= 1;
            return Err(self.err("表达式或声明符嵌套过深（超过 1024 层）"));
        }
        Ok(())
    }

    // ---- token 工具 ----
    fn current_span(&self) -> crate::diagnostics::Span {
        self.peek()
            .map(|token| crate::diagnostics::Span::new(&token.file, token.line, token.column))
            .unwrap_or_default()
    }

    fn peek(&self) -> Option<&Token> {
        // Eof 对解析逻辑不可见（等价于 token 流耗尽），但保留在 toks 中，
        // 供文件末尾错误的定位回退使用（见 parse_program_diagnostic）。
        self.toks.get(self.pos).filter(|t| t.kind != TokKind::Eof)
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
    fn expect_punct(&mut self, s: &str) -> Result<(), String> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", s)))
        }
    }
    fn err(&self, msg: &str) -> String {
        match self.peek() {
            Some(t) => format!("第 {} 行: {}", t.line, msg),
            None => format!("文件末尾: {}", msg),
        }
    }

    fn is_punct_any(&self, ss: &[&str]) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokKind::Punct && ss.contains(&t.text.as_str()))
    }

    /// 收集整型常量表达式 token（到 stop 中任一标点为止，不消费该标点）；
    /// 已登记的枚举常量替换为数值 token（严格求值前预处理）。
    fn collect_const_expr(&mut self, stop: &[&str]) -> Result<Vec<Token>, String> {
        let mut toks = Vec::new();
        while !self.is_punct_any(stop) {
            if let Some(t) = self.peek().cloned() {
                if t.kind == TokKind::Ident {
                    if let Some(v) = self.enum_constants.get(&t.text) {
                        let mut num = t;
                        num.kind = TokKind::Number;
                        num.text = v.to_string();
                        toks.push(num);
                        self.pos += 1;
                        continue;
                    }
                }
                toks.push(t);
                self.pos += 1;
            } else {
                return Err(self.err("常量表达式意外结束"));
            }
        }
        Ok(toks)
    }

    // ---- 表达式（§3.3）----

    /// 表达式：assignment (',' assignment)* （§3.3.17 逗号运算符）
    pub fn parse_expr(&mut self) -> Result<Node, String> {
        let span = self.current_span();
        self.parse_expr_inner()
            .map(|node| node.with_fallback_span(span))
    }

    fn parse_expr_inner(&mut self) -> Result<Node, String> {
        let mut node = self.parse_assign()?;
        while self.eat(",") {
            let rhs = self.parse_assign()?;
            node = Node::Comma(Box::new(node), Box::new(rhs));
        }
        Ok(node)
    }

    /// 赋值（§3.3.16，右结合）
    fn parse_assign(&mut self) -> Result<Node, String> {
        let lhs = self.parse_cond()?;
        const OPS: [(&str, AssignOp); 11] = [
            ("=", AssignOp::Plain),
            ("+=", AssignOp::Add),
            ("-=", AssignOp::Sub),
            ("*=", AssignOp::Mul),
            ("/=", AssignOp::Div),
            ("%=", AssignOp::Mod),
            ("<<=", AssignOp::Shl),
            (">>=", AssignOp::Shr),
            ("&=", AssignOp::BitAnd),
            ("^=", AssignOp::BitXor),
            ("|=", AssignOp::BitOr),
        ];
        for (s, op) in OPS {
            if self.is_punct(s) {
                self.pos += 1;
                let rhs = self.parse_assign()?; // 右结合
                return Ok(Node::Assign(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    /// 条件表达式（§3.3.15）
    fn parse_cond(&mut self) -> Result<Node, String> {
        let cond = self.parse_logor()?;
        if self.eat("?") {
            let then = self.parse_expr()?;
            self.expect_punct(":")?;
            let els = self.parse_cond()?;
            return Ok(Node::Cond(Box::new(cond), Box::new(then), Box::new(els)));
        }
        Ok(cond)
    }
    // ---- 二元运算优先级链 ----

    fn parse_logor(&mut self) -> Result<Node, String> {
        self.parse_binary(&[("||", BinaryOp::LogOr)], Self::parse_logand)
    }
    fn parse_logand(&mut self) -> Result<Node, String> {
        self.parse_binary(&[("&&", BinaryOp::LogAnd)], Self::parse_bitor)
    }
    fn parse_bitor(&mut self) -> Result<Node, String> {
        self.parse_binary(&[("|", BinaryOp::BitOr)], Self::parse_bitxor)
    }
    fn parse_bitxor(&mut self) -> Result<Node, String> {
        self.parse_binary(&[("^", BinaryOp::BitXor)], Self::parse_bitand)
    }
    fn parse_bitand(&mut self) -> Result<Node, String> {
        self.parse_binary(&[("&", BinaryOp::BitAnd)], Self::parse_equality)
    }
    fn parse_equality(&mut self) -> Result<Node, String> {
        self.parse_binary(
            &[("==", BinaryOp::Eq), ("!=", BinaryOp::Ne)],
            Self::parse_relational,
        )
    }
    fn parse_relational(&mut self) -> Result<Node, String> {
        self.parse_binary(
            &[
                ("<", BinaryOp::Lt),
                (">", BinaryOp::Gt),
                ("<=", BinaryOp::Le),
                (">=", BinaryOp::Ge),
            ],
            Self::parse_shift,
        )
    }
    fn parse_shift(&mut self) -> Result<Node, String> {
        self.parse_binary(
            &[("<<", BinaryOp::Shl), (">>", BinaryOp::Shr)],
            Self::parse_additive,
        )
    }
    fn parse_additive(&mut self) -> Result<Node, String> {
        self.parse_binary(
            &[("+", BinaryOp::Add), ("-", BinaryOp::Sub)],
            Self::parse_multiplicative,
        )
    }
    fn parse_multiplicative(&mut self) -> Result<Node, String> {
        self.parse_binary(
            &[
                ("*", BinaryOp::Mul),
                ("/", BinaryOp::Div),
                ("%", BinaryOp::Mod),
            ],
            Self::parse_cast,
        )
    }

    /// 通用二元链：按优先级逐层下降
    fn parse_binary(
        &mut self,
        ops: &[(&str, BinaryOp)],
        next: fn(&mut Self) -> Result<Node, String>,
    ) -> Result<Node, String> {
        let mut lhs = next(self)?;
        loop {
            let mut matched = None;
            for (s, op) in ops {
                if self.is_punct(s) {
                    matched = Some(*op);
                    self.pos += 1;
                    break;
                }
            }
            match matched {
                Some(op) => {
                    let rhs = next(self)?;
                    lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                None => break,
            }
        }
        Ok(lhs)
    }

    /// 类型转换（§3.3.4）：( type-name ) cast-expression
    fn parse_cast(&mut self) -> Result<Node, String> {
        // 深度护栏包一层：cast/unary 相互递归，括号嵌套也会经过这里
        self.enter_depth()?;
        let result = self.parse_cast_guarded();
        self.expr_depth -= 1;
        result
    }

    fn parse_cast_guarded(&mut self) -> Result<Node, String> {
        if self.is_punct("(") && self.peek2_is_type_start() {
            self.pos += 1; // (
            let ty = self.parse_cast_type()?;
            self.expect_punct(")")?;
            let e = self.parse_cast()?;
            return Ok(Node::Cast(ty, Box::new(e)));
        }
        self.parse_unary()
    }

    /// ( 后是否可能是类型名（cast 判定）
    fn peek2_is_type_start(&self) -> bool {
        match self.toks.get(self.pos + 1) {
            Some(t) if t.kind == TokKind::Ident => {
                matches!(
                    t.text.as_str(),
                    "void"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "struct"
                        | "union"
                        | "enum"
                        | "const"
                        | "volatile"
                ) || self.typedefs.contains_key(&t.text)
            }
            _ => false,
        }
    }

    /// 类型名（§3.5.5 简化版）：declaration-specifiers + 可选 * 指针
    fn parse_cast_type(&mut self) -> Result<Type, String> {
        let specs = self.parse_decl_specs()?;
        let mut ty = specs.ty;
        while self.eat("*") {
            ty = Type::Ptr(Box::new(ty));
        }
        Ok(ty)
    }

    /// 一元运算符（§3.3.3）
    fn parse_unary(&mut self) -> Result<Node, String> {
        if self.eat("!") {
            return Ok(Node::Unary(UnaryOp::Not, Box::new(self.parse_cast()?)));
        }
        if self.eat("~") {
            return Ok(Node::Unary(UnaryOp::BitNot, Box::new(self.parse_cast()?)));
        }
        if self.eat("+") {
            // +x 就是 x（一元正号无操作）
            return self.parse_cast();
        }
        if self.eat("-") {
            return Ok(Node::Unary(UnaryOp::Neg, Box::new(self.parse_cast()?)));
        }
        if self.eat("*") {
            return Ok(Node::Unary(UnaryOp::Deref, Box::new(self.parse_cast()?)));
        }
        if self.eat("&") {
            return Ok(Node::Unary(UnaryOp::Addr, Box::new(self.parse_cast()?)));
        }
        if self.eat("++") {
            return Ok(Node::Unary(UnaryOp::PreInc, Box::new(self.parse_cast()?)));
        }
        if self.eat("--") {
            return Ok(Node::Unary(UnaryOp::PreDec, Box::new(self.parse_cast()?)));
        }
        if self.is_punct("sizeof") {
            // sizeof 是关键字，但 tokenize 里是 Ident！特判文本
        }
        if matches!(self.peek(), Some(t) if t.kind == TokKind::Ident && t.text == "sizeof") {
            self.pos += 1;
            if self.eat("(") {
                // ( 已被吃掉，当前 token 是类型名（sizeof(int)）或表达式
                if self.is_type_start() {
                    // sizeof(类型名)：编译期常量。
                    // sizeof(void) 等无大小的类型是约束违反（C89 §3.3.3.4）。
                    let ty = self.parse_cast_type()?;
                    self.expect_punct(")")?;
                    let sz = self
                        .tt
                        .size_of(&ty)
                        .map_err(|e| self.err(&format!("sizeof 错误: {}", e)))?;
                    return Ok(Node::Num(sz as i64));
                }
                // sizeof(表达式)
                let e = self.parse_expr()?;
                self.expect_punct(")")?;
                return Ok(Node::SizeofExpr(Box::new(e)));
            }
            let e = self.parse_unary()?;
            return Ok(Node::SizeofExpr(Box::new(e)));
        }
        self.parse_postfix()
    }

    /// 后缀运算符（§3.3.2）
    fn parse_postfix(&mut self) -> Result<Node, String> {
        let mut node = self.parse_primary()?;
        loop {
            if self.eat("[") {
                let idx = self.parse_expr()?;
                self.expect_punct("]")?;
                node = Node::Subscript(Box::new(node), Box::new(idx));
            } else if self.eat("(") {
                // 函数调用参数
                let is_va_arg = matches!(node.inner(), Node::Ident(name) if name == "va_arg");
                let mut args = Vec::new();
                if !self.is_punct(")") {
                    loop {
                        // va_arg(ap, 类型名)：内置宏的第二个实参是类型而非
                        // 表达式（C89 §4.8.1.1）。类型名折叠为 Ident 文本
                        // （书写形式），语义层跳过、代码生成层识别 int/double。
                        if is_va_arg && args.len() == 1 && self.is_type_start() {
                            let type_start = self.pos;
                            let span = self.current_span();
                            self.parse_cast_type()?;
                            let text: String = self.toks[type_start..self.pos]
                                .iter()
                                .map(|t| t.text.clone())
                                .collect::<Vec<_>>()
                                .join(" ");
                            args.push(Node::Ident(text).with_fallback_span(span));
                        } else {
                            args.push(self.parse_assign()?);
                        }
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect_punct(")")?;
                node = Node::Call(Box::new(node), args);
            } else if self.eat(".") {
                let name = self.expect_ident()?;
                node = Node::Member(Box::new(node), name, false);
            } else if self.eat("->") {
                let name = self.expect_ident()?;
                node = Node::Member(Box::new(node), name, true);
            } else if self.eat("++") {
                node = Node::Unary(UnaryOp::PostInc, Box::new(node));
            } else if self.eat("--") {
                node = Node::Unary(UnaryOp::PostDec, Box::new(node));
            } else {
                break;
            }
        }
        Ok(node)
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(t) if t.kind == TokKind::Ident => {
                if is_keyword(&t.text) {
                    return Err(self.err(&format!("关键字 '{}' 不能用作标识符", t.text)));
                }
                let s = t.text.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => Err(self.err("expected identifier")),
        }
    }

    /// 基本表达式（§3.3.1）
    fn parse_primary(&mut self) -> Result<Node, String> {
        let t = self
            .peek()
            .cloned()
            .ok_or_else(|| self.err("表达式意外结束"))?;
        match t.kind {
            TokKind::Number => {
                self.pos += 1;
                if is_float_literal(&t.text) {
                    // 去后缀 f/F/l/L 后解析为 f64
                    let clean = t.text.trim_end_matches(['f', 'F', 'l', 'L']);
                    let v = clean.parse::<f64>().map_err(|e| {
                        format!("第 {} 行: 浮点字面量错误 {}: {}", t.line, t.text, e)
                    })?;
                    Ok(Node::FNum(v))
                } else {
                    // 后缀按 C89 §3.1.3.2 合法集合校验（ll/uu/lul 等非法）
                    let (parsed, suff_u, suff_l) =
                        parse_int_literal(&t.text).map_err(|e| self.err(&e))?;
                    let value = Node::Num(parsed);
                    if parsed > u32::MAX as i64 {
                        return Err(self.err("整数常量超出教学型 32 位数据模型范围"));
                    }
                    // 无后缀常量超出 int 范围自动升为无符号（§3.1.3.2）
                    let unsigned = suff_u || parsed > i32::MAX as i64;
                    let long = suff_l || (!unsigned && parsed > i32::MAX as i64);
                    let ty = match (unsigned, long) {
                        (true, true) => Some(Type::ULong),
                        (true, false) => Some(Type::UInt),
                        (false, true) => Some(Type::Long),
                        _ => None,
                    };
                    Ok(ty.map_or(value.clone(), |ty| Node::Cast(ty, Box::new(value))))
                }
            }
            TokKind::String => {
                // 相邻字符串字面量在翻译阶段 6 拼接，例如 "ab" "cd" → "abcd"
                let mut parts = Vec::new();
                while self
                    .peek()
                    .map(|t| t.kind == TokKind::String)
                    .unwrap_or(false)
                {
                    let t = self.peek().cloned().unwrap();
                    self.pos += 1;
                    // 只去掉最外层引号，不能误删末尾的转义引号。
                    let s = t
                        .text
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                        .unwrap_or(&t.text)
                        .to_string();
                    parts.push(s);
                }
                Ok(Node::Str(parts.concat()))
            }
            TokKind::Char => {
                self.pos += 1;
                Ok(Node::Num(parse_char_value(&t.text)?))
            }
            TokKind::Ident => {
                // 关键字在任何上下文中都不能用作标识符（C89 §3.1.1）
                if is_keyword(&t.text) {
                    return Err(self.err(&format!("关键字 '{}' 不能用作标识符", t.text)));
                }
                self.pos += 1;
                // 枚举常量 → 编译期数值
                if let Some(v) = self.enum_constants.get(&t.text) {
                    return Ok(Node::Num(*v));
                }
                Ok(Node::Ident(t.text))
            }
            TokKind::Punct if t.text == "(" => {
                self.pos += 1;
                let e = self.parse_expr()?;
                self.expect_punct(")")?;
                Ok(e)
            }
            _ => Err(self.err(&format!("无法解析的表达式起始 token: {}", t.text))),
        }
    }

    // ---- 顶层与声明（§3.5 / §3.7）----

    /// 解析整个翻译单元（translation-unit）
    pub fn parse_program(&mut self) -> Result<Vec<Node>, String> {
        let mut items = Vec::new();
        while let Some(t) = self.peek() {
            if t.kind == TokKind::Eof {
                break;
            }
            items.extend(self.parse_external_declaration()?);
        }
        Ok(items)
    }

    pub fn parse_program_diagnostic(&mut self) -> crate::diagnostics::CompileResult<Vec<Node>> {
        self.parse_program().map_err(|message| {
            let span = self
                .peek()
                .map(|t| crate::diagnostics::Span::new(&t.file, t.line, t.column))
                .or_else(|| {
                    // token 流耗尽时（EOF 被 pipeline 过滤掉），回退到最后一个
                    // token 的位置，避免错误定位退化成 <unknown>。
                    self.toks
                        .last()
                        .map(|t| crate::diagnostics::Span::new(&t.file, t.line, t.column))
                })
                .unwrap_or_default();
            crate::diagnostics::CompileError::parse(span, message)
        })
    }

    /// 外部声明：函数定义 / 函数声明 / 变量声明（§3.7）
    fn parse_external_declaration(&mut self) -> Result<Vec<Node>, String> {
        let span = self.current_span();
        self.parse_external_declaration_inner().map(|nodes| {
            nodes
                .into_iter()
                .map(|node| node.with_fallback_span(span.clone()))
                .collect()
        })
    }

    fn parse_external_declaration_inner(&mut self) -> Result<Vec<Node>, String> {
        let specs = self.parse_decl_specs()?;
        // struct/union/enum 类型说明符允许无声明符（struct { ... }; 定义）；
        // 其余类型必须跟声明符（int; / typedef int; 是约束违反，C89 §3.5）
        if self.is_punct(";") {
            if !matches!(specs.ty, Type::Struct(_) | Type::Enum(_)) {
                return Err(self.err("声明缺少声明符（仅 struct/union/enum 允许无声明符）"));
            }
            self.pos += 1;
            return Ok(vec![Node::NullStmt]);
        }
        let base_ty = specs.ty.clone();
        let (ty, name, fparams) = self.parse_declarator(base_ty.clone())?;
        let mut specs = specs;
        specs.ty = ty;

        // 函数（定义或声明）：declarator 已吃掉参数列表
        if let Some((mut params, is_variadic, has_prototype)) = fparams {
            // 函数名遮蔽同名枚举常量（C89 普通标识符命名空间，§3.1.2.3）
            self.enum_constants.remove(&name);
            if !has_prototype && !params.is_empty() && !self.is_punct("{") && !self.is_punct(";") {
                self.parse_old_style_parameter_declarations(&mut params)?;
            }
            if self.is_punct("{") {
                // 函数定义
                self.pos += 1;
                // 形参遮蔽外层枚举常量/typedef 名（仅函数体内，C89 §3.1.2.1）；
                // 函数体结束恢复外层作用域。
                let saved_enums = self.enum_constants.clone();
                let saved_typedefs = self.typedefs.clone();
                for p in &params {
                    self.enum_constants.remove(&p.name);
                    self.typedefs.remove(&p.name);
                }
                let body = self.parse_block_body()?;
                self.enum_constants = saved_enums;
                self.typedefs = saved_typedefs;
                return Ok(vec![Node::FuncDef {
                    ret: specs.ty,
                    name,
                    storage: specs.storage,
                    params,
                    is_variadic,
                    has_prototype,
                    body: Box::new(body),
                }]);
            }
            // 函数声明
            self.expect_punct(";")?;
            return Ok(vec![Node::FuncDecl {
                ret: specs.ty,
                name,
                params,
                is_variadic,
                has_prototype,
            }]);
        }

        // 变量声明（第一个 declarator 已解析；每个声明符独立 init）
        let init = if self.eat("=") {
            Some(self.parse_initializer()?)
        } else {
            None
        };
        if let Type::Array(inner, None) = &specs.ty {
            match init.as_ref().map(Node::inner) {
                Some(Node::InitList(items)) => {
                    specs.ty = Type::Array(inner.clone(), Some(items.len()))
                }
                Some(Node::Str(value)) => {
                    // 数组大小按转义解码后的实际字节数 + NUL（"\x41" 是 1 字节）
                    let len = crate::expr_eval::decode_string_escapes(value)
                        .map(|bytes| bytes.len() + 1)
                        .unwrap_or(value.len() + 1);
                    specs.ty = Type::Array(inner.clone(), Some(len))
                }
                _ => {}
            }
        }
        if specs.storage == Storage::Typedef {
            self.typedefs.insert(name.clone(), specs.ty.clone());
        }
        // 声明登记的名字遮蔽同名枚举常量（C89 普通标识符命名空间）
        self.enum_constants.remove(&name);
        let mut declarations = vec![Node::Decl {
            specs: specs.clone(),
            name,
            init: init.map(Box::new),
        }];
        // 后续 declarator：每一个声明符保留独立类型、名字和初始化器。
        while self.eat(",") {
            let (ty, name, _f) = self.parse_declarator(base_ty.clone())?;
            let init = if self.eat("=") {
                Some(Box::new(self.parse_initializer()?))
            } else {
                None
            };
            let mut item_specs = specs.clone();
            item_specs.ty = ty;
            if let Type::Array(inner, None) = &item_specs.ty {
                match init.as_deref().map(Node::inner) {
                    Some(Node::InitList(items)) => {
                        item_specs.ty = Type::Array(inner.clone(), Some(items.len()))
                    }
                    Some(Node::Str(value)) => {
                        let len = crate::expr_eval::decode_string_escapes(value)
                            .map(|bytes| bytes.len() + 1)
                            .unwrap_or(value.len() + 1);
                        item_specs.ty = Type::Array(inner.clone(), Some(len))
                    }
                    _ => {}
                }
            }
            if item_specs.storage == Storage::Typedef {
                self.typedefs.insert(name.clone(), item_specs.ty.clone());
            }
            self.enum_constants.remove(&name);
            declarations.push(Node::Decl {
                specs: item_specs,
                name,
                init,
            });
        }
        self.expect_punct(";")?;
        Ok(declarations)
    }

    /// 声明说明符：存储类 + 类型 + 限定符（§3.5.1~3.5.3）
    #[allow(clippy::while_let_loop)]
    fn parse_decl_specs(&mut self) -> Result<DeclSpec, String> {
        let mut specs = DeclSpec::new();
        let mut saw_type = false;
        loop {
            let Some(t) = self.peek() else { break };
            if t.kind != TokKind::Ident {
                break;
            }
            // typedef 名感知：查符号表返回真实类型。
            // 已有类型说明符时不再吸收 typedef 名，留给声明符位置——
            // typedef int T; 中 T 是声明符而非类型说明符（C89 §3.5.6）。
            if !saw_type {
                if let Some(real) = self.typedefs.get(&t.text) {
                    specs.ty = real.clone();
                    saw_type = true;
                    self.pos += 1;
                    continue;
                }
            }
            match t.text.as_str() {
                "typedef" => {
                    specs.storage = Storage::Typedef;
                    self.pos += 1;
                }
                "static" => {
                    specs.storage = Storage::Static;
                    self.pos += 1;
                }
                "extern" => {
                    specs.storage = Storage::Extern;
                    self.pos += 1;
                }
                "auto" => {
                    specs.storage = Storage::Auto;
                    self.pos += 1;
                }
                "register" => {
                    specs.storage = Storage::Register;
                    self.pos += 1;
                }
                "void" => {
                    specs.ty = Type::Void;
                    saw_type = true;
                    self.pos += 1;
                }
                "char" => {
                    specs.ty = Type::Char;
                    saw_type = true;
                    self.pos += 1;
                }
                "short" => {
                    specs.ty = Type::Short;
                    saw_type = true;
                    self.pos += 1;
                }
                "int" => {
                    if !matches!(specs.ty, Type::Short | Type::Long) {
                        specs.ty = Type::Int;
                    }
                    saw_type = true;
                    self.pos += 1;
                }
                "long" => {
                    specs.ty = Type::Long;
                    saw_type = true;
                    self.pos += 1;
                }
                "float" => {
                    specs.ty = Type::Float;
                    saw_type = true;
                    self.pos += 1;
                }
                "double" => {
                    specs.ty = Type::Double;
                    saw_type = true;
                    self.pos += 1;
                }
                "signed" => {
                    specs.is_unsigned = false;
                    self.pos += 1;
                }
                "unsigned" => {
                    specs.is_unsigned = true;
                    self.pos += 1;
                }
                "const" => {
                    specs.is_const = true;
                    self.pos += 1;
                }
                "volatile" => {
                    specs.is_volatile = true;
                    self.pos += 1;
                }
                "enum" => {
                    self.pos += 1;
                    let mut tag = String::new();
                    if let Some(t2) = self.peek() {
                        if t2.kind == TokKind::Ident {
                            tag = t2.text.clone();
                            self.pos += 1;
                        }
                    }
                    if self.eat("{") {
                        // 枚举定义：name [= 常量表达式] 列表
                        let mut values: Vec<(String, i64)> = Vec::new();
                        let mut next_val = 0i64;
                        while !self.is_punct("}") {
                            let name = self.expect_ident()?;
                            let val = if self.eat("=") {
                                // 收集常量表达式到 , 或 }；已登记的枚举常量
                                // 代入为数值 token（B = A + 1 需要 A 的值）。
                                // 严格求值：未定义的标识符报错（C89 整型常量表达式）。
                                let toks = self.collect_const_expr(&[",", "}"])?;
                                eval_expr_strict(&toks)
                                    .map_err(|e| self.err(&format!("枚举值错误: {}", e)))?
                            } else {
                                next_val
                            };
                            // 立即登记：后续枚举项可引用本项（enum { A=5, B=A+1 }）
                            self.enum_constants.insert(name.clone(), val);
                            values.push((name, val));
                            next_val = val
                                .checked_add(1)
                                .ok_or_else(|| self.err("枚举常量隐式值溢出"))?;
                            if !self.eat(",") {
                                break;
                            }
                        }
                        self.expect_punct("}")?;
                        if tag.is_empty() {
                            tag = format!("__enum_{}", self.tt.enums.len());
                        }
                        // 同作用域 tag 不得重定义（C89 §3.5.2.3）
                        if self.tt.enums.contains_key(&tag) {
                            return Err(self.err(&format!("枚举 tag '{}' 重复定义", tag)));
                        }
                        let def = crate::types::EnumDef {
                            name: tag.clone(),
                            values,
                        };
                        self.tt.enums.insert(tag.clone(), def);
                        specs.ty = Type::Enum(tag);
                    } else {
                        specs.ty = Type::Enum(tag);
                    }
                    saw_type = true;
                }
                "struct" | "union" => {
                    let is_union = t.text == "union";
                    let struct_line = t.line;
                    self.pos += 1;
                    // tag（可选）
                    let mut tag = String::new();
                    if let Some(t2) = self.peek() {
                        if t2.kind == TokKind::Ident {
                            tag = t2.text.clone();
                            self.pos += 1;
                        }
                    }
                    if self.eat("{") {
                        // 定义：struct Tag { members }
                        let mut members: Vec<(String, Type, Option<usize>)> = Vec::new();
                        while !self.is_punct("}") {
                            if self.peek().is_none() {
                                return Err(self.err("struct 定义缺少 }"));
                            }
                            let mspecs = self.parse_decl_specs()?;
                            loop {
                                // 无名位域：int : 0; 没有声明符
                                let (mty, mname, _f) = if self.is_punct(":") {
                                    (mspecs.ty.clone(), String::new(), None)
                                } else {
                                    self.parse_declarator(mspecs.ty.clone())?
                                };
                                // 位域：: width（C89 §3.5.2.1，宽度是整型常量表达式）
                                let mut bit_width = None;
                                if self.is_punct(":") {
                                    self.pos += 1;
                                    let width_line =
                                        self.peek().map(|t| t.line).unwrap_or(struct_line);
                                    let toks = self.collect_const_expr(&[",", ";"])?;
                                    if toks.is_empty() {
                                        return Err(self.err("位域宽度缺失"));
                                    }
                                    let v = eval_expr_strict(&toks).map_err(|e| {
                                        format!("第 {} 行: 位域宽度错误: {}", width_line, e)
                                    })?;
                                    if v < 0 {
                                        return Err(format!("第 {} 行: 位域宽度为负", width_line));
                                    }
                                    bit_width = Some(v as usize);
                                }
                                // 位域的有效类型：int / unsigned int（含 signed）
                                let eff_ty = if mspecs.is_unsigned {
                                    match mty.clone() {
                                        Type::Char => Type::UChar,
                                        Type::Short => Type::UShort,
                                        Type::Int => Type::UInt,
                                        Type::Long => Type::ULong,
                                        other => other,
                                    }
                                } else {
                                    mty.clone()
                                };
                                if bit_width.is_some()
                                    && !matches!(eff_ty.unqualified(), Type::Int | Type::UInt)
                                {
                                    return Err(format!(
                                        "第 {} 行: 位域类型必须是 int/signed int/unsigned int（C89 §3.5.2.1）",
                                        struct_line
                                    ));
                                }
                                let final_ty = if bit_width.is_some() {
                                    eff_ty
                                } else {
                                    mty.clone()
                                };
                                // 成员名不得重复（C89 §3.5.2.1）；匿名位域（空名）不受限
                                if !mname.is_empty() && members.iter().any(|(n, _, _)| *n == mname)
                                {
                                    return Err(format!(
                                        "第 {} 行: 成员 '{}' 重复定义",
                                        struct_line, mname
                                    ));
                                }
                                members.push((mname, final_ty, bit_width));
                                if !self.eat(",") {
                                    break;
                                }
                            }
                            self.expect_punct(";")?;
                        }
                        // C89 §3.5.2.1：struct/union 至少需要一个成员
                        if members.is_empty() {
                            return Err(format!(
                                "第 {} 行: struct/union 定义不能为空（至少需要一个成员）",
                                struct_line
                            ));
                        }
                        self.pos += 1; // }
                                       // 匿名结构体：自动命名
                        if tag.is_empty() {
                            tag = format!("__anon_{}", self.tt.structs.len());
                        }
                        // 同作用域 tag 不得重定义（C89 §3.5.2.3）；
                        // 前向声明 struct S; 不插入类型表，不受此检查影响
                        if self.tt.structs.contains_key(&tag) {
                            return Err(format!(
                                "第 {} 行: struct/union tag '{}' 重复定义",
                                struct_line, tag
                            ));
                        }
                        let def = self.tt.layout_struct(&tag, members, is_union)?;
                        self.tt.structs.insert(tag.clone(), def);
                        specs.ty = Type::Struct(tag);
                    } else {
                        // 引用已定义结构体
                        specs.ty = Type::Struct(tag);
                    }
                    saw_type = true;
                }
                _ => break,
            }
        }
        let _ = saw_type;
        if specs.is_unsigned {
            specs.ty = match specs.ty {
                Type::Char => Type::UChar,
                Type::Short => Type::UShort,
                Type::Int => Type::UInt,
                Type::Long => Type::ULong,
                _ => return Err(self.err("unsigned 只能修饰整数类型")),
            };
            specs.is_unsigned = false;
        }
        specs.ty = specs.ty.qualify(specs.is_const, specs.is_volatile);
        specs.is_const = false;
        specs.is_volatile = false;
        Ok(specs)
    }

    /// 完整声明符（§3.5.4）：返回 (完整类型, 名字, 函数参数)
    fn parse_declarator(&mut self, base: Type) -> Result<DeclaratorResult, String> {
        let node = self.parse_declarator_tree()?;
        let (ty, name, _) = self.build_type(&node, base)?;
        match ty {
            Type::Function {
                ret,
                params,
                is_variadic,
                has_prototype,
            } => Ok((*ret, name, Some((params, is_variadic, has_prototype)))),
            other => Ok((other, name, None)),
        }
    }

    fn parse_declarator_tree(&mut self) -> Result<DeclNode, String> {
        // 深度护栏：***** 指针链与括号声明符递归
        self.enter_depth()?;
        let result = self.parse_declarator_tree_guarded();
        self.expr_depth -= 1;
        result
    }

    fn parse_declarator_tree_guarded(&mut self) -> Result<DeclNode, String> {
        // * 修饰整个后续声明符（§3.5.4.1）
        if self.eat("*") {
            let mut is_const = false;
            let mut is_volatile = false;
            while self.is_ident("const") || self.is_ident("volatile") {
                if self.is_ident("const") {
                    is_const = true;
                } else {
                    is_volatile = true;
                }
                self.pos += 1;
            }
            return Ok(DeclNode::Pointer(
                Box::new(self.parse_declarator_tree()?),
                is_const,
                is_volatile,
            ));
        }
        if self.is_punct("(") {
            // 括号声明符：改变结合优先级
            self.pos += 1;
            let inner = self.parse_declarator_tree()?;
            self.expect_punct(")")?;
            return self.apply_suffix(inner);
        }
        let name = self.expect_ident()?;
        self.apply_suffix(DeclNode::Name(name))
    }

    /// 后缀：数组 [n] / 函数 (params)（§3.5.4.2 / 3.5.4.3）
    /// 顺序包外层：先写的后缀离 Name 近（树内层），构建时外层类型先应用
    fn apply_suffix(&mut self, node: DeclNode) -> Result<DeclNode, String> {
        let mut node = node;
        loop {
            if self.eat("[") {
                let size = self.parse_array_size()?;
                node = DeclNode::Array(Box::new(node), size);
            } else if self.is_punct("(") {
                self.pos += 1;
                let (params, variadic, has_prototype) = self.parse_params()?;
                node = DeclNode::Func(Box::new(node), params, variadic, has_prototype);
            } else {
                break;
            }
        }
        Ok(node)
    }

    /// 数组大小（常量表达式，可省略）
    /// 初始化器（§3.5.7）：assignment-expression 或 { 列表 }
    fn parse_initializer(&mut self) -> Result<Node, String> {
        if self.eat("{") {
            let mut items = Vec::new();
            if !self.is_punct("}") {
                loop {
                    items.push(self.parse_initializer()?);
                    if self.eat(",") {
                        if self.is_punct("}") {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect_punct("}")?;
            Ok(Node::InitList(items))
        } else {
            self.parse_assign()
        }
    }

    fn parse_array_size(&mut self) -> Result<Option<usize>, String> {
        if self.eat("]") {
            return Ok(None);
        }
        let mut toks = Vec::new();
        while !self.is_punct("]") {
            // sizeof：用真实解析器处理（支持 sizeof(类型名) 与 sizeof(表达式)），
            // 结果折叠为常量 token，与其余常量表达式一起交给 eval_expr_strict。
            if matches!(self.peek(), Some(t) if t.kind == TokKind::Ident && t.text == "sizeof") {
                let line = self.peek().map(|t| t.line).unwrap_or(0);
                self.pos += 1;
                let node = if self.eat("(") {
                    if self.is_type_start() {
                        let ty = self.parse_cast_type()?;
                        self.expect_punct(")")?;
                        let sz = self
                            .tt
                            .size_of(&ty)
                            .map_err(|e| self.err(&format!("数组大小错误: {}", e)))?;
                        Node::Num(sz as i64)
                    } else {
                        let e = self.parse_expr()?;
                        self.expect_punct(")")?;
                        Node::SizeofExpr(Box::new(e))
                    }
                } else {
                    let e = self.parse_unary()?;
                    Node::SizeofExpr(Box::new(e))
                };
                let v = eval_const_size(&node, &self.tt, &self.typedefs)
                    .map_err(|e| self.err(&format!("数组大小错误: {}", e)))?;
                toks.push(Token::new(TokKind::Number, v.to_string(), line, false));
                continue;
            }
            if let Some(t) = self.peek().cloned() {
                // 枚举常量 → 数值 token（常量求值器只认数值 token）
                if t.kind == TokKind::Ident {
                    if let Some(v) = self.enum_constants.get(&t.text) {
                        let mut num = t;
                        num.kind = TokKind::Number;
                        num.text = v.to_string();
                        toks.push(num);
                        self.pos += 1;
                        continue;
                    }
                }
                toks.push(t);
                self.pos += 1;
            } else {
                return Err(self.err("数组大小缺少 ]"));
            }
        }
        self.expect_punct("]")?;
        // 严格求值：数组界是整型常量表达式，未定义标识符报错
        let v = eval_expr_strict(&toks).map_err(|e| self.err(&format!("数组大小错误: {}", e)))?;
        if v < 0 {
            return Err(self.err("数组大小为负"));
        }
        Ok(Some(v as usize))
    }

    /// 从根向叶子传递 base（螺旋法）：树内层节点 → 类型外层
    fn build_type(&self, node: &DeclNode, base: Type) -> Result<DeclaratorResult, String> {
        match node {
            DeclNode::Name(n) => Ok((base, n.clone(), None)),
            DeclNode::Pointer(inner, is_const, is_volatile) => {
                let pointer = Type::Ptr(Box::new(base)).qualify(*is_const, *is_volatile);
                let (t, n, f) = self.build_type(inner, pointer)?;
                Ok((t, n, f))
            }
            DeclNode::Array(inner, sz) => {
                let (t, n, f) = self.build_type(inner, Type::Array(Box::new(base), *sz))?;
                Ok((t, n, f))
            }
            DeclNode::Func(inner, params, variadic, has_prototype) => self.build_type(
                inner,
                Type::Function {
                    ret: Box::new(base),
                    params: params.clone(),
                    is_variadic: *variadic,
                    has_prototype: *has_prototype,
                },
            ),
        }
    }

    fn parse_old_style_parameter_declarations(
        &mut self,
        params: &mut [Param],
    ) -> Result<(), String> {
        while !self.is_punct("{") {
            let specs = self.parse_decl_specs()?;
            loop {
                let (ty, name, function) = self.parse_declarator(specs.ty.clone())?;
                if function.is_some() {
                    return Err(self.err("旧式参数不能声明为函数"));
                }
                let parameter = params
                    .iter_mut()
                    .find(|parameter| parameter.name == name)
                    .ok_or_else(|| self.err(&format!("旧式参数声明了未知名字 '{}'", name)))?;
                parameter.ty = match ty {
                    Type::Array(inner, _) => Type::Ptr(inner),
                    Type::Function { .. } => Type::Ptr(Box::new(ty)),
                    other => other,
                };
                if self.eat(",") {
                    continue;
                }
                self.expect_punct(";")?;
                break;
            }
        }
        Ok(())
    }

    /// 参数列表（已在 "(" 之后，负责消费到 ")"）
    fn parse_params(&mut self) -> Result<(Vec<Param>, bool, bool), String> {
        let mut params = Vec::new();
        let mut variadic = false;
        if self.eat(")") {
            return Ok((params, false, false)); // f()：参数未指定
        }
        if self.is_ident("void") {
            // f(void)：仅当 void 后紧跟 ) 时才是空参数表；void *p 等走普通参数路径
            let followed_by_paren = matches!(
                self.toks.get(self.pos + 1),
                Some(t) if t.kind == TokKind::Punct && t.text == ")"
            );
            if followed_by_paren {
                self.pos += 1; // 消费 void
                self.pos += 1; // 消费 )
                return Ok((params, false, true));
            }
        }
        let starts_identifier_list = matches!(self.peek(), Some(token)
            if token.kind == TokKind::Ident
                && !self.is_type_start()
                && !self.typedefs.contains_key(&token.text));
        if starts_identifier_list {
            loop {
                let name = self.expect_ident()?;
                // 参数名不得重复（C89 §3.7.1 约束）
                if params.iter().any(|p| p.name == name) {
                    return Err(self.err(&format!("参数 '{}' 重复声明", name)));
                }
                params.push(Param {
                    ty: Type::Int,
                    name,
                });
                if self.eat(",") {
                    continue;
                }
                self.expect_punct(")")?;
                return Ok((params, false, false));
            }
        }
        loop {
            // 变参标记 ...
            if self.is_punct("...") {
                self.pos += 1;
                variadic = true;
                self.expect_punct(")")?;
                break;
            }
            let specs = self.parse_decl_specs()?;
            // 参数声明符可选（支持无名参数 int f(int); 与 const void *f(...) 的无名指针参数）
            let (ty, name, _f) = if self.is_punct(",") || self.is_punct(")") || self.is_punct("...")
            {
                (specs.ty.clone(), String::new(), None)
            } else if self.is_punct("*") {
                let save = self.pos;
                self.pos += 1;
                let mut pointer_const = false;
                let mut pointer_volatile = false;
                while self.is_ident("const") || self.is_ident("volatile") {
                    if self.is_ident("const") {
                        pointer_const = true;
                    } else {
                        pointer_volatile = true;
                    }
                    self.pos += 1;
                }
                if self.is_punct(",") || self.is_punct(")") || self.is_punct("...") {
                    (
                        Type::Ptr(Box::new(specs.ty.clone()))
                            .qualify(pointer_const, pointer_volatile),
                        String::new(),
                        None,
                    )
                } else {
                    self.pos = save;
                    self.parse_declarator(specs.ty.clone())?
                }
            } else {
                self.parse_declarator(specs.ty.clone())?
            };
            let ty = match ty {
                Type::Array(inner, _) => Type::Ptr(inner),
                Type::Function { .. } => Type::Ptr(Box::new(ty)),
                other => other,
            };
            params.push(Param { ty, name });
            if self.eat(",") {
                continue;
            }
            self.expect_punct(")")?;
            break;
        }
        Ok((params, variadic, true))
    }

    /// 块体：在 "{" 之后解析到匹配的 "}"
    fn parse_block_body(&mut self) -> Result<Node, String> {
        // 块内声明会遮蔽外层枚举常量/typedef 名；块结束时恢复外层可见性
        // （C89 §3.1.2.1：typedef 名的作用域止于块末尾）。
        let saved_enums = self.enum_constants.clone();
        let saved_typedefs = self.typedefs.clone();
        let mut stmts = Vec::new();
        while !self.is_punct("}") {
            if self.peek().is_none() {
                return Err(self.err("块缺少右花括号 }"));
            }
            stmts.push(self.parse_statement()?);
        }
        self.pos += 1; // }
        self.enum_constants = saved_enums;
        self.typedefs = saved_typedefs;
        Ok(Node::Block(stmts))
    }

    // ---- 语句（§3.6）----

    /// 解析一条语句
    pub fn parse_statement(&mut self) -> Result<Node, String> {
        let span = self.current_span();
        self.parse_statement_inner()
            .map(|node| node.with_fallback_span(span))
    }

    fn parse_statement_inner(&mut self) -> Result<Node, String> {
        // 复合语句（块）：块内 typedef/枚举常量遮蔽外层，块结束恢复
        if self.eat("{") {
            let saved_enums = self.enum_constants.clone();
            let saved_typedefs = self.typedefs.clone();
            let mut stmts = Vec::new();
            while !self.is_punct("}") {
                if self.peek().is_none() {
                    return Err(self.err("块缺少右花括号 }"));
                }
                stmts.push(self.parse_statement()?);
            }
            self.pos += 1; // }
            self.enum_constants = saved_enums;
            self.typedefs = saved_typedefs;
            return Ok(Node::Block(stmts));
        }

        // 关键字语句
        if let Some(t) = self.peek() {
            if t.kind == TokKind::Ident {
                match t.text.as_str() {
                    "if" => return self.parse_if(),
                    "while" => return self.parse_while(),
                    "do" => return self.parse_do(),
                    "for" => return self.parse_for(),
                    "switch" => return self.parse_switch(),
                    "case" => return self.parse_case(),
                    "default" => return self.parse_default(),
                    "break" => {
                        self.pos += 1;
                        self.expect_punct(";")?;
                        return Ok(Node::Break);
                    }
                    "continue" => {
                        self.pos += 1;
                        self.expect_punct(";")?;
                        return Ok(Node::Continue);
                    }
                    "return" => {
                        self.pos += 1;
                        if self.is_punct(";") {
                            self.pos += 1;
                            return Ok(Node::Return(None));
                        }
                        let e = self.parse_expr()?;
                        self.expect_punct(";")?;
                        return Ok(Node::Return(Some(Box::new(e))));
                    }
                    "goto" => {
                        self.pos += 1;
                        let name = self.expect_ident()?;
                        self.expect_punct(";")?;
                        return Ok(Node::Goto(name));
                    }
                    _ => {}
                }
            }
        }

        // 标签 label:（标识符 + 冒号）。C89 标签是独立命名空间，
        // 可与 typedef 名同名（typedef int L; L: ...），必须先于
        // 块内声明的类型开头判定。
        if let Some(t) = self.peek() {
            if t.kind == TokKind::Ident {
                if let Some(t2) = self.toks.get(self.pos + 1) {
                    if t2.kind == TokKind::Punct && t2.text == ":" {
                        if is_keyword(&t.text) {
                            return Err(self.err(&format!("关键字 '{}' 不能用作标签", t.text)));
                        }
                        let name = t.text.clone();
                        self.pos += 2;
                        let stmt = self.parse_statement()?;
                        return Ok(Node::Label(name, Box::new(stmt)));
                    }
                }
            }
        }

        // 块内声明（C89：类型关键字或 typedef 名开头）
        if self.is_type_start() {
            return self.parse_local_decl();
        }

        // 空语句
        if self.eat(";") {
            return Ok(Node::NullStmt);
        }

        // 表达式语句
        let e = self.parse_expr()?;
        self.expect_punct(";")?;
        Ok(Node::ExprStmt(Box::new(e)))
    }

    fn is_ident(&self, s: &str) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokKind::Ident && t.text == s)
    }

    /// 当前 token 是否可能开始一条声明（类型关键字或 typedef 名）
    fn is_type_start(&self) -> bool {
        match self.peek() {
            Some(t) if t.kind == TokKind::Ident => {
                matches!(
                    t.text.as_str(),
                    "void"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "typedef"
                        | "static"
                        | "extern"
                        | "auto"
                        | "register"
                        | "const"
                        | "volatile"
                        | "struct"
                        | "union"
                        | "enum"
                ) || self.typedefs.contains_key(&t.text)
            }
            _ => false,
        }
    }

    /// 块内变量声明（§3.5）
    fn parse_local_decl(&mut self) -> Result<Node, String> {
        let specs = self.parse_decl_specs()?;
        // struct/union/enum 类型说明符允许无声明符（struct { ... }; 定义）；
        // 其余类型必须跟声明符（int; / typedef int; 是约束违反）
        if self.is_punct(";") {
            if !matches!(specs.ty, Type::Struct(_) | Type::Enum(_)) {
                return Err(self.err("声明缺少声明符（仅 struct/union/enum 允许无声明符）"));
            }
            self.pos += 1;
            return Ok(Node::NullStmt);
        }
        let base_ty = specs.ty.clone();
        let (ty, name, _f) = self.parse_declarator(base_ty.clone())?;
        let mut specs = specs;
        specs.ty = ty;
        let init = if self.eat("=") {
            Some(Box::new(self.parse_initializer()?))
        } else {
            None
        };
        // 数组大小推断：int a[] = {1,2,3} → [3]
        if let Type::Array(inner, None) = &specs.ty {
            match init.as_deref().map(Node::inner) {
                Some(Node::InitList(items)) => {
                    specs.ty = Type::Array(inner.clone(), Some(items.len()))
                }
                Some(Node::Str(value)) => {
                    let len = crate::expr_eval::decode_string_escapes(value)
                        .map(|bytes| bytes.len() + 1)
                        .unwrap_or(value.len() + 1);
                    specs.ty = Type::Array(inner.clone(), Some(len))
                }
                _ => {}
            }
        }
        let mut declarations = vec![Node::Decl {
            specs: specs.clone(),
            name: name.clone(),
            init,
        }];
        // 块内 typedef 登记（与外部声明一致，C89 §3.5.6）；
        // 声明的名字遮蔽同名枚举常量。
        if specs.storage == Storage::Typedef {
            self.typedefs.insert(name.clone(), specs.ty.clone());
        }
        self.enum_constants.remove(&name);
        while self.eat(",") {
            let (ty, name, _f) = self.parse_declarator(base_ty.clone())?;
            let init = if self.eat("=") {
                Some(Box::new(self.parse_initializer()?))
            } else {
                None
            };
            let mut item_specs = specs.clone();
            item_specs.ty = ty;
            if let Type::Array(inner, None) = &item_specs.ty {
                if let Some(Node::InitList(items)) = init.as_deref() {
                    item_specs.ty = Type::Array(inner.clone(), Some(items.len()));
                }
            }
            if item_specs.storage == Storage::Typedef {
                self.typedefs.insert(name.clone(), item_specs.ty.clone());
            }
            self.enum_constants.remove(&name);
            declarations.push(Node::Decl {
                specs: item_specs,
                name,
                init,
            });
        }
        self.expect_punct(";")?;
        if declarations.len() == 1 {
            Ok(declarations.pop().unwrap())
        } else {
            // 多声明符展开块：属于外层块作用域（与用户花括号区分）
            Ok(Node::DeclBlock(declarations))
        }
    }

    /// if 语句（§3.6.4.1，else 悬挂：递归下降天然匹配最近的 if）
    fn parse_if(&mut self) -> Result<Node, String> {
        self.pos += 1; // if
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        let then = self.parse_statement()?;
        let els = if self.is_ident("else") {
            self.pos += 1;
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        Ok(Node::If(Box::new(cond), Box::new(then), els))
    }

    /// while 语句（§3.6.5.1）
    fn parse_while(&mut self) -> Result<Node, String> {
        self.pos += 1;
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        let body = self.parse_statement()?;
        Ok(Node::While(Box::new(cond), Box::new(body)))
    }

    /// do-while 语句（§3.6.5.2）
    fn parse_do(&mut self) -> Result<Node, String> {
        self.pos += 1;
        let body = self.parse_statement()?;
        if !self.is_ident("while") {
            return Err(self.err("do 语句缺少 while"));
        }
        self.pos += 1;
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        self.expect_punct(";")?;
        Ok(Node::DoWhile(Box::new(body), Box::new(cond)))
    }

    /// for 语句（§3.6.5.3；C89 的 init 是表达式，非声明）
    fn parse_for(&mut self) -> Result<Node, String> {
        self.pos += 1;
        self.expect_punct("(")?;
        let init = if self.eat(";") {
            None
        } else {
            let e = self.parse_expr()?;
            self.expect_punct(";")?;
            Some(Box::new(e))
        };
        let cond = if self.eat(";") {
            None
        } else {
            let e = self.parse_expr()?;
            self.expect_punct(";")?;
            Some(Box::new(e))
        };
        let post = if self.is_punct(")") {
            None
        } else {
            let e = self.parse_expr()?;
            Some(Box::new(e))
        };
        self.expect_punct(")")?;
        let body = self.parse_statement()?;
        Ok(Node::For(init, cond, post, Box::new(body)))
    }

    /// switch 语句（§3.6.4.2）
    fn parse_switch(&mut self) -> Result<Node, String> {
        self.pos += 1;
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        let body = self.parse_statement()?;
        Ok(Node::Switch(Box::new(cond), Box::new(body)))
    }

    /// case 标签（§3.6.1）
    fn parse_case(&mut self) -> Result<Node, String> {
        self.pos += 1;
        let v = self.parse_expr()?;
        self.expect_punct(":")?;
        let stmt = self.parse_statement()?;
        Ok(Node::Case(Box::new(v), Box::new(stmt)))
    }

    /// default 标签（§3.6.1）
    fn parse_default(&mut self) -> Result<Node, String> {
        self.pos += 1;
        self.expect_punct(":")?;
        let stmt = self.parse_statement()?;
        Ok(Node::Default(Box::new(stmt)))
    }
}

/// 求值数组维数的整型常量表达式（支持 sizeof、算术、条件表达式）。
/// `sizeof(类型名)` 已在 parse_unary 折叠为 Num；这里处理剩余的
/// `sizeof(表达式)` —— sizeof 的结果是常量表达式（C89 §3.3.3.4），
/// 大小只取决于操作数类型，不取决于值。
fn eval_const_size(
    node: &Node,
    tt: &crate::types::TypeTable,
    typedefs: &HashMap<String, Type>,
) -> Result<i64, String> {
    match node.inner() {
        Node::Num(v) => Ok(*v),
        Node::SizeofExpr(e) => const_expr_size(e, tt, typedefs),
        Node::Unary(op, e) => {
            let v = eval_const_size(e, tt, typedefs)?;
            match op {
                UnaryOp::Neg => Ok(v.wrapping_neg()),
                UnaryOp::Not => Ok((v == 0) as i64),
                UnaryOp::BitNot => Ok(!v),
                _ => Err("数组大小中不支持该运算符".into()),
            }
        }
        Node::Binary(op, l, r) => {
            let a = eval_const_size(l, tt, typedefs)?;
            let b = eval_const_size(r, tt, typedefs)?;
            match op {
                BinaryOp::Add => Ok(a.wrapping_add(b)),
                BinaryOp::Sub => Ok(a.wrapping_sub(b)),
                BinaryOp::Mul => Ok(a.wrapping_mul(b)),
                BinaryOp::Div if b == 0 => Err("除零".into()),
                BinaryOp::Div => a.checked_div(b).ok_or_else(|| "除零".into()),
                BinaryOp::Mod if b == 0 => Err("模零".into()),
                BinaryOp::Mod => a.checked_rem(b).ok_or_else(|| "模零".into()),
                BinaryOp::Shl if (0..64).contains(&b) => Ok(a.wrapping_shl(b as u32)),
                BinaryOp::Shr if (0..64).contains(&b) => Ok(a.wrapping_shr(b as u32)),
                BinaryOp::Lt => Ok((a < b) as i64),
                BinaryOp::Gt => Ok((a > b) as i64),
                BinaryOp::Le => Ok((a <= b) as i64),
                BinaryOp::Ge => Ok((a >= b) as i64),
                BinaryOp::Eq => Ok((a == b) as i64),
                BinaryOp::Ne => Ok((a != b) as i64),
                BinaryOp::BitAnd => Ok(a & b),
                BinaryOp::BitXor => Ok(a ^ b),
                BinaryOp::BitOr => Ok(a | b),
                BinaryOp::LogAnd => Ok((a != 0 && b != 0) as i64),
                BinaryOp::LogOr => Ok((a != 0 || b != 0) as i64),
                _ => Err("数组大小中不支持该运算符".into()),
            }
        }
        Node::Cond(c, a, b) => {
            if eval_const_size(c, tt, typedefs)? != 0 {
                eval_const_size(a, tt, typedefs)
            } else {
                eval_const_size(b, tt, typedefs)
            }
        }
        Node::Cast(_, e) => eval_const_size(e, tt, typedefs),
        Node::Comma(_, r) => eval_const_size(r, tt, typedefs),
        _ => Err("数组大小必须是整数常量表达式".into()),
    }
}

/// 求 sizeof(表达式) 的操作数类型大小（教学型 32 位数据模型）：
/// 整型/指针/float = 4 字节，double = 8 字节。sizeof 只看类型不看值，
/// 所以对整型常量表达式做结构上的类型推导即可（C89 §3.3.3.4）。
fn const_expr_size(
    node: &Node,
    tt: &crate::types::TypeTable,
    typedefs: &HashMap<String, Type>,
) -> Result<i64, String> {
    match node.inner() {
        Node::Num(_) | Node::Str(_) => Ok(4),
        Node::FNum(_) => Ok(8),
        Node::SizeofExpr(_) => Ok(4),
        Node::Cast(ty, _) => tt
            .size_of(ty)
            .map(|s| s as i64)
            .map_err(|err| format!("sizeof 的操作数大小未知: {}", err)),
        Node::Ident(name) => match typedefs.get(name) {
            Some(ty) => tt
                .size_of(ty)
                .map(|s| s as i64)
                .map_err(|err| format!("sizeof 的操作数大小未知: {}", err)),
            None => Err(format!("sizeof 的操作数 '{}' 不是常量", name)),
        },
        Node::Unary(_, e) => const_expr_size(e, tt, typedefs),
        Node::Binary(_, l, r) => {
            let a = const_expr_size(l, tt, typedefs)?;
            let b = const_expr_size(r, tt, typedefs)?;
            Ok(a.max(b))
        }
        Node::Cond(_, a, b) => {
            let x = const_expr_size(a, tt, typedefs)?;
            let y = const_expr_size(b, tt, typedefs)?;
            Ok(x.max(y))
        }
        Node::Comma(_, r) => const_expr_size(r, tt, typedefs),
        _ => Err("sizeof 的操作数不是常量".into()),
    }
}

/// 便捷入口：从预处理后的 token 流解析一个完整表达式
pub fn parse_expr_tokens(toks: &[Token]) -> Result<Node, String> {
    let mut p = Parser::new(toks.to_vec());
    let node = p.parse_expr()?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::preprocess::Preprocessor;

    /// 预处理 + 解析表达式，返回 AST 字符串
    fn expr(src: &str) -> String {
        let toks = Preprocessor::new().run(src).unwrap();
        let toks: Vec<Token> = toks
            .into_iter()
            .filter(|t| t.kind != TokKind::Eof)
            .collect();
        let node = parse_expr_tokens(&toks).unwrap();
        node.to_string()
    }

    #[test]
    fn test_primary() {
        assert_eq!(expr("42"), "42");
        assert_eq!(expr("abc"), "abc");
        assert_eq!(expr("\"hi\""), "\"hi\"");
    }

    #[test]
    fn test_adjacent_string_concatenation() {
        assert_eq!(expr("\"ab\" \"cd\""), "\"abcd\"");
        assert_eq!(expr("\"ab\" \"cd\" \"ef\""), "\"abcdef\"");
        assert_eq!(expr("\"a\" + \"b\""), "(\"a\" + \"b\")");
        assert_eq!(expr("\"a\" \"b\" \"c\""), "\"abc\"");
    }

    #[test]
    fn test_precedence() {
        assert_eq!(expr("1 + 2 * 3"), "(1 + (2 * 3))");
        assert_eq!(expr("(1 + 2) * 3"), "((1 + 2) * 3)");
        assert_eq!(expr("1 - 2 - 3"), "((1 - 2) - 3)"); // 左结合
        assert_eq!(expr("1 << 2 + 3"), "(1 << (2 + 3))"); // 移位低于加减
        assert_eq!(expr("1 < 2 == 3"), "((1 < 2) == 3)");
        assert_eq!(expr("1 & 2 | 3 ^ 4"), "((1 & 2) | (3 ^ 4))");
        assert_eq!(expr("a && b || c"), "((a && b) || c)");
    }

    #[test]
    fn test_unary() {
        assert_eq!(expr("-x"), "(-x)");
        assert_eq!(expr("!flag"), "(!flag)");
        assert_eq!(expr("~v"), "(~v)");
        assert_eq!(expr("*p"), "(*p)");
        assert_eq!(expr("&a"), "(&a)");
        assert_eq!(expr("++a"), "(++a)");
        assert_eq!(expr("a++"), "(a++)");
        assert_eq!(expr("--a"), "(--a)");
        assert_eq!(expr("a--"), "(a--)");
        assert_eq!(expr("-a * b"), "((-a) * b)");
    }

    #[test]
    fn test_assign() {
        assert_eq!(expr("a = 1"), "(a = 1)");
        assert_eq!(expr("a = b = 1"), "(a = (b = 1))"); // 右结合
        assert_eq!(expr("a += 2"), "(a += 2)");
        assert_eq!(expr("a <<= 3"), "(a <<= 3)");
    }

    #[test]
    fn test_conditional() {
        assert_eq!(expr("a ? b : c"), "(a ? b : c)");
        assert_eq!(expr("a ? b : c ? d : e"), "(a ? b : (c ? d : e))"); // 右结合
    }

    #[test]
    fn test_call() {
        assert_eq!(expr("f()"), "f()");
        assert_eq!(expr("f(1, 2 + 3)"), "f(1, (2 + 3))");
        assert_eq!(expr("g(x)(y)"), "g(x)(y)");
    }

    #[test]
    fn test_subscript_member() {
        assert_eq!(expr("a[i + 1]"), "a[(i + 1)]");
        assert_eq!(expr("p->x"), "p->x");
        assert_eq!(expr("s.y"), "s.y");
        assert_eq!(expr("a[0][1]"), "a[0][1]");
    }

    #[test]
    fn test_comma() {
        assert_eq!(expr("a, b"), "(a, b)");
        assert_eq!(expr("f(a, b), c"), "(f(a, b), c)");
    }

    #[test]
    fn test_sizeof() {
        assert_eq!(expr("sizeof x"), "sizeof(x)");
        assert_eq!(expr("sizeof(x + 1)"), "sizeof((x + 1))");
    }

    #[test]
    fn test_parse_error() {
        let toks = Preprocessor::new().run("1 +").unwrap();
        let toks: Vec<Token> = toks
            .into_iter()
            .filter(|t| t.kind != TokKind::Eof)
            .collect();
        assert!(parse_expr_tokens(&toks).is_err());
    }

    // ---- 语句测试 ----

    fn stmt(src: &str) -> String {
        let toks = Preprocessor::new().run(src).unwrap();
        let toks: Vec<Token> = toks
            .into_iter()
            .filter(|t| t.kind != TokKind::Eof)
            .collect();
        let mut p = Parser::new(toks);
        p.parse_statement().unwrap().to_string()
    }

    #[test]
    fn test_stmt_block() {
        assert_eq!(stmt("{ a = 1; b = 2; }"), "{ (a = 1); (b = 2); }");
        assert_eq!(stmt("{}"), "{  }");
    }

    #[test]
    fn test_stmt_expr() {
        assert_eq!(stmt("x = 1;"), "(x = 1);");
        assert_eq!(stmt(";"), ";");
    }

    #[test]
    fn test_stmt_if() {
        assert_eq!(stmt("if (x) y = 1;"), "if (x) (y = 1);");
        assert_eq!(
            stmt("if (x) y = 1; else y = 2;"),
            "if (x) (y = 1); else (y = 2);"
        );
        // else 悬挂：匹配最近的 if
        assert_eq!(stmt("if (a) if (b) c; else d;"), "if (a) if (b) c; else d;");
    }

    #[test]
    fn test_stmt_while_do() {
        assert_eq!(
            stmt("while (i < 10) i = i + 1;"),
            "while ((i < 10)) (i = (i + 1));"
        );
        assert_eq!(
            stmt("do i++; while (i < 10);"),
            "do (i++); while ((i < 10))"
        );
    }

    #[test]
    fn test_stmt_for() {
        assert_eq!(
            stmt("for (i = 0; i < 10; i++) sum = sum + i;"),
            "for ((i = 0); (i < 10); (i++)) (sum = (sum + i));"
        );
        // 空部分
        assert_eq!(stmt("for (;;) break;"), "for (; ; ) break;");
    }

    #[test]
    fn test_stmt_switch() {
        assert_eq!(
            stmt("switch (x) { case 1: y = 1; break; default: y = 0; }"),
            "switch (x) { case 1: (y = 1); break; default: (y = 0); }"
        );
    }

    #[test]
    fn test_stmt_return() {
        assert_eq!(stmt("return;"), "return;");
        assert_eq!(stmt("return 42;"), "return 42;");
        assert_eq!(stmt("return a + b;"), "return (a + b);");
    }

    #[test]
    fn test_stmt_goto_label() {
        assert_eq!(stmt("goto done;"), "goto done;");
        assert_eq!(stmt("done: return;"), "done: return;");
    }

    #[test]
    fn test_stmt_missing_semicolon() {
        let toks = Preprocessor::new().run("x = 1").unwrap();
        let toks: Vec<Token> = toks
            .into_iter()
            .filter(|t| t.kind != TokKind::Eof)
            .collect();
        let mut p = Parser::new(toks);
        assert!(p.parse_statement().is_err());
    }

    // ---- 声明与函数测试 ----

    fn prog(src: &str) -> String {
        let toks = Preprocessor::new().run(src).unwrap();
        let toks: Vec<Token> = toks
            .into_iter()
            .filter(|t| t.kind != TokKind::Eof)
            .collect();
        let mut p = Parser::new(toks);
        let items = p.parse_program().unwrap();
        items
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_func_def_basic() {
        assert_eq!(
            prog("int main(void) { return 42; }"),
            "int main() { return 42; }"
        );
    }

    #[test]
    fn test_func_def_with_params() {
        assert_eq!(
            prog("int add(int a, int b) { return a + b; }"),
            "int add(int a, int b) { return (a + b); }"
        );
    }

    #[test]
    fn test_var_decl() {
        assert_eq!(prog("int x;"), "int x;");
        assert_eq!(prog("int y = 5;"), "int y = 5;");
        assert_eq!(prog("char c = 'a';"), "char c = 97;"); // 字符常量在 AST 中是数值
    }

    #[test]
    fn test_var_decl_multi() {
        assert_eq!(
            prog("int a = 1, b = 2;"),
            "int a = 1;
int b = 2;"
        );
    }

    #[test]
    fn test_global_and_func() {
        let src = "int g;\nint main(void) { return g; }";
        assert_eq!(prog(src), "int g;\nint main() { return g; }");
    }

    #[test]
    fn test_func_decl() {
        assert_eq!(prog("int f(int x);"), "int f(int x);");
    }

    #[test]
    fn test_typedef() {
        // typedef int myint; 后 myint 是类型名
        assert_eq!(
            prog("typedef int myint;\nmyint x = 3;"),
            "typedef int myint;\nint x = 3;"
        );
    }

    #[test]
    fn test_unsigned() {
        assert_eq!(prog("unsigned int u;"), "unsigned int u;");
    }

    #[test]
    fn test_static_var() {
        assert_eq!(prog("static int s;"), "static int s;");
    }

    #[test]
    fn test_pointer_decl() {
        // P3 简化：指针声明解析名字（指针信息暂存）
        assert_eq!(prog("int *p;"), "*int p;");
    }

    #[test]
    fn test_full_main() {
        let src = "int main(void) {\n    int x = 1;\n    int y = 2;\n    if (x < y) {\n        return x;\n    }\n    return 0;\n}";
        assert_eq!(
            prog(src),
            "int main() { int x = 1; int y = 2; if ((x < y)) { return x; } return 0; }"
        );
    }

    // ---- P4-2 声明符测试 ----

    #[test]
    fn test_ptr_decl() {
        assert_eq!(prog("int *p;"), "*int p;");
        assert_eq!(prog("int **pp;"), "**int pp;");
        assert_eq!(prog("char *s = \"hi\";"), "*char s = \"hi\";");
    }

    #[test]
    fn test_array_decl() {
        assert_eq!(prog("int a[10];"), "[10]int a;");
        assert_eq!(prog("int a[3][4];"), "[3][4]int a;");
        assert_eq!(prog("int *p[2];"), "[2]*int p;");
    }

    #[test]
    fn test_spiral_decl() {
        // int (*fp[3])(int)：fp 是"3 个指向函数的指针"的数组
        assert_eq!(prog("int (*fp[3])(int);"), "[3]*fn(int) -> int fp;");
        // 数组的指针：int (*pa)[5];
        assert_eq!(prog("int (*pa)[5];"), "*[5]int pa;");
    }

    #[test]
    fn test_old_style_function_definition() {
        assert_eq!(
            prog("int add(a, b) int a; int b; { return a + b; }"),
            "int add(int a, int b) { return (a + b); }"
        );
    }

    #[test]
    fn test_typedef_real_type() {
        // typedef 真实类型：不再是 int
        assert_eq!(prog("typedef int *ip;\nip p;"), "typedef *int ip;\n*int p;");
    }

    #[test]
    fn test_unnamed_param() {
        // 无名参数
        assert_eq!(prog("int f(int, char);"), "int f(int, char);");
    }

    #[test]
    fn test_array_param() {
        // 数组参数（函数声明）
        assert_eq!(prog("int sum(int a[10]);"), "int sum(*int a);");
    }

    #[test]
    fn test_ptr_func_def() {
        // 返回指针的函数
        assert_eq!(
            prog("int *get(void) { return 0; }"),
            "*int get() { return 0; }"
        );
    }

    #[test]
    fn test_void_ptr() {
        assert_eq!(prog("void *buf;"), "*void buf;");
    }
    fn assert_all_nodes_have_spans(node: &Node) {
        assert!(node.span().is_some(), "node has no span: {node:?}");
        match node.inner() {
            Node::Unary(_, value)
            | Node::SizeofExpr(value)
            | Node::Cast(_, value)
            | Node::ExprStmt(value)
            | Node::Default(value)
            | Node::Label(_, value) => {
                assert_all_nodes_have_spans(value);
            }
            Node::Binary(_, left, right)
            | Node::Assign(_, left, right)
            | Node::Comma(left, right)
            | Node::Subscript(left, right)
            | Node::While(left, right)
            | Node::DoWhile(left, right)
            | Node::Switch(left, right)
            | Node::Case(left, right) => {
                assert_all_nodes_have_spans(left);
                assert_all_nodes_have_spans(right);
            }
            Node::Cond(condition, yes, no) => {
                assert_all_nodes_have_spans(condition);
                assert_all_nodes_have_spans(yes);
                assert_all_nodes_have_spans(no);
            }
            Node::Call(function, arguments) => {
                assert_all_nodes_have_spans(function);
                for argument in arguments {
                    assert_all_nodes_have_spans(argument);
                }
            }
            Node::Member(object, _, _) => assert_all_nodes_have_spans(object),
            Node::InitList(items) | Node::Block(items) => {
                for item in items {
                    assert_all_nodes_have_spans(item);
                }
            }
            Node::If(condition, yes, no) => {
                assert_all_nodes_have_spans(condition);
                assert_all_nodes_have_spans(yes);
                if let Some(no) = no {
                    assert_all_nodes_have_spans(no);
                }
            }
            Node::For(init, condition, post, body) => {
                if let Some(value) = init {
                    assert_all_nodes_have_spans(value);
                }
                if let Some(value) = condition {
                    assert_all_nodes_have_spans(value);
                }
                if let Some(value) = post {
                    assert_all_nodes_have_spans(value);
                }
                assert_all_nodes_have_spans(body);
            }
            Node::Return(Some(value)) => assert_all_nodes_have_spans(value),
            Node::Decl {
                init: Some(value), ..
            } => assert_all_nodes_have_spans(value),
            Node::FuncDef { body, .. } => assert_all_nodes_have_spans(body),
            Node::Located(_, _) => unreachable!("inner() removes location wrappers"),
            _ => {}
        }
    }

    #[test]
    fn parsed_ast_every_node_has_source_span() {
        let source = "int main(void) { int x = 1; if (x) return x + 2; return 0; }";
        let tokens = Preprocessor::new().run(source).unwrap();
        let mut parser = Parser::new(tokens);
        let items = parser.parse_program().unwrap();
        for item in &items {
            assert_all_nodes_have_spans(item);
        }
        let span = items[0].span().unwrap();
        assert_eq!(span.file, "<source>");
        assert_eq!((span.line, span.column), (1, 1));
    }
}
