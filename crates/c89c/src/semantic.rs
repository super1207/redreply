//! Minimal semantic validation before code generation.

use crate::ast::{BinaryOp, Node, Param, Type, UnaryOp};
use crate::diagnostics::{CompileError, CompileResult, Span};
use crate::lexer::Token;
use crate::types::TypeTable;
use std::collections::{HashMap, HashSet};

const BUILTINS: &[&str] = &["va_start", "va_arg", "va_end"];

#[derive(Clone)]
struct FunctionSignature {
    ret: Type,
    params: Vec<Param>,
    is_variadic: bool,
    has_prototype: bool,
}

fn compatible_types(left: &Type, right: &Type) -> bool {
    let left = left.unqualified();
    let right = right.unqualified();
    match (left, right) {
        (Type::Ptr(left), Type::Ptr(right)) => compatible_types(left, right),
        (
            Type::Function {
                ret: left_ret,
                params: left_params,
                is_variadic: left_variadic,
                has_prototype: left_prototype,
            },
            Type::Function {
                ret: right_ret,
                params: right_params,
                is_variadic: right_variadic,
                has_prototype: right_prototype,
            },
        ) => {
            compatible_types(left_ret, right_ret)
                && (!left_prototype
                    || !right_prototype
                    || (left_variadic == right_variadic
                        && left_params.len() == right_params.len()
                        && left_params
                            .iter()
                            .zip(right_params)
                            .all(|(left, right)| compatible_types(&left.ty, &right.ty))))
        }
        // C89 §3.5.4.3：不完整数组与同元素类型的完成数组兼容
        //（extern int a[]; int a[3]; 合法重声明）
        (Type::Array(left_elem, None), Type::Array(right_elem, Some(_)))
        | (Type::Array(left_elem, Some(_)), Type::Array(right_elem, None)) => {
            compatible_types(left_elem, right_elem)
        }
        _ => left == right,
    }
}

struct Context<'a> {
    file: &'a str,
    tokens: &'a [Token],
    /// 类型表（可选）：结构体初始化器成员数检查等需要
    types: Option<&'a TypeTable>,
}

impl Context<'_> {
    fn span(&self, node: &Node, text: Option<&str>) -> Span {
        node.span()
            .cloned()
            .or_else(|| {
                text.and_then(|text| self.tokens.iter().find(|token| token.text == text))
                    .map(|token| Span::new(&token.file, token.line, token.column))
            })
            .unwrap_or_else(|| Span::new(self.file, 1, 1))
    }

    fn token_span(&self, text: Option<&str>) -> Span {
        text.and_then(|text| self.tokens.iter().find(|token| token.text == text))
            .map(|token| Span::new(&token.file, token.line, token.column))
            .unwrap_or_else(|| Span::new(self.file, 1, 1))
    }

    fn error<T>(
        &self,
        node: &Node,
        text: Option<&str>,
        message: impl Into<String>,
    ) -> CompileResult<T> {
        Err(CompileError::semantic(self.span(node, text), message))
    }

    fn token_error<T>(&self, text: Option<&str>, message: impl Into<String>) -> CompileResult<T> {
        Err(CompileError::semantic(self.token_span(text), message))
    }
}

/// 从 token 流恢复函数声明（FuncDecl）的存储类：解析器生成的 FuncDecl 节点
/// 不携带 storage 字段，但声明首 token 若是存储类关键字仍留在 token 流中，
/// 用节点 span（声明起点行/列）定位并识别（`static int f();` → Static）。
fn func_decl_storage(item: &Node, context: &Context<'_>) -> crate::ast::Storage {
    let Some(span) = item.span() else {
        return crate::ast::Storage::None;
    };
    if let Some(token) = context.tokens.iter().find(|token| {
        token.file == span.file && token.line == span.line && token.column == span.column
    }) {
        match token.text.as_str() {
            "static" => return crate::ast::Storage::Static,
            "extern" => return crate::ast::Storage::Extern,
            "auto" => return crate::ast::Storage::Auto,
            "register" => return crate::ast::Storage::Register,
            _ => {}
        }
    }
    crate::ast::Storage::None
}

pub fn validate(items: &[Node], file: &str) -> CompileResult<()> {
    validate_with_tokens(items, file, &[])
}

pub fn validate_with_tokens(items: &[Node], file: &str, tokens: &[Token]) -> CompileResult<()> {
    validate_impl(items, file, tokens, None)
}

/// 带类型表的语义校验：结构体初始化器成员数等检查需要类型表。
pub fn validate_with_types(
    items: &[Node],
    file: &str,
    tokens: &[Token],
    types: &TypeTable,
) -> CompileResult<()> {
    validate_impl(items, file, tokens, Some(types))
}

fn validate_impl<'a>(
    items: &'a [Node],
    file: &'a str,
    tokens: &'a [Token],
    types: Option<&'a TypeTable>,
) -> CompileResult<()> {
    let context = Context { file, tokens, types };
    // 枚举常量值必须可表示为 int（i32），溢出报错（C89 §3.5.2.2）。
    if let Some(tt) = types {
        for def in tt.enums.values() {
            for (name, value) in &def.values {
                if *value > i32::MAX as i64 || *value < i32::MIN as i64 {
                    return context.token_error(
                        Some(name),
                        format!("枚举常量 '{name}' 的值 {value} 超出 int 范围"),
                    );
                }
            }
        }
    }
    let mut globals = HashSet::new();
    let mut global_types = HashMap::new();
    let mut functions = HashSet::new();
    let mut signatures: HashMap<String, FunctionSignature> = HashMap::new();
    // 文件作用域对象声明记录：(类型, 是否已定义, 存储类)——用于重复声明/定义/
    // 链接性冲突检查（C89 §3.1.2.2/§3.5）。函数定义集合用于重复定义检查。
    let mut object_decls: HashMap<String, (Type, bool, crate::ast::Storage)> = HashMap::new();
    // typedef 名集合与函数链接性记录：typedef 名不能再声明为对象/函数；
    // 函数与对象一样记录 static/extern 并做内部/外部链接冲突矩阵。
    let mut typedef_names: HashSet<String> = HashSet::new();
    let mut func_linkages: HashMap<String, crate::ast::Storage> = HashMap::new();
    let mut func_defs: HashSet<String> = HashSet::new();
    for item in items {
        match item.inner() {
            Node::Decl {
                specs, name, init, ..
            } => {
                if specs.storage == crate::ast::Storage::Typedef {
                    // typedef 名占用普通标识符名字空间（C89 §3.5.6）：单独登记，
                    // 不参与对象符号表。
                    typedef_names.insert(name.clone());
                    continue;
                }
                if typedef_names.contains(name) {
                    return context.error(
                        item,
                        Some(name),
                        format!("typedef 名 '{name}' 不能重新声明为对象"),
                    );
                }
                if functions.contains(name) {
                    return context.error(
                        item,
                        Some(name),
                        format!("'{name}' 已声明为函数，不能作为对象重新声明"),
                    );
                }
                globals.insert(name.clone());
                let has_init = init.is_some();
                if let Some((prev_ty, prev_init, prev_storage)) = object_decls.get(name) {
                    if !compatible_types(prev_ty, &specs.ty) {
                        return context.error(
                            item,
                            Some(name),
                            format!("对象 '{name}' 的重复声明类型不兼容"),
                        );
                    }
                    // C89 §3.5.4.3：同一对象的重复声明必须限定符组合一致
                    if !exactly_qualified_compatible(prev_ty, &specs.ty) {
                        return context.error(
                            item,
                            Some(name),
                            format!("对象 '{name}' 的重复声明限定符不一致"),
                        );
                    }
                    let prev_is_static = *prev_storage == crate::ast::Storage::Static;
                    let new_is_static = specs.storage == crate::ast::Storage::Static;
                    let linkage_conflict = match (prev_is_static, new_is_static) {
                        (true, false) => specs.storage != crate::ast::Storage::Extern,
                        (false, true) => true,
                        _ => false,
                    };
                    if linkage_conflict {
                        return context.error(
                            item,
                            Some(name),
                            format!("对象 '{name}' 的内部/外部链接声明冲突"),
                        );
                    }
                    if *prev_init && has_init {
                        return context
                            .error(item, Some(name), format!("对象 '{name}' 被重复定义"));
                    }
                    global_types.insert(name.clone(), specs.ty.clone());
                    object_decls.insert(
                        name.clone(),
                        (specs.ty.clone(), *prev_init || has_init, specs.storage),
                    );
                } else {
                    global_types.insert(name.clone(), specs.ty.clone());
                    object_decls.insert(name.clone(), (specs.ty.clone(), has_init, specs.storage));
                }
            }
            Node::FuncDecl {
                ret,
                name,
                params,
                is_variadic,
                has_prototype,
            }
            | Node::FuncDef {
                ret,
                name,
                params,
                is_variadic,
                has_prototype,
                ..
            } => {
                if typedef_names.contains(name) {
                    return context.error(
                        item,
                        Some(name),
                        format!("typedef 名 '{name}' 不能重新声明为函数"),
                    );
                }
                if globals.contains(name) {
                    return context.error(
                        item,
                        Some(name),
                        format!("'{name}' 已声明为对象，不能作为函数重新声明"),
                    );
                }
                if matches!(item.inner(), Node::FuncDef { .. }) {
                    if func_defs.contains(name) {
                        return context
                            .error(item, Some(name), format!("函数 '{name}' 被重复定义"));
                    }
                    func_defs.insert(name.clone());
                }
                // 函数存储类：FuncDef 自带；FuncDecl 从声明首 token 恢复
                //（`static int f();` 与 `extern int f();` 的链接冲突检查）。
                let storage = match item.inner() {
                    Node::FuncDef { storage, .. } => *storage,
                    _ => func_decl_storage(item, &context),
                };
                if let Some(prev_storage) = func_linkages.get(name) {
                    let prev_is_static = *prev_storage == crate::ast::Storage::Static;
                    let new_is_static = storage == crate::ast::Storage::Static;
                    let linkage_conflict = match (prev_is_static, new_is_static) {
                        (true, false) => storage != crate::ast::Storage::Extern,
                        (false, true) => true,
                        _ => false,
                    };
                    if linkage_conflict {
                        return context.error(
                            item,
                            Some(name),
                            format!("函数 '{name}' 的内部/外部链接声明冲突"),
                        );
                    }
                }
                func_linkages.insert(name.clone(), storage);
                validate_function_signature(ret, params, &context, item)?;
                functions.insert(name.clone());
                let next = FunctionSignature {
                    ret: ret.clone(),
                    params: params.clone(),
                    is_variadic: *is_variadic,
                    has_prototype: *has_prototype,
                };
                if let Some(previous) = signatures.get(name) {
                    let params_compatible = !previous.has_prototype
                        || !next.has_prototype
                        || (previous.is_variadic == next.is_variadic
                            && previous.params.len() == next.params.len()
                            && previous
                                .params
                                .iter()
                                .zip(&next.params)
                                .all(|(left, right)| compatible_types(&left.ty, &right.ty)));
                    if !compatible_types(&previous.ret, &next.ret) || !params_compatible {
                        return context.error(
                            item,
                            Some(name),
                            format!("函数 '{}' 的声明或定义与先前原型不兼容", name),
                        );
                    }
                }
                if next.has_prototype || !signatures.contains_key(name) {
                    signatures.insert(name.clone(), next);
                }
            }
            _ => {}
        }
    }
    validate_file_scope_objects(items, &context)?;
    for item in items {
        if let Node::FuncDef {
            ret, params, body, ..
        } = item.inner()
        {
            validate_function(
                body,
                params,
                &context,
                &globals,
                &global_types,
                &functions,
                &signatures,
                ret,
            )?;
        }
    }
    Ok(())
}

