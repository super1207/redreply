//! AST（抽象语法树）定义
//!
//! 对照 C89 标准：
//! - §3.3 表达式（P1 已实现）
//! - §3.6 语句（P2 待实现）
//! - §3.5 声明（P3 待实现）

use crate::diagnostics::Span;
use std::fmt;

/// 基本类型（§3.5.2）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Char,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    Float,
    Double,
    /// 类型限定符。限定符位于类型树中，因此可区分 const 指针与指向 const 的指针。
    Qualified {
        ty: Box<Type>,
        is_const: bool,
        is_volatile: bool,
    },
    /// 指针：指向的类型
    Ptr(Box<Type>),
    /// 函数类型。函数对象本身没有大小，但可由指针引用。
    Function {
        ret: Box<Type>,
        params: Vec<Param>,
        is_variadic: bool,
        has_prototype: bool,
    },
    /// 数组：元素类型 + 长度（None = 不完整类型 []）
    Array(Box<Type>, Option<usize>),
    /// 结构体/联合体（按 tag 名查 TypeTable）
    Struct(String),
    /// 枚举（按 tag 名查 TypeTable）
    Enum(String),
    /// typedef 别名（名字在符号表解析）
    Typedef(&'static str),
}

impl Type {
    pub fn unqualified(&self) -> &Type {
        match self {
            Type::Qualified { ty, .. } => ty.unqualified(),
            other => other,
        }
    }

    pub fn without_top_qualifiers(&self) -> Type {
        match self {
            Type::Qualified { ty, .. } => ty.as_ref().clone(),
            other => other.clone(),
        }
    }

    pub fn is_const_qualified(&self) -> bool {
        matches!(self, Type::Qualified { is_const: true, .. })
    }

    pub fn is_volatile_qualified(&self) -> bool {
        matches!(
            self,
            Type::Qualified {
                is_volatile: true,
                ..
            }
        )
    }

    pub fn is_floating(&self) -> bool {
        matches!(self.unqualified(), Type::Float | Type::Double)
    }

    pub fn is_arithmetic(&self) -> bool {
        self.is_integer() || self.is_floating()
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self.unqualified(), Type::Ptr(_))
    }

    pub fn is_pointer_like(&self) -> bool {
        matches!(self.unqualified(), Type::Ptr(_) | Type::Array(_, _))
    }

    pub fn is_struct(&self) -> bool {
        matches!(self.unqualified(), Type::Struct(_))
    }

    pub fn pointed_type(&self) -> Option<&Type> {
        match self.unqualified() {
            Type::Ptr(inner) | Type::Array(inner, _) => Some(inner),
            _ => None,
        }
    }

    pub fn qualify(self, is_const: bool, is_volatile: bool) -> Type {
        if !is_const && !is_volatile {
            self
        } else {
            Type::Qualified {
                ty: Box::new(self),
                is_const,
                is_volatile,
            }
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self.unqualified(),
            Type::Char
                | Type::UChar
                | Type::Short
                | Type::UShort
                | Type::Int
                | Type::UInt
                | Type::Long
                | Type::ULong
                | Type::Enum(_)
        )
    }

    pub fn is_unsigned_integer(&self) -> bool {
        matches!(
            self.unqualified(),
            Type::UChar | Type::UShort | Type::UInt | Type::ULong
        )
    }

    pub fn integer_promotion(&self) -> Type {
        match self.unqualified() {
            Type::Char | Type::UChar | Type::Short | Type::UShort | Type::Enum(_) => Type::Int,
            other => other.clone(),
        }
    }

    pub fn usual_arithmetic(left: &Type, right: &Type) -> Option<Type> {
        if matches!(left.unqualified(), Type::Double) || matches!(right.unqualified(), Type::Double)
        {
            return Some(Type::Double);
        }
        if matches!(left.unqualified(), Type::Float) || matches!(right.unqualified(), Type::Float) {
            return Some(Type::Float);
        }
        if !left.is_integer() || !right.is_integer() {
            return None;
        }
        let left = left.integer_promotion();
        let right = right.integer_promotion();
        if matches!(left, Type::ULong)
            || matches!(right, Type::ULong)
            || (matches!(left, Type::Long) && matches!(right, Type::UInt))
            || (matches!(left, Type::UInt) && matches!(right, Type::Long))
        {
            Some(Type::ULong)
        } else if matches!(left, Type::Long) || matches!(right, Type::Long) {
            Some(Type::Long)
        } else if matches!(left, Type::UInt) || matches!(right, Type::UInt) {
            Some(Type::UInt)
        } else {
            Some(Type::Int)
        }
    }
}

