//! Minimal semantic validation before code generation.

use crate::ast::{BinaryOp, Node, Param, Type, UnaryOp};
use crate::diagnostics::{CompileError, CompileResult, Span};
use crate::lexer::Token;
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
        _ => left == right,
    }
}

struct Context<'a> {
    file: &'a str,
    tokens: &'a [Token],
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

pub fn validate(items: &[Node], file: &str) -> CompileResult<()> {
    validate_with_tokens(items, file, &[])
}

pub fn validate_with_tokens(items: &[Node], file: &str, tokens: &[Token]) -> CompileResult<()> {
    let context = Context { file, tokens };
    let mut globals = HashSet::new();
    let mut global_types = HashMap::new();
    let mut functions = HashSet::new();
    let mut signatures: HashMap<String, FunctionSignature> = HashMap::new();
    for item in items {
        match item.inner() {
            Node::Decl { specs, name, .. } => {
                globals.insert(name.clone());
                global_types.insert(name.clone(), specs.ty.clone());
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

fn is_constant_initializer(node: &Node) -> bool {
    match node.inner() {
        Node::Num(_) | Node::FNum(_) | Node::Str(_) | Node::Ident(_) => true,
        Node::Unary(UnaryOp::Addr | UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot, value)
        | Node::Cast(_, value) => is_constant_initializer(value),
        Node::Binary(_, left, right) | Node::Comma(left, right) => {
            is_constant_initializer(left) && is_constant_initializer(right)
        }
        Node::Cond(condition, yes, no) => {
            is_constant_initializer(condition)
                && is_constant_initializer(yes)
                && is_constant_initializer(no)
        }
        Node::InitList(items) => items.iter().all(is_constant_initializer),
        _ => false,
    }
}

/// 地址常量的基形式：数组名 / 字符串字面量（含下标链，如 arr[1]、arr[1][2]）。
fn is_address_constant_base(node: &Node) -> bool {
    match node.inner() {
        Node::Ident(_) | Node::Str(_) => true,
        Node::Subscript(base, index) => {
            is_address_constant_base(base) && eval_integer_constant(index).is_some()
        }
        _ => false,
    }
}

/// 指针地址常量：&静态对象（含下标形式）、数组名/字符串字面量 ± 整数常量
/// （C89 §3.4 地址常量）。
fn is_pointer_address_constant(node: &Node) -> bool {
    match node.inner() {
        Node::Unary(UnaryOp::Addr, value) => is_address_constant_base(value),
        Node::Binary(BinaryOp::Add, left, right) => {
            (is_address_constant_base(left) && eval_integer_constant(right).is_some())
                || (eval_integer_constant(left).is_some() && is_address_constant_base(right))
        }
        Node::Binary(BinaryOp::Sub, left, right) => {
            is_address_constant_base(left) && eval_integer_constant(right).is_some()
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
}

fn check_static_initializer(ty: &Type, node: &Node) -> StaticInit {
    match ty.unqualified() {
        Type::Array(elem, _) => match node.inner() {
            Node::Str(_) => {
                if matches!(elem.unqualified(), Type::Char | Type::UChar) {
                    StaticInit::Ok
                } else {
                    StaticInit::StringArray
                }
            }
            Node::InitList(items) => {
                if items.iter().all(is_constant_initializer) {
                    StaticInit::Ok
                } else {
                    StaticInit::NotConstant
                }
            }
            _ => StaticInit::NotConstant,
        },
        Type::Struct(_) => match node.inner() {
            Node::InitList(items) if items.iter().all(is_constant_initializer) => StaticInit::Ok,
            _ => StaticInit::NotConstant,
        },
        Type::Ptr(_) => match node.inner() {
            Node::Ident(_) | Node::Str(_) => StaticInit::Ok,
            Node::Unary(UnaryOp::Addr, value) if is_address_constant_base(value) => StaticInit::Ok,
            Node::Binary(_, _, _) if is_pointer_address_constant(node) => StaticInit::Ok,
            _ if eval_integer_constant(node) == Some(0) => StaticInit::Ok,
            _ => StaticInit::NotConstant,
        },
        Type::Float | Type::Double => match node.inner() {
            Node::FNum(_) => StaticInit::Ok,
            _ if eval_integer_constant(node).is_some() => StaticInit::Ok,
            _ => StaticInit::NotConstant,
        },
        ty if ty.is_integer() => {
            if eval_integer_constant(node).is_some() {
                StaticInit::Ok
            } else {
                StaticInit::NotConstant
            }
        }
        _ => StaticInit::NotConstant,
    }
}

fn validate_object_type(
    ty: &Type,
    storage: crate::ast::Storage,
    context: &Context<'_>,
    node: &Node,
) -> CompileResult<()> {
    match ty {
        Type::Qualified { ty, .. } => validate_object_type(ty, storage, context, node),
        Type::Void => context.error(node, None, "对象不能声明为 void 类型"),
        Type::Array(element, length) => {
            if matches!(length, Some(0)) {
                return context.error(node, None, "数组长度必须大于零");
            }
            if length.is_none() && storage != crate::ast::Storage::Extern {
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
    for item in items {
        if let Node::Decl { specs, init, .. } = item.inner() {
            if matches!(
                specs.storage,
                crate::ast::Storage::Auto | crate::ast::Storage::Register
            ) {
                return context.error(item, None, "文件作用域对象不能使用 auto 或 register");
            }
            if specs.storage != crate::ast::Storage::Typedef {
                validate_object_type(&specs.ty, specs.storage, context, item)?;
            }
            if let Some(init) = init {
                if specs.storage == crate::ast::Storage::Extern {
                    return context.error(item, None, "extern 声明不能带初始化器");
                }
                match check_static_initializer(&specs.ty, init) {
                    StaticInit::Ok => {}
                    StaticInit::StringArray => {
                        return context.error(init, None, "字符串字面量只能初始化字符数组")
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
) -> CompileResult<()> {
    let mut const_objects = HashSet::new();
    let mut register_objects = HashSet::new();
    fn collect(
        node: &Node,
        context: &Context<'_>,
        const_objects: &mut HashSet<String>,
        register_objects: &mut HashSet<String>,
    ) -> CompileResult<()> {
        match node.inner() {
            Node::Decl { specs, name, init } => {
                if specs.storage != crate::ast::Storage::Typedef {
                    validate_object_type(&specs.ty, specs.storage, context, node)?;
                }
                if specs.ty.is_const_qualified() {
                    const_objects.insert(name.clone());
                }
                if specs.storage == crate::ast::Storage::Register {
                    register_objects.insert(name.clone());
                }
                if specs.storage == crate::ast::Storage::Extern && init.is_some() {
                    return context.error(node, None, "块作用域 extern 声明不能带初始化器");
                }
                if specs.storage == crate::ast::Storage::Static {
                    if let Some(init) = init {
                        match check_static_initializer(&specs.ty, init) {
                            StaticInit::Ok => {}
                            StaticInit::StringArray => {
                                return context
                                    .error(init, None, "字符串字面量只能初始化字符数组")
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
                    collect(child, context, const_objects, register_objects)?;
                }
            }
            Node::If(_, yes, no) => {
                collect(yes, context, const_objects, register_objects)?;
                if let Some(no) = no {
                    collect(no, context, const_objects, register_objects)?;
                }
            }
            Node::While(_, b)
            | Node::DoWhile(b, _)
            | Node::For(_, _, _, b)
            | Node::Switch(_, b)
            | Node::Case(_, b)
            | Node::Default(b)
            | Node::Label(_, b) => collect(b, context, const_objects, register_objects)?,
            _ => {}
        }
        Ok(())
    }
    collect(body, context, &mut const_objects, &mut register_objects)?;

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
    validate_block_objects(body, context, &types, return_type)?;
    validate_calls(body, context, &types, signatures)?;
    let mut labels = HashSet::new();
    let mut gotos = Vec::new();
    collect_labels(body, context, &mut labels, &mut gotos)?;
    for label in gotos {
        if !labels.contains(&label) {
            return context.token_error(Some(&label), format!("goto 引用了未定义标签 '{label}'"));
        }
    }
    walk(body, context, globals, functions, &locals, 0, 0)
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
        Node::Subscript(array, index) => {
            check(array)?;
            check(index)?;
            if let Some(ty) = expression_type(array, types) {
                if !ty.is_pointer_like() {
                    return context.error(node, Some("["), "下标运算左侧必须是数组或指针");
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

/// 空指针常量：值为 0 的整型常量表达式（C89 §6.2.2.3）
fn is_null_pointer_constant(mut node: &Node) -> bool {
    while let Node::Located(inner, _) = node.inner() {
        node = inner;
    }
    matches!(node.inner(), Node::Num(0))
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