/// 算术常量表达式（整数/浮点字面量及其算术组合，不含对象引用——
/// 枚举常量已在解析期折叠为 Num）。用于浮点/整型静态初始化器检查。
fn is_arithmetic_constant(node: &Node) -> bool {
    match node.inner() {
        Node::Num(_) | Node::FNum(_) => true,
        Node::Unary(UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot, value)
        | Node::Cast(_, value) => is_arithmetic_constant(value),
        Node::Binary(_, left, right) | Node::Comma(left, right) => {
            is_arithmetic_constant(left) && is_arithmetic_constant(right)
        }
        Node::Cond(condition, yes, no) => {
            is_arithmetic_constant(condition)
                && is_arithmetic_constant(yes)
                && is_arithmetic_constant(no)
        }
        _ => false,
    }
}

/// 静态初始化器检查上下文中的对象符号表：名字 → 类型。
/// 文件作用域初始化器只能引用文件作用域对象；块内 static 初始化器
/// 只能引用文件作用域对象（自动对象地址不是常量，C89 §3.4/§3.5.7）。
type ObjectMap = HashMap<String, Type>;

/// 地址常量的基形式：数组名 / 字符串字面量（含下标链，如 arr[1]、arr[1][2]）、
/// 静态对象地址 &x、&x 的递归形式。根标识符必须是已知静态对象。
fn is_address_constant_base(node: &Node, objects: &ObjectMap) -> bool {
    match node.inner() {
        Node::Str(_) => true,
        Node::Ident(name) => objects.contains_key(name),
        Node::Unary(UnaryOp::Addr, value) => is_address_constant_base(value, objects),
        Node::Subscript(base, index) => {
            is_address_constant_base(base, objects) && eval_integer_constant(index).is_some()
        }
        _ => false,
    }
}

/// 指针地址常量：&静态对象（含下标形式）、数组名/字符串字面量 ± 整数常量
/// （C89 §3.4 地址常量）。
fn is_pointer_address_constant(node: &Node, objects: &ObjectMap) -> bool {
    match node.inner() {
        Node::Unary(UnaryOp::Addr, value) => is_address_constant_base(value, objects),
        Node::Binary(BinaryOp::Add, left, right) => {
            (is_address_constant_base(left, objects) && eval_integer_constant(right).is_some())
                || (eval_integer_constant(left).is_some() && is_address_constant_base(right, objects))
        }
        Node::Binary(BinaryOp::Sub, left, right) => {
            is_address_constant_base(left, objects) && eval_integer_constant(right).is_some()
        }
        _ => false,
    }
}

/// 静态初始化器检查结果。
enum StaticInit {
    Ok,
    /// 初始化器不是常量表达式
    NotConstant,
    /// 字符串字面量初始化了非字符数组
    StringArray,
    /// 字符串字面量初始化了非字符指针
    StringPtr,
    /// 初始化器数量超过数组/结构体成员数
    TooMany,
    /// 数组初始化器元素类型与元素类型不兼容
    ElemType,
    /// 常量表达式求值失败（如除零）
    Unevaluable,
}

fn check_static_initializer(ty: &Type, node: &Node, objects: &ObjectMap) -> StaticInit {
    match ty.unqualified() {
        Type::Array(elem, length) => match node.inner() {
            Node::Str(value) => {
                if matches!(elem.unqualified(), Type::Char | Type::UChar) {
                    if let Some(len) = length {
                        // 按转义解码后的实际字节数 + NUL 检查长度
                        let decoded = crate::expr_eval::decode_string_escapes(value)
                            .map(|bytes| bytes.len())
                            .unwrap_or(value.len());
                        if decoded + 1 > *len {
                            return StaticInit::TooMany;
                        }
                    }
                    StaticInit::Ok
                } else {
                    StaticInit::StringArray
                }
            }
            Node::InitList(items) => {
                if let Some(len) = length {
                    if items.len() > *len {
                        return StaticInit::TooMany;
                    }
                }
                // 逐元素校验类型（C89 §3.5.7）：元素须与数组元素类型赋值兼容
                for item in items {
                    if let Some(actual) = expression_type(item, objects) {
                        if !assignment_compatible(elem, &actual, item) {
                            return StaticInit::ElemType;
                        }
                    }
                }
                if items
                    .iter()
                    .all(|item| is_static_init_constant(item, objects))
                {
                    StaticInit::Ok
                } else {
                    StaticInit::NotConstant
                }
            }
            _ => StaticInit::NotConstant,
        },
        Type::Struct(_) => match node.inner() {
            Node::InitList(items) => {
                if items.iter().all(|item| is_static_init_constant(item, objects)) {
                    StaticInit::Ok
                } else {
                    StaticInit::NotConstant
                }
            }
            _ => StaticInit::NotConstant,
        },
        Type::Ptr(pointee) => match node.inner() {
            Node::Str(_) => {
                if matches!(pointee.unqualified(), Type::Char | Type::UChar) {
                    StaticInit::Ok
                } else {
                    StaticInit::StringPtr
                }
            }
            _ if eval_integer_constant(node) == Some(0) => StaticInit::Ok,
            Node::Ident(name) => match objects.get(name).map(Type::unqualified) {
                // 数组名/函数名退化为地址常量（C89 §3.2.2.1/§3.4）
                Some(Type::Array(_, _)) | Some(Type::Function { .. }) => StaticInit::Ok,
                // 对象的值不是常量（int *p = x 非法，须 &x）
                Some(_) | None => StaticInit::NotConstant,
            },
            _ if is_pointer_address_constant(node, objects) => StaticInit::Ok,
            _ => StaticInit::NotConstant,
        },
        Type::Float | Type::Double => {
            if eval_float_constant(node).is_some() {
                StaticInit::Ok
            } else if is_arithmetic_constant(node) {
                // 结构上是算术常量但求值失败（如 1/0）→ 约束违规
                StaticInit::Unevaluable
            } else {
                StaticInit::NotConstant
            }
        }
        ty if ty.is_integer() => {
            if eval_integer_constant(node).is_some() || eval_float_constant(node).is_some() {
                StaticInit::Ok
            } else if is_arithmetic_constant(node) {
                // 结构上是算术常量但求值失败（如 1/0）→ 约束违规
                StaticInit::Unevaluable
            } else {
                StaticInit::NotConstant
            }
        }
        _ => StaticInit::NotConstant,
    }
}