/// 存储类（§3.5.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    None,
    Typedef,
    Static,
    Extern,
    Auto,
    Register,
}

/// 声明说明符（declaration-specifiers）
#[derive(Debug, Clone)]
pub struct DeclSpec {
    pub storage: Storage,
    pub ty: Type,
    pub is_const: bool,
    pub is_volatile: bool,
    pub is_unsigned: bool,
}

impl Default for DeclSpec {
    fn default() -> Self {
        Self::new()
    }
}

impl DeclSpec {
    pub fn new() -> Self {
        DeclSpec {
            storage: Storage::None,
            ty: Type::Int,
            is_const: false,
            is_volatile: false,
            is_unsigned: false,
        }
    }
}

/// 函数参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
}

/// 一元运算符（§3.3.3）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,     // -
    Not,     // !
    BitNot,  // ~
    Addr,    // &
    Deref,   // *
    PreInc,  // ++x
    PreDec,  // --x
    PostInc, // x++
    PostDec, // x--
}

/// 二元运算符（§3.3.5 ~ 3.3.14）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    LogAnd,
    LogOr,
}

/// 赋值运算符（§3.3.16）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Plain, // =
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    BitAnd,
    BitXor,
    BitOr,
}

/// AST 节点
#[derive(Debug, Clone)]
pub enum Node {
    /// 节点自身的源位置。解析器会在表达式、语句和外部声明边界生成该包装。
    Located(Box<Node>, Span),

    // ---- 表达式（§3.3）----
    Num(i64),
    /// 浮点字面量（教学简化按 float 处理；C89 标准为 double）
    FNum(f64),
    Str(String),
    Ident(String),
    Unary(UnaryOp, Box<Node>),
    Binary(BinaryOp, Box<Node>, Box<Node>),
    Assign(AssignOp, Box<Node>, Box<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
    Call(Box<Node>, Vec<Node>),
    Subscript(Box<Node>, Box<Node>),
    Member(Box<Node>, String, bool), // bool: true 表示 ->
    Comma(Box<Node>, Box<Node>),
    SizeofExpr(Box<Node>),
    /// 类型转换 (int)x（§3.3.4）
    Cast(Type, Box<Node>),
    /// 聚合初始化 {1, 2, 3}
    InitList(Vec<Node>),

    // ---- 语句（§3.6）----
    Block(Vec<Node>),
    If(Box<Node>, Box<Node>, Option<Box<Node>>),
    While(Box<Node>, Box<Node>),
    DoWhile(Box<Node>, Box<Node>),
    For(
        Option<Box<Node>>,
        Option<Box<Node>>,
        Option<Box<Node>>,
        Box<Node>,
    ),
    Switch(Box<Node>, Box<Node>),
    Case(Box<Node>, Box<Node>),
    Default(Box<Node>),
    Break,
    Continue,
    Return(Option<Box<Node>>),
    Goto(String),
    Label(String, Box<Node>),
    ExprStmt(Box<Node>),
    NullStmt,

    // ---- 声明与函数（§3.5 / §3.7）----
    Decl {
        specs: DeclSpec,
        name: String,
        init: Option<Box<Node>>,
    },
    FuncDef {
        ret: Type,
        name: String,
        /// 函数定义的存储类（static 内部链接 / extern 等）
        storage: Storage,
        params: Vec<Param>,
        is_variadic: bool,
        has_prototype: bool,
        body: Box<Node>,
    },
    FuncDecl {
        ret: Type,
        name: String,
        params: Vec<Param>,
        is_variadic: bool,
        has_prototype: bool,
    },
}

impl Node {
    pub fn with_span(self, span: Span) -> Self {
        Self::Located(Box::new(self), span)
    }