/// 单个静态初始化器元素是否为常量：算术常量、字符串、或地址常量
/// （&静态对象/数组名 ± 整数）。对象的值本身不是常量。
fn is_static_init_constant(node: &Node, objects: &ObjectMap) -> bool {
    match node.inner() {
        Node::Num(_) | Node::FNum(_) | Node::Str(_) => true,
        Node::Unary(UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot, value)
        | Node::Cast(_, value) => is_static_init_constant(value, objects),
        Node::Binary(_, left, right) | Node::Comma(left, right) => {
            is_static_init_constant(left, objects) && is_static_init_constant(right, objects)
        }
        Node::Cond(condition, yes, no) => {
            is_static_init_constant(condition, objects)
                && is_static_init_constant(yes, objects)
                && is_static_init_constant(no, objects)
        }
        Node::InitList(items) => items.iter().all(|item| is_static_init_constant(item, objects)),
        _ => is_pointer_address_constant(node, objects),
    }
}

/// 块作用域（auto）初始化器是否超过数组长度或结构体成员数（C89 §3.5.7）。
/// static/文件作用域路径走 check_static_initializer 的 TooMany。
fn block_initializer_too_many(ty: &Type, node: &Node, types: Option<&TypeTable>) -> bool {
    match ty.unqualified() {
        Type::Array(elem, Some(len)) => match node.inner() {
            Node::InitList(items) => items.len() > *len,
            Node::Str(value) => {
                if matches!(elem.unqualified(), Type::Char | Type::UChar) {
                    // 按转义解码后的实际字节数 + NUL 检查长度
                    let decoded = crate::expr_eval::decode_string_escapes(value)
                        .map(|bytes| bytes.len())
                        .unwrap_or(value.len());
                    decoded + 1 > *len
                } else {
                    false
                }
            }
            _ => false,
        },
        Type::Struct(tag) => match node.inner() {
            Node::InitList(items) => types
                .and_then(|tt| tt.structs.get(tag))
                .is_some_and(|def| items.len() > def.members.len()),
            _ => false,
        },
        _ => false,
    }
}

fn validate_object_type(
    ty: &Type,
    storage: crate::ast::Storage,
    context: &Context<'_>,
    node: &Node,
    file_scope: bool,
) -> CompileResult<()> {
    match ty {
        Type::Qualified { ty, .. } => validate_object_type(ty, storage, context, node, file_scope),
        Type::Void => context.error(node, None, "对象不能声明为 void 类型"),
        Type::Array(element, length) => {
            if matches!(length, Some(0)) {
                return context.error(node, None, "数组长度必须大于零");
            }
            // C89 §3.7.2：内部链接（static）的 tentative 定义不得为不完整类型；
            // 外部链接的 int a[]; 合法（翻译单元末尾按 int a[1] 完成）。
            // 块作用域（非 extern）数组必须完整（C89 §3.1.2.5）。
            if length.is_none()
                && storage != crate::ast::Storage::Extern
                && (storage == crate::ast::Storage::Static || !file_scope)
            {
                return context.error(node, None, "对象使用了不完整数组类型");
            }
            if matches!(element.as_ref(), Type::Void | Type::Function { .. }) {
                return context.error(node, None, "数组元素必须是完整对象类型");
            }
            validate_nested_object_type(element, context, node)
        }
        Type::Function { .. } => context.error(node, None, "对象不能具有函数类型"),
        _ => Ok(()),
    }
}

fn validate_nested_object_type(ty: &Type, context: &Context<'_>, node: &Node) -> CompileResult<()> {
    match ty {
        Type::Qualified { ty, .. } => validate_nested_object_type(ty, context, node),
        Type::Array(element, Some(length)) => {
            if *length == 0 {
                return context.error(node, None, "数组长度必须大于零");
            }
            validate_nested_object_type(element, context, node)
        }
        Type::Array(_, None) | Type::Void | Type::Function { .. } => {
            context.error(node, None, "对象包含不完整或非法的子对象类型")
        }
        _ => Ok(()),
    }
}

fn qualifiers(ty: &Type) -> (bool, bool, &Type) {
    match ty {
        Type::Qualified {
            ty,
            is_const,
            is_volatile,
        } => (*is_const, *is_volatile, ty),
        other => (false, false, other),
    }
}

fn exactly_qualified_compatible(left: &Type, right: &Type) -> bool {
    let (left_const, left_volatile, left) = qualifiers(left);
    let (right_const, right_volatile, right) = qualifiers(right);
    left_const == right_const
        && left_volatile == right_volatile
        && match (left.unqualified(), right.unqualified()) {
            (Type::Ptr(left), Type::Ptr(right)) => exactly_qualified_compatible(left, right),
            _ => compatible_types(left, right),
        }
}

fn pointer_assignment_compatible(destination: &Type, source: &Type) -> bool {
    let destination = destination.unqualified();
    let source = source.unqualified();
    let Type::Ptr(destination) = destination else {
        return false;
    };
    let source = match source {
        Type::Ptr(source) | Type::Array(source, _) => source,
        _ => return false,
    };
    let (dest_const, dest_volatile, dest_base) = qualifiers(destination);
    let (src_const, src_volatile, src_base) = qualifiers(source);
    if (src_const && !dest_const) || (src_volatile && !dest_volatile) {
        return false;
    }
    if matches!(dest_base.unqualified(), Type::Void) || matches!(src_base.unqualified(), Type::Void)
    {
        return true;
    }
    if dest_base.is_pointer() || src_base.is_pointer() {
        exactly_qualified_compatible(destination, source)
    } else {
        compatible_types(dest_base, src_base)
    }
}

fn assignment_compatible(destination: &Type, source: &Type, source_node: &Node) -> bool {
    // 字符串字面量可以初始化字符数组（C89 §3.5.7）
    if matches!(source_node.inner(), Node::Str(_))
        && matches!(
            destination.unqualified(),
            Type::Array(elem, _) if matches!(elem.unqualified(), Type::Char | Type::UChar)
        )
    {
        return true;
    }
    if destination.is_arithmetic() && source.is_arithmetic() {
        return true;
    }
    if destination.is_struct() || source.is_struct() {
        return destination.is_struct()
            && source.is_struct()
            && compatible_types(destination, source);
    }
    if destination.is_pointer() {
        return pointer_assignment_compatible(destination, source)
            || (source.is_integer() && eval_integer_constant(source_node) == Some(0));
    }
    compatible_types(destination, source)
}

fn validate_function_signature(
    ret: &Type,
    params: &[Param],
    context: &Context<'_>,
    node: &Node,
) -> CompileResult<()> {
    if matches!(ret.unqualified(), Type::Array(_, _) | Type::Function { .. }) {
        return context.error(node, None, "函数不能返回数组或函数类型");
    }
    for parameter in params {
        if matches!(
            parameter.ty.unqualified(),
            Type::Void | Type::Array(_, _) | Type::Function { .. }
        ) {
            return context.error(node, None, "函数参数必须使用完整对象类型或调整后的指针类型");
        }
    }
    Ok(())
}

fn validate_file_scope_objects(items: &[Node], context: &Context<'_>) -> CompileResult<()> {
    // 文件作用域对象表：静态初始化器中的地址常量只能引用这些对象
    let objects: ObjectMap = items
        .iter()
        .filter_map(|item| match item.inner() {
            Node::Decl { specs, name, .. } => Some((name.clone(), specs.ty.clone())),
            _ => None,
        })
        .collect();
    for item in items {
        if let Node::Decl { specs, init, .. } = item.inner() {
            if matches!(
                specs.storage,
                crate::ast::Storage::Auto | crate::ast::Storage::Register
            ) {
                return context.error(item, None, "文件作用域对象不能使用 auto 或 register");
            }
            if specs.storage != crate::ast::Storage::Typedef {
                validate_object_type(&specs.ty, specs.storage, context, item, true)?;
            }
            if let Some(init) = init {
                match check_static_initializer(&specs.ty, init, &objects) {
                    StaticInit::Ok => {}
                    StaticInit::StringArray => {
                        return context.error(init, None, "字符串字面量只能初始化字符数组")
                    }
                    StaticInit::StringPtr => {
                        return context.error(init, None, "字符串字面量只能初始化字符指针或字符数组")
                    }
                    StaticInit::TooMany => {
                        return context.error(init, None, "初始化器数量超过数组长度")
                    }
                    StaticInit::ElemType => {
                        return context.error(init, None, "初始化器元素类型与数组元素类型不兼容")
                    }
                    StaticInit::Unevaluable => {
                        return context.error(init, None, "静态初始化器常量表达式无法求值（如除零）")
                    }
                    StaticInit::NotConstant => {
                        return context.error(item, None, "静态存储期对象需要常量初始化器")
                    }
                }
            }
        }
    }
    Ok(())
}