    pub fn span(&self) -> Option<&Span> {
        match self {
            Self::Located(_, span) => Some(span),
            _ => None,
        }
    }

    pub fn inner(&self) -> &Node {
        match self {
            Self::Located(node, _) => node.inner(),
            _ => self,
        }
    }

    /// Remove location wrappers before passing the tree to the legacy code generator.
    /// Attach a fallback span to this node and every unlocated descendant.
    /// Descendants that already have a more precise parser span keep it.
    pub fn with_fallback_span(self, span: Span) -> Self {
        if let Self::Located(node, own_span) = self {
            return Self::Located(
                Box::new(node.with_fallback_span(own_span.clone())),
                own_span,
            );
        }
        let value = match self {
            Self::Unary(op, node) => {
                Self::Unary(op, Box::new(node.with_fallback_span(span.clone())))
            }
            Self::Binary(op, a, b) => Self::Binary(
                op,
                Box::new(a.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Assign(op, a, b) => Self::Assign(
                op,
                Box::new(a.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Cond(c, a, b) => Self::Cond(
                Box::new(c.with_fallback_span(span.clone())),
                Box::new(a.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Call(f, args) => Self::Call(
                Box::new(f.with_fallback_span(span.clone())),
                args.into_iter()
                    .map(|n| n.with_fallback_span(span.clone()))
                    .collect(),
            ),
            Self::Subscript(a, b) => Self::Subscript(
                Box::new(a.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Member(a, name, arrow) => {
                Self::Member(Box::new(a.with_fallback_span(span.clone())), name, arrow)
            }
            Self::Comma(a, b) => Self::Comma(
                Box::new(a.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::SizeofExpr(a) => Self::SizeofExpr(Box::new(a.with_fallback_span(span.clone()))),
            Self::Cast(t, a) => Self::Cast(t, Box::new(a.with_fallback_span(span.clone()))),
            Self::InitList(items) => Self::InitList(
                items
                    .into_iter()
                    .map(|n| n.with_fallback_span(span.clone()))
                    .collect(),
            ),
            Self::Block(items) => Self::Block(
                items
                    .into_iter()
                    .map(|n| n.with_fallback_span(span.clone()))
                    .collect(),
            ),
            Self::If(c, a, b) => Self::If(
                Box::new(c.with_fallback_span(span.clone())),
                Box::new(a.with_fallback_span(span.clone())),
                b.map(|n| Box::new(n.with_fallback_span(span.clone()))),
            ),
            Self::While(c, b) => Self::While(
                Box::new(c.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::DoWhile(b, c) => Self::DoWhile(
                Box::new(b.with_fallback_span(span.clone())),
                Box::new(c.with_fallback_span(span.clone())),
            ),
            Self::For(i, c, p, b) => Self::For(
                i.map(|n| Box::new(n.with_fallback_span(span.clone()))),
                c.map(|n| Box::new(n.with_fallback_span(span.clone()))),
                p.map(|n| Box::new(n.with_fallback_span(span.clone()))),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Switch(c, b) => Self::Switch(
                Box::new(c.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Case(c, b) => Self::Case(
                Box::new(c.with_fallback_span(span.clone())),
                Box::new(b.with_fallback_span(span.clone())),
            ),
            Self::Default(b) => Self::Default(Box::new(b.with_fallback_span(span.clone()))),
            Self::Return(e) => {
                Self::Return(e.map(|n| Box::new(n.with_fallback_span(span.clone()))))
            }
            Self::Label(name, b) => Self::Label(name, Box::new(b.with_fallback_span(span.clone()))),
            Self::ExprStmt(e) => Self::ExprStmt(Box::new(e.with_fallback_span(span.clone()))),
            Self::Decl { specs, name, init } => Self::Decl {
                specs,
                name,
                init: init.map(|n| Box::new(n.with_fallback_span(span.clone()))),
            },
            Self::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                has_prototype,
                body,
            } => Self::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                has_prototype,
                body: Box::new(body.with_fallback_span(span.clone())),
            },
            other => other,
        };
        Self::Located(Box::new(value), span)
    }

    pub fn without_spans(self) -> Self {
        match self {
            Self::Located(node, _) => node.without_spans(),
            Self::Unary(op, node) => Self::Unary(op, Box::new(node.without_spans())),
            Self::Binary(op, a, b) => {
                Self::Binary(op, Box::new(a.without_spans()), Box::new(b.without_spans()))
            }
            Self::Assign(op, a, b) => {
                Self::Assign(op, Box::new(a.without_spans()), Box::new(b.without_spans()))
            }
            Self::Cond(c, a, b) => Self::Cond(
                Box::new(c.without_spans()),
                Box::new(a.without_spans()),
                Box::new(b.without_spans()),
            ),
            Self::Call(f, args) => Self::Call(
                Box::new(f.without_spans()),
                args.into_iter().map(Self::without_spans).collect(),
            ),
            Self::Subscript(a, b) => {
                Self::Subscript(Box::new(a.without_spans()), Box::new(b.without_spans()))
            }
            Self::Member(a, name, arrow) => Self::Member(Box::new(a.without_spans()), name, arrow),
            Self::Comma(a, b) => {
                Self::Comma(Box::new(a.without_spans()), Box::new(b.without_spans()))
            }
            Self::SizeofExpr(a) => Self::SizeofExpr(Box::new(a.without_spans())),
            Self::Cast(t, a) => Self::Cast(t, Box::new(a.without_spans())),
            Self::InitList(items) => {
                Self::InitList(items.into_iter().map(Self::without_spans).collect())
            }
            Self::Block(items) => Self::Block(items.into_iter().map(Self::without_spans).collect()),
            Self::If(c, a, b) => Self::If(
                Box::new(c.without_spans()),
                Box::new(a.without_spans()),
                b.map(|x| Box::new(x.without_spans())),
            ),
            Self::While(c, b) => {
                Self::While(Box::new(c.without_spans()), Box::new(b.without_spans()))
            }
            Self::DoWhile(b, c) => {
                Self::DoWhile(Box::new(b.without_spans()), Box::new(c.without_spans()))
            }
            Self::For(i, c, p, b) => Self::For(
                i.map(|x| Box::new(x.without_spans())),
                c.map(|x| Box::new(x.without_spans())),
                p.map(|x| Box::new(x.without_spans())),
                Box::new(b.without_spans()),
            ),
            Self::Switch(c, b) => {
                Self::Switch(Box::new(c.without_spans()), Box::new(b.without_spans()))
            }
            Self::Case(c, b) => {
                Self::Case(Box::new(c.without_spans()), Box::new(b.without_spans()))
            }
            Self::Default(b) => Self::Default(Box::new(b.without_spans())),
            Self::Return(e) => Self::Return(e.map(|x| Box::new(x.without_spans()))),
            Self::Label(name, b) => Self::Label(name, Box::new(b.without_spans())),
            Self::ExprStmt(e) => Self::ExprStmt(Box::new(e.without_spans())),
            Self::Decl { specs, name, init } => Self::Decl {
                specs,
                name,
                init: init.map(|node| Box::new(node.without_spans())),
            },
            Self::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                has_prototype,
                body,
            } => Self::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                has_prototype,
                body: Box::new(body.without_spans()),
            },
            other => other,
        }
    }
}

/// AST 调试打印（人类可读）
impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Located(node, _) => write!(f, "{}", node),
            Node::Num(v) => write!(f, "{}", v),
            Node::FNum(v) => write!(f, "{}", v),
            Node::Str(s) => write!(f, "\"{}\"", s),
            Node::Ident(s) => write!(f, "{}", s),
            Node::Unary(op, e) => {
                match op {
                    UnaryOp::PostInc => return write!(f, "({}++)", e),
                    UnaryOp::PostDec => return write!(f, "({}--)", e),
                    _ => {}
                }
                let s = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Addr => "&",
                    UnaryOp::Deref => "*",
                    UnaryOp::PreInc => "++",
                    UnaryOp::PreDec => "--",
                    _ => "",
                };
                write!(f, "({}{})", s, e)
            }
            Node::Binary(op, a, b) => write!(f, "({} {} {})", a, binop_str(*op), b),
            Node::Assign(op, a, b) => write!(f, "({} {} {})", a, assignop_str(*op), b),
            Node::Cond(c, a, b) => write!(f, "({} ? {} : {})", c, a, b),
            Node::Call(fn_, args) => {
                let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                write!(f, "{}({})", fn_, args.join(", "))
            }
            Node::Subscript(a, b) => write!(f, "{}[{}]", a, b),
            Node::Member(a, name, arrow) => {
                if *arrow {
                    write!(f, "{}->{}", a, name)
                } else {
                    write!(f, "{}.{}", a, name)
                }
            }
            Node::Comma(a, b) => write!(f, "({}, {})", a, b),
            Node::SizeofExpr(e) => write!(f, "sizeof({})", e),
            Node::Cast(t, e) => write!(f, "({}){}", type_str(t.clone()), e),
            Node::InitList(items) => {
                let inner: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                write!(f, "{{ {} }}", inner.join(", "))
            }
            Node::Block(stmts) => {
                let inner: Vec<String> = stmts.iter().map(|s| s.to_string()).collect();
                write!(f, "{{ {} }}", inner.join(" "))
            }
            Node::If(c, t, e) => {
                if let Some(e) = e {
                    write!(f, "if ({}) {} else {}", c, t, e)
                } else {
                    write!(f, "if ({}) {}", c, t)
                }
            }
            Node::While(c, b) => write!(f, "while ({}) {}", c, b),
            Node::DoWhile(b, c) => write!(f, "do {} while ({})", b, c),
            Node::For(i, c, p, b) => {
                let i = i.as_ref().map(|x| x.to_string()).unwrap_or_default();
                let c = c.as_ref().map(|x| x.to_string()).unwrap_or_default();
                let p = p.as_ref().map(|x| x.to_string()).unwrap_or_default();
                write!(f, "for ({}; {}; {}) {}", i, c, p, b)
            }
            Node::Switch(c, b) => write!(f, "switch ({}) {}", c, b),
            Node::Case(v, s) => write!(f, "case {}: {}", v, s),
            Node::Default(s) => write!(f, "default: {}", s),
            Node::Break => write!(f, "break;"),
            Node::Continue => write!(f, "continue;"),
            Node::Return(Some(e)) => write!(f, "return {};", e),
            Node::Return(None) => write!(f, "return;"),
            Node::Goto(n) => write!(f, "goto {};", n),
            Node::Label(n, s) => write!(f, "{}: {}", n, s),
            Node::ExprStmt(e) => write!(f, "{};", e),
            Node::NullStmt => write!(f, ";"),
            Node::Decl { specs, name, init } => {
                let storage = match specs.storage {
                    Storage::Typedef => "typedef ",
                    Storage::Static => "static ",
                    Storage::Extern => "extern ",
                    Storage::Auto => "auto ",
                    Storage::Register => "register ",
                    Storage::None => "",
                };
                let s = if specs.is_unsigned { "unsigned " } else { "" };
                let init = init
                    .as_ref()
                    .map(|i| format!(" = {}", i))
                    .unwrap_or_default();
                write!(
                    f,
                    "{}{}{} {}{};",
                    storage,
                    s,
                    type_str(specs.ty.clone()),
                    name,
                    init
                )
            }
            Node::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                body,
                ..
            } => {
                let storage = match storage {
                    Storage::Typedef => "typedef ",
                    Storage::Static => "static ",
                    Storage::Extern => "extern ",
                    Storage::Auto => "auto ",
                    Storage::Register => "register ",
                    Storage::None => "",
                };
                let mut ps: Vec<String> = params
                    .iter()
                    .map(|p| {
                        if p.name.is_empty() {
                            type_str(p.ty.clone())
                        } else {
                            format!("{} {}", type_str(p.ty.clone()), p.name)
                        }
                    })
                    .collect();
                if *is_variadic {
                    ps.push("...".into());
                }
                write!(
                    f,
                    "{}{} {}({}) {}",
                    storage,
                    type_str(ret.clone()),
                    name,
                    ps.join(", "),
                    body
                )
            }
            Node::FuncDecl {
                ret,
                name,
                params,
                is_variadic,
                ..
            } => {
                let mut ps: Vec<String> = params
                    .iter()
                    .map(|p| {
                        if p.name.is_empty() {
                            type_str(p.ty.clone())
                        } else {
                            format!("{} {}", type_str(p.ty.clone()), p.name)
                        }
                    })
                    .collect();
                if *is_variadic {
                    ps.push("...".into());
                }
                write!(f, "{} {}({});", type_str(ret.clone()), name, ps.join(", "))
            }
        }
    }
}

fn binop_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitXor => "^",
        BinaryOp::BitOr => "|",
        BinaryOp::LogAnd => "&&",
        BinaryOp::LogOr => "||",
    }
}

fn assignop_str(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Plain => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::Mul => "*=",
        AssignOp::Div => "/=",
        AssignOp::Mod => "%=",
        AssignOp::Shl => "<<=",
        AssignOp::Shr => ">>=",
        AssignOp::BitAnd => "&=",
        AssignOp::BitXor => "^=",
        AssignOp::BitOr => "|=",
    }
}

fn type_str(t: Type) -> String {
    match t {
        Type::Void => "void".into(),
        Type::Char => "char".into(),
        Type::UChar => "unsigned char".into(),
        Type::Short => "short".into(),
        Type::UShort => "unsigned short".into(),
        Type::Int => "int".into(),
        Type::UInt => "unsigned int".into(),
        Type::Long => "long".into(),
        Type::ULong => "unsigned long".into(),
        Type::Float => "float".into(),
        Type::Double => "double".into(),
        Type::Qualified {
            ty,
            is_const,
            is_volatile,
        } => {
            let mut qualifiers = Vec::new();
            if is_const {
                qualifiers.push("const");
            }
            if is_volatile {
                qualifiers.push("volatile");
            }
            format!("{} {}", qualifiers.join(" "), type_str(*ty))
        }
        Type::Ptr(inner) => format!("*{}", type_str(*inner)),
        Type::Function {
            ret,
            params,
            is_variadic,
            has_prototype,
        } => {
            let mut values = params
                .into_iter()
                .map(|param| type_str(param.ty))
                .collect::<Vec<_>>();
            if is_variadic {
                values.push("...".into());
            }
            if !has_prototype {
                values.clear();
            }
            format!("fn({}) -> {}", values.join(", "), type_str(*ret))
        }
        Type::Array(inner, n) => {
            let n = n.map(|n| n.to_string()).unwrap_or_default();
            format!("[{}]{}", n, type_str(*inner))
        }
        Type::Struct(tag) => format!("struct {}", tag),
        Type::Enum(tag) => format!("enum {}", tag),
        Type::Typedef(_) => "typedef".into(),
    }
}