fn modified_root_name(node: &Node) -> Option<&str> {
    match node.inner() {
        Node::Ident(name) => Some(name),
        Node::Member(object, _, false) | Node::Subscript(object, _) => modified_root_name(object),
        _ => None,
    }
}

fn lvalue_is_const(node: &Node, types: &HashMap<String, Type>) -> bool {
    match node.inner() {
        Node::Ident(name) => types
            .get(name)
            .map(Type::is_const_qualified)
            .unwrap_or(false),
        Node::Unary(UnaryOp::Deref, pointer) => expression_type(pointer, types)
            .and_then(|ty| ty.pointed_type().cloned())
            .map(|ty| ty.is_const_qualified())
            .unwrap_or(false),
        Node::Subscript(array, _) => expression_type(array, types)
            .and_then(|ty| ty.pointed_type().cloned())
            .map(|ty| ty.is_const_qualified())
            .unwrap_or(false),
        Node::Member(object, _, false) => lvalue_is_const(object, types),
        Node::Member(object, _, true) => expression_type(object, types)
            .and_then(|ty| ty.pointed_type().cloned())
            .map(|ty| ty.is_const_qualified())
            .unwrap_or(false),
        _ => false,
    }
}

fn validate_block_objects(
    body: &Node,
    context: &Context<'_>,
    types: &HashMap<String, Type>,
    return_type: &Type,
    globals: &ObjectMap,
) -> CompileResult<()> {
    let mut const_objects = HashSet::new();
    let mut register_objects = HashSet::new();
    fn collect(
        node: &Node,
        context: &Context<'_>,
        const_objects: &mut HashSet<String>,
        register_objects: &mut HashSet<String>,
        globals: &ObjectMap,
    ) -> CompileResult<()> {
        match node.inner() {
            Node::Decl { specs, name, init } => {
                if specs.storage != crate::ast::Storage::Typedef {
                    validate_object_type(&specs.ty, specs.storage, context, node, false)?;
                }
                if specs.ty.is_const_qualified() {
                    const_objects.insert(name.clone());
                }
                if specs.storage == crate::ast::Storage::Register {
                    register_objects.insert(name.clone());
                }
                if specs.storage == crate::ast::Storage::Static {
                    if let Some(init) = init {
                        match check_static_initializer(&specs.ty, init, globals) {
                            StaticInit::Ok => {}
                            StaticInit::StringArray => {
                                return context
                                    .error(init, None, "字符串字面量只能初始化字符数组")
                            }
                            StaticInit::StringPtr => {
                                return context.error(
                                    init,
                                    None,
                                    "字符串字面量只能初始化字符指针或字符数组",
                                )
                            }
                            StaticInit::TooMany => {
                                return context.error(init, None, "初始化器数量超过数组长度")
                            }
                            StaticInit::ElemType => {
                                return context.error(
                                    init,
                                    None,
                                    "初始化器元素类型与数组元素类型不兼容",
                                )
                            }
                            StaticInit::Unevaluable => {
                                return context.error(
                                    init,
                                    None,
                                    "静态初始化器常量表达式无法求值（如除零）",
                                )
                            }
                            StaticInit::NotConstant => {
                                return context
                                    .error(init, None, "局部 static 对象需要常量初始化器")
                            }
                        }
                    }
                }
            }
            Node::Block(nodes) => {
                for child in nodes {
                    collect(child, context, const_objects, register_objects, globals)?;
                }
            }
            Node::If(_, yes, no) => {
                collect(yes, context, const_objects, register_objects, globals)?;
                if let Some(no) = no {
                    collect(no, context, const_objects, register_objects, globals)?;
                }
            }
            Node::While(_, b)
            | Node::DoWhile(b, _)
            | Node::For(_, _, _, b)
            | Node::Switch(_, b)
            | Node::Case(_, b)
            | Node::Default(b)
            | Node::Label(_, b) => collect(b, context, const_objects, register_objects, globals)?,
            _ => {}
        }
        Ok(())
    }
    collect(body, context, &mut const_objects, &mut register_objects, globals)?;

    fn check(
        node: &Node,
        context: &Context<'_>,
        types: &HashMap<String, Type>,
        return_type: &Type,
        const_objects: &HashSet<String>,
        register_objects: &HashSet<String>,
    ) -> CompileResult<()> {
        let recurse = |child: &Node| {
            check(
                child,
                context,
                types,
                return_type,
                const_objects,
                register_objects,
            )
        };
        match node.inner() {
            Node::Assign(_, lhs, rhs) => {
                let const_lvalue = lvalue_is_const(lhs, types);
                if const_lvalue
                    || modified_root_name(lhs).is_some_and(|name| const_objects.contains(name))
                {
                    return context.error(lhs, None, "不能修改 const 对象");
                }
                recurse(lhs)?;
                recurse(rhs)?;
            }
            Node::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                value,
            ) => {
                let const_lvalue = lvalue_is_const(value, types);
                if const_lvalue
                    || modified_root_name(value).is_some_and(|name| const_objects.contains(name))
                {
                    return context.error(value, None, "不能修改 const 对象");
                }
                recurse(value)?;
            }
            Node::Unary(UnaryOp::Addr, value) => {
                if modified_root_name(value).is_some_and(|name| register_objects.contains(name)) {
                    return context.error(value, None, "不能取得 register 对象的地址");
                }
                recurse(value)?;
            }
            Node::Return(value) => match (return_type, value) {
                (ty, Some(value)) if matches!(ty.unqualified(), Type::Void) => {
                    return context.error(value, None, "void 函数不能返回值")
                }
                (ty, None) if !matches!(ty.unqualified(), Type::Void) => {
                    return context.error(node, None, "非 void 函数必须返回值")
                }
                (expected, Some(value)) => {
                    if let Some(actual) = expression_type(value, types) {
                        if !assignment_compatible(expected, &actual, value) {
                            return context.error(
                                value,
                                None,
                                "返回表达式类型与函数返回类型不兼容",
                            );
                        }
                    }
                    recurse(value)?;
                }
                _ => {}
            },
            Node::Block(nodes) | Node::InitList(nodes) => {
                for child in nodes {
                    recurse(child)?;
                }
            }
            Node::If(c, a, b) => {
                recurse(c)?;
                recurse(a)?;
                if let Some(b) = b {
                    recurse(b)?;
                }
            }
            Node::While(c, b) | Node::DoWhile(b, c) | Node::Switch(c, b) | Node::Case(c, b) => {
                recurse(c)?;
                recurse(b)?;
            }
            Node::For(i, c, p, b) => {
                for child in [i.as_deref(), c.as_deref(), p.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    recurse(child)?;
                }
                recurse(b)?;
            }
            Node::Default(b)
            | Node::Label(_, b)
            | Node::ExprStmt(b)
            | Node::SizeofExpr(b)
            | Node::Cast(_, b)
            | Node::Member(b, _, _) => recurse(b)?,
            Node::Unary(_, b) => recurse(b)?,
            Node::Binary(_, a, b) | Node::Comma(a, b) | Node::Subscript(a, b) => {
                recurse(a)?;
                recurse(b)?;
            }
            Node::Cond(c, a, b) => {
                recurse(c)?;
                recurse(a)?;
                recurse(b)?;
            }
            Node::Call(f, args) => {
                recurse(f)?;
                for arg in args {
                    recurse(arg)?;
                }
            }
            Node::Decl {
                specs,
                init: Some(init),
                ..
            } => {
                // 块作用域（auto）初始化器元素数不能超过数组长度/结构体成员数
                //（块 static 已在 collect 里做 TooMany 检查）
                if specs.storage != crate::ast::Storage::Static
                    && block_initializer_too_many(&specs.ty, init, context.types)
                {
                    return context.error(init, None, "初始化器数量超过数组长度或结构体成员数");
                }
                if !matches!(init.inner(), Node::InitList(_)) {
                    if let Some(actual) = expression_type(init, types) {
                        if !assignment_compatible(&specs.ty, &actual, init) {
                            return context.error(init, None, "初始化器类型与对象类型不兼容");
                        }
                    }
                }
                recurse(init)?;
            }
            _ => {}
        }
        Ok(())
    }
    check(
        body,
        context,
        types,
        return_type,
        &const_objects,
        &register_objects,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_function(
    body: &Node,
    params: &[Param],
    context: &Context<'_>,
    globals: &HashSet<String>,
    global_types: &HashMap<String, Type>,
    functions: &HashSet<String>,
    signatures: &HashMap<String, FunctionSignature>,
    return_type: &Type,
) -> CompileResult<()> {
    let mut locals: HashSet<String> = params.iter().map(|param| param.name.clone()).collect();
    collect_locals(body, &mut locals);
    let mut types = global_types.clone();
    types.extend(
        params
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.ty.clone())),
    );
    collect_local_types(body, &mut types);
    validate_expression_types(body, context, &types)?;
    validate_block_objects(body, context, &types, return_type, global_types)?;
    validate_calls(body, context, &types, signatures)?;
    let mut labels = HashSet::new();
    let mut gotos = Vec::new();
    collect_labels(body, context, &mut labels, &mut gotos)?;
    for label in gotos {
        if !labels.contains(&label) {
            return context.token_error(Some(&label), format!("goto 引用了未定义标签 '{label}'"));
        }
    }
    walk(body, context, globals, functions, &locals, &types, 0, 0)
}

fn collect_locals(node: &Node, locals: &mut HashSet<String>) {
    match node.inner() {
        Node::Decl { name, .. } => {
            locals.insert(name.clone());
        }
        Node::Block(nodes) => {
            for node in nodes {
                collect_locals(node, locals);
            }
        }
        Node::If(_, yes, no) => {
            collect_locals(yes, locals);
            if let Some(no) = no {
                collect_locals(no, locals);
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body)
        | Node::Label(_, body) => collect_locals(body, locals),
        _ => {}
    }
}

fn collect_local_types(node: &Node, types: &mut HashMap<String, Type>) {
    match node.inner() {
        Node::Decl { specs, name, .. } => {
            types.insert(name.clone(), specs.ty.clone());
        }
        Node::Block(nodes) => {
            for child in nodes {
                collect_local_types(child, types);
            }
        }
        Node::If(_, yes, no) => {
            collect_local_types(yes, types);
            if let Some(no) = no {
                collect_local_types(no, types);
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body)
        | Node::Label(_, body) => collect_local_types(body, types),
        _ => {}
    }
}

fn expression_type(node: &Node, types: &HashMap<String, Type>) -> Option<Type> {
    match node.inner() {
        Node::Num(_) => Some(Type::Int),
        Node::FNum(_) => Some(Type::Double),
        Node::Str(_) => Some(Type::Ptr(Box::new(Type::Char))),
        Node::Ident(name) => types.get(name).cloned(),
        Node::Cast(ty, _) => Some(ty.clone()),
        Node::Unary(UnaryOp::Addr, value) => {
            expression_type(value, types).map(|ty| Type::Ptr(Box::new(ty)))
        }
        Node::Unary(UnaryOp::Deref, value) => match expression_type(value, types) {
            Some(ty) => match ty.unqualified() {
                Type::Ptr(inner) | Type::Array(inner, _) => Some((**inner).clone()),
                _ => None,
            },
            _ => None,
        },
        Node::Unary(UnaryOp::Not, _) => Some(Type::Int),
        Node::Unary(UnaryOp::BitNot, value) => {
            expression_type(value, types).map(|ty| ty.integer_promotion())
        }
        Node::Unary(_, value) => expression_type(value, types),
        Node::Subscript(array, _) => match expression_type(array, types) {
            Some(ty) => match ty.unqualified() {
                Type::Ptr(inner) | Type::Array(inner, _) => Some((**inner).clone()),
                _ => None,
            },
            _ => None,
        },
        Node::Assign(_, lhs, _) => expression_type(lhs, types),
        Node::Binary(op, left, right) => {
            let left = expression_type(left, types)?;
            let right = expression_type(right, types)?;
            if matches!(
                op,
                BinaryOp::Lt
                    | BinaryOp::Gt
                    | BinaryOp::Le
                    | BinaryOp::Ge
                    | BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::LogAnd
                    | BinaryOp::LogOr
            ) {
                Some(Type::Int)
            } else if matches!(op, BinaryOp::Sub)
                && left.is_pointer_like()
                && right.is_pointer_like()
            {
                // 指针相减得 ptrdiff_t（本实现为 int，C89 §3.3.6）
                Some(Type::Int)
            } else if left.is_pointer_like() {
                Some(left.without_top_qualifiers())
            } else if right.is_pointer_like() {
                Some(right.without_top_qualifiers())
            } else {
                Type::usual_arithmetic(&left, &right)
            }
        }
        Node::Cond(_, yes, no) => {
            let yes = expression_type(yes, types);
            let no = expression_type(no, types);
            match (yes, no) {
                (Some(yes), Some(no)) if yes.is_arithmetic() && no.is_arithmetic() => {
                    Type::usual_arithmetic(&yes, &no)
                }
                (Some(yes), Some(no)) if compatible_types(&yes, &no) => Some(yes),
                (Some(yes), Some(no)) if yes.is_pointer() && no.is_integer() => {
                    Some(yes.without_top_qualifiers())
                }
                (Some(yes), Some(no)) if no.is_pointer() && yes.is_integer() => {
                    Some(no.without_top_qualifiers())
                }
                (Some(yes), _) => Some(yes),
                (_, no) => no,
            }
        }
        Node::Comma(_, right) => expression_type(right, types),
        _ => None,
    }
}

fn validate_expression_types(
    node: &Node,
    context: &Context<'_>,
    types: &HashMap<String, Type>,
) -> CompileResult<()> {
    let check = |child: &Node| validate_expression_types(child, context, types);
    match node.inner() {
        Node::Binary(op, left, right) => {
            check(left)?;
            check(right)?;
            if let (Some(left), Some(right)) =
                (expression_type(left, types), expression_type(right, types))
            {
                let pointer = Type::is_pointer_like;
                let integer = Type::is_integer;
                let arithmetic = Type::is_arithmetic;
                let valid = match op {
                    BinaryOp::Add | BinaryOp::Sub => {
                        (arithmetic(&left) && arithmetic(&right))
                            || (pointer(&left) && integer(&right))
                            || (*op == BinaryOp::Add && integer(&left) && pointer(&right))
                            || (*op == BinaryOp::Sub && pointer(&left) && pointer(&right))
                    }
                    BinaryOp::Mul | BinaryOp::Div => arithmetic(&left) && arithmetic(&right),
                    BinaryOp::Mod
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::BitAnd
                    | BinaryOp::BitXor
                    | BinaryOp::BitOr => integer(&left) && integer(&right),
                    BinaryOp::LogAnd | BinaryOp::LogOr => {
                        (arithmetic(&left) || pointer(&left))
                            && (arithmetic(&right) || pointer(&right))
                    }
                    _ => true,
                };
                if !valid {
                    return context.error(
                        node,
                        None,
                        format!("运算符 {:?} 的操作数类型不兼容", op),
                    );
                }
            }
        }
        Node::Unary(UnaryOp::Deref, value) => {
            check(value)?;
            if let Some(ty) = expression_type(value, types) {
                if !ty.is_pointer_like() {
                    return context.error(node, Some("*"), "解引用操作数必须是指针");
                }
            }
        }
        Node::Unary(op @ (UnaryOp::Neg | UnaryOp::BitNot), value) => {
            check(value)?;
            if let Some(ty) = expression_type(value, types) {
                // C89 §3.3.3：一元 - 要求算术类型；~ 要求整型
                if *op == UnaryOp::BitNot {
                    if !ty.is_integer() {
                        return context.error(node, Some("~"), "按位取反操作数必须是整型");
                    }
                } else if !ty.is_arithmetic() {
                    return context.error(node, Some("-"), "一元负号操作数必须是算术类型");
                }
            }
        }
        Node::Subscript(array, index) => {
            check(array)?;
            check(index)?;
            if let Some(ty) = expression_type(array, types) {
                if !ty.is_pointer_like() {
                    return context.error(node, Some("["), "下标运算左侧必须是数组或指针");
                }
            }
            if let Some(ty) = expression_type(index, types) {
                if !ty.is_integer() {
                    return context.error(node, Some("["), "下标必须是整型表达式");
                }
            }
        }
        Node::Block(nodes) | Node::InitList(nodes) => {
            for child in nodes {
                check(child)?;
            }
        }
        Node::If(c, yes, no) => {
            check(c)?;
            check(yes)?;
            if let Some(no) = no {
                check(no)?;
            }
        }
        Node::Switch(c, b) => {
            check(c)?;
            check(b)?;
            if let Some(ty) = expression_type(c, types) {
                if !ty.is_integer() {
                    return context
                        .error(c, Some("switch"), "switch 控制表达式必须是整型");
                }
            }
        }
        Node::While(c, b) | Node::DoWhile(b, c) | Node::Case(c, b) => {
            check(c)?;
            check(b)?;
        }
        Node::For(i, c, p, b) => {
            if let Some(i) = i {
                check(i)?;
            }
            if let Some(c) = c {
                check(c)?;
            }
            if let Some(p) = p {
                check(p)?;
            }
            check(b)?;
        }
        Node::Default(b) | Node::Label(_, b) => check(b)?,
        Node::Assign(op, l, r) => {
            check(l)?;
            check(r)?;
            if let Some(left) = expression_type(l, types) {
                if matches!(
                    left.unqualified(),
                    Type::Array(_, _) | Type::Function { .. }
                ) {
                    return context.error(l, None, "赋值运算左侧不是可修改对象类型");
                }
                if let Some(right) = expression_type(r, types) {
                    if *op == crate::ast::AssignOp::Plain {
                        if !assignment_compatible(&left, &right, r) {
                            return context.error(node, None, "赋值两侧类型不兼容");
                        }
                    } else if left.is_struct() || right.is_struct() {
                        return context.error(node, None, "结构体或联合体不支持复合赋值");
                    }
                }
            }
        }
        Node::Comma(l, r) => {
            check(l)?;
            check(r)?;
        }
        Node::Cond(c, y, n) => {
            check(c)?;
            check(y)?;
            check(n)?;
            if let (Some(yes), Some(no)) = (expression_type(y, types), expression_type(n, types)) {
                // 两分支须互相兼容：算术类型做 usual arithmetic conversions；
                // 指针与指针须指向兼容类型；指针可与空指针常量（整型常量 0）配对。
                let yes_ptr = yes.is_pointer_like();
                let no_ptr = no.is_pointer_like();
                let compatible = if yes.is_arithmetic() && no.is_arithmetic() {
                    true
                } else if yes_ptr && no_ptr {
                    compatible_types(&yes, &no)
                } else if yes_ptr && no.is_integer() {
                    eval_integer_constant(n) == Some(0)
                } else if no_ptr && yes.is_integer() {
                    eval_integer_constant(y) == Some(0)
                } else {
                    compatible_types(&yes, &no)
                };
                if !compatible {
                    return context.error(node, None, "条件表达式两分支类型不兼容");
                }
            }
        }
        Node::Call(f, args) => {
            check(f)?;
            for arg in args {
                check(arg)?;
            }
        }
        Node::Member(o, _, _)
        | Node::Unary(_, o)
        | Node::SizeofExpr(o)
        | Node::Cast(_, o)
        | Node::ExprStmt(o) => check(o)?,
        Node::Return(Some(value))
        | Node::Decl {
            init: Some(value), ..
        } => check(value)?,
        _ => {}
    }
    Ok(())
}

fn function_type_of(
    node: &Node,
    types: &HashMap<String, Type>,
    signatures: &HashMap<String, FunctionSignature>,
) -> Option<FunctionSignature> {
    match node.inner() {
        Node::Ident(name) => signatures.get(name).cloned().or_else(|| {
            types.get(name).and_then(|ty| match ty.unqualified() {
                Type::Ptr(inner) => match inner.unqualified() {
                    Type::Function {
                        ret,
                        params,
                        is_variadic,
                        has_prototype,
                    } => Some(FunctionSignature {
                        ret: (**ret).clone(),
                        params: params.clone(),
                        is_variadic: *is_variadic,
                        has_prototype: *has_prototype,
                    }),
                    _ => None,
                },
                _ => None,
            })
        }),
        Node::Unary(UnaryOp::Deref, inner) => function_type_of(inner, types, signatures),
        _ => None,
    }
}

fn argument_type(
    node: &Node,
    types: &HashMap<String, Type>,
    signatures: &HashMap<String, FunctionSignature>,
) -> Option<Type> {
    match node.inner() {
        Node::Call(callee, _) => function_type_of(callee, types, signatures).map(|sig| sig.ret),
        _ => expression_type(node, types),
    }
}

/// 空指针常量：值为 0 的整型常量表达式（C89 §6.2.2.3）。
/// 按常量表达式求值判断，使 `1-1`、`(char*)0` 等也算空指针常量。
fn is_null_pointer_constant(node: &Node) -> bool {
    eval_integer_constant(node) == Some(0)
}

fn parameter_accepts(parameter: &Type, argument: &Node, argument_type: &Type) -> bool {
    if parameter.is_pointer() {
        if argument_type.is_pointer_like() {
            return pointer_assignment_compatible(parameter, argument_type);
        }
        // 整型字面量 0 作为空指针常量传给指针参数（strtok(0, "...")）
        return is_null_pointer_constant(argument);
    }
    compatible_types(parameter, argument_type)
        || (parameter.is_arithmetic() && argument_type.is_arithmetic())
}

fn validate_calls(
    node: &Node,
    context: &Context<'_>,
    types: &HashMap<String, Type>,
    signatures: &HashMap<String, FunctionSignature>,
) -> CompileResult<()> {
    if let Node::Call(callee, args) = node.inner() {
        if let Some(signature) = function_type_of(callee, types, signatures) {
            if signature.has_prototype {
                let required = signature.params.len();
                let count_ok = if signature.is_variadic {
                    args.len() >= required
                } else {
                    args.len() == required
                };
                if !count_ok {
                    return context.error(
                        node,
                        None,
                        format!(
                            "函数调用参数数量不匹配：需要 {}{}，实际 {}",
                            required,
                            if signature.is_variadic {
                                " 个以上"
                            } else {
                                " 个"
                            },
                            args.len()
                        ),
                    );
                }
                for (index, (parameter, argument)) in signature.params.iter().zip(args).enumerate()
                {
                    if let Some(argument_type) = argument_type(argument, types, signatures) {
                        if !parameter_accepts(&parameter.ty, argument, &argument_type) {
                            return context.error(
                                argument,
                                None,
                                format!("第 {} 个函数参数类型与原型不兼容", index + 1),
                            );
                        }
                    }
                }
            }
        }
    }
    match node.inner() {
        Node::Block(nodes) | Node::InitList(nodes) => {
            for child in nodes {
                validate_calls(child, context, types, signatures)?;
            }
        }
        Node::If(c, yes, no) => {
            validate_calls(c, context, types, signatures)?;
            validate_calls(yes, context, types, signatures)?;
            if let Some(no) = no {
                validate_calls(no, context, types, signatures)?;
            }
        }
        Node::While(c, body) | Node::DoWhile(body, c) => {
            validate_calls(c, context, types, signatures)?;
            validate_calls(body, context, types, signatures)?;
        }
        Node::For(init, cond, post, body) => {
            for child in [init.as_deref(), cond.as_deref(), post.as_deref()]
                .into_iter()
                .flatten()
            {
                validate_calls(child, context, types, signatures)?;
            }
            validate_calls(body, context, types, signatures)?;
        }
        Node::Switch(expr, body) | Node::Case(expr, body) => {
            validate_calls(expr, context, types, signatures)?;
            validate_calls(body, context, types, signatures)?;
        }
        Node::Default(body)
        | Node::Label(_, body)
        | Node::ExprStmt(body)
        | Node::Return(Some(body))
        | Node::Unary(_, body)
        | Node::SizeofExpr(body)
        | Node::Cast(_, body)
        | Node::Member(body, _, _) => {
            validate_calls(body, context, types, signatures)?;
        }
        Node::Decl {
            init: Some(init), ..
        } => validate_calls(init, context, types, signatures)?,
        Node::Assign(_, left, right)
        | Node::Binary(_, left, right)
        | Node::Comma(left, right)
        | Node::Subscript(left, right) => {
            validate_calls(left, context, types, signatures)?;
            validate_calls(right, context, types, signatures)?;
        }
        Node::Cond(c, yes, no) => {
            validate_calls(c, context, types, signatures)?;
            validate_calls(yes, context, types, signatures)?;
            validate_calls(no, context, types, signatures)?;
        }
        Node::Call(callee, args) => {
            validate_calls(callee, context, types, signatures)?;
            for arg in args {
                validate_calls(arg, context, types, signatures)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_labels(
    node: &Node,
    context: &Context<'_>,
    labels: &mut HashSet<String>,
    gotos: &mut Vec<String>,
) -> CompileResult<()> {
    match node.inner() {
        Node::Goto(name) => gotos.push(name.clone()),
        Node::Label(name, body) => {
            if !labels.insert(name.clone()) {
                return context.token_error(Some(name), format!("重复标签 '{name}'"));
            }
            collect_labels(body, context, labels, gotos)?;
        }
        Node::Block(nodes) => {
            for node in nodes {
                collect_labels(node, context, labels, gotos)?;
            }
        }
        Node::If(_, yes, no) => {
            collect_labels(yes, context, labels, gotos)?;
            if let Some(no) = no {
                collect_labels(no, context, labels, gotos)?;
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body) => collect_labels(body, context, labels, gotos)?,
        _ => {}
    }
    Ok(())
}

fn eval_integer_constant(node: &Node) -> Option<i64> {
    match node.inner() {
        Node::Num(value) => Some(*value),
        Node::Unary(op, value) => {
            let value = eval_integer_constant(value)?;
            match op {
                UnaryOp::Neg => Some(value.wrapping_neg()),
                UnaryOp::Not => Some((value == 0) as i64),
                UnaryOp::BitNot => Some(!value),
                _ => None,
            }
        }
        Node::Binary(op, left, right) => {
            let left = eval_integer_constant(left)?;
            let right = eval_integer_constant(right)?;
            match op {
                BinaryOp::Add => Some(left.wrapping_add(right)),
                BinaryOp::Sub => Some(left.wrapping_sub(right)),
                BinaryOp::Mul => Some(left.wrapping_mul(right)),
                BinaryOp::Div if right != 0 => left.checked_div(right),
                BinaryOp::Mod if right != 0 => left.checked_rem(right),
                BinaryOp::Shl if (0..64).contains(&right) => Some(left.wrapping_shl(right as u32)),
                BinaryOp::Shr if (0..64).contains(&right) => Some(left.wrapping_shr(right as u32)),
                BinaryOp::Lt => Some((left < right) as i64),
                BinaryOp::Gt => Some((left > right) as i64),
                BinaryOp::Le => Some((left <= right) as i64),
                BinaryOp::Ge => Some((left >= right) as i64),
                BinaryOp::Eq => Some((left == right) as i64),
                BinaryOp::Ne => Some((left != right) as i64),
                BinaryOp::BitAnd => Some(left & right),
                BinaryOp::BitXor => Some(left ^ right),
                BinaryOp::BitOr => Some(left | right),
                BinaryOp::LogAnd => Some((left != 0 && right != 0) as i64),
                BinaryOp::LogOr => Some((left != 0 || right != 0) as i64),
                _ => None,
            }
        }
        Node::Cond(condition, yes, no) => {
            if eval_integer_constant(condition)? != 0 {
                eval_integer_constant(yes)
            } else {
                eval_integer_constant(no)
            }
        }
        Node::Cast(_, value) => eval_integer_constant(value),
        _ => None,
    }
}

/// 求值浮点常量表达式（静态初始化器/数据段用）。
/// C89 语义：两侧均为整数常量时按整型语义求值后转换（1/2 == 0），
/// 否则按 double 算术。
pub fn eval_float_constant(node: &Node) -> Option<f64> {
    match node.inner() {
        Node::FNum(value) => Some(*value),
        Node::Unary(UnaryOp::Neg, value) => eval_float_constant(value).map(|v| -v),
        Node::Cast(_, value) => eval_float_constant(value),
        Node::Binary(op, left, right) => {
            let left_int = eval_integer_constant(left);
            let right_int = eval_integer_constant(right);
            if let (Some(left), Some(right)) = (left_int, right_int) {
                // 两侧都是整数常量：按整型语义求值，再转浮点
                let value = match op {
                    BinaryOp::Add => left.wrapping_add(right),
                    BinaryOp::Sub => left.wrapping_sub(right),
                    BinaryOp::Mul => left.wrapping_mul(right),
                    BinaryOp::Div if right != 0 => left.checked_div(right)?,
                    BinaryOp::Mod if right != 0 => left.checked_rem(right)?,
                    BinaryOp::Lt => (left < right) as i64,
                    BinaryOp::Gt => (left > right) as i64,
                    BinaryOp::Le => (left <= right) as i64,
                    BinaryOp::Ge => (left >= right) as i64,
                    BinaryOp::Eq => (left == right) as i64,
                    BinaryOp::Ne => (left != right) as i64,
                    BinaryOp::LogAnd => (left != 0 && right != 0) as i64,
                    BinaryOp::LogOr => (left != 0 || right != 0) as i64,
                    _ => return None,
                };
                Some(value as f64)
            } else {
                let left = eval_float_constant(left)?;
                let right = eval_float_constant(right)?;
                match op {
                    BinaryOp::Add => Some(left + right),
                    BinaryOp::Sub => Some(left - right),
                    BinaryOp::Mul => Some(left * right),
                    BinaryOp::Div => Some(left / right),
                    BinaryOp::Lt => Some((left < right) as i64 as f64),
                    BinaryOp::Gt => Some((left > right) as i64 as f64),
                    BinaryOp::Le => Some((left <= right) as i64 as f64),
                    BinaryOp::Ge => Some((left >= right) as i64 as f64),
                    BinaryOp::Eq => Some((left == right) as i64 as f64),
                    BinaryOp::Ne => Some((left != right) as i64 as f64),
                    _ => None,
                }
            }
        }
        Node::Cond(condition, yes, no) => {
            if eval_integer_constant(condition)? != 0 {
                eval_float_constant(yes)
            } else {
                eval_float_constant(no)
            }
        }
        Node::Comma(_, right) => eval_float_constant(right),
        // 整数常量在浮点上下文（double d = 1;）
        _ if eval_integer_constant(node).is_some() => {
            eval_integer_constant(node).map(|v| v as f64)
        }
        _ => None,
    }
}

fn validate_switch_cases(
    node: &Node,
    context: &Context<'_>,
    values: &mut HashSet<i64>,
    has_default: &mut bool,
) -> CompileResult<()> {
    match node.inner() {
        // A nested switch owns its case/default labels.
        Node::Switch(_, _) => {}
        Node::Case(value, body) => {
            let Some(value) = eval_integer_constant(value) else {
                return context.error(node, Some("case"), "case 值必须是整数常量表达式");
            };
            if !values.insert(value) {
                return context.error(node, Some("case"), format!("重复的 case 值 {value}"));
            }
            validate_switch_cases(body, context, values, has_default)?;
        }
        Node::Default(body) => {
            if *has_default {
                return context.error(node, Some("default"), "一个 switch 中只能有一个 default");
            }
            *has_default = true;
            validate_switch_cases(body, context, values, has_default)?;
        }
        Node::Block(nodes) => {
            for node in nodes {
                validate_switch_cases(node, context, values, has_default)?;
            }
        }
        Node::If(_, yes, no) => {
            validate_switch_cases(yes, context, values, has_default)?;
            if let Some(no) = no {
                validate_switch_cases(no, context, values, has_default)?;
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Label(_, body) => validate_switch_cases(body, context, values, has_default)?,
        _ => {}
    }
    Ok(())
}

fn walk(
    node: &Node,
    context: &Context<'_>,
    globals: &HashSet<String>,
    functions: &HashSet<String>,
    locals: &HashSet<String>,
    types: &HashMap<String, Type>,
    loop_depth: usize,
    switch_depth: usize,
) -> CompileResult<()> {
    let recurse = |node: &Node, loop_depth, switch_depth| {
        walk(
            node,
            context,
            globals,
            functions,
            locals,
            types,
            loop_depth,
            switch_depth,
        )
    };
    match node.inner() {
        Node::Ident(name)
            if !locals.contains(name) && !globals.contains(name) && !functions.contains(name) =>
        {
            return context.error(node, Some(name), format!("使用了未声明的标识符 '{name}'"));
        }
        Node::Assign(_, lhs, rhs) => {
            if !matches!(
                lhs.inner(),
                Node::Ident(_)
                    | Node::Unary(UnaryOp::Deref, _)
                    | Node::Subscript(_, _)
                    | Node::Member(_, _, _)
            ) {
                return context.error(node, Some("="), "赋值运算左侧不是可修改左值");
            }
            recurse(lhs, loop_depth, switch_depth)?;
            recurse(rhs, loop_depth, switch_depth)?;
        }
        Node::Call(callee, args) => {
            let mut skip_type_argument = false;
            if let Node::Ident(name) = callee.inner() {
                if !functions.contains(name)
                    && !locals.contains(name)
                    && !BUILTINS.contains(&name.as_str())
                {
                    return context.error(node, Some(name), format!("调用了未知函数 '{name}'"));
                }
                skip_type_argument = name == "va_arg";
            } else {
                recurse(callee, loop_depth, switch_depth)?;
            }
            // 被调表达式必须是函数或函数指针（C89 §3.3.2.2）
            if let Some(ty) = expression_type(callee, types) {
                let callable = matches!(ty.unqualified(), Type::Function { .. })
                    || matches!(
                        ty.unqualified(),
                        Type::Ptr(inner) if matches!(inner.unqualified(), Type::Function { .. })
                    );
                if !callable {
                    return context.error(callee, None, "调用了一个非函数对象");
                }
            }
            for (index, arg) in args.iter().enumerate() {
                if !(skip_type_argument && index == 1) {
                    recurse(arg, loop_depth, switch_depth)?;
                }
            }
        }
        Node::Break if loop_depth == 0 && switch_depth == 0 => {
            return context.error(node, Some("break"), "break 只能出现在循环或 switch 中")
        }
        Node::Continue if loop_depth == 0 => {
            return context.error(node, Some("continue"), "continue 只能出现在循环中")
        }
        Node::Block(nodes) | Node::InitList(nodes) => {
            for node in nodes {
                recurse(node, loop_depth, switch_depth)?;
            }
        }
        Node::If(cond, yes, no) => {
            recurse(cond, loop_depth, switch_depth)?;
            recurse(yes, loop_depth, switch_depth)?;
            if let Some(no) = no {
                recurse(no, loop_depth, switch_depth)?;
            }
        }
        Node::While(cond, body) => {
            recurse(cond, loop_depth, switch_depth)?;
            recurse(body, loop_depth + 1, switch_depth)?;
        }
        Node::DoWhile(body, cond) => {
            recurse(body, loop_depth + 1, switch_depth)?;
            recurse(cond, loop_depth, switch_depth)?;
        }
        Node::For(init, cond, post, body) => {
            if let Some(value) = init {
                recurse(value, loop_depth, switch_depth)?;
            }
            if let Some(value) = cond {
                recurse(value, loop_depth, switch_depth)?;
            }
            if let Some(value) = post {
                recurse(value, loop_depth, switch_depth)?;
            }
            recurse(body, loop_depth + 1, switch_depth)?;
        }
        Node::Switch(expr, body) => {
            recurse(expr, loop_depth, switch_depth)?;
            let mut values = HashSet::new();
            let mut has_default = false;
            validate_switch_cases(body, context, &mut values, &mut has_default)?;
            recurse(body, loop_depth, switch_depth + 1)?;
        }
        Node::Case(expr, body) => {
            if switch_depth == 0 {
                return context.error(node, Some("case"), "case 只能出现在 switch 中");
            }
            recurse(expr, loop_depth, switch_depth)?;
            recurse(body, loop_depth, switch_depth)?;
        }
        Node::Default(body) => {
            if switch_depth == 0 {
                return context.error(node, Some("default"), "default 只能出现在 switch 中");
            }
            recurse(body, loop_depth, switch_depth)?;
        }
        Node::Label(_, body) => recurse(body, loop_depth, switch_depth)?,
        Node::Unary(
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
            expr,
        ) => {
            if !matches!(
                expr.inner(),
                Node::Ident(_)
                    | Node::Unary(UnaryOp::Deref, _)
                    | Node::Subscript(_, _)
                    | Node::Member(_, _, _)
            ) {
                return context.error(node, Some("++"), "自增自减操作数不是可修改左值");
            }
            recurse(expr, loop_depth, switch_depth)?;
        }
        Node::Unary(_, expr)
        | Node::SizeofExpr(expr)
        | Node::Cast(_, expr)
        | Node::ExprStmt(expr) => recurse(expr, loop_depth, switch_depth)?,
        Node::Binary(_, left, right) | Node::Comma(left, right) | Node::Subscript(left, right) => {
            recurse(left, loop_depth, switch_depth)?;
            recurse(right, loop_depth, switch_depth)?;
        }
        Node::Cond(cond, yes, no) => {
            recurse(cond, loop_depth, switch_depth)?;
            recurse(yes, loop_depth, switch_depth)?;
            recurse(no, loop_depth, switch_depth)?;
        }
        Node::Member(object, _, _) => recurse(object, loop_depth, switch_depth)?,
        Node::Return(Some(expr)) => recurse(expr, loop_depth, switch_depth)?,
        Node::Decl {
            init: Some(init), ..
        } => recurse(init, loop_depth, switch_depth)?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::TokKind;
    use crate::parser::Parser;
    use crate::preprocess::Preprocessor;

    fn check(source: &str) -> CompileResult<()> {
        let tokens = Preprocessor::new().run_diagnostic(source)?;
        let parser_tokens = tokens
            .iter()
            .filter(|token| token.kind != TokKind::Eof)
            .cloned()
            .collect();
        let mut parser = Parser::new(parser_tokens);
        let items = parser.parse_program_diagnostic()?;
        validate_with_tokens(&items, "test.c", &tokens)
    }

    #[test]
    fn rejects_unknown_identifier_at_token() {
        let error = check("int main(void) { return missing; }").unwrap_err();
        assert_eq!(error.stage, crate::diagnostics::DiagnosticStage::Semantic);
        assert_eq!(error.span.column, 25);
    }

    #[test]
    fn rejects_break_outside_loop() {
        let error = check("int main(void) { break; }").unwrap_err();
        assert_eq!(error.span.column, 18);
    }

    #[test]
    fn rejects_duplicate_case() {
        let error = check(
            "int main(void) { int x = 1; switch (x) { case 1: return 1; case 1: return 2; } }",
        )
        .unwrap_err();
        assert!(error.message.contains("重复的 case"));
    }

    #[test]
    fn rejects_duplicate_default() {
        let error = check(
            "int main(void) { int x = 1; switch (x) { default: return 1; default: return 2; } }",
        )
        .unwrap_err();
        assert!(error.message.contains("只能有一个 default"));
    }

    #[test]
    fn rejects_nonconstant_case() {
        let error =
            check("int main(void) { int x = 1; switch (x) { case x: return 1; } }").unwrap_err();
        assert!(error.message.contains("整数常量表达式"));
    }

    #[test]
    fn allows_same_case_value_in_nested_switch() {
        assert!(check(
            "int main(void) { switch (1) { case 1: switch (1) { case 1: return 1; } } }"
        )
        .is_ok());
    }

    #[test]
    fn accepts_unsigned_type_specifiers() {
        assert!(check("unsigned long x; unsigned char y; int main(void) { return y; }").is_ok());
    }

    #[test]
    fn accepts_const_without_initializer() {
        assert!(check("const int global; int main(void) { const int local; return 0; }").is_ok());
    }

    #[test]
    fn rejects_nonconstant_static_initializer() {
        let error =
            check("int source; int main(void) { static int value = source; return value; }")
                .unwrap_err();
        assert!(error.message.contains("常量初始化器"));
    }

    #[test]
    fn rejects_const_assignment() {
        let error = check("int main(void) { const int x = 1; x = 2; return x; }").unwrap_err();
        assert!(error.message.contains("const"));
    }

    #[test]
    fn rejects_write_through_pointer_to_const() {
        let error = check("int main(void) { int x = 1; const int *p = &x; *p = 2; return x; }")
            .unwrap_err();
        assert!(error.message.contains("const"));
    }

    #[test]
    fn rejects_write_to_member_through_pointer_to_const_struct() {
        let error = check(
            "struct S { int x; }; int main(void) { struct S s; const struct S *p = &s; p->x = 2; return 0; }",
        )
        .unwrap_err();
        assert!(error.message.contains("const"));
    }

    #[test]
    fn allows_write_through_const_pointer_to_mutable_object() {
        assert!(
            check("int main(void) { int x = 1; int * const p = &x; *p = 2; return x; }").is_ok()
        );
    }

    #[test]
    fn rejects_array_and_incompatible_struct_assignment() {
        assert!(check("int main(void) { int a[2], b[2]; a = b; return 0; }").is_err());
        assert!(check("struct A { int x; }; struct B { int x; }; int main(void) { struct A a; struct B b; a = b; return 0; }").is_err());
    }

    #[test]
    fn rejects_register_address() {
        let error = check("int main(void) { register int x = 1; return *&x; }").unwrap_err();
        assert!(error.message.contains("register"));
    }

    #[test]
    fn rejects_pointer_plus_float() {
        let error = check("int main(void) { int *p; return p + 1.0; }").unwrap_err();
        assert!(error.message.contains("操作数类型不兼容"));
    }

    #[test]
    fn rejects_float_bitwise_operator() {
        let error = check("int main(void) { return 1.0 & 2; }").unwrap_err();
        assert!(error.message.contains("操作数类型不兼容"));
    }

    #[test]
    fn rejects_increment_of_non_lvalue() {
        let error = check("int main(void) { return (1 + 2)++; }").unwrap_err();
        assert!(error.message.contains("自增自减操作数不是可修改左值"));
    }

    #[test]
    fn rejects_default_outside_switch() {
        let error = check("int main(void) { default: return 1; }").unwrap_err();
        assert!(error.message.contains("default 只能出现在 switch"));
    }

    #[test]
    fn rejects_case_outside_switch() {
        let error = check("int main(void) { case 1: return 1; }").unwrap_err();
        assert!(error.message.contains("case 只能出现在 switch"));
    }

    #[test]
    fn rejects_incompatible_function_prototypes() {
        let error = check("int f(int value); double f(int value); int main(void) { return 0; }")
            .unwrap_err();
        assert!(error.message.contains("声明或定义与先前原型不兼容"));
    }

    #[test]
    fn rejects_prototype_call_argument_count() {
        let error = check("int f(int value); int main(void) { return f(); }").unwrap_err();
        assert!(error.message.contains("参数数量不匹配"));
    }

    #[test]
    fn accepts_unspecified_parameter_declaration() {
        assert!(check("int f(); int main(void) { return f(1, 2, 3); }").is_ok());
    }

    #[test]
    fn accepts_void_prototype_without_arguments() {
        assert!(check("int f(void); int main(void) { return f(); }").is_ok());
    }
    #[test]
    fn accepts_known_function() {
        assert!(check("int add(int a,int b); int main(void){return add(1,2);}").is_ok());
    }
}
