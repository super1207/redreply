//! 代码生成：AST → RARS 风格 RISC-V 汇编（RV32IM）
//!
//! 策略（教学简化）：
//! - 表达式结果放 a0
//! - 二元运算：右操作数压栈暂存
//! - 局部变量在栈帧（sp 偏移寻址）
//! - 全局变量在 .data 段
//! - 参数从 a0 起传入，存栈帧

use crate::ast::{AssignOp, BinaryOp, Node, Param, Storage, Type, UnaryOp};
use crate::diagnostics::{CompileError, CompileResult, Span};
use crate::lexer::Token;
use crate::types::TypeTable;
use std::collections::{HashMap, HashSet};

pub struct Codegen {
    out: String,
    lbl: usize,
    /// 当前函数的帧大小（字节）
    frame_size: i32,
    /// 局部变量 → (sp 偏移, 类型)
    locals: HashMap<String, (i32, Type)>,
    /// 全局变量名 → 类型
    globals: HashMap<String, Type>,
    global_inits: HashMap<String, Node>,
    /// 局部 static 对象：标签、类型、常量初始化器。
    static_objects: Vec<(String, Type, Option<Node>)>,
    /// 当前函数可见的 static 名称到数据段标签。
    current_statics: HashMap<String, (String, Type)>,
    /// 类型表（size_of 用）
    tt: TypeTable,
    /// 函数名 → (返回类型, 具名参数, 变参标志)
    func_info: HashMap<String, (Type, Vec<Param>, bool)>,
    /// 函数名 → 返回类型（浮点返回值约定 f10）
    func_ret: HashMap<String, Type>,
    /// 字符串字面量 (label, 内容)
    strings: Vec<(String, String)>,
    /// 浮点字面量 (label, 值)
    floats: Vec<(String, f64)>,
    /// 静态指针常量别名（&arr[i] / "str"+k / arr+k 等带偏移地址常量）：
    /// 目标数据标签 → [(字节偏移, 别名标签)]。别名标签在目标对象的数据
    /// 发射中落在对应字节偏移处，使 `.word 别名` 能表示 label±offset。
    ptr_aliases: HashMap<String, Vec<(usize, String)>>,
    /// 当前函数的返回标签
    ret_label: String,
    /// struct 返回函数中隐藏 sret 指针保存的栈帧偏移。
    sret_slot: Option<i32>,
    /// 当前函数的声明返回类型。
    current_ret: Type,
    /// struct 返回调用节点到当前函数栈帧临时结果对象偏移。
    struct_call_temps: HashMap<usize, i32>,
    /// 可被 break 跳出的最内层循环或 switch 的结束标签。
    break_labels: Vec<String>,
    /// 最内层循环的 continue 目标标签。
    continue_labels: Vec<String>,
    /// 当前函数内 C 标签名到汇编标签的映射。
    user_labels: HashMap<String, String>,
    /// 嵌套 switch 的 case/default 节点到汇编标签的映射。
    switch_labels: Vec<HashMap<usize, String>>,
    /// checked API 在内部失败时不会返回部分汇编。
    internal_error: Option<String>,
    /// 是否链接了 crt0（-l 模式）：main 返回给 _start 而不是直接 exit
    with_crt0: bool,
    /// 局部标签前缀：多个翻译单元拼接时避免 .L<n> 冲突
    label_prefix: String,
}

impl Codegen {
    pub fn new() -> Self {
        Codegen {
            out: String::new(),
            lbl: 0,
            frame_size: 0,
            locals: HashMap::new(),
            globals: HashMap::new(),
            global_inits: HashMap::new(),
            static_objects: Vec::new(),
            current_statics: HashMap::new(),
            tt: TypeTable::new(),
            func_ret: HashMap::new(),
            func_info: HashMap::new(),
            strings: Vec::new(),
            floats: Vec::new(),
            ptr_aliases: HashMap::new(),
            ret_label: String::new(),
            sret_slot: None,
            current_ret: Type::Void,
            struct_call_temps: HashMap::new(),
            break_labels: Vec::new(),
            continue_labels: Vec::new(),
            user_labels: HashMap::new(),
            switch_labels: Vec::new(),
            internal_error: None,
            with_crt0: false,
            label_prefix: String::new(),
        }
    }

    fn label(&mut self) -> String {
        let l = format!(".L{}{}", self.label_prefix, self.lbl);
        self.lbl += 1;
        l
    }

    fn emit(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn fail_codegen(&mut self, message: impl Into<String>) {
        if self.internal_error.is_none() {
            self.internal_error = Some(message.into());
        }
    }

    /// 结构体按值传参/返回时的尺寸上限：拷贝序列按字节逐条展开，
    /// 超过该值会导致汇编体积与编译时间失控
    const MAX_STRUCT_BYVAL: usize = 4096;

    fn user_label(&mut self, name: &str) -> String {
        if let Some(label) = self.user_labels.get(name) {
            return label.clone();
        }
        let label = self.label();
        self.user_labels.insert(name.to_owned(), label.clone());
        label
    }

    fn node_id(node: &Node) -> usize {
        node as *const Node as usize
    }

    fn collect_switch_labels<'a>(node: &'a Node, labels: &mut Vec<&'a Node>) {
        match node {
            Node::Switch(_, _) => {}
            Node::Case(_, body) | Node::Default(body) => {
                labels.push(node);
                Self::collect_switch_labels(body, labels);
            }
            Node::Block(nodes) | Node::DeclBlock(nodes) => {
                for node in nodes {
                    Self::collect_switch_labels(node, labels);
                }
            }
            Node::If(_, yes, no) => {
                Self::collect_switch_labels(yes, labels);
                if let Some(no) = no {
                    Self::collect_switch_labels(no, labels);
                }
            }
            Node::While(_, body)
            | Node::DoWhile(body, _)
            | Node::For(_, _, _, body)
            | Node::Label(_, body) => Self::collect_switch_labels(body, labels),
            _ => {}
        }
    }

    /// 生成完整程序汇编
    pub fn generate(&mut self, items: &[Node]) -> String {
        // 第一遍：收集全局变量、初始化器和完整函数签名。
        self.collect_data(items);
        self.collect_floats(items);
        self.collect_function_signatures(items);
        self.collect_static_objects(items);
        self.collect_ptr_aliases(items);
        // .data 段
        let mut data = String::new();
        data.push_str(".data\n");
        // tentative definition（int x; int x;）合并为同一对象：只发射一次标签。
        let mut emitted_globals: HashSet<String> = HashSet::new();
        for item in items {
            if let Node::Decl { specs, name, .. } = item {
                if !matches!(specs.storage, Storage::Typedef | Storage::Extern)
                    && emitted_globals.insert(name.clone())
                {
                    // 外部链接的对象需导出符号（static 是内部链接，不导出）
                    if specs.storage != Storage::Static {
                        data.push_str(&format!(".globl {}\n", name));
                    }
                    self.emit_global_data(&mut data, name, &specs.ty);
                }
            }
        }
        for (lbl, content) in &self.strings {
            if self.ptr_aliases.contains_key(lbl) {
                // 有指针常量别名指向该字符串内部 → 逐字节发射，便于在偏移处放别名标签。
                let mut bytes = crate::expr_eval::decode_string_escapes(content)
                    .expect("非法字符串转义应在代码生成校验阶段被拦截");
                bytes.push(0);
                data.push_str(&format!("{}:\n", lbl));
                for (index, byte) in bytes.iter().enumerate() {
                    self.emit_alias_labels(&mut data, lbl, index);
                    data.push_str(&format!("    .byte {}\n", byte));
                }
            } else {
                data.push_str(&format!("{}: .asciiz \"{}\"\n", lbl, content));
            }
        }
        for (lbl, v) in &self.floats {
            // double 常量（8 字节，需 8 对齐）
            data.push_str(&format!(".align 3\n{}: .double {}\n", lbl, v));
        }
        for (label, ty, init) in &self.static_objects {
            self.emit_global_data_value(&mut data, label, ty, init.as_ref());
        }
        // .text 段
        let mut text = String::new();
        text.push_str(".text\n");
        for item in items {
            if let Node::FuncDef { .. } = item {
                self.gen_func(item);
            }
        }
        text.push_str(&self.out);
        format!("{}\n{}", data, text)
    }

    /// 收集全局变量与字符串字面量
    fn collect_data(&mut self, items: &[Node]) {
        let prefix = self.label_prefix.clone();
        fn walk(
            node: &Node,
            globals: &mut HashMap<String, Type>,
            strings: &mut Vec<(String, String)>,
            in_func: bool,
            prefix: &str,
        ) {
            match node {
                // 解析器会给表达式和语句加 Located 包装；先解包，
                // 否则其中的字符串字面量不会进入字符串数据表。
                Node::Located(inner, _) => walk(inner, globals, strings, in_func, prefix),
                Node::Decl {
                    specs, name, init, ..
                } => {
                    if !in_func {
                        globals.insert(name.clone(), specs.ty.clone());
                    }
                    if let Some(init) = init.as_deref() {
                        walk(init, globals, strings, in_func, prefix);
                    }
                }
                Node::FuncDef { body, .. } => walk(body, globals, strings, true, prefix),
                Node::Block(stmts) | Node::DeclBlock(stmts) => {
                    for s in stmts {
                        walk(s, globals, strings, in_func, prefix);
                    }
                }
                Node::If(condition, then_branch, else_branch) => {
                    walk(condition, globals, strings, in_func, prefix);
                    walk(then_branch, globals, strings, in_func, prefix);
                    if let Some(else_branch) = else_branch {
                        walk(else_branch, globals, strings, in_func, prefix);
                    }
                }
                Node::While(condition, body) => {
                    walk(condition, globals, strings, in_func, prefix);
                    walk(body, globals, strings, in_func, prefix);
                }
                Node::DoWhile(body, condition) => {
                    walk(body, globals, strings, in_func, prefix);
                    walk(condition, globals, strings, in_func, prefix);
                }
                Node::For(init, condition, post, body) => {
                    if let Some(init) = init {
                        walk(init, globals, strings, in_func, prefix);
                    }
                    if let Some(condition) = condition {
                        walk(condition, globals, strings, in_func, prefix);
                    }
                    if let Some(post) = post {
                        walk(post, globals, strings, in_func, prefix);
                    }
                    walk(body, globals, strings, in_func, prefix);
                }
                Node::Switch(expression, body) => {
                    walk(expression, globals, strings, in_func, prefix);
                    walk(body, globals, strings, in_func, prefix);
                }
                Node::Case(value, statement) => {
                    walk(value, globals, strings, in_func, prefix);
                    walk(statement, globals, strings, in_func, prefix);
                }
                Node::Default(s) => walk(s, globals, strings, in_func, prefix),
                Node::Label(_, s) => walk(s, globals, strings, in_func, prefix),
                Node::ExprStmt(e) | Node::Return(Some(e)) => {
                    walk(e, globals, strings, in_func, prefix)
                }
                Node::Assign(_, l, r) | Node::Binary(_, l, r) => {
                    walk(l, globals, strings, in_func, prefix);
                    walk(r, globals, strings, in_func, prefix);
                }
                Node::Cond(c, t, e) => {
                    walk(c, globals, strings, in_func, prefix);
                    walk(t, globals, strings, in_func, prefix);
                    walk(e, globals, strings, in_func, prefix);
                }
                Node::Unary(_, expression) => walk(expression, globals, strings, in_func, prefix),
                Node::Subscript(array, index) => {
                    walk(array, globals, strings, in_func, prefix);
                    walk(index, globals, strings, in_func, prefix);
                }
                Node::Call(f, args) => {
                    walk(f, globals, strings, in_func, prefix);
                    for a in args {
                        walk(a, globals, strings, in_func, prefix);
                    }
                }
                Node::Member(expression, _, _) => {
                    walk(expression, globals, strings, in_func, prefix)
                }
                Node::Comma(left, right) => {
                    walk(left, globals, strings, in_func, prefix);
                    walk(right, globals, strings, in_func, prefix);
                }
                Node::SizeofExpr(e) | Node::Cast(_, e) => {
                    walk(e, globals, strings, in_func, prefix)
                }
                Node::InitList(items) => {
                    for item in items {
                        walk(item, globals, strings, in_func, prefix);
                    }
                }
                Node::Str(s) => {
                    let lbl = format!(".Lstr{}{}", prefix, strings.len());
                    strings.push((lbl, s.clone()));
                }

                _ => {}
            }
        }
        for item in items {
            walk(item, &mut self.globals, &mut self.strings, false, &prefix);
        }
        fn collect_inits(node: &Node, in_func: bool, inits: &mut HashMap<String, Node>) {
            match node {
                Node::Decl {
                    name,
                    init: Some(init),
                    ..
                } if !in_func => {
                    inits.insert(name.clone(), (*init.clone()).clone());
                }
                Node::FuncDef { body, .. } => collect_inits(body, true, inits),
                Node::Block(nodes) | Node::DeclBlock(nodes) => {
                    for node in nodes {
                        collect_inits(node, in_func, inits);
                    }
                }
                _ => {}
            }
        }
        for item in items {
            collect_inits(item, false, &mut self.global_inits);
        }
    }

    fn collect_static_objects(&mut self, items: &[Node]) {
        let prefix = self.label_prefix.clone();
        fn walk(
            node: &Node,
            function: &str,
            out: &mut Vec<(String, Type, Option<Node>)>,
            counter: &mut usize,
            prefix: &str,
        ) {
            match node.inner() {
                Node::FuncDef { name, body, .. } => {
                    let mut local_counter = 0usize;
                    walk(body, name, out, &mut local_counter, prefix);
                }
                Node::Block(nodes) | Node::DeclBlock(nodes) => {
                    for child in nodes {
                        walk(child, function, out, counter, prefix);
                    }
                }
                Node::If(_, a, b) => {
                    walk(a, function, out, counter, prefix);
                    if let Some(b) = b {
                        walk(b, function, out, counter, prefix);
                    }
                }
                Node::While(_, b)
                | Node::DoWhile(b, _)
                | Node::For(_, _, _, b)
                | Node::Switch(_, b)
                | Node::Case(_, b)
                | Node::Default(b)
                | Node::Label(_, b) => walk(b, function, out, counter, prefix),
                Node::Decl { specs, name, init } if specs.storage == Storage::Static => {
                    let label = format!(".Lstatic_{}_{}_{}", prefix, function, *counter);
                    *counter += 1;
                    out.push((label, specs.ty.clone(), init.as_deref().cloned()));
                }
                _ => {}
            }
        }
        self.static_objects.clear();
        let mut counter = 0;
        for item in items {
            walk(item, "", &mut self.static_objects, &mut counter, &prefix);
        }
    }

    fn emit_global_data_value(
        &self,
        data: &mut String,
        name: &str,
        ty: &Type,
        init: Option<&Node>,
    ) {
        let align = self.tt.align_of(ty).unwrap_or(1);
        if align > 1 {
            data.push_str(&format!(".align {}\n", align.trailing_zeros()));
        }
        data.push_str(&format!("{}:\n", name));
        let mut pos = 0usize;
        self.emit_global_object(data, ty, init, name, &mut pos);
        self.emit_alias_labels(data, name, pos);
    }

    fn collect_function_signatures(&mut self, items: &[Node]) {
        for item in items {
            match item {
                Node::FuncDecl {
                    ret,
                    name,
                    params,
                    is_variadic,
                    ..
                }
                | Node::FuncDef {
                    ret,
                    name,
                    params,
                    is_variadic,
                    ..
                } => {
                    self.func_ret.insert(name.clone(), ret.clone());
                    self.func_info
                        .insert(name.clone(), (ret.clone(), params.clone(), *is_variadic));
                }
                _ => {}
            }
        }
    }

    /// 收集浮点字面量（独立遍历，避免改 walk 签名）
    fn collect_floats(&mut self, items: &[Node]) {
        let prefix = self.label_prefix.clone();
        fn walk(node: &Node, floats: &mut Vec<(String, f64)>, prefix: &str) {
            match node {
                Node::FNum(v) => {
                    if !floats.iter().any(|(_, f)| f.to_bits() == v.to_bits()) {
                        let lbl = format!(".Lfloat{}{}", prefix, floats.len());
                        floats.push((lbl, *v));
                    }
                }
                Node::Block(stmts) | Node::DeclBlock(stmts) | Node::InitList(stmts) => {
                    for s in stmts {
                        walk(s, floats, prefix);
                    }
                }
                Node::If(c, t, e) => {
                    walk(c, floats, prefix);
                    walk(t, floats, prefix);
                    if let Some(e) = e {
                        walk(e, floats, prefix);
                    }
                }
                Node::While(c, b) => {
                    walk(c, floats, prefix);
                    walk(b, floats, prefix);
                }
                Node::DoWhile(b, c) => {
                    walk(b, floats, prefix);
                    walk(c, floats, prefix);
                }
                Node::For(i, c, p, b) => {
                    if let Some(i) = i {
                        walk(i, floats, prefix);
                    }
                    if let Some(c) = c {
                        walk(c, floats, prefix);
                    }
                    if let Some(p) = p {
                        walk(p, floats, prefix);
                    }
                    walk(b, floats, prefix);
                }
                Node::Switch(e, b) => {
                    walk(e, floats, prefix);
                    walk(b, floats, prefix);
                }
                Node::Case(_, b) | Node::Default(b) | Node::Label(_, b) => {
                    walk(b, floats, prefix)
                }
                Node::ExprStmt(e)
                | Node::Return(Some(e))
                | Node::Unary(_, e)
                | Node::SizeofExpr(e)
                | Node::Member(e, _, _) => walk(e, floats, prefix),
                Node::Subscript(a, i) => {
                    walk(a, floats, prefix);
                    walk(i, floats, prefix);
                }
                Node::Assign(_, l, r) | Node::Binary(_, l, r) | Node::Comma(l, r) => {
                    walk(l, floats, prefix);
                    walk(r, floats, prefix);
                }
                Node::Cast(_, e) => walk(e, floats, prefix),
                Node::Cond(c, t, e) => {
                    walk(c, floats, prefix);
                    walk(t, floats, prefix);
                    walk(e, floats, prefix);
                }
                Node::Call(f, args) => {
                    walk(f, floats, prefix);
                    for a in args {
                        walk(a, floats, prefix);
                    }
                }
                Node::FuncDef { body, .. } => walk(body, floats, prefix),
                Node::Decl { init: Some(i), .. } => walk(i, floats, prefix),
                _ => {}
            }
        }
        for item in items {
            walk(item, &mut self.floats, &prefix);
        }
    }

    /// 收集函数体内的局部变量，分配栈偏移
    fn collect_locals(&mut self, body: &Node, base: i32) {
        let mut n = base;
        // 单函数栈帧上限：超过即报编译错误，防止帧大小回绕和 VM 无法容纳的帧
        const MAX_FRAME: i64 = 1 << 28;
        fn walk(
            node: &Node,
            locals: &mut HashMap<String, (i32, Type)>,
            n: &mut i32,
            tt: &TypeTable,
        ) -> Result<(), String> {
            match node {
                Node::Block(stmts) | Node::DeclBlock(stmts) => {
                    for s in stmts {
                        walk(s, locals, n, tt)?;
                    }
                }
                Node::Decl { specs, name, .. }
                    if !matches!(
                        specs.storage,
                        Storage::Static | Storage::Extern | Storage::Typedef
                    ) =>
                {
                    if !locals.contains_key(name) {
                        // 数组按实际大小分配（TypeTable.size_of）
                        let sz = tt.size_of(&specs.ty).unwrap_or(4).max(4);
                        let align = tt.align_of(&specs.ty).unwrap_or(4).min(8) as i32;
                        // 巨型局部数组(如 int a[0x7fffffff])会让 i32 栈偏移回绕，
                        // 用 checked 运算把它变成可报告的编译错误
                        let aligned = (*n as i64)
                            .checked_add(align as i64 - 1)
                            .map(|v| v & !(align as i64 - 1))
                            .ok_or_else(|| format!("局部变量`{name}`栈偏移溢出"))?;
                        let next = aligned
                            .checked_add(sz as i64)
                            .ok_or_else(|| format!("局部变量`{name}`过大，栈帧溢出"))?;
                        if next > MAX_FRAME {
                            return Err(format!("局部变量`{name}`过大，栈帧超过上限"));
                        }
                        let next_i32: i32 = next
                            .try_into()
                            .map_err(|_| format!("局部变量`{name}`过大，栈帧溢出"))?;
                        *n = aligned as i32;
                        locals.insert(name.clone(), (*n, specs.ty.clone()));
                        *n = next_i32;
                    }
                }
                Node::If(_, t, e) => {
                    walk(t, locals, n, tt)?;
                    if let Some(e) = e {
                        walk(e, locals, n, tt)?;
                    }
                }
                Node::While(_, b) | Node::DoWhile(b, _) => walk(b, locals, n, tt)?,
                Node::For(_, _, _, b) => walk(b, locals, n, tt)?,
                Node::Switch(_, b) => walk(b, locals, n, tt)?,
                Node::Case(_, s) => walk(s, locals, n, tt)?,
                Node::Default(s) => walk(s, locals, n, tt)?,
                Node::Label(_, s) => walk(s, locals, n, tt)?,
                Node::ExprStmt(e) | Node::Return(Some(e)) => walk(e, locals, n, tt)?,
                Node::Assign(_, l, r) | Node::Binary(_, l, r) | Node::Cond(l, r, _) => {
                    walk(l, locals, n, tt)?;
                    walk(r, locals, n, tt)?;
                }
                Node::Unary(_, e) | Node::Subscript(e, _) | Node::Comma(e, _) => {
                    walk(e, locals, n, tt)?
                }
                Node::Call(f, args) => {
                    walk(f, locals, n, tt)?;
                    for a in args {
                        walk(a, locals, n, tt)?;
                    }
                }
                Node::Member(e, _, _) | Node::SizeofExpr(e) => walk(e, locals, n, tt)?,
                _ => {}
            }
            Ok(())
        }
        if let Err(message) = walk(body, &mut self.locals, &mut n, &self.tt) {
            self.fail_codegen(message);
            return;
        }
        self.frame_size = n;
    }
    fn collect_struct_call_temps(&mut self, body: &Node) {
        fn walk(node: &Node, codegen: &mut Codegen, next: &mut i32) {
            if let Node::Call(_, _) = node.inner() {
                if let Some(ty @ Type::Struct(_)) = codegen.type_of(node) {
                    let align = codegen.tt.align_of(&ty).unwrap_or(1).min(8) as i32;
                    *next = (*next + align - 1) & !(align - 1);
                    codegen
                        .struct_call_temps
                        .insert(Codegen::node_id(node), *next);
                    *next += codegen.tt.size_of(&ty).unwrap_or(0) as i32;
                }
            }
            match node.inner() {
                Node::Block(nodes) | Node::DeclBlock(nodes) | Node::InitList(nodes) => {
                    for child in nodes {
                        walk(child, codegen, next);
                    }
                }
                Node::If(c, yes, no) => {
                    walk(c, codegen, next);
                    walk(yes, codegen, next);
                    if let Some(no) = no {
                        walk(no, codegen, next);
                    }
                }
                Node::While(c, body) | Node::DoWhile(body, c) => {
                    walk(c, codegen, next);
                    walk(body, codegen, next);
                }
                Node::For(init, cond, post, body) => {
                    for child in [init.as_deref(), cond.as_deref(), post.as_deref()]
                        .into_iter()
                        .flatten()
                    {
                        walk(child, codegen, next);
                    }
                    walk(body, codegen, next);
                }
                Node::Switch(expr, body) | Node::Case(expr, body) => {
                    walk(expr, codegen, next);
                    walk(body, codegen, next);
                }
                Node::Default(body)
                | Node::Label(_, body)
                | Node::ExprStmt(body)
                | Node::Return(Some(body))
                | Node::Unary(_, body)
                | Node::SizeofExpr(body)
                | Node::Cast(_, body)
                | Node::Member(body, _, _) => walk(body, codegen, next),
                Node::Decl {
                    init: Some(init), ..
                } => walk(init, codegen, next),
                Node::Assign(_, left, right)
                | Node::Binary(_, left, right)
                | Node::Comma(left, right)
                | Node::Subscript(left, right) => {
                    walk(left, codegen, next);
                    walk(right, codegen, next);
                }
                Node::Cond(c, yes, no) => {
                    walk(c, codegen, next);
                    walk(yes, codegen, next);
                    walk(no, codegen, next);
                }
                Node::Call(callee, args) => {
                    walk(callee, codegen, next);
                    for arg in args {
                        walk(arg, codegen, next);
                    }
                }
                _ => {}
            }
        }
        self.struct_call_temps.clear();
        let mut next = self.frame_size;
        walk(body, self, &mut next);
        self.frame_size = next;
    }

    /// 生成函数（含序言/尾声）
    fn gen_func(&mut self, func: &Node) {
        let (ret, name, storage, params, _is_variadic, body) = match func {
            Node::FuncDef {
                ret,
                name,
                storage,
                params,
                is_variadic,
                body,
                ..
            } => (
                ret.clone(),
                name.clone(),
                *storage,
                params.clone(),
                *is_variadic,
                body,
            ),
            _ => unreachable!(),
        };
        self.func_ret.insert(name.clone(), ret.clone());
        self.break_labels.clear();
        self.continue_labels.clear();
        self.user_labels.clear();
        self.switch_labels.clear();
        self.func_info
            .insert(name.clone(), (ret.clone(), params.clone(), _is_variadic));
        self.current_ret = ret.clone();
        self.sret_slot = Self::is_struct_ty(&ret).then_some(0);
        // 参数槽布局：int/double 8 字节，struct 按实际大小（8 对齐）
        self.locals.clear();
        self.current_statics.clear();
        let mut static_index = 0usize;
        let current_statics_prefix = self.label_prefix.clone();
        fn collect_current(
            node: &Node,
            statics: &mut HashMap<String, (String, Type)>,
            function: &str,
            index: &mut usize,
            prefix: &str,
        ) {
            match node.inner() {
                Node::Block(nodes) | Node::DeclBlock(nodes) => {
                    for child in nodes {
                        collect_current(child, statics, function, index, prefix);
                    }
                }
                Node::If(_, a, b) => {
                    collect_current(a, statics, function, index, prefix);
                    if let Some(b) = b {
                        collect_current(b, statics, function, index, prefix);
                    }
                }
                Node::While(_, b)
                | Node::DoWhile(b, _)
                | Node::For(_, _, _, b)
                | Node::Switch(_, b)
                | Node::Case(_, b)
                | Node::Default(b)
                | Node::Label(_, b) => collect_current(b, statics, function, index, prefix),
                Node::Decl { specs, name, .. } if specs.storage == Storage::Static => {
                    let label = format!(".Lstatic_{}_{}_{}", prefix, function, *index);
                    *index += 1;
                    statics.insert(name.clone(), (label, specs.ty.clone()));
                }
                _ => {}
            }
        }
        collect_current(
            body,
            &mut self.current_statics,
            &name,
            &mut static_index,
            &current_statics_prefix,
        );
        let mut base = if self.sret_slot.is_some() { 8 } else { 0 };
        let mut struct_params: Vec<(String, usize)> = Vec::new(); // (name, size)
        let _struct_off = 0usize;
        for p in &params {
            let size = if Self::is_struct_ty(&p.ty) {
                let size = self.tt.size_of(&p.ty).unwrap_or(8);
                // 按值传递的结构体会被逐字节展开成拷贝指令，过大的结构体
                // 会让汇编体积与编译时间爆炸（且尺寸截断为 i32 后破坏帧布局），
                // 超限时直接报编译错误并中止，请改用指针传递
                if size > Self::MAX_STRUCT_BYVAL {
                    self.fail_codegen(format!(
                        "结构体参数`{}`按值传递过大({size}字节，上限{}字节)，请改用指针传递",
                        p.name,
                        Self::MAX_STRUCT_BYVAL
                    ));
                    return;
                }
                struct_params.push((p.name.clone(), size));
                size
            } else {
                8
            };
            base = (base + 7) & !7;
            self.locals.insert(p.name.clone(), (base, p.ty.clone()));
            base += ((size + 7) & !7) as i32;
        }
        self.collect_locals(body, base);
        self.collect_struct_call_temps(body);
        // +16：ra/s0 各 4 字节，另留 t6 槽（调用侧 mv t6, sp 传入第一个变参
        // 地址；va_start 须从帧内读取，不能依赖 caller-saved t6 存活）。
        let frame = ((self.frame_size + 16) + 7) & !7;
        self.frame_size = frame;
        // 参数偏移

        self.ret_label = self.label();

        // static 函数是内部链接，不导出（C89 §3.1.2.2）
        if storage != Storage::Static {
            self.emit(&format!(".globl {}", name));
        }
        self.emit(&format!("{}:", name));
        if frame > 2047 {
            // 帧大于 12 位立即数范围：li + sub 分两步调整 sp
            self.emit(&format!("    li t2, {}", frame));
            self.emit("    sub sp, sp, t2");
        } else {
            self.emit(&format!("    addi sp, sp, -{}", frame));
        }
        // 保存 ra 和帧指针 s0
        let (rb, ro) = self.address_parts("sp", frame - 4, "t2");
        self.emit(&format!("    sw ra, {}({})", ro, rb));
        let (rb, ro) = self.address_parts("sp", frame - 8, "t2");
        self.emit(&format!("    sw s0, {}({})", ro, rb));
        self.emit("    mv s0, sp");
        // t6 = 第一个变参地址（调用侧在压完变参后设置）。它是 caller-saved，
        // 须在序言立即存档，供函数体任意位置的 va_start 读取。
        // 槽位必须在帧内（frame-12，帧公式已预留 +16）：写到帧外 -12(s0)
        // 会被函数体内 sp 之下的 8 字节临时压栈覆盖。
        {
            let (rb, ro) = self.address_parts("s0", frame - 12, "t2");
            self.emit(&format!("    sw t6, {}({})", ro, rb));
        }
        // 保存参数到栈帧（相对 s0）：int → aN，double → f10+fi，struct → 从 t5 拷贝
        {
            if self.sret_slot.is_some() {
                self.emit("    sw a0, 0(s0)");
            }
            let mut ai = usize::from(self.sret_slot.is_some());
            let mut fi = 0usize;
            let mut overflow = 0usize;
            let mut struct_off = 0usize;
            for p in &params {
                if Self::is_struct_ty(&p.ty) {
                    // struct：从 t5 + struct_off 拷贝 size 字节到参数槽
                    let sz = struct_params
                        .iter()
                        .find(|(n, _)| n == &p.name)
                        .map(|(_, s)| *s)
                        .unwrap_or(8);
                    let slot = self.locals.get(&p.name).map(|x| x.0).unwrap_or(0);
                    for k in 0..sz {
                        let (rb, ro) = self.address_parts("t5", struct_off as i32 + k as i32, "t3");
                        self.emit(&format!("    lbu t1, {}({})", ro, rb));
                        let (rb, ro) = self.address_parts("s0", slot + k as i32, "t2");
                        self.emit(&format!("    sb t1, {}({})", ro, rb));
                    }
                    struct_off += (sz + 7) & !7;
                } else if Self::is_float_ty(&p.ty) {
                    let slot = self.locals.get(&p.name).map(|x| x.0).unwrap_or(0);
                    if fi < 22 {
                        if matches!(p.ty.unqualified(), Type::Float) {
                            // 调用侧按 double 位型传 float 实参（fa 寄存器 8 字节），
                            // 形参槽是 4 字节单精度：先窄化再 fsw，与 load/store 约定一致。
                            // 必须就地转换 fcvt.s.d fN, fN：用共享 scratch（如 f11）会
                            // 覆盖尚未入槽的后续浮点形参。
                            let (rb, ro) = self.address_parts("s0", slot, "t2");
                            self.emit(&format!("    fcvt.s.d f{}, f{}", 10 + fi, 10 + fi));
                            self.emit(&format!("    fsw f{}, {}({})", 10 + fi, ro, rb));
                        } else {
                            let (rb, ro) = self.address_parts("s0", slot, "t2");
                            self.emit(&format!("    fsd f{}, {}({})", 10 + fi, ro, rb));
                        }
                    } else {
                        let (rb, ro) = self.address_parts("s0", frame + overflow as i32 * 8, "t2");
                        self.emit(&format!("    fld f0, {}({})", ro, rb));
                        if matches!(p.ty.unqualified(), Type::Float) {
                            // 溢出路径：f0 是刚 fld 进来的 scratch，就地窄化即可
                            self.emit("    fcvt.s.d f0, f0");
                            let (rb, ro) = self.address_parts("s0", slot, "t2");
                            self.emit(&format!("    fsw f0, {}({})", ro, rb));
                        } else {
                            let (rb, ro) = self.address_parts("s0", slot, "t2");
                            self.emit(&format!("    fsd f0, {}({})", ro, rb));
                        }
                        overflow += 1;
                    }
                    fi += 1;
                } else {
                    let slot = self.locals.get(&p.name).map(|x| x.0).unwrap_or(0);
                    if ai < 8 {
                        let (rb, ro) = self.address_parts("s0", slot, "t2");
                        self.emit(&format!("    sw a{}, {}({})", ai, ro, rb));
                    } else {
                        let (rb, ro) = self.address_parts("s0", frame + overflow as i32 * 8, "t2");
                        self.emit(&format!("    lw t1, {}({})", ro, rb));
                        let (rb, ro) = self.address_parts("s0", slot, "t2");
                        self.emit(&format!("    sw t1, {}({})", ro, rb));
                        overflow += 1;
                    }
                    ai += 1;
                }
            }
        }

        // 函数体
        self.gen_stmt(body);

        // 尾声
        self.emit(&format!("{}:", self.ret_label));
        let (rb, ro) = self.address_parts("sp", frame - 8, "t2");
        self.emit(&format!("    lw s0, {}({})", ro, rb));
        let (rb, ro) = self.address_parts("sp", frame - 4, "t2");
        self.emit(&format!("    lw ra, {}({})", ro, rb));
        if frame > 2047 {
            self.emit(&format!("    li t2, {}", frame));
            self.emit("    add sp, sp, t2");
        } else {
            self.emit(&format!("    addi sp, sp, {}", frame));
        }
        if name == "main" && !self.with_crt0 {
            // main 没有调用者（无 crt0），用 exit 系统调用结束（退出码在 a0）
            self.emit("    li a7, 10");
            self.emit("    ecall");
        } else {
            self.emit("    ret");
        }
        self.emit("");
    }

    fn scalar_directive(ty: &Type) -> &'static str {
        match ty.unqualified() {
            Type::Char | Type::UChar => ".byte",
            Type::Short | Type::UShort => ".half",
            Type::Float => ".float",
            Type::Double => ".double",
            _ => ".word",
        }
    }

    fn constant_data_value(&self, ty: &Type, node: Option<&Node>) -> String {
        match (ty.unqualified(), node.map(Node::inner)) {
            // 浮点初始化器可以是常量算术表达式（double g = 1.0 + 2.0）
            (Type::Float | Type::Double, Some(node)) => {
                crate::semantic::eval_float_constant(node)
                    .unwrap_or(0.0)
                    .to_string()
            }
            (Type::Ptr(_), Some(node)) => self.static_ptr_value(node),
            // 整型对象的浮点常量初始化器按 C 转换语义截断（int x = 3.7 → 3）
            (_, Some(node)) => eval_integer_constant(node)
                .or_else(|| crate::semantic::eval_float_constant(node).map(|v| v as i64))
                .unwrap_or(0)
                .to_string(),
            _ => "0".into(),
        }
    }

    /// 静态指针常量的数据初值：目标标签、带偏移的别名标签，或 0。
    fn static_ptr_value(&self, node: &Node) -> String {
        match node.inner() {
            Node::Ident(name) => name.clone(),
            Node::Str(content) => self
                .strings
                .iter()
                .find(|(_, value)| value == content)
                .map(|(label, _)| label.clone())
                .unwrap_or_else(|| "0".into()),
            Node::Unary(UnaryOp::Addr, value) => match value.inner() {
                Node::Ident(name) => name.clone(),
                _ => self.static_ptr_encoded(node),
            },
            _ => self.static_ptr_encoded(node),
        }
    }

    /// 带偏移的地址常量 → 别名标签（零偏移时直接用目标标签）。
    fn static_ptr_encoded(&self, node: &Node) -> String {
        match self.static_ptr_target(node) {
            Some((label, 0)) => label,
            Some((label, offset)) if offset > 0 => self
                .ptr_aliases
                .get(&label)
                .and_then(|aliases| {
                    aliases
                        .iter()
                        .find(|(o, _)| *o == offset as usize)
                        .map(|(_, alias)| alias.clone())
                })
                .unwrap_or_else(|| "0".into()),
            _ => "0".into(),
        }
    }

    /// 求地址常量基表达式的 (目标数据标签, 字节偏移)：
    /// 支持全局对象名、字符串字面量与下标链（arr / "str" / arr[1] / arr[1][2]）。
    /// 目标无法静态解析时返回 None。
    fn address_base_offset(&self, node: &Node) -> Option<(String, i64)> {
        match node.inner() {
            Node::Ident(name) if self.globals.contains_key(name) => Some((name.clone(), 0)),
            Node::Str(content) => self
                .strings
                .iter()
                .find(|(_, value)| value == content)
                .map(|(label, _)| (label.clone(), 0)),
            // &x + k / &arr[i]：取地址后基表达式相同
            Node::Unary(UnaryOp::Addr, value) => self.address_base_offset(value),
            Node::Subscript(base, index) => {
                let (label, offset) = self.address_base_offset(base)?;
                let index = eval_integer_constant(index)?;
                let elem_size = self
                    .type_of(node)
                    .and_then(|ty| self.tt.size_of(&ty).ok())
                    .unwrap_or(1) as i64;
                Some((label, offset + index.wrapping_mul(elem_size)))
            }
            _ => None,
        }
    }

    /// 指针算术步长：数组/指针所指向元素的字节大小。
    fn pointee_size(&self, node: &Node) -> Option<usize> {
        let ty = self.type_of(node)?;
        let pointed = ty.pointed_type()?;
        self.tt.size_of(pointed).ok()
    }

    /// 静态指针常量：&expr 或 指针 ± 整数常量。返回 (目标标签, 字节偏移)。
    fn static_ptr_target(&self, node: &Node) -> Option<(String, i64)> {
        match node.inner() {
            Node::Unary(UnaryOp::Addr, value) => self.address_base_offset(value),
            Node::Binary(BinaryOp::Add, left, right) => {
                // 参与算术的一侧必须是真正的指针/数组，普通整数运算
                //（如全局 int g 的 g + 1）不是地址常量
                if Self::is_ptr_like(&self.type_of(left))
                    && self.address_base_offset(left).is_some()
                {
                    let (label, offset) = self.address_base_offset(left)?;
                    let k = eval_integer_constant(right)?;
                    let size = self.pointee_size(left).unwrap_or(1) as i64;
                    Some((label, offset + k.wrapping_mul(size)))
                } else if Self::is_ptr_like(&self.type_of(right))
                    && self.address_base_offset(right).is_some()
                {
                    let (label, offset) = self.address_base_offset(right)?;
                    let k = eval_integer_constant(left)?;
                    let size = self.pointee_size(right).unwrap_or(1) as i64;
                    Some((label, offset + k.wrapping_mul(size)))
                } else {
                    None
                }
            }
            Node::Binary(BinaryOp::Sub, left, right) => {
                if !Self::is_ptr_like(&self.type_of(left)) {
                    return None;
                }
                let (label, offset) = self.address_base_offset(left)?;
                let k = eval_integer_constant(right)?;
                let size = self.pointee_size(left).unwrap_or(1) as i64;
                Some((label, offset - k.wrapping_mul(size)))
            }
            _ => None,
        }
    }

    /// 类型是否可作为指针算术的操作数（指针或数组）
    fn is_ptr_like(ty: &Option<Type>) -> bool {
        matches!(
            ty.as_ref().map(Type::unqualified),
            Some(Type::Ptr(_)) | Some(Type::Array(_, _))
        )
    }

    /// 收集带偏移的静态指针常量，为其在目标对象的数据发射中登记别名标签。
    /// 别名标签将落在目标标签的对应字节偏移处，使 `.word 别名` 等于 目标+偏移。
    fn collect_ptr_aliases(&mut self, items: &[Node]) {
        let prefix = self.label_prefix.clone();
        let mut counter = 0usize;
        let mut errors: Vec<String> = Vec::new();
        fn walk(
            node: &Node,
            cg: &Codegen,
            out: &mut HashMap<String, Vec<(usize, String)>>,
            counter: &mut usize,
            errors: &mut Vec<String>,
            prefix: &str,
        ) {
            // 任何带偏移的地址常量（含嵌套在聚合初始化器里的）都登记别名。
            if let Some((label, offset)) = cg.static_ptr_target(node) {
                if offset < 0 {
                    errors.push("静态指针初始化器使用了负偏移，无法静态表达".into());
                } else if offset != 0 {
                    let aliases = out.entry(label.clone()).or_default();
                    if !aliases.iter().any(|(o, _)| *o == offset as usize) {
                        let alias = format!(".Loff{}{}", prefix, *counter);
                        *counter += 1;
                        aliases.push((offset as usize, alias));
                    }
                }
            }
            match node.inner() {
                Node::Block(nodes) | Node::DeclBlock(nodes) | Node::InitList(nodes) => {
                    for child in nodes {
                        walk(child, cg, out, counter, errors, prefix);
                    }
                }
                Node::If(_, yes, no) => {
                    walk(yes, cg, out, counter, errors, prefix);
                    if let Some(no) = no {
                        walk(no, cg, out, counter, errors, prefix);
                    }
                }
                Node::While(_, body)
                | Node::DoWhile(body, _)
                | Node::For(_, _, _, body)
                | Node::Switch(_, body)
                | Node::Case(_, body)
                | Node::Default(body)
                | Node::Label(_, body) => walk(body, cg, out, counter, errors, prefix),
                Node::Decl { init: Some(init), .. } => {
                    walk(init, cg, out, counter, errors, prefix)
                }
                Node::ExprStmt(e) | Node::Return(Some(e)) | Node::Unary(_, e) => {
                    walk(e, cg, out, counter, errors, prefix)
                }
                Node::Assign(_, l, r)
                | Node::Binary(_, l, r)
                | Node::Comma(l, r)
                | Node::Subscript(l, r) => {
                    walk(l, cg, out, counter, errors, prefix);
                    walk(r, cg, out, counter, errors, prefix);
                }
                Node::Cond(c, y, n) => {
                    walk(c, cg, out, counter, errors, prefix);
                    walk(y, cg, out, counter, errors, prefix);
                    walk(n, cg, out, counter, errors, prefix);
                }
                Node::Call(f, args) => {
                    walk(f, cg, out, counter, errors, prefix);
                    for arg in args {
                        walk(arg, cg, out, counter, errors, prefix);
                    }
                }
                Node::Member(e, _, _) | Node::Cast(_, e) | Node::SizeofExpr(e) => {
                    walk(e, cg, out, counter, errors, prefix)
                }
                Node::FuncDef { body, .. } => walk(body, cg, out, counter, errors, prefix),
                _ => {}
            }
        }
        let mut aliases: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        // 别名标签只服务于数据段初始化器：只扫描文件作用域对象的初始化器与
        // 函数内静态对象的初始化器。函数体的运行时表达式一律在运行时用
        // la/addi 计算，无需别名——扫函数体会把普通整数运算（如全局 int g
        // 的 g - 1）误判为地址常量，还会遇到局部变量遮蔽全局的情况。
        for item in items {
            if let Node::Decl { init: Some(init), .. } = item {
                walk(init, self, &mut aliases, &mut counter, &mut errors, &prefix);
            }
        }
        for (_, _, init) in &self.static_objects {
            if let Some(init) = init {
                walk(init, self, &mut aliases, &mut counter, &mut errors, &prefix);
            }
        }
        self.ptr_aliases.extend(aliases);
        for error in errors {
            self.fail_codegen(error);
        }
    }

    /// 在目标对象的字节位置 pos 处放置请求的别名标签。
    fn emit_alias_labels(&self, data: &mut String, target: &str, pos: usize) {
        if let Some(aliases) = self.ptr_aliases.get(target) {
            for (offset, alias) in aliases {
                if *offset == pos {
                    data.push_str(&format!("{}:\n", alias));
                }
            }
        }
    }

    /// 发射 .space count 字节；在别名偏移处拆分，便于放置别名标签。
    fn emit_space(&self, data: &mut String, target: &str, pos: &mut usize, count: usize) {
        let mut emitted = 0usize;
        while emitted < count {
            self.emit_alias_labels(data, target, *pos);
            let chunk = self
                .ptr_aliases
                .get(target)
                .and_then(|aliases| {
                    aliases
                        .iter()
                        .filter(|(o, _)| *o > *pos && *o < *pos + count)
                        .map(|(o, _)| *o - *pos)
                        .min()
                })
                .unwrap_or(count - emitted)
                .max(1);
            data.push_str(&format!("    .space {}\n", chunk));
            *pos += chunk;
            emitted += chunk;
        }
    }

    fn emit_global_data(&self, data: &mut String, name: &str, ty: &Type) {
        let align = self.tt.align_of(ty).unwrap_or(1);
        let power = align.trailing_zeros();
        if power > 0 {
            data.push_str(&format!(".align {}\n", power));
        }
        data.push_str(&format!("{}:\n", name));
        let init = self.global_inits.get(name);
        let mut pos = 0usize;
        self.emit_global_object(data, ty, init.map(|n| n as &Node), name, &mut pos);
        // 一过界指针（&arr[N]）的别名落在对象末尾。
        self.emit_alias_labels(data, name, pos);
    }

    fn is_aggregate(ty: &Type) -> bool {
        matches!(ty.unqualified(), Type::Array(_, _) | Type::Struct(_))
    }

    /// 按 C 的初始化子对象规则消费一组初始化项。
    fn emit_global_sequence(
        &self,
        data: &mut String,
        ty: &Type,
        items: &[Node],
        cursor: &mut usize,
        target: &str,
        pos: &mut usize,
    ) {
        match ty.unqualified() {
            Type::Array(elem, Some(len)) => {
                if matches!(elem.unqualified(), Type::Char | Type::UChar) {
                    if let Some(Node::Str(value)) = items.get(*cursor).map(Node::inner) {
                        // 校验阶段已拦截非法转义；此处失败说明不变量被破坏，必须暴露
                        let mut bytes = crate::expr_eval::decode_string_escapes(value)
                            .expect("非法字符串转义应在代码生成校验阶段被拦截");
                        bytes.push(0);
                        let used = bytes.len().min(*len);
                        // 逐字节发射：别名标签可能指向字符串内部（"str"+k）。
                        for byte in &bytes[..used] {
                            self.emit_alias_labels(data, target, *pos);
                            data.push_str(&format!("    .byte {}\n", byte));
                            *pos += 1;
                        }
                        if used < *len {
                            self.emit_space(data, target, pos, len - used);
                        }
                        *cursor += 1;
                        return;
                    }
                }
                for _ in 0..*len {
                    if *cursor >= items.len() {
                        self.emit_global_object(data, elem, None, target, pos);
                    } else if matches!(items[*cursor].inner(), Node::InitList(_)) {
                        let item = &items[*cursor];
                        *cursor += 1;
                        self.emit_global_object(data, elem, Some(item), target, pos);
                    } else if Self::is_aggregate(elem) {
                        self.emit_global_sequence(data, elem, items, cursor, target, pos);
                    } else {
                        let item = &items[*cursor];
                        *cursor += 1;
                        self.emit_global_object(data, elem, Some(item), target, pos);
                    }
                }
            }
            Type::Struct(tag) => {
                if let Some(def) = self.tt.structs.get(tag) {
                    if def.is_union {
                        if let Some(first) = def.members.first() {
                            if *cursor >= items.len() {
                                self.emit_global_object(data, &first.ty, None, target, pos);
                            } else if matches!(items[*cursor].inner(), Node::InitList(_)) {
                                let item = &items[*cursor];
                                *cursor += 1;
                                self.emit_global_object(data, &first.ty, Some(item), target, pos);
                            } else if Self::is_aggregate(&first.ty) {
                                self.emit_global_sequence(
                                    data, &first.ty, items, cursor, target, pos,
                                );
                            } else {
                                let item = &items[*cursor];
                                *cursor += 1;
                                self.emit_global_object(data, &first.ty, Some(item), target, pos);
                            }
                            let used = self.tt.size_of(&first.ty).unwrap_or(0);
                            if def.size > used {
                                self.emit_space(data, target, pos, def.size - used);
                            }
                        } else if def.size > 0 {
                            self.emit_space(data, target, pos, def.size);
                        }
                    } else {
                        let mut layout_cursor = 0;
                        for member in &def.members {
                            if member.offset > layout_cursor {
                                self.emit_space(
                                    data,
                                    target,
                                    pos,
                                    member.offset - layout_cursor,
                                );
                            }
                            if *cursor >= items.len() {
                                self.emit_global_object(data, &member.ty, None, target, pos);
                            } else if matches!(items[*cursor].inner(), Node::InitList(_)) {
                                let item = &items[*cursor];
                                *cursor += 1;
                                self.emit_global_object(
                                    data,
                                    &member.ty,
                                    Some(item),
                                    target,
                                    pos,
                                );
                            } else if Self::is_aggregate(&member.ty) {
                                self.emit_global_sequence(
                                    data, &member.ty, items, cursor, target, pos,
                                );
                            } else {
                                let item = &items[*cursor];
                                *cursor += 1;
                                self.emit_global_object(
                                    data,
                                    &member.ty,
                                    Some(item),
                                    target,
                                    pos,
                                );
                            }
                            layout_cursor =
                                member.offset + self.tt.size_of(&member.ty).unwrap_or(0);
                        }
                        if def.size > layout_cursor {
                            self.emit_space(data, target, pos, def.size - layout_cursor);
                        }
                    }
                }
            }
            _ => {
                self.emit_alias_labels(data, target, *pos);
                if *cursor < items.len() {
                    let item = &items[*cursor];
                    *cursor += 1;
                    data.push_str(&format!(
                        "    {} {}\n",
                        Self::scalar_directive(ty),
                        self.constant_data_value(ty, Some(item))
                    ));
                } else {
                    data.push_str(&format!("    {} 0\n", Self::scalar_directive(ty)));
                }
                *pos += self.tt.size_of(ty).unwrap_or(4);
            }
        }
    }

    fn emit_global_object(
        &self,
        data: &mut String,
        ty: &Type,
        init: Option<&Node>,
        target: &str,
        pos: &mut usize,
    ) {
        if let Some(Node::Str(value)) = init.map(Node::inner) {
            if let Type::Array(element, Some(len)) = ty.unqualified() {
                if matches!(element.unqualified(), Type::Char | Type::UChar) {
                    // 校验阶段已拦截非法转义；此处失败说明不变量被破坏，必须暴露
                    let mut bytes = crate::expr_eval::decode_string_escapes(value)
                        .expect("非法字符串转义应在代码生成校验阶段被拦截");
                    bytes.push(0);
                    let used = bytes.len().min(*len);
                    // 逐字节发射：别名标签可能指向字符串内部（"str"+k）。
                    for byte in &bytes[..used] {
                        self.emit_alias_labels(data, target, *pos);
                        data.push_str(&format!("    .byte {}\n", byte));
                        *pos += 1;
                    }
                    if used < *len {
                        self.emit_space(data, target, pos, len - used);
                    }
                    return;
                }
            }
        }
        if let Some(Node::InitList(items)) = init.map(Node::inner) {
            let mut cursor = 0;
            self.emit_global_sequence(data, ty, items, &mut cursor, target, pos);
        } else {
            match ty.unqualified() {
                Type::Array(_, Some(_)) | Type::Struct(_) => {
                    self.emit_space(data, target, pos, self.tt.size_of(ty).unwrap_or(0));
                }
                _ => {
                    self.emit_alias_labels(data, target, *pos);
                    data.push_str(&format!(
                        "    {} {}\n",
                        Self::scalar_directive(ty),
                        self.constant_data_value(ty, init)
                    ));
                    *pos += self.tt.size_of(ty).unwrap_or(4);
                }
            }
        }
    }

    fn emit_local_initializer(&mut self, name: &str, ty: &Type, init: &Node) {
        self.emit_local_zero(name, ty, 0);
        self.emit_local_object(name, ty, init, 0);
    }

    fn emit_local_zero(&mut self, name: &str, ty: &Type, base: usize) {
        match ty.unqualified() {
            Type::Array(elem, Some(len)) => {
                for i in 0..*len {
                    self.emit_local_zero(name, elem, base + i * self.tt.size_of(elem).unwrap_or(4));
                }
            }
            Type::Struct(tag) => {
                if let Some(def) = self.tt.structs.get(tag).cloned() {
                    for m in def.members {
                        self.emit_local_zero(name, &m.ty, base + m.offset);
                    }
                }
            }
            Type::Float | Type::Double => {
                self.emit_local_leaf(name, ty, base, None);
            }
            _ => self.emit_local_leaf(name, ty, base, None),
        }
    }

    fn emit_local_sequence(
        &mut self,
        name: &str,
        ty: &Type,
        items: &[Node],
        cursor: &mut usize,
        base: usize,
    ) {
        match ty.unqualified() {
            Type::Array(elem, Some(len)) => {
                if matches!(elem.unqualified(), Type::Char | Type::UChar) {
                    if let Some(Node::Str(_)) = items.get(*cursor).map(Node::inner) {
                        let item = items[*cursor].clone();
                        *cursor += 1;
                        self.emit_local_object(name, ty, &item, base);
                        return;
                    }
                }
                let size = self.tt.size_of(elem).unwrap_or(4);
                for index in 0..*len {
                    if *cursor >= items.len() {
                        break;
                    }
                    let offset = base + index * size;
                    if matches!(items[*cursor].inner(), Node::InitList(_)) {
                        let item = items[*cursor].clone();
                        *cursor += 1;
                        self.emit_local_object(name, elem, &item, offset);
                    } else if Self::is_aggregate(elem) {
                        self.emit_local_sequence(name, elem, items, cursor, offset);
                    } else {
                        let item = items[*cursor].clone();
                        *cursor += 1;
                        self.emit_local_object(name, elem, &item, offset);
                    }
                }
            }
            Type::Struct(tag) => {
                if let Some(def) = self.tt.structs.get(tag).cloned() {
                    if def.is_union {
                        if let Some(member) = def.members.first() {
                            if *cursor < items.len() {
                                let offset = base + member.offset;
                                if matches!(items[*cursor].inner(), Node::InitList(_)) {
                                    let item = items[*cursor].clone();
                                    *cursor += 1;
                                    self.emit_local_object(name, &member.ty, &item, offset);
                                } else if Self::is_aggregate(&member.ty) {
                                    self.emit_local_sequence(
                                        name, &member.ty, items, cursor, offset,
                                    );
                                } else {
                                    let item = items[*cursor].clone();
                                    *cursor += 1;
                                    self.emit_local_object(name, &member.ty, &item, offset);
                                }
                            }
                        }
                    } else {
                        for member in def.members {
                            if *cursor >= items.len() {
                                break;
                            }
                            let offset = base + member.offset;
                            if matches!(items[*cursor].inner(), Node::InitList(_)) {
                                let item = items[*cursor].clone();
                                *cursor += 1;
                                self.emit_local_object(name, &member.ty, &item, offset);
                            } else if Self::is_aggregate(&member.ty) {
                                self.emit_local_sequence(name, &member.ty, items, cursor, offset);
                            } else {
                                let item = items[*cursor].clone();
                                *cursor += 1;
                                self.emit_local_object(name, &member.ty, &item, offset);
                            }
                        }
                    }
                }
            }
            _ => {
                if *cursor < items.len() {
                    let item = items[*cursor].clone();
                    *cursor += 1;
                    self.emit_local_object(name, ty, &item, base);
                }
            }
        }
    }

    fn emit_aggregate_copy(&mut self, destination: &str, source: &str, size: usize) {
        if size > Self::MAX_STRUCT_BYVAL {
            self.fail_codegen(format!(
                "聚合赋值对象过大({size}字节，上限{}字节)，请改用 memcpy/指针",
                Self::MAX_STRUCT_BYVAL
            ));
            return;
        }
        for offset in 0..size {
            // 偏移可能超出 ±2047：经 address_parts 生成 li+add 寻址
            let (rb, ro) = self.address_parts(source, offset as i32, "t4");
            self.emit(&format!("    lbu t1, {}({})", ro, rb));
            let (rb, ro) = self.address_parts(destination, offset as i32, "t4");
            self.emit(&format!("    sb t1, {}({})", ro, rb));
        }
    }

    fn emit_local_object(&mut self, name: &str, ty: &Type, init: &Node, base: usize) {
        match (ty.unqualified(), init.inner()) {
            (Type::Array(elem, Some(len)), Node::Str(value))
                if matches!(elem.unqualified(), Type::Char | Type::UChar) =>
            {
                let decoded = match crate::expr_eval::decode_string_escapes(value) {
                    Ok(bytes) => bytes,
                    Err(message) => {
                        self.fail_codegen(message);
                        Vec::new()
                    }
                };
                for (i, byte) in decoded
                    .iter()
                    .copied()
                    .chain(std::iter::once(0))
                    .take(*len)
                    .enumerate()
                {
                    self.emit_local_leaf(name, elem, base + i, Some(&Node::Num(byte as i64)));
                }
            }
            (Type::Array(_, Some(_)) | Type::Struct(_), Node::InitList(items)) => {
                let mut cursor = 0;
                self.emit_local_sequence(name, ty, items, &mut cursor, base);
            }
            (Type::Struct(_), _) => {
                self.gen_expr(init);
                // 源指针压栈保存：gen_addr 在槽偏移 >2047 时会用 t2 做寻址
                // scratch，直接 mv t2, a0 会在那时被破坏，导致从错地址拷贝。
                // 压 8 字节保持 sp 恒 8 对齐（双精度访存依赖该不变式）。
                self.emit("    addi sp, sp, -8");
                self.emit("    sw a0, 0(sp)");
                self.gen_addr(&Node::Ident(name.to_owned()));
                if base != 0 {
                    let (rb, ro) = self.address_parts("a0", base as i32, "t4");
                    self.emit(&format!("    addi a0, {}, {}", rb, ro));
                }
                self.emit("    mv t3, a0");
                self.emit("    lw t2, 0(sp)");
                self.emit("    addi sp, sp, 8");
                self.emit_aggregate_copy("t3", "t2", self.tt.size_of(ty).unwrap_or(0));
                self.emit("    mv a0, t3");
            }
            _ => self.emit_local_leaf(name, ty, base, Some(init)),
        }
    }

    fn emit_local_leaf(&mut self, name: &str, ty: &Type, offset: usize, value: Option<&Node>) {
        if let Some(value) = value {
            self.gen_expr(value);
        } else {
            self.emit("    li a0, 0");
        }
        let value_is_float = self
            .type_of(value.unwrap_or(&Node::Num(0)))
            .map(|t| Self::is_float_ty(&t))
            .unwrap_or(false);
        if Self::is_float_ty(ty) && !value_is_float {
            let source = self.type_of(value.unwrap_or(&Node::Num(0)));
            self.emit_int_to_double(source.as_ref());
        }
        if !Self::is_float_ty(ty) && value_is_float {
            if ty.is_unsigned_integer() {
                self.emit("    fcvt.wu.d a0, f10, rtz");
            } else {
                self.emit("    fcvt.w.d a0, f10, rtz");
            }
        }
        self.emit_integer_conversion(ty);
        self.emit("    addi sp, sp, -8");
        if Self::is_float_ty(ty) {
            self.emit("    fsd f10, 0(sp)");
        } else {
            self.emit("    sw a0, 0(sp)");
        }
        self.gen_addr(&Node::Ident(name.to_owned()));
        if offset != 0 {
            // 偏移可能超出 ±2047（大数组元素初始化）
            let (rb, ro) = self.address_parts("a0", offset as i32, "t4");
            self.emit(&format!("    addi a0, {}, {}", rb, ro));
        }
        self.emit("    mv t3, a0");
        if Self::is_float_ty(ty) {
            self.emit("    fld f10, 0(sp)");
        } else {
            self.emit("    lw a0, 0(sp)");
        }
        self.emit("    addi sp, sp, 8");
        self.store_to_address("t3", Some(ty));
    }

    /// 生成语句
    fn gen_stmt(&mut self, node: &Node) {
        match node {
            Node::Block(stmts) | Node::DeclBlock(stmts) => {
                for s in stmts {
                    self.gen_stmt(s);
                }
            }
            Node::Decl { specs, name, init } => {
                if specs.storage == Storage::Static {
                    return;
                }
                if let Some(init) = init {
                    let ty = self
                        .locals
                        .get(name)
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(Type::Int);
                    self.emit_local_initializer(name, &ty, init);
                }
            }
            Node::ExprStmt(e) => {
                self.gen_expr(e);
            }
            Node::NullStmt => {}
            Node::Return(e) => {
                if let Some(e) = e {
                    if Self::is_struct_ty(&self.current_ret) {
                        self.gen_expr(e);
                        self.emit("    mv t2, a0");
                        self.emit("    lw t3, 0(s0)");
                        let size = self.tt.size_of(&self.current_ret).unwrap_or(0);
                        if size > Self::MAX_STRUCT_BYVAL {
                            self.fail_codegen(format!(
                                "结构体按值返回过大({size}字节，上限{}字节)，请改用指针返回",
                                Self::MAX_STRUCT_BYVAL
                            ));
                            return;
                        }
                        for offset in 0..size {
                            // 大结构体：偏移超出 ±2047 时经 li+add 寻址
                            let (rb, ro) = self.address_parts("t2", offset as i32, "t4");
                            self.emit(&format!("    lbu t1, {}({})", ro, rb));
                            let (rb, ro) = self.address_parts("t3", offset as i32, "t4");
                            self.emit(&format!("    sb t1, {}({})", ro, rb));
                        }
                        self.emit("    mv a0, t3");
                    } else {
                        let source = self.type_of(e);
                        self.gen_expr(e);
                        let target = self.current_ret.clone();
                        if Self::is_float_ty(&target)
                            && !source.as_ref().map(Self::is_float_ty).unwrap_or(false)
                        {
                            self.emit_int_to_double(source.as_ref());
                        } else if !Self::is_float_ty(&target)
                            && source.as_ref().map(Self::is_float_ty).unwrap_or(false)
                        {
                            if target.is_unsigned_integer() {
                                self.emit("    fcvt.wu.d a0, f10, rtz");
                            } else {
                                self.emit("    fcvt.w.d a0, f10, rtz");
                            }
                        }
                        if matches!(target.unqualified(), Type::Float) {
                            // 返回值按单精度收敛（float 只有 24 位尾数）
                            self.emit("    fcvt.s.d f10, f10");
                            self.emit("    fcvt.d.s f10, f10");
                        }
                        self.emit_integer_conversion(&target);
                    }
                }
                self.emit(&format!("    j {}", self.ret_label));
            }
            Node::If(c, t, e) => {
                let else_l = self.label();
                let end_l = self.label();
                self.gen_expr(c);
                let condition_type = self.type_of(c);
                self.emit_truthy(condition_type.as_ref());
                self.emit(&format!("    beqz a0, {}", else_l));
                self.gen_stmt(t);
                self.emit(&format!("    j {}", end_l));
                self.emit(&format!("{}:", else_l));
                if let Some(e) = e {
                    self.gen_stmt(e);
                }
                self.emit(&format!("{}:", end_l));
            }
            Node::While(c, b) => {
                let begin = self.label();
                let end = self.label();
                self.break_labels.push(end.clone());
                self.continue_labels.push(begin.clone());
                self.emit(&format!("{}:", begin));
                self.gen_expr(c);
                let condition_type = self.type_of(c);
                self.emit_truthy(condition_type.as_ref());
                self.emit(&format!("    beqz a0, {}", end));
                self.gen_stmt(b);
                self.emit(&format!("    j {}", begin));
                self.emit(&format!("{}:", end));
                self.continue_labels.pop();
                self.break_labels.pop();
            }
            Node::DoWhile(b, c) => {
                let begin = self.label();
                let cond_label = self.label();
                let end = self.label();
                self.break_labels.push(end.clone());
                self.continue_labels.push(cond_label.clone());
                self.emit(&format!("{}:", begin));
                self.gen_stmt(b);
                self.emit(&format!("{}:", cond_label));
                self.gen_expr(c);
                let condition_type = self.type_of(c);
                self.emit_truthy(condition_type.as_ref());
                self.emit(&format!("    bnez a0, {}", begin));
                self.emit(&format!("{}:", end));
                self.continue_labels.pop();
                self.break_labels.pop();
            }
            Node::For(i, c, p, b) => {
                let begin = self.label();
                let post_label = self.label();
                let end = self.label();
                if let Some(i) = i {
                    self.gen_expr(i);
                }
                self.break_labels.push(end.clone());
                self.continue_labels.push(post_label.clone());
                self.emit(&format!("{}:", begin));
                if let Some(c) = c {
                    self.gen_expr(c);
                    let condition_type = self.type_of(c);
                    self.emit_truthy(condition_type.as_ref());
                    self.emit(&format!("    beqz a0, {}", end));
                }
                self.gen_stmt(b);
                self.emit(&format!("{}:", post_label));
                if let Some(p) = p {
                    self.gen_expr(p);
                }
                self.emit(&format!("    j {}", begin));
                self.emit(&format!("{}:", end));
                self.continue_labels.pop();
                self.break_labels.pop();
            }
            Node::Switch(expr, body) => {
                let end = self.label();
                let mut nodes = Vec::new();
                Self::collect_switch_labels(body, &mut nodes);
                let mut labels = HashMap::new();
                let mut cases = Vec::new();
                let mut default_label = None;
                for node in nodes {
                    let label = self.label();
                    labels.insert(Self::node_id(node), label.clone());
                    match node {
                        Node::Case(value, _) => {
                            if let Some(value) = eval_integer_constant(value) {
                                cases.push((value, label));
                            }
                        }
                        Node::Default(_) => default_label = Some(label),
                        _ => {}
                    }
                }
                self.gen_expr(expr);
                for (value, label) in cases {
                    self.emit(&format!("    li t0, {}", value));
                    self.emit(&format!("    beq a0, t0, {}", label));
                }
                self.emit(&format!(
                    "    j {}",
                    default_label.as_deref().unwrap_or(&end)
                ));
                self.break_labels.push(end.clone());
                self.switch_labels.push(labels);
                self.gen_stmt(body);
                self.switch_labels.pop();
                self.break_labels.pop();
                self.emit(&format!("{}:", end));
            }
            node @ Node::Case(_, body) | node @ Node::Default(body) => {
                let id = Self::node_id(node);
                if let Some(label) = self
                    .switch_labels
                    .last()
                    .and_then(|map| map.get(&id))
                    .cloned()
                {
                    self.emit(&format!("{}:", label));
                }
                self.gen_stmt(body);
            }
            Node::Break => {
                if let Some(label) = self.break_labels.last() {
                    self.emit(&format!("    j {}", label));
                }
            }
            Node::Continue => {
                if let Some(label) = self.continue_labels.last() {
                    self.emit(&format!("    j {}", label));
                }
            }
            Node::Goto(name) => {
                let label = self.user_label(name);
                self.emit(&format!("    j {}", label));
            }
            Node::Label(name, body) => {
                let label = self.user_label(name);
                self.emit(&format!("{}:", label));
                self.gen_stmt(body);
            }
            other => {
                self.fail_codegen(format!("代码生成器无法处理语句节点: {:?}", other));
            }
        }
    }

    /// 浮点类型判断（float/double）
    fn is_float_ty(t: &Type) -> bool {
        matches!(t.unqualified(), Type::Float | Type::Double)
    }

    fn is_struct_ty(t: &Type) -> bool {
        matches!(t.unqualified(), Type::Struct(_))
    }

    fn is_array_or_struct_ty(t: &Type) -> bool {
        matches!(t.unqualified(), Type::Array(_, _) | Type::Struct(_))
    }

    fn arithmetic_is_unsigned(left: Option<&Type>, right: Option<&Type>) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => Type::usual_arithmetic(left, right)
                .map(|ty| ty.is_unsigned_integer())
                .unwrap_or(false),
            (Some(ty), None) | (None, Some(ty)) => ty.integer_promotion().is_unsigned_integer(),
            _ => false,
        }
    }

    fn emit_int_to_double(&mut self, ty: Option<&Type>) {
        if ty.map(Type::is_unsigned_integer).unwrap_or(false) {
            self.emit("    fcvt.d.wu f10, a0");
        } else {
            self.emit("    fcvt.d.w f10, a0");
        }
    }

    fn emit_integer_conversion(&mut self, ty: &Type) {
        match ty.unqualified() {
            Type::Char => {
                self.emit("    slli a0, a0, 24");
                self.emit("    srai a0, a0, 24");
            }
            Type::UChar => self.emit("    andi a0, a0, 255"),
            Type::Short => {
                self.emit("    slli a0, a0, 16");
                self.emit("    srai a0, a0, 16");
            }
            Type::UShort => {
                self.emit("    slli a0, a0, 16");
                self.emit("    srli a0, a0, 16");
            }
            _ => {}
        }
    }

    /// 表达式类型推断（指针算术用，简化版）
    fn type_of(&self, node: &Node) -> Option<Type> {
        match node {
            Node::Located(inner, _) => self.type_of(inner),
            Node::Num(_) => Some(Type::Int),
            Node::FNum(_) => Some(Type::Double),
            Node::Str(_) => Some(Type::Ptr(Box::new(Type::Char))),
            Node::Ident(name) => self
                .locals
                .get(name)
                .map(|(_, ty)| ty.clone())
                .or_else(|| self.current_statics.get(name).map(|(_, ty)| ty.clone()))
                .or_else(|| self.globals.get(name).cloned())
                .or_else(|| {
                    self.func_info.get(name).map(|(ret, params, is_variadic)| {
                        Type::Ptr(Box::new(Type::Function {
                            ret: Box::new(ret.clone()),
                            params: params.clone(),
                            is_variadic: *is_variadic,
                            has_prototype: true,
                        }))
                    })
                }),
            Node::Unary(UnaryOp::Addr, expr) => {
                self.type_of(expr).map(|ty| Type::Ptr(Box::new(ty)))
            }
            Node::Unary(UnaryOp::Deref, expr) => match self.type_of(expr) {
                Some(ty) => match ty.unqualified() {
                    // *数组名：数组先退化为指针，再按元素类型取值（C89 §3.3.3.2）
                    Type::Ptr(inner) | Type::Array(inner, _) => Some((**inner).clone()),
                    _ => None,
                },
                _ => None,
            },
            Node::Unary(UnaryOp::Not, _) => Some(Type::Int),
            Node::Unary(UnaryOp::BitNot, expr) => {
                self.type_of(expr).map(|ty| ty.integer_promotion())
            }
            Node::Unary(_, expr) => self.type_of(expr),
            Node::Subscript(array, _) => match self.type_of(array) {
                Some(ty) => match ty.unqualified() {
                    Type::Array(elem, _) | Type::Ptr(elem) => Some((**elem).clone()),
                    _ => None,
                },
                _ => None,
            },
            Node::Member(object, name, is_arrow) => {
                let object_type = self.type_of(object);
                let tag = if *is_arrow {
                    object_type.and_then(|ty| match ty.unqualified() {
                        Type::Ptr(inner) => match inner.unqualified() {
                            Type::Struct(tag) => Some(tag.clone()),
                            _ => None,
                        },
                        _ => None,
                    })
                } else {
                    object_type.and_then(|ty| match ty.unqualified() {
                        Type::Struct(tag) => Some(tag.clone()),
                        _ => None,
                    })
                };
                tag.and_then(|tag| {
                    self.tt
                        .structs
                        .get(&tag)
                        .and_then(|st| st.members.iter().find(|m| m.name == *name))
                        .map(|m| m.ty.clone())
                })
            }
            Node::Binary(op, left, right) => {
                if matches!(
                    op,
                    BinaryOp::LogAnd
                        | BinaryOp::LogOr
                        | BinaryOp::Lt
                        | BinaryOp::Gt
                        | BinaryOp::Le
                        | BinaryOp::Ge
                        | BinaryOp::Eq
                        | BinaryOp::Ne
                ) {
                    return Some(Type::Int);
                }
                let left_type = self.type_of(left);
                let right_type = self.type_of(right);
                let left_value = left_type.as_ref().map(Type::without_top_qualifiers);
                let right_value = right_type.as_ref().map(Type::without_top_qualifiers);
                match op {
                    BinaryOp::Add => match (left_value, right_value) {
                        (Some(Type::Ptr(inner)), _) | (Some(Type::Array(inner, _)), _) => {
                            Some(Type::Ptr(inner))
                        }
                        (_, Some(Type::Ptr(inner))) => Some(Type::Ptr(inner)),
                        (Some(a), Some(b)) => Type::usual_arithmetic(&a, &b),
                        _ => Some(Type::Int),
                    },
                    BinaryOp::Sub => match (left_value, right_value) {
                        (Some(Type::Ptr(_)), Some(Type::Ptr(_))) => Some(Type::Int),
                        (Some(Type::Ptr(inner)), _) | (Some(Type::Array(inner, _)), _) => {
                            Some(Type::Ptr(inner))
                        }
                        (Some(a), Some(b)) => Type::usual_arithmetic(&a, &b),
                        _ => Some(Type::Int),
                    },
                    BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::BitAnd
                    | BinaryOp::BitXor
                    | BinaryOp::BitOr => match (left_value, right_value) {
                        (Some(a), Some(b)) => Type::usual_arithmetic(&a, &b),
                        _ => Some(Type::Int),
                    },
                    BinaryOp::Shl | BinaryOp::Shr => left_value
                        .map(|ty| ty.integer_promotion())
                        .or(Some(Type::Int)),
                    _ => Some(Type::Int),
                }
            }
            Node::Assign(_, lhs, _) => self.type_of(lhs),
            Node::Cond(_, yes, no) => {
                let yes_type = self.type_of(yes);
                let no_type = self.type_of(no);
                if yes_type == no_type {
                    yes_type
                } else {
                    match (yes_type, no_type) {
                        (Some(a), Some(b)) if a.is_pointer() && b.is_integer() => {
                            Some(a.without_top_qualifiers())
                        }
                        (Some(a), Some(b)) if b.is_pointer() && a.is_integer() => {
                            Some(b.without_top_qualifiers())
                        }
                        (Some(a), Some(b)) => Type::usual_arithmetic(&a, &b),
                        _ => Some(Type::Int),
                    }
                }
            }
            Node::Comma(_, right) => self.type_of(right),
            Node::SizeofExpr(_) => Some(Type::Int),
            Node::Cast(ty, _) => Some(ty.clone()),
            Node::Call(function, args) => match function.inner() {
                Node::Ident(name) if name == "va_arg" => {
                    if matches!(args.get(1).map(Node::inner), Some(Node::Ident(t)) if t == "double")
                    {
                        Some(Type::Double)
                    } else {
                        Some(Type::Int)
                    }
                }
                Node::Ident(name) if self.func_ret.contains_key(name) => {
                    self.func_ret.get(name).cloned()
                }
                _ => match self.type_of(function) {
                    Some(ty) => match ty.unqualified() {
                        Type::Ptr(inner) => match inner.unqualified() {
                            Type::Function { ret, .. } => Some((**ret).clone()),
                            _ => None,
                        },
                        Type::Function { ret, .. } => Some((**ret).clone()),
                        _ => None,
                    },
                    _ => None,
                },
            },
            _ => None,
        }
    }

    /// 生成左值地址（a0 = 地址；数组/下标/解引用用）
    fn gen_addr(&mut self, node: &Node) {
        match node {
            Node::Ident(name) => {
                if let Some((off, _)) = self.locals.get(name) {
                    let off = *off;
                    if (-2048..=2047).contains(&off) {
                        self.emit(&format!("    addi a0, s0, {}", off));
                    } else {
                        self.emit(&format!("    li t2, {}", off));
                        self.emit("    add a0, s0, t2");
                    }
                } else if let Some((label, _)) = self.current_statics.get(name) {
                    self.emit(&format!("    la a0, {}", label));
                } else {
                    self.emit(&format!("    la a0, {}", name));
                }
            }
            Node::Subscript(arr, idx) => {
                // &arr[i] = arr + i × size
                self.gen_expr(idx);
                // 元素大小
                let sz = self.subscript_elem_size(arr);
                if sz != 1 {
                    self.emit(&format!("    li t0, {}", sz));
                    self.emit("    mul a0, a0, t0");
                }
                self.emit("    addi sp, sp, -8");
                self.emit("    sw a0, 0(sp)");
                if self
                    .type_of(arr)
                    .as_ref()
                    .map(|ty| matches!(ty.unqualified(), Type::Array(_, _)))
                    .unwrap_or(false)
                {
                    self.gen_addr(arr);
                } else {
                    self.gen_expr(arr);
                }
                self.emit("    lw t1, 0(sp)");
                self.emit("    addi sp, sp, 8");
                self.emit("    add a0, a0, t1");
            }
            Node::Unary(UnaryOp::Deref, expr) => {
                self.gen_expr(expr);
            }
            Node::Member(obj, name, is_arrow) => {
                if *is_arrow {
                    // ptr->x：obj 是指针值
                    self.gen_expr(obj);
                } else if self
                    .type_of(obj)
                    .as_ref()
                    .map(Type::is_struct)
                    .unwrap_or(false)
                    && matches!(obj.inner(), Node::Call(_, _))
                {
                    // 返回 struct 的调用表达式本身产出结果对象地址。
                    self.gen_expr(obj);
                } else {
                    // obj.x：obj 是 struct 变量地址
                    self.gen_addr(obj);
                }
                let off = self.member_offset(obj, name);
                if off != 0 {
                    // 大结构体成员偏移可能超出 ±2047
                    let (rb, ro) = self.address_parts("a0", off as i32, "t4");
                    self.emit(&format!("    addi a0, {}, {}", rb, ro));
                }
            }
            _ => {
                self.gen_expr(node);
            }
        }
    }

    /// 成员偏移（从 struct 类型表查）
    fn member_offset(&self, obj: &Node, name: &str) -> usize {
        // 支持 struct 变量（.）和 struct 指针（->）
        let ty = match self.type_of(obj) {
            Some(ty) => match ty.unqualified() {
                Type::Struct(tag) => Some(tag.clone()),
                Type::Ptr(inner) => match inner.unqualified() {
                    Type::Struct(tag) => Some(tag.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        if let Some(tag) = ty {
            if let Some(s) = self.tt.structs.get(&tag) {
                if let Some(off) = s.member_offset(name) {
                    return off;
                }
            }
        }
        0
    }

    /// 位域成员信息：(位偏移, 宽度, 是否带符号)
    fn member_bitfield(&self, obj: &Node, name: &str) -> Option<(usize, usize, bool)> {
        let ty = match self.type_of(obj) {
            Some(ty) => match ty.unqualified() {
                Type::Struct(tag) => Some(tag.clone()),
                Type::Ptr(inner) => match inner.unqualified() {
                    Type::Struct(tag) => Some(tag.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let tag = ty?;
        let member = self
            .tt
            .structs
            .get(&tag)?
            .members
            .iter()
            .find(|m| m.name == name)?;
        let width = member.bit_width?;
        Some((
            member.bit_offset,
            width,
            matches!(member.ty.unqualified(), Type::Int),
        ))
    }

    fn node_is_bitfield(&self, node: &Node) -> bool {
        matches!(node.inner(), Node::Member(obj, name, _) if self.member_bitfield(obj, name).is_some())
    }

    /// a0 = 位域单元地址 → a0 = 位域值（按符号解释）
    fn emit_bitfield_load(&mut self, bit_offset: usize, width: usize, signed: bool) {
        self.emit("    lw t1, 0(a0)");
        if bit_offset != 0 {
            self.emit(&format!("    srli a0, t1, {}", bit_offset));
        } else {
            self.emit("    mv a0, t1");
        }
        if width < 32 {
            let mask = (1u64 << width) - 1;
            self.emit(&format!("    li t2, {}", mask));
            self.emit("    and a0, a0, t2");
            if signed {
                self.emit(&format!("    slli a0, a0, {}", 32 - width));
                self.emit(&format!("    srai a0, a0, {}", 32 - width));
            }
        }
    }

    /// t3 = 位域单元地址，a0 = 值 → 读改写写入位域
    fn emit_bitfield_store(&mut self, bit_offset: usize, width: usize) {
        self.emit("    lw t1, 0(t3)");
        let mask = if width >= 32 {
            0xFFFF_FFFFu64
        } else {
            (1u64 << width) - 1
        };
        let field_mask = mask << bit_offset;
        self.emit(&format!("    li t2, {}", field_mask));
        self.emit("    xori t2, t2, -1");
        self.emit("    and t1, t1, t2");
        self.emit(&format!("    li t2, {}", field_mask));
        if bit_offset != 0 {
            self.emit(&format!("    slli a0, a0, {}", bit_offset));
        }
        self.emit("    and a0, a0, t2");
        self.emit("    or a0, a0, t1");
        self.emit("    sw a0, 0(t3)");
    }

    /// 下标元素大小（从数组类型推断）
    fn subscript_elem_size(&self, arr: &Node) -> usize {
        match self.type_of(arr) {
            Some(ty) => match ty.unqualified() {
                Type::Array(elem, _) => self.tt.size_of(elem).unwrap_or(4),
                Type::Ptr(inner) => self.tt.size_of(inner).unwrap_or(4),
                _ => 4,
            },
            _ => 4,
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.locals.contains_key(name)
    }

    /// 局部变量寻址：加载到 a0
    fn gen_load_local(&mut self, name: &str) {
        if let Some(off) = self.locals.get(name) {
            let (rb, ro) = self.address_parts("s0", off.0, "t2");
            self.emit(&format!("    lw a0, {}({})", ro, rb));
        } else {
            self.gen_addr(&Node::Ident(name.to_owned()));
            self.emit("    lw a0, 0(a0)");
        }
    }

    /// 以 base 寄存器 + 偏移寻址。偏移超出 I/S 型 12 位立即数编码范围
    /// （±2047）时，先用 li + add 把地址算进 scratch 临时寄存器（默认 t2），
    /// 返回 (最终基址寄存器, 最终偏移)。scratch 在调用点不得持有活值。
    fn address_parts(&mut self, base: &str, offset: i32, scratch: &str) -> (String, i32) {
        if (-2048..=2047).contains(&offset) {
            (base.to_string(), offset)
        } else {
            self.emit(&format!("    li {}, {}", scratch, offset));
            self.emit(&format!("    add {}, {}, {}", scratch, base, scratch));
            (scratch.to_string(), 0)
        }
    }
    fn emit_truthy(&mut self, ty: Option<&Type>) {
        if ty.map(Self::is_float_ty).unwrap_or(false) {
            self.emit("    fcvt.d.w f11, zero");
            self.emit("    feq.d a0, f10, f11");
            self.emit("    xori a0, a0, 1");
        } else {
            self.emit("    snez a0, a0");
        }
    }

    fn load_from_address(&mut self, ty: Option<&Type>) {
        match ty.map(Type::unqualified) {
            Some(Type::Double) => self.emit("    fld f10, 0(a0)"),
            Some(Type::Float) => {
                self.emit("    flw f11, 0(a0)");
                self.emit("    fcvt.d.s f10, f11");
            }
            Some(Type::Char) => self.emit("    lb a0, 0(a0)"),
            Some(Type::UChar) => self.emit("    lbu a0, 0(a0)"),
            Some(Type::Short) => self.emit("    lh a0, 0(a0)"),
            Some(Type::UShort) => self.emit("    lhu a0, 0(a0)"),
            // 数组和结构体表达式以对象地址表示；下标结果仍为数组时需要
            // 继续退化为指针，不能把对象开头误当成 4 字节标量加载。
            // 函数设计符同理：*fp ≡ fp（C89 §3.3.3.2），a0 中已是函数地址，
            // 再按标量加载会把函数首址当数据读（文本段不在 mem 中恒得 0）。
            Some(Type::Array(_, _)) | Some(Type::Struct(_)) | Some(Type::Function { .. }) => {}
            _ => self.emit("    lw a0, 0(a0)"),
        }
    }

    fn store_to_address(&mut self, address: &str, ty: Option<&Type>) {
        match ty.map(Type::unqualified) {
            Some(Type::Double) => self.emit(&format!("    fsd f10, 0({})", address)),
            Some(Type::Float) => {
                self.emit("    fcvt.s.d f11, f10");
                self.emit(&format!("    fsw f11, 0({})", address));
            }
            Some(Type::Char) | Some(Type::UChar) => {
                self.emit(&format!("    sb a0, 0({})", address))
            }
            Some(Type::Short) | Some(Type::UShort) => {
                self.emit(&format!("    sh a0, 0({})", address))
            }
            _ => self.emit(&format!("    sw a0, 0({})", address)),
        }
    }

    /// 生成表达式（结果在 a0；浮点结果在 f10）
    fn gen_expr(&mut self, node: &Node) {
        match node {
            Node::Num(v) => {
                self.emit(&format!("    li a0, {}", v));
            }
            Node::FNum(v) => {
                // 浮点字面量：fld 到 f10（64 位 double）。若遍历器遗漏了该字面量
                // （floats 未登记），返回结构化 codegen 错误而不是越界 panic。
                if let Some((label, _)) = self
                    .floats
                    .iter()
                    .find(|(_, val)| val.to_bits() == v.to_bits())
                {
                    let label = label.clone();
                    self.emit(&format!("    la a0, {}", label));
                    self.emit("    fld f10, 0(a0)");
                } else {
                    self.fail_codegen(format!("浮点字面量未登记: {}", v));
                    self.emit("    fcvt.d.w f10, zero");
                }
            }
            Node::Cast(ty, e) => {
                // 类型转换（§3.3.4）
                let src = self.type_of(e).unwrap_or(Type::Int);
                let src_is_float = Self::is_float_ty(&src);
                self.gen_expr(e);
                match (src, ty) {
                    (_, t @ (Type::Char | Type::UChar | Type::Short | Type::UShort)) => {
                        // 浮点 src 时值在 f10，须先转整数到 a0，再按目标宽度窄化；
                        // 否则 a0 中是无关垃圾位，窄化得到错误结果。
                        if src_is_float {
                            if matches!(t.unqualified(), Type::UChar | Type::UShort) {
                                self.emit("    fcvt.wu.d a0, f10, rtz");
                            } else {
                                self.emit("    fcvt.w.d a0, f10, rtz");
                            }
                        }
                        self.emit_integer_conversion(t);
                    }
                    (s, t) if Self::is_float_ty(t) && !Self::is_float_ty(&s) => {
                        if matches!(t.unqualified(), Type::Float) {
                            // 单精度转换：先按 float 舍入（24 位尾数），再扩回内部
                            // double 表示；(int)(float)16777217 必须得 16777216。
                            if s.is_unsigned_integer() {
                                self.emit("    fcvt.s.wu f11, a0");
                            } else {
                                self.emit("    fcvt.s.w f11, a0");
                            }
                            self.emit("    fcvt.d.s f10, f11");
                        } else if s.is_unsigned_integer() {
                            self.emit("    fcvt.d.wu f10, a0");
                        } else {
                            self.emit("    fcvt.d.w f10, a0");
                        }
                    }
                    (s, t) if matches!(t, Type::Int | Type::Long) && Self::is_float_ty(&s) => {
                        self.emit("    fcvt.w.d a0, f10, rtz");
                    }
                    (s, t) if matches!(t, Type::UInt | Type::ULong) && Self::is_float_ty(&s) => {
                        self.emit("    fcvt.wu.d a0, f10, rtz");
                    }
                    // 以下两臂只在 src 为浮点时到达（非浮点 src 已被上一臂拦截）。
                    // C 语义：float 参与运算前提升回 double → 窄化到单精度再扩回。
                    (_, Type::Float) => {
                        self.emit("    fcvt.s.d f10, f10");
                        self.emit("    fcvt.d.s f10, f10");
                    }
                    (_, Type::Double) => {}
                    _ => {}
                }
            }
            Node::Str(s) => {
                // 查字符串字面量标签（collect_data 时登记）。即使遍历器将来再次
                // 漏掉某种 AST 路径，也应返回结构化 codegen 错误，而不是越界 panic。
                if let Some((label, _)) = self.strings.iter().find(|(_, content)| content == s) {
                    let label = label.clone();
                    self.emit(&format!("    la a0, {}", label));
                } else {
                    self.fail_codegen(format!("字符串字面量未登记: {:?}", s));
                    self.emit("    li a0, 0");
                }
            }
            Node::Ident(name) => {
                // 数组/函数名退化为地址；浮点变量 → f10；普通变量加载值到 a0。
                // 变量优先于同名函数（局部变量可遮蔽全局函数名，如 frexp 的形参 exp）。
                let is_object = self.locals.contains_key(name)
                    || self.current_statics.contains_key(name)
                    || self.globals.contains_key(name);
                if self.func_info.contains_key(name) && !is_object {
                    self.emit(&format!("    la a0, {}", name));
                    return;
                }
                let ty = self.type_of(&Node::Ident(name.clone()));
                if ty
                    .as_ref()
                    .map(Self::is_array_or_struct_ty)
                    .unwrap_or(false)
                {
                    self.gen_addr(&Node::Ident(name.clone()));
                } else if ty
                    .as_ref()
                    .map(|t| matches!(t.unqualified(), Type::Float))
                    .unwrap_or(false)
                {
                    // float 变量：flw 低 32 位 → fcvt.s.d 转 double
                    let off = self.locals.get(name).map(|x| x.0).unwrap_or(0);
                    if self.is_local(name) {
                        let (rb, ro) = self.address_parts("s0", off, "t2");
                        self.emit(&format!("    flw f11, {}({})", ro, rb));
                    } else {
                        self.gen_addr(&Node::Ident(name.clone()));
                        self.emit("    flw f11, 0(a0)");
                    }
                    self.emit("    fcvt.d.s f10, f11");
                } else if ty
                    .as_ref()
                    .map(|t| matches!(t.unqualified(), Type::Double))
                    .unwrap_or(false)
                {
                    let off = self.locals.get(name).map(|x| x.0).unwrap_or(0);
                    if self.is_local(name) {
                        let (rb, ro) = self.address_parts("s0", off, "t2");
                        self.emit(&format!("    fld f10, {}({})", ro, rb));
                    } else {
                        self.gen_addr(&Node::Ident(name.clone()));
                        self.emit("    fld f10, 0(a0)");
                    }
                } else {
                    match ty.as_ref().map(Type::unqualified) {
                        Some(Type::Char) => {
                            self.gen_addr(&Node::Ident(name.clone()));
                            self.emit("    lb a0, 0(a0)");
                        }
                        Some(Type::UChar) => {
                            self.gen_addr(&Node::Ident(name.clone()));
                            self.emit("    lbu a0, 0(a0)");
                        }
                        Some(Type::Short) => {
                            self.gen_addr(&Node::Ident(name.clone()));
                            self.emit("    lh a0, 0(a0)");
                        }
                        Some(Type::UShort) => {
                            self.gen_addr(&Node::Ident(name.clone()));
                            self.emit("    lhu a0, 0(a0)");
                        }
                        _ => self.gen_load_local(name),
                    }
                }
            }
            Node::Assign(op, lhs, rhs) => {
                let lhs_type = self.type_of(lhs);
                let lhs_float = lhs_type.as_ref().map(Self::is_float_ty).unwrap_or(false);
                // setjmp 赋值：调用后重新计算左值地址（longjmp 跳回时栈槽不可靠）
                if *op == AssignOp::Plain
                    && matches!(lhs.inner(), Node::Ident(_))
                    && matches!(rhs.inner(), Node::Call(f, _) if matches!(f.inner(), Node::Ident(n) if n == "setjmp"))
                {
                    self.gen_expr(rhs);
                    self.emit("    mv t1, a0");
                    self.gen_addr(lhs);
                    self.emit("    mv t3, a0");
                    self.emit("    mv a0, t1");
                    if let Some(ty) = lhs_type.as_ref() {
                        self.emit_integer_conversion(ty);
                    }
                    self.store_to_address("t3", lhs_type.as_ref());
                    return;
                }
                if let Some((bbit, bwidth, bsigned)) = match lhs.inner() {
                    Node::Member(obj, name, _) => self.member_bitfield(obj, name),
                    _ => None,
                } {
                    // 位域赋值：单元地址入栈，RHS 求值后读改写
                    self.gen_addr(lhs);
                    self.emit("    addi sp, sp, -8");
                    self.emit("    sw a0, 0(sp)");
                    self.gen_expr(rhs);
                    // 位域基底类型必为整型：浮点 RHS 需先转换到 a0，
                    // 否则 store 会把求值后残留的旧 a0（如浮点常量地址）写进字段
                    if self
                        .type_of(rhs)
                        .map(|ty| Self::is_float_ty(&ty))
                        .unwrap_or(false)
                    {
                        let unsigned = lhs_type
                            .as_ref()
                            .map(Type::is_unsigned_integer)
                            .unwrap_or(false);
                        if unsigned {
                            self.emit("    fcvt.wu.d a0, f10, rtz");
                        } else {
                            self.emit("    fcvt.w.d a0, f10, rtz");
                        }
                    }
                    if *op != AssignOp::Plain {
                        self.emit("    addi sp, sp, -8");
                        self.emit("    sw a0, 0(sp)");
                        self.emit("    lw a0, 8(sp)");
                        self.emit_bitfield_load(bbit, bwidth, bsigned);
                        self.emit("    lw t1, 0(sp)");
                        self.emit("    addi sp, sp, 8");
                        let unsigned = lhs_type
                            .as_ref()
                            .map(Type::is_unsigned_integer)
                            .unwrap_or(false);
                        self.emit_binop_assign(*op, unsigned);
                    }
                    self.emit("    lw t3, 0(sp)");
                    self.emit("    addi sp, sp, 8");
                    if let Some(ty) = lhs_type.as_ref() {
                        self.emit_integer_conversion(ty);
                    }
                    self.emit_bitfield_store(bbit, bwidth);
                    // 赋值表达式的值是赋值后的字段值：store 后 a0 是整字单元，
                    // 先移出字段位并掩码，再按字段有符号性还原
                    if bbit != 0 {
                        self.emit(&format!("    srli a0, a0, {}", bbit));
                    }
                    if bwidth < 32 {
                        let mask = (1u64 << bwidth) - 1;
                        self.emit(&format!("    li t0, {}", mask));
                        self.emit("    and a0, a0, t0");
                        if bsigned {
                            self.emit(&format!("    slli a0, a0, {}", 32 - bwidth));
                            self.emit(&format!("    srai a0, a0, {}", 32 - bwidth));
                        }
                    }
                } else {
                    self.gen_addr(lhs);
                    self.emit("    addi sp, sp, -8");
                    self.emit("    sw a0, 0(sp)");
                    self.gen_expr(rhs);
                    let rhs_float = self
                        .type_of(rhs)
                        .map(|ty| Self::is_float_ty(&ty))
                        .unwrap_or(false);
                    if lhs_float && !rhs_float {
                        let rhs_type = self.type_of(rhs);
                        self.emit_int_to_double(rhs_type.as_ref());
                    } else if !lhs_float && rhs_float {
                        if lhs_type
                            .as_ref()
                            .map(Type::is_unsigned_integer)
                            .unwrap_or(false)
                        {
                            self.emit("    fcvt.wu.d a0, f10, rtz");
                        } else {
                            self.emit("    fcvt.w.d a0, f10, rtz");
                        }
                    }
                    self.emit("    lw t3, 0(sp)");
                    self.emit("    addi sp, sp, 8");
                    if *op == AssignOp::Plain
                        && lhs_type.as_ref().map(Self::is_struct_ty).unwrap_or(false)
                    {
                        self.emit("    mv t2, a0");
                        let size = lhs_type
                            .as_ref()
                            .and_then(|ty| self.tt.size_of(ty).ok())
                            .unwrap_or(0);
                        self.emit_aggregate_copy("t3", "t2", size);
                        self.emit("    mv a0, t3");
                    } else if *op == AssignOp::Plain {
                        if let Some(ty) = lhs_type.as_ref() {
                            self.emit_integer_conversion(ty);
                        }
                        self.store_to_address("t3", lhs_type.as_ref());
                        // C89 §3.3.16：赋值表达式的值等于转换到左值类型后的值。
                        // store 只窄化存入内存的副本，f10 仍是未舍入的 double，
                        // 这里把 f10 收敛到 (double)(float)f10。
                        if lhs_type
                            .as_ref()
                            .map(|ty| matches!(ty.unqualified(), Type::Float))
                            .unwrap_or(false)
                        {
                            self.emit("    fcvt.s.d f10, f10");
                            self.emit("    fcvt.d.s f10, f10");
                        }
                    } else if lhs_float {
                        self.emit("    fsgnj.d f12, f10, f10");
                        self.emit("    mv a0, t3");
                        self.load_from_address(lhs_type.as_ref());
                        match op {
                            AssignOp::Add => self.emit("    fadd.d f10, f10, f12"),
                            AssignOp::Sub => self.emit("    fsub.d f10, f10, f12"),
                            AssignOp::Mul => self.emit("    fmul.d f10, f10, f12"),
                            AssignOp::Div => self.emit("    fdiv.d f10, f10, f12"),
                            _ => self.emit("    fadd.d f10, f10, f12"),
                        }
                        self.store_to_address("t3", lhs_type.as_ref());
                        // 复合赋值的表达式值同样收敛到 float（同普通赋值）
                        if lhs_type
                            .as_ref()
                            .map(|ty| matches!(ty.unqualified(), Type::Float))
                            .unwrap_or(false)
                        {
                            self.emit("    fcvt.s.d f10, f10");
                            self.emit("    fcvt.d.s f10, f10");
                        }
                    } else {
                        self.emit("    mv t2, a0");
                        self.emit("    mv a0, t3");
                        self.load_from_address(lhs_type.as_ref());
                        self.emit("    mv t1, t2");
                        // 指针 ± 整数：整数按元素大小缩放；指针 - 指针（C89 §3.3.6，
                        // 结果为元素差）不缩放，sub 后按元素大小除（与 Binary 路径一致）。
                        let rhs_type = self.type_of(rhs);
                        let lhs_ptr = Self::is_pointer_like(&lhs_type);
                        let rhs_ptr = Self::is_pointer_like(&rhs_type);
                        if matches!(op, AssignOp::Add | AssignOp::Sub) && !rhs_ptr {
                            if let Some(size) = self.pointer_elem_size(lhs_type.as_ref()) {
                                if size != 1 {
                                    self.emit(&format!("    li t0, {}", size));
                                    self.emit("    mul t1, t1, t0");
                                }
                            }
                        }
                        let unsigned = lhs_type
                            .as_ref()
                            .map(Type::is_unsigned_integer)
                            .unwrap_or(false);
                        self.emit_binop_assign(*op, unsigned);
                        if *op == AssignOp::Sub && lhs_ptr && rhs_ptr {
                            let size = self.pointer_elem_size(lhs_type.as_ref()).unwrap_or(1);
                            if size != 1 {
                                self.emit(&format!("    li t0, {}", size));
                                self.emit("    div a0, a0, t0");
                            }
                        }
                        if let Some(ty) = lhs_type.as_ref() {
                            self.emit_integer_conversion(ty);
                        }
                        self.store_to_address("t3", lhs_type.as_ref());
                    }
                }
            }
            Node::Binary(op, l, r) => {
                if matches!(op, BinaryOp::LogAnd | BinaryOp::LogOr) {
                    let short_label = self.label();
                    let end_label = self.label();
                    let left_type = self.type_of(l);
                    self.gen_expr(l);
                    self.emit_truthy(left_type.as_ref());
                    match op {
                        BinaryOp::LogAnd => self.emit(&format!("    beqz a0, {}", short_label)),
                        BinaryOp::LogOr => self.emit(&format!("    bnez a0, {}", short_label)),
                        _ => unreachable!(),
                    }
                    let right_type = self.type_of(r);
                    self.gen_expr(r);
                    self.emit_truthy(right_type.as_ref());
                    self.emit(&format!("    j {}", end_label));
                    self.emit(&format!("{}:", short_label));
                    self.emit(if *op == BinaryOp::LogAnd {
                        "    li a0, 0"
                    } else {
                        "    li a0, 1"
                    });
                    self.emit(&format!("{}:", end_label));
                } else {
                    let lt = self.type_of(l);
                    let rt = self.type_of(r);
                    let lf = lt.as_ref().map(Self::is_float_ty).unwrap_or(false);
                    let rf = rt.as_ref().map(Self::is_float_ty).unwrap_or(false);
                    // setjmp 与整数常数比较：常数在调用后重新 li 载入，
                    // 避免 longjmp 跳回时栈上的常数槽已被覆盖。
                    let lhs_is_setjmp = matches!(l.inner(), Node::Call(f, _)
                        if matches!(f.inner(), Node::Ident(n) if n == "setjmp"));
                    let rhs_is_setjmp = matches!(r.inner(), Node::Call(f, _)
                        if matches!(f.inner(), Node::Ident(n) if n == "setjmp"));
                    if (lhs_is_setjmp || rhs_is_setjmp) && !lf && !rf {
                        let const_val = if lhs_is_setjmp {
                            match r.inner() {
                                Node::Num(v) => Some(*v),
                                _ => None,
                            }
                        } else {
                            match l.inner() {
                                Node::Num(v) => Some(*v),
                                _ => None,
                            }
                        };
                        if let Some(cv) = const_val {
                            self.gen_expr(if lhs_is_setjmp { l } else { r });
                            if lhs_is_setjmp {
                                self.emit(&format!("    li t1, {}", cv));
                            } else {
                                self.emit("    mv t1, a0");
                                self.emit(&format!("    li a0, {}", cv));
                            }
                            self.emit_binop(*op, false);
                            return;
                        }
                    }
                    if lf || rf {
                        // 浮点路径：右操作数转 f64 后 fsd 压栈
                        self.gen_expr(r);
                        if !rf {
                            self.emit_int_to_double(rt.as_ref());
                        }
                        self.emit("    addi sp, sp, -8");
                        self.emit("    fsd f10, 0(sp)");
                        self.gen_expr(l);
                        if !lf {
                            self.emit_int_to_double(lt.as_ref());
                        }
                        self.emit_float_binop(*op);
                    } else {
                        // 整数路径：a0 压栈（统一 8 字节槽，保持 sp 8 对齐，
                        // 否则后续浮点 fsd 压栈会地址未对齐）
                        self.gen_expr(r);
                        self.emit("    addi sp, sp, -8");
                        self.emit("    sw a0, 0(sp)");
                        self.gen_expr(l);
                        self.emit("    lw t1, 0(sp)");
                        self.emit("    addi sp, sp, 8");
                        // 指针算术缩放（P4-3）
                        self.emit_ptr_scale(*op, &lt, &rt);
                        let unsigned = if Self::is_pointer_like(&lt) && Self::is_pointer_like(&rt) {
                            true
                        } else if matches!(op, BinaryOp::Shr) {
                            lt.as_ref()
                                .map(|ty| ty.integer_promotion().is_unsigned_integer())
                                .unwrap_or(false)
                        } else {
                            Self::arithmetic_is_unsigned(lt.as_ref(), rt.as_ref())
                        };
                        self.emit_binop(*op, unsigned);
                        if *op == BinaryOp::Sub
                            && Self::is_pointer_like(&lt)
                            && Self::is_pointer_like(&rt)
                        {
                            let size = self.pointer_elem_size(lt.as_ref()).unwrap_or(1);
                            if size != 1 {
                                self.emit(&format!("    li t0, {}", size));
                                self.emit("    div a0, a0, t0");
                            }
                        }
                    }
                }
            }
            Node::Member(..) => {
                let ty = self.type_of(node);
                if let Some((bbit, bwidth, bsigned)) = match node.inner() {
                    Node::Member(obj, name, _) => self.member_bitfield(obj, name),
                    _ => None,
                } {
                    self.gen_addr(node);
                    self.emit_bitfield_load(bbit, bwidth, bsigned);
                } else {
                    self.gen_addr(node);
                    self.load_from_address(ty.as_ref());
                }
            }
            Node::Subscript(arr, idx) => {
                // 值 = *(arr + idx × size)；double 元素 → f10
                let _ = arr;
                self.gen_addr(node);
                let ty = self.type_of(node);
                self.load_from_address(ty.as_ref());
                let _ = idx;
            }
            Node::Unary(op, e) => match op {
                UnaryOp::Addr => {
                    if self.node_is_bitfield(e) {
                        self.fail_codegen("不能对位域取地址（C89 §3.3.3.2）");
                        self.emit("    li a0, 0");
                    } else {
                        self.gen_addr(e);
                    }
                }
                UnaryOp::Deref => {
                    let ty = self.type_of(node);
                    self.gen_expr(e);
                    self.load_from_address(ty.as_ref());
                }
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                    if let Some((bbit, bwidth, bsigned)) = match e.inner() {
                        Node::Member(obj, name, _) => self.member_bitfield(obj, name),
                        _ => None,
                    } {
                        let is_post = matches!(op, UnaryOp::PostInc | UnaryOp::PostDec);
                        self.gen_addr(e);
                        self.emit("    addi sp, sp, -8");
                        self.emit("    sw a0, 0(sp)");
                        self.emit_bitfield_load(bbit, bwidth, bsigned);
                        if is_post {
                            // 旧值存 t4：emit_bitfield_store 会用 t1/t2 做 scratch
                            self.emit("    mv t4, a0");
                        }
                        if matches!(op, UnaryOp::PreInc | UnaryOp::PostInc) {
                            self.emit("    addi a0, a0, 1");
                        } else {
                            self.emit("    addi a0, a0, -1");
                        }
                        self.emit("    lw t3, 0(sp)");
                        self.emit("    addi sp, sp, 8");
                        self.emit_bitfield_store(bbit, bwidth);
                        if is_post {
                            self.emit("    mv a0, t4");
                        }
                    } else {
                        let ty = self.type_of(e).unwrap_or(Type::Int);
                        let is_post = matches!(op, UnaryOp::PostInc | UnaryOp::PostDec);
                        let step = match ty {
                            Type::Ptr(ref inner) => self.tt.size_of(inner).unwrap_or(4) as i64,
                            _ => 1,
                        };
                        self.gen_addr(e);
                        self.emit("    addi sp, sp, -8");
                        self.emit("    sw a0, 0(sp)");
                        self.load_from_address(Some(&ty));
                        if Self::is_float_ty(&ty) {
                            if is_post {
                                self.emit("    fsgnj.d f13, f10, f10");
                            }
                            self.emit(&format!("    li a0, {}", step));
                            self.emit("    fcvt.d.w f12, a0");
                            if matches!(op, UnaryOp::PreInc | UnaryOp::PostInc) {
                                self.emit("    fadd.d f10, f10, f12");
                            } else {
                                self.emit("    fsub.d f10, f10, f12");
                            }
                            self.emit("    lw t1, 0(sp)");
                            self.emit("    addi sp, sp, 8");
                            self.store_to_address("t1", Some(&ty));
                            if is_post {
                                self.emit("    fsgnj.d f10, f13, f13");
                            }
                        } else {
                            if is_post {
                                self.emit("    mv t2, a0");
                            }
                            self.emit(&format!("    li t0, {}", step));
                            if matches!(op, UnaryOp::PreInc | UnaryOp::PostInc) {
                                self.emit("    add a0, a0, t0");
                            } else {
                                self.emit("    sub a0, a0, t0");
                            }
                            self.emit("    lw t1, 0(sp)");
                            self.emit("    addi sp, sp, 8");
                            self.emit_integer_conversion(&ty);
                            self.store_to_address("t1", Some(&ty));
                            if is_post {
                                self.emit("    mv a0, t2");
                            }
                        }
                    }
                }
                UnaryOp::Neg => {
                    self.gen_expr(e);
                    if self
                        .type_of(node)
                        .map(|ty| Self::is_float_ty(&ty))
                        .unwrap_or(false)
                    {
                        self.emit("    fneg.d f10, f10");
                    } else {
                        self.emit("    neg a0, a0");
                    }
                }
                UnaryOp::Not => {
                    self.gen_expr(e);
                    let ty = self.type_of(e);
                    self.emit_truthy(ty.as_ref());
                    self.emit("    xori a0, a0, 1");
                }
                UnaryOp::BitNot => {
                    self.gen_expr(e);
                    self.emit("    xori a0, a0, -1");
                }
            },
            Node::Cond(c, t, e) => {
                let else_l = self.label();
                let end_l = self.label();
                self.gen_expr(c);
                let condition_type = self.type_of(c);
                self.emit_truthy(condition_type.as_ref());
                self.emit(&format!("    beqz a0, {}", else_l));
                let result_type = self.type_of(node);
                let then_type = self.type_of(t);
                self.gen_expr(t);
                if result_type.as_ref().map(Self::is_float_ty).unwrap_or(false)
                    && !then_type.as_ref().map(Self::is_float_ty).unwrap_or(false)
                {
                    self.emit_int_to_double(then_type.as_ref());
                }
                self.emit(&format!("    j {}", end_l));
                self.emit(&format!("{}:", else_l));
                let else_type = self.type_of(e);
                self.gen_expr(e);
                if result_type.as_ref().map(Self::is_float_ty).unwrap_or(false)
                    && !else_type.as_ref().map(Self::is_float_ty).unwrap_or(false)
                {
                    self.emit_int_to_double(else_type.as_ref());
                }
                self.emit(&format!("{}:", end_l));
            }
            Node::Call(f, args) => {
                // va_* 特判
                if let Node::Ident(name) = f.as_ref() {
                    // va_start(ap, last)：ap = 第一个变参地址（序言存档于帧内槽）
                    if name == "va_start" {
                        if let Some(Node::Ident(ap)) = args.first() {
                            let off = self.locals.get(ap).map(|x| x.0).unwrap_or(0);
                            let (rb, ro) = self.address_parts("s0", off, "t2");
                            // 与序言的 sw t6, frame-12(s0) 对应（帧内槽）
                            let (tb, to) = self.address_parts("s0", self.frame_size - 12, "t3");
                            self.emit(&format!("    lw t1, {}({})", to, tb));
                            self.emit(&format!("    sw t1, {}({})", ro, rb));
                        }
                        return;
                    }
                    // va_arg(ap, type)：读 + 推进 8 字节
                    if name == "va_arg" {
                        if let Some(Node::Ident(ap)) = args.first() {
                            let off = self.locals.get(ap).map(|x| x.0).unwrap_or(0);
                            let (rb, ro) = self.address_parts("s0", off, "t2");
                            self.emit(&format!("    lw t1, {}({})", ro, rb));
                            let is_double = matches!(args.get(1), Some(Node::Ident(t)) if t.as_str() == "double");
                            if is_double {
                                self.emit("    fld f10, 0(t1)");
                            } else {
                                self.emit("    lw a0, 0(t1)");
                            }
                            self.emit("    addi t2, t1, 8");
                            // 此处 t2 持有要回写的指针值，寻址 scratch 换用 t3
                            let (rb, ro) = self.address_parts("s0", off, "t3");
                            self.emit(&format!("    sw t2, {}({})", ro, rb));
                        }
                        return;
                    }
                    // va_end(ap)：无操作
                    if name == "va_end" {
                        return;
                    }
                    // setjmp：longjmp 会跳回调用点，结果不能依赖调用前压栈的
                    // 中间值（栈槽可能被期间执行的代码覆盖）。直接传参调用。
                    if name == "setjmp" {
                        if let Some(env) = args.first() {
                            self.gen_expr(env);
                        }
                        self.emit("    call setjmp");
                        return;
                    }
                }
                // 直接调用只在本名字未被局部对象（函数指针变量）遮蔽时成立；
                // 被遮蔽时走间接 jalr 路径（与 Ident 表达式的遮蔽处理一致）
                let direct_name = match f.inner() {
                    Node::Ident(name)
                        if self.func_info.contains_key(name)
                            && !self.locals.contains_key(name)
                            && !self.current_statics.contains_key(name) =>
                    {
                        Some(name.clone())
                    }
                    _ => None,
                };
                let indirect_signature = match self.type_of(f) {
                    Some(ty) => match ty.unqualified() {
                        Type::Ptr(inner) => match inner.unqualified() {
                            Type::Function {
                                ret,
                                params,
                                is_variadic,
                                ..
                            } => Some(((**ret).clone(), params.clone(), *is_variadic)),
                            _ => None,
                        },
                        Type::Function {
                            ret,
                            params,
                            is_variadic,
                            ..
                        } => Some(((**ret).clone(), params.clone(), *is_variadic)),
                        _ => None,
                    },
                    None => None,
                };
                let (return_type, params, variadic) = direct_name
                    .as_ref()
                    .and_then(|name| self.func_info.get(name).cloned())
                    .or(indirect_signature)
                    .unwrap_or((Type::Int, Vec::new(), false));
                let fixed_count = if variadic {
                    params.len().min(args.len())
                } else {
                    args.len()
                };
                let fixed_types: Vec<Type> = args
                    .iter()
                    .take(fixed_count)
                    .enumerate()
                    .map(|(index, argument)| {
                        params
                            .get(index)
                            .map(|parameter| parameter.ty.clone())
                            .or_else(|| self.type_of(argument))
                            .unwrap_or(Type::Int)
                    })
                    .collect();
                let return_struct = Self::is_struct_ty(&return_type);
                let indirect = direct_name.is_none();
                if indirect {
                    self.gen_expr(f);
                    self.emit("    addi sp, sp, -8");
                    self.emit("    sw a0, 0(sp)");
                }
                let return_temp = self.struct_call_temps.get(&Self::node_id(node)).copied();
                // 固定参数：先求值压栈（统一 8 字节槽；double 压 f10，int 压 a0）
                for (index, a) in args.iter().take(fixed_count).enumerate() {
                    let target_type = &fixed_types[index];
                    if Self::is_struct_ty(target_type) {
                        continue;
                    }
                    let source_float = self
                        .type_of(a)
                        .map(|ty| Self::is_float_ty(&ty))
                        .unwrap_or(false);
                    let target_float = Self::is_float_ty(target_type);
                    self.gen_expr(a);
                    if target_float && !source_float {
                        let source_type = self.type_of(a);
                        self.emit_int_to_double(source_type.as_ref());
                    } else if !target_float && source_float {
                        if target_type.is_unsigned_integer() {
                            self.emit("    fcvt.wu.d a0, f10, rtz");
                        } else {
                            self.emit("    fcvt.w.d a0, f10, rtz");
                        }
                    }
                    self.emit_integer_conversion(target_type);
                    self.emit("    addi sp, sp, -8");
                    if target_float {
                        self.emit("    fsd f10, 0(sp)");
                    } else {
                        self.emit("    sw a0, 0(sp)");
                    }
                }
                // struct 参数单独处理（压栈拷贝 + t5）
                let struct_args: Vec<&Node> = args
                    .iter()
                    .take(fixed_count)
                    .zip(&fixed_types)
                    .filter_map(|(argument, ty)| Self::is_struct_ty(ty).then_some(argument))
                    .collect();
                let mut struct_total = 0usize;
                for a in &struct_args {
                    let size = self
                        .tt
                        .size_of(&self.type_of(a).unwrap_or(Type::Int))
                        .unwrap_or(8);
                    struct_total += (size + 7) & !7;
                }
                // 固定参数压栈暂存（排除 struct；变参处理完再弹，避免 a0 被覆盖）
                let mut targets: Vec<(bool, usize)> = Vec::new();
                let mut ai = usize::from(return_struct);
                let mut fi = 0usize;
                for ty in &fixed_types {
                    if Self::is_struct_ty(ty) {
                        continue;
                    }
                    let is_fp = Self::is_float_ty(ty);
                    if is_fp {
                        targets.push((true, fi));
                        fi += 1;
                    } else {
                        targets.push((false, ai));
                        ai += 1;
                    }
                }
                // struct 参数：逆序压栈拷贝，第一个 struct 最后压 → 栈顶（t5）
                for a in struct_args.iter().rev() {
                    let sz = self
                        .tt
                        .size_of(&self.type_of(a).unwrap_or(Type::Int))
                        .unwrap_or(8);
                    if sz > Self::MAX_STRUCT_BYVAL {
                        self.fail_codegen(format!(
                            "结构体按值传参过大({sz}字节，上限{}字节)，请改用指针传递",
                            Self::MAX_STRUCT_BYVAL
                        ));
                        return;
                    }
                    // 压栈 sz 字节（8 对齐）；大结构体经 li+sub 调整 sp
                    let align = (sz + 7) & !7;
                    if align <= 2047 {
                        self.emit(&format!("    addi sp, sp, -{}", align));
                    } else {
                        self.emit(&format!("    li t1, {}", align));
                        self.emit("    sub sp, sp, t1");
                    }
                    self.gen_expr(a); // a0 = struct 对象地址（含函数返回临时对象）
                    for k in 0..sz {
                        // 偏移可能超出 ±2047：经 address_parts 生成 li+add 寻址
                        let (rb, ro) = self.address_parts("a0", k as i32, "t0");
                        self.emit(&format!("    lbu t1, {}({})", ro, rb));
                        let (rb, ro) = self.address_parts("sp", k as i32, "t0");
                        self.emit(&format!("    sb t1, {}({})", ro, rb));
                    }
                }
                if !struct_args.is_empty() {
                    self.emit("    mv t5, sp");
                }
                // 变参：逆序压栈（8 字节槽），第一个变参最后压 → 栈顶
                let mut varg_count = 0usize;
                for a in args.iter().skip(fixed_count).rev() {
                    let is_fp = self
                        .type_of(a)
                        .map(|t| Self::is_float_ty(&t))
                        .unwrap_or(false);
                    if is_fp {
                        self.gen_expr(a);
                        self.emit("    addi sp, sp, -8");
                        self.emit("    fsd f10, 0(sp)");
                    } else {
                        self.gen_expr(a);
                        self.emit("    addi sp, sp, -8");
                        self.emit("    sw a0, 0(sp)");
                    }
                    varg_count += 1;
                }
                if varg_count > 0 {
                    // 逆序压栈后栈顶 = 第一个变参
                    self.emit("    mv t6, sp");
                }
                // 寄存器耗尽后的固定参数使用连续 8 字节栈槽。
                let overflow_count = targets
                    .iter()
                    .filter(|(is_fp, reg)| (*is_fp && *reg >= 22) || (!*is_fp && *reg >= 8))
                    .count();
                let overflow_total = overflow_count * 8;
                if overflow_total > 0 {
                    // 与帧分配一致：超出 12 位立即数范围走 li+sub
                    if overflow_total > 2047 {
                        self.emit(&format!("    li t1, {}", overflow_total));
                        self.emit("    sub sp, sp, t1");
                    } else {
                        self.emit(&format!("    addi sp, sp, -{}", overflow_total));
                    }
                }
                // 从求值暂存区恢复寄存器参数，并复制溢出参数到调用栈顶。
                let non_struct_fixed = targets.len();
                let mut overflow_index = 0usize;
                for (i, (is_fp, reg)) in targets.iter().copied().enumerate() {
                    let source = overflow_total
                        + struct_total
                        + varg_count * 8
                        + (non_struct_fixed - 1 - i) * 8;
                    // source 可能超出 ±2047（大结构体/多变参）
                    let (rb, ro) = self.address_parts("sp", source as i32, "t2");
                    if is_fp && reg < 22 {
                        self.emit(&format!("    fld f{}, {}({})", 10 + reg, ro, rb));
                    } else if !is_fp && reg < 8 {
                        self.emit(&format!("    lw a{}, {}({})", reg, ro, rb));
                    } else if is_fp {
                        self.emit(&format!("    fld f0, {}({})", ro, rb));
                        self.emit(&format!("    fsd f0, {}(sp)", overflow_index * 8));
                        overflow_index += 1;
                    } else {
                        self.emit(&format!("    lw t1, {}({})", ro, rb));
                        self.emit(&format!("    sw t1, {}(sp)", overflow_index * 8));
                        overflow_index += 1;
                    }
                }
                if let Some(offset) = return_temp {
                    let (rb, ro) = self.address_parts("s0", offset as i32, "t2");
                    self.emit(&format!("    addi a0, {}, {}", rb, ro));
                }
                if indirect {
                    let target_offset =
                        overflow_total + struct_total + varg_count * 8 + targets.len() * 8;
                    let (rb, ro) = self.address_parts("sp", target_offset as i32, "t2");
                    self.emit(&format!("    lw t4, {}({})", ro, rb));
                    self.emit("    jalr ra, t4, 0");
                } else if let Some(name) = &direct_name {
                    self.emit(&format!("    call {}", name));
                }
                // 恢复栈（保留 struct 返回对象槽，供当前表达式继续使用）。
                let total_slots = overflow_total
                    + varg_count * 8
                    + targets.len() * 8
                    + struct_total
                    + usize::from(indirect) * 8;
                if total_slots > 0 {
                    if total_slots <= 2047 {
                        self.emit(&format!("    addi sp, sp, {}", total_slots));
                    } else {
                        self.emit(&format!("    li t2, {}", total_slots));
                        self.emit("    add sp, sp, t2");
                    }
                }
                if let Some(offset) = return_temp {
                    let (rb, ro) = self.address_parts("s0", offset as i32, "t2");
                    self.emit(&format!("    addi a0, {}, {}", rb, ro));
                }
                // 浮点函数返回值约定：在 f10
                if Self::is_float_ty(&return_type) {
                    // 结果已在 f10（无操作）
                }
            }
            Node::Comma(a, b) => {
                self.gen_expr(a);
                self.gen_expr(b);
            }
            Node::SizeofExpr(e) => {
                // 用表达式类型求真实大小
                if self.node_is_bitfield(e) {
                    self.fail_codegen("不能对位域应用 sizeof");
                    self.emit("    li a0, 4");
                } else {
                    let t = self.type_of(e).unwrap_or(Type::Int);
                    let sz = self.tt.size_of(&t).unwrap_or(4);
                    self.emit(&format!("    li a0, {}", sz));
                }
            }
            other => {
                self.fail_codegen(format!("代码生成器无法处理表达式节点: {:?}", other));
            }
        }
    }

    fn is_pointer_like(ty: &Option<Type>) -> bool {
        ty.as_ref().map(Type::is_pointer_like).unwrap_or(false)
    }

    fn pointer_elem_size(&self, ty: Option<&Type>) -> Option<usize> {
        ty.and_then(Type::pointed_type)
            .map(|inner| self.tt.size_of(inner).unwrap_or(4))
    }

    /// a0 为左操作数，t1 为右操作数。加减前按所指元素大小缩放整数操作数。
    fn emit_ptr_scale(&mut self, op: BinaryOp, left: &Option<Type>, right: &Option<Type>) {
        match op {
            BinaryOp::Add if Self::is_pointer_like(left) && !Self::is_pointer_like(right) => {
                let size = self.pointer_elem_size(left.as_ref()).unwrap_or(1);
                if size != 1 {
                    self.emit(&format!("    li t0, {}", size));
                    self.emit("    mul t1, t1, t0");
                }
            }
            BinaryOp::Add if !Self::is_pointer_like(left) && Self::is_pointer_like(right) => {
                let size = self.pointer_elem_size(right.as_ref()).unwrap_or(1);
                if size != 1 {
                    self.emit(&format!("    li t0, {}", size));
                    self.emit("    mul a0, a0, t0");
                }
            }
            BinaryOp::Sub if Self::is_pointer_like(left) && !Self::is_pointer_like(right) => {
                let size = self.pointer_elem_size(left.as_ref()).unwrap_or(1);
                if size != 1 {
                    self.emit(&format!("    li t0, {}", size));
                    self.emit("    mul t1, t1, t0");
                }
            }
            _ => {}
        }
    }

    /// 浮点二元运算（float 位模式在 a0/t1）：转 fa0/fa1 运算后转回 a0
    fn emit_float_binop(&mut self, op: BinaryOp) {
        // 左在 f10，右在栈（f64）→ 出栈到 f11 后运算
        self.emit("    fld f11, 0(sp)");
        self.emit("    addi sp, sp, 8");
        match op {
            BinaryOp::Add => self.emit("    fadd.d f10, f10, f11"),
            BinaryOp::Sub => self.emit("    fsub.d f10, f10, f11"),
            BinaryOp::Mul => self.emit("    fmul.d f10, f10, f11"),
            BinaryOp::Div => self.emit("    fdiv.d f10, f10, f11"),
            // 浮点比较：结果放 a0（0/1）
            BinaryOp::Eq => self.emit("    feq.d a0, f10, f11"),
            BinaryOp::Ne => {
                self.emit("    feq.d a0, f10, f11");
                self.emit("    xori a0, a0, 1");
            }
            BinaryOp::Lt => self.emit("    flt.d a0, f10, f11"),
            BinaryOp::Gt => self.emit("    flt.d a0, f11, f10"),
            BinaryOp::Le => self.emit("    fle.d a0, f10, f11"),
            BinaryOp::Ge => self.emit("    fle.d a0, f11, f10"),
            _ => self.emit("    fadd.d f10, f10, f11"),
        }
    }

    /// 二元运算：a0 op t1 → a0
    fn emit_binop(&mut self, op: BinaryOp, unsigned: bool) {
        let s = match op {
            BinaryOp::Add => "add a0, a0, t1",
            BinaryOp::Sub => "sub a0, a0, t1",
            BinaryOp::Mul => "mul a0, a0, t1",
            BinaryOp::Div if unsigned => "divu a0, a0, t1",
            BinaryOp::Div => "div a0, a0, t1",
            BinaryOp::Mod if unsigned => "remu a0, a0, t1",
            BinaryOp::Mod => "rem a0, a0, t1",
            BinaryOp::Shl => "sll a0, a0, t1",
            BinaryOp::Shr if unsigned => "srl a0, a0, t1",
            BinaryOp::Shr => "sra a0, a0, t1",
            BinaryOp::Lt if unsigned => "sltu a0, a0, t1",
            BinaryOp::Lt => "slt a0, a0, t1",
            BinaryOp::Gt if unsigned => "sltu a0, t1, a0",
            BinaryOp::Gt => "slt a0, t1, a0",
            BinaryOp::Le if unsigned => "sltu t0, t1, a0\n    xori a0, t0, 1",
            BinaryOp::Le => "slt t0, t1, a0\n    xori a0, t0, 1",
            BinaryOp::Ge if unsigned => "sltu t0, a0, t1\n    xori a0, t0, 1",
            BinaryOp::Ge => "slt t0, a0, t1\n    xori a0, t0, 1",
            BinaryOp::Eq => "sub a0, a0, t1\n    seqz a0, a0",
            BinaryOp::Ne => "sub a0, a0, t1\n    snez a0, a0",
            BinaryOp::BitAnd => "and a0, a0, t1",
            BinaryOp::BitXor => "xor a0, a0, t1",
            BinaryOp::BitOr => "or a0, a0, t1",
            BinaryOp::LogAnd => "and a0, a0, t1",
            BinaryOp::LogOr => "or a0, a0, t1",
        };
        for line in s.lines() {
            self.emit(line);
        }
    }

    /// 复合赋值运算：a0 op t1 → a0
    fn emit_binop_assign(&mut self, op: AssignOp, unsigned: bool) {
        let s = match op {
            AssignOp::Add => "add a0, a0, t1",
            AssignOp::Sub => "sub a0, a0, t1",
            AssignOp::Mul => "mul a0, a0, t1",
            AssignOp::Div if unsigned => "divu a0, a0, t1",
            AssignOp::Div => "div a0, a0, t1",
            AssignOp::Mod if unsigned => "remu a0, a0, t1",
            AssignOp::Mod => "rem a0, a0, t1",
            AssignOp::Shl => "sll a0, a0, t1",
            AssignOp::Shr if unsigned => "srl a0, a0, t1",
            AssignOp::Shr => "sra a0, a0, t1",
            AssignOp::BitAnd => "and a0, a0, t1",
            AssignOp::BitXor => "xor a0, a0, t1",
            AssignOp::BitOr => "or a0, a0, t1",
            AssignOp::Plain => unreachable!(),
        };
        for line in s.lines() {
            self.emit(line);
        }
    }
}

fn eval_integer_constant(node: &Node) -> Option<i64> {
    match node.inner() {
        Node::Num(v) => Some(*v),
        Node::Unary(op, e) => {
            let v = eval_integer_constant(e)?;
            match op {
                UnaryOp::Neg => Some(v.wrapping_neg()),
                UnaryOp::Not => Some((v == 0) as i64),
                UnaryOp::BitNot => Some(!v),
                _ => None,
            }
        }
        Node::Binary(op, l, r) => {
            let a = eval_integer_constant(l)?;
            let b = eval_integer_constant(r)?;
            match op {
                BinaryOp::Add => Some(a.wrapping_add(b)),
                BinaryOp::Sub => Some(a.wrapping_sub(b)),
                BinaryOp::Mul => Some(a.wrapping_mul(b)),
                BinaryOp::Div if b != 0 => a.checked_div(b),
                BinaryOp::Mod if b != 0 => a.checked_rem(b),
                BinaryOp::Shl if (0..64).contains(&b) => Some(a.wrapping_shl(b as u32)),
                BinaryOp::Shr if (0..64).contains(&b) => Some(a.wrapping_shr(b as u32)),
                BinaryOp::Lt => Some((a < b) as i64),
                BinaryOp::Gt => Some((a > b) as i64),
                BinaryOp::Le => Some((a <= b) as i64),
                BinaryOp::Ge => Some((a >= b) as i64),
                BinaryOp::Eq => Some((a == b) as i64),
                BinaryOp::Ne => Some((a != b) as i64),
                BinaryOp::BitAnd => Some(a & b),
                BinaryOp::BitXor => Some(a ^ b),
                BinaryOp::BitOr => Some(a | b),
                BinaryOp::LogAnd => Some((a != 0 && b != 0) as i64),
                BinaryOp::LogOr => Some((a != 0 || b != 0) as i64),
                _ => None,
            }
        }
        Node::Cond(c, a, b) => {
            if eval_integer_constant(c)? != 0 {
                eval_integer_constant(a)
            } else {
                eval_integer_constant(b)
            }
        }
        Node::Cast(ty, e) => Some(narrow_constant(ty, eval_integer_constant(e)?)),
        _ => None,
    }
}

/// 编译期强转窄化（C89 §3.3.4）：按目标类型截断/符号扩展。
/// 本目标 RV32 上 long 与 int 同为 32 位。
fn narrow_constant(ty: &Type, v: i64) -> i64 {
    match ty.unqualified() {
        Type::Char => v as u8 as i8 as i64,
        Type::UChar => v as u8 as i64,
        Type::Short => v as u16 as i16 as i64,
        Type::UShort => v as u16 as i64,
        Type::Int | Type::Long => v as u32 as i32 as i64,
        Type::UInt | Type::ULong => v as u32 as i64,
        _ => v,
    }
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new()
    }
}

fn codegen_span(node: &Node, file: &str, tokens: &[Token]) -> Span {
    node.span().cloned().unwrap_or_else(|| {
        tokens
            .iter()
            .find(|token| !token.text.is_empty())
            .map(|token| Span::new(&token.file, token.line, token.column))
            .unwrap_or_else(|| Span::new(file, 1, 1))
    })
}

fn codegen_error<T>(
    node: &Node,
    file: &str,
    tokens: &[Token],
    message: impl Into<String>,
) -> CompileResult<T> {
    Err(CompileError::codegen(
        codegen_span(node, file, tokens),
        message,
    ))
}

/// 类型是否为含位域成员的结构体
fn struct_has_bitfields(tt: &TypeTable, ty: &Type) -> bool {
    match ty.unqualified() {
        Type::Struct(tag) => tt
            .structs
            .get(tag)
            .map(|s| s.members.iter().any(|m| m.bit_width.is_some()))
            .unwrap_or(false),
        _ => false,
    }
}

fn codegen_error_at_text<T>(
    node: &Node,
    file: &str,
    tokens: &[Token],
    text: &str,
    message: impl Into<String>,
) -> CompileResult<T> {
    let base = codegen_span(node, file, tokens);
    let span = tokens
        .iter()
        .filter(|token| {
            token.line > base.line || (token.line == base.line && token.column >= base.column)
        })
        .find(|token| {
            token.text == text
                || token
                    .text
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    == Some(text)
        })
        .map(|token| Span::new(&token.file, token.line, token.column))
        .unwrap_or(base);
    Err(CompileError::codegen(span, message))
}

fn validate_codegen_type(
    ty: &Type,
    node: &Node,
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    match ty {
        Type::Qualified { ty, .. } | Type::Array(ty, _) => {
            validate_codegen_type(ty, node, tt, file, tokens)?;
        }
        Type::Ptr(_) => {}
        Type::Struct(_) => {
            tt.size_of(ty).map_err(|message| {
                CompileError::codegen(codegen_span(node, file, tokens), message)
            })?;
        }
        Type::Typedef(_) => {
            return codegen_error(node, file, tokens, "代码生成前仍存在未解析的 typedef 类型");
        }
        Type::Function { ret, params, .. } => {
            validate_codegen_type(ret, node, tt, file, tokens)?;
            for parameter in params {
                validate_codegen_type(&parameter.ty, node, tt, file, tokens)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct BuiltinContext<'a> {
    is_variadic: bool,
    fixed_params: &'a [Param],
    automatic_objects: HashSet<String>,
}

fn validate_builtin_codegen(
    call: &Node,
    name: &str,
    args: &[Node],
    context: Option<&BuiltinContext<'_>>,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    if !matches!(name, "va_start" | "va_arg" | "va_end") {
        return Ok(());
    }
    let Some(context) = context else {
        return codegen_error(call, file, tokens, format!("{} 只能在函数体中使用", name));
    };
    let expected = match name {
        "va_start" | "va_arg" => 2,
        "va_end" => 1,
        _ => return Ok(()),
    };
    if args.len() != expected {
        return codegen_error(
            call,
            file,
            tokens,
            format!("{} 需要 {} 个参数，实际 {} 个", name, expected, args.len()),
        );
    }
    if name == "va_start" && !context.is_variadic {
        return codegen_error(call, file, tokens, format!("{} 只能用于变参函数", name));
    }
    let Some(Node::Ident(ap)) = args.first().map(Node::inner) else {
        return codegen_error(
            call,
            file,
            tokens,
            format!("{} 的第一个参数必须是局部对象", name),
        );
    };
    if !context.automatic_objects.contains(ap) {
        return codegen_error(
            &args[0],
            file,
            tokens,
            format!("{} 的游标对象必须具有自动存储期", name),
        );
    }
    match name {
        "va_start" => {
            let Some(Node::Ident(last)) = args.get(1).map(Node::inner) else {
                return codegen_error(
                    call,
                    file,
                    tokens,
                    "va_start 的第二个参数必须是最后一个固定形参",
                );
            };
            if context
                .fixed_params
                .last()
                .map(|parameter| parameter.name.as_str())
                != Some(last)
            {
                return codegen_error(
                    &args[1],
                    file,
                    tokens,
                    "va_start 的第二个参数必须是最后一个固定形参",
                );
            }
        }
        "va_arg" => {
            let supported = matches!(args.get(1).map(Node::inner), Some(Node::Ident(ty)) if ty == "int" || ty == "double");
            if !supported {
                let text = match args[1].inner() {
                    Node::Ident(name) => name.as_str(),
                    _ => "va_arg",
                };
                return codegen_error_at_text(
                    &args[1],
                    file,
                    tokens,
                    text,
                    "va_arg 后端目前仅支持 int 和 double",
                );
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_automatic_objects(node: &Node, objects: &mut HashSet<String>) {
    match node.inner() {
        Node::Decl { specs, name, .. }
            if !matches!(
                specs.storage,
                Storage::Static | Storage::Extern | Storage::Typedef
            ) =>
        {
            objects.insert(name.clone());
        }
        Node::Block(nodes) | Node::DeclBlock(nodes) => {
            for child in nodes {
                collect_automatic_objects(child, objects);
            }
        }
        Node::If(_, yes, no) => {
            collect_automatic_objects(yes, objects);
            if let Some(no) = no {
                collect_automatic_objects(no, objects);
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body)
        | Node::Label(_, body) => collect_automatic_objects(body, objects),
        _ => {}
    }
}

fn validate_codegen_initializer(
    node: &Node,
    context: Option<&BuiltinContext<'_>>,
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    match node.inner() {
        Node::InitList(items) => {
            for item in items {
                validate_codegen_initializer(item, context, tt, file, tokens)?;
            }
            Ok(())
        }
        _ => validate_codegen_expr(node, context, tt, file, tokens),
    }
}

fn validate_codegen_expr(
    node: &Node,
    context: Option<&BuiltinContext<'_>>,
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    match node.inner() {
        Node::Num(_) | Node::FNum(_) | Node::Ident(_) => Ok(()),
        Node::Str(s) => crate::expr_eval::decode_string_escapes(s)
            .map(|_| ())
            .map_err(|message| codegen_error::<()>(node, file, tokens, message).unwrap_err()),
        Node::Unary(_, value) | Node::SizeofExpr(value) => {
            validate_codegen_expr(value, context, tt, file, tokens)
        }
        Node::Cast(ty, value) => {
            validate_codegen_type(ty, node, tt, file, tokens)?;
            validate_codegen_expr(value, context, tt, file, tokens)
        }
        Node::Binary(_, left, right)
        | Node::Assign(_, left, right)
        | Node::Subscript(left, right)
        | Node::Comma(left, right) => {
            validate_codegen_expr(left, context, tt, file, tokens)?;
            validate_codegen_expr(right, context, tt, file, tokens)
        }
        Node::Cond(condition, yes, no) => {
            validate_codegen_expr(condition, context, tt, file, tokens)?;
            validate_codegen_expr(yes, context, tt, file, tokens)?;
            validate_codegen_expr(no, context, tt, file, tokens)
        }
        Node::Member(object, _, _) => validate_codegen_expr(object, context, tt, file, tokens),
        Node::Call(callee, args) => {
            if let Node::Ident(name) = callee.inner() {
                validate_builtin_codegen(node, name, args, context, file, tokens)?;
            }
            validate_codegen_expr(callee, context, tt, file, tokens)?;
            for (index, argument) in args.iter().enumerate() {
                if matches!(callee.inner(), Node::Ident(name) if name == "va_arg") && index == 1 {
                    continue;
                }
                validate_codegen_expr(argument, context, tt, file, tokens)?;
            }
            Ok(())
        }
        other => codegen_error(
            node,
            file,
            tokens,
            format!("代码生成器无法处理表达式节点: {:?}", other),
        ),
    }
}

fn validate_codegen_stmt(
    node: &Node,
    context: &BuiltinContext<'_>,
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    match node.inner() {
        Node::Block(nodes) | Node::DeclBlock(nodes) => {
            for child in nodes {
                validate_codegen_stmt(child, context, tt, file, tokens)?;
            }
            Ok(())
        }
        Node::Decl { specs, init, .. } => {
            if specs.storage != Storage::Typedef {
                validate_codegen_type(&specs.ty, node, tt, file, tokens)?;
            }
            if let Some(init) = init {
                if struct_has_bitfields(tt, &specs.ty) {
                    return codegen_error(
                        node,
                        file,
                        tokens,
                        "位域结构体的初始化器暂不支持（C89 位域布局由实现定义）",
                    );
                }
                validate_codegen_initializer(init, Some(context), tt, file, tokens)?;
            }
            Ok(())
        }
        Node::ExprStmt(expr) => validate_codegen_expr(expr, Some(context), tt, file, tokens),
        Node::NullStmt | Node::Break | Node::Continue | Node::Goto(_) => Ok(()),
        Node::Return(value) => {
            if let Some(value) = value {
                validate_codegen_expr(value, Some(context), tt, file, tokens)?;
            }
            Ok(())
        }
        Node::If(condition, yes, no) => {
            validate_codegen_expr(condition, Some(context), tt, file, tokens)?;
            validate_codegen_stmt(yes, context, tt, file, tokens)?;
            if let Some(no) = no {
                validate_codegen_stmt(no, context, tt, file, tokens)?;
            }
            Ok(())
        }
        Node::While(condition, body) | Node::DoWhile(body, condition) => {
            validate_codegen_expr(condition, Some(context), tt, file, tokens)?;
            validate_codegen_stmt(body, context, tt, file, tokens)
        }
        Node::For(init, condition, post, body) => {
            for expr in [init.as_deref(), condition.as_deref(), post.as_deref()]
                .into_iter()
                .flatten()
            {
                validate_codegen_expr(expr, Some(context), tt, file, tokens)?;
            }
            validate_codegen_stmt(body, context, tt, file, tokens)
        }
        Node::Switch(expr, body) | Node::Case(expr, body) => {
            validate_codegen_expr(expr, Some(context), tt, file, tokens)?;
            validate_codegen_stmt(body, context, tt, file, tokens)
        }
        Node::Default(body) | Node::Label(_, body) => {
            validate_codegen_stmt(body, context, tt, file, tokens)
        }
        other => codegen_error(
            node,
            file,
            tokens,
            format!("代码生成器无法处理语句节点: {:?}", other),
        ),
    }
}

#[derive(Default)]
struct BackendSymbols {
    defined_objects: HashSet<String>,
    declared_objects: HashSet<String>,
    defined_functions: HashSet<String>,
    declared_functions: HashSet<String>,
}

fn collect_backend_declarations(node: &Node, symbols: &mut BackendSymbols) {
    match node.inner() {
        Node::Decl { specs, name, .. } => {
            if specs.storage == Storage::Extern {
                symbols.declared_objects.insert(name.clone());
            }
        }
        Node::Block(nodes) | Node::DeclBlock(nodes) => {
            for child in nodes {
                collect_backend_declarations(child, symbols);
            }
        }
        Node::If(_, yes, no) => {
            collect_backend_declarations(yes, symbols);
            if let Some(no) = no {
                collect_backend_declarations(no, symbols);
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body)
        | Node::Label(_, body) => collect_backend_declarations(body, symbols),
        _ => {}
    }
}

fn collect_local_names(node: &Node, names: &mut HashSet<String>) {
    match node.inner() {
        Node::Decl { specs, name, .. }
            if !matches!(specs.storage, Storage::Extern | Storage::Typedef) =>
        {
            names.insert(name.clone());
        }
        Node::Block(nodes) | Node::DeclBlock(nodes) => {
            for child in nodes {
                collect_local_names(child, names);
            }
        }
        Node::If(_, yes, no) => {
            collect_local_names(yes, names);
            if let Some(no) = no {
                collect_local_names(no, names);
            }
        }
        Node::While(_, body)
        | Node::DoWhile(body, _)
        | Node::For(_, _, _, body)
        | Node::Switch(_, body)
        | Node::Case(_, body)
        | Node::Default(body)
        | Node::Label(_, body) => collect_local_names(body, names),
        _ => {}
    }
}

fn validate_backend_reference(
    node: &Node,
    locals: &HashSet<String>,
    symbols: &BackendSymbols,
    lib_symbols: Option<&HashSet<String>>,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    let recurse = |child: &Node| {
        validate_backend_reference(child, locals, symbols, lib_symbols, file, tokens)
    };
    match node.inner() {
        Node::Ident(name) => {
            if locals.contains(name) {
                return Ok(());
            }
            let unresolved_object =
                symbols.declared_objects.contains(name) && !symbols.defined_objects.contains(name);
            let unresolved_function = symbols.declared_functions.contains(name)
                && !symbols.defined_functions.contains(name);
            let supplied_by_lib = lib_symbols.map(|set| set.contains(name)).unwrap_or(false);
            if (unresolved_object || unresolved_function) && !supplied_by_lib {
                return codegen_error_at_text(
                    node,
                    file,
                    tokens,
                    name,
                    format!("当前工具链没有链接器，外部符号 '{}' 已声明但没有定义", name),
                );
            }
            Ok(())
        }
        Node::Unary(_, value) | Node::SizeofExpr(value) | Node::Cast(_, value) => recurse(value),
        Node::Binary(_, left, right)
        | Node::Assign(_, left, right)
        | Node::Subscript(left, right)
        | Node::Comma(left, right) => {
            recurse(left)?;
            recurse(right)
        }
        Node::Cond(condition, yes, no) => {
            recurse(condition)?;
            recurse(yes)?;
            recurse(no)
        }
        Node::Member(object, _, _) => recurse(object),
        Node::Call(callee, args) => {
            recurse(callee)?;
            for (index, argument) in args.iter().enumerate() {
                if matches!(callee.inner(), Node::Ident(name) if name == "va_arg") && index == 1 {
                    continue;
                }
                recurse(argument)?;
            }
            Ok(())
        }
        Node::InitList(items) | Node::Block(items) | Node::DeclBlock(items) => {
            for child in items {
                recurse(child)?;
            }
            Ok(())
        }
        Node::If(condition, yes, no) => {
            recurse(condition)?;
            recurse(yes)?;
            if let Some(no) = no {
                recurse(no)?;
            }
            Ok(())
        }
        Node::While(condition, body)
        | Node::DoWhile(body, condition)
        | Node::Switch(condition, body)
        | Node::Case(condition, body) => {
            recurse(condition)?;
            recurse(body)
        }
        Node::For(init, condition, post, body) => {
            for expr in [init.as_deref(), condition.as_deref(), post.as_deref()]
                .into_iter()
                .flatten()
            {
                recurse(expr)?;
            }
            recurse(body)
        }
        Node::Default(body)
        | Node::Label(_, body)
        | Node::ExprStmt(body)
        | Node::Return(Some(body)) => recurse(body),
        Node::Decl {
            init: Some(init), ..
        } => recurse(init),
        Node::Num(_)
        | Node::FNum(_)
        | Node::Str(_)
        | Node::Decl { init: None, .. }
        | Node::NullStmt
        | Node::Return(None)
        | Node::Break
        | Node::Continue
        | Node::Goto(_)
        | Node::FuncDecl { .. } => Ok(()),
        Node::FuncDef { body, .. } => recurse(body),
        other => codegen_error(
            node,
            file,
            tokens,
            format!("代码生成器无法检查符号引用节点: {:?}", other),
        ),
    }
}

fn validate_backend_symbols(
    items: &[Node],
    lib_symbols: Option<&HashSet<String>>,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    let mut symbols = BackendSymbols::default();
    for item in items {
        match item.inner() {
            Node::Decl { specs, name, .. } => {
                if specs.storage == Storage::Extern {
                    symbols.declared_objects.insert(name.clone());
                } else if specs.storage != Storage::Typedef {
                    symbols.defined_objects.insert(name.clone());
                }
            }
            Node::FuncDecl { name, .. } => {
                symbols.declared_functions.insert(name.clone());
            }
            Node::FuncDef { name, body, .. } => {
                symbols.defined_functions.insert(name.clone());
                collect_backend_declarations(body, &mut symbols);
            }
            _ => {}
        }
    }
    for item in items {
        match item.inner() {
            Node::Decl {
                init: Some(init), ..
            } => {
                validate_backend_reference(
                    init,
                    &HashSet::new(),
                    &symbols,
                    lib_symbols,
                    file,
                    tokens,
                )?;
            }
            Node::FuncDef { params, body, .. } => {
                let mut locals = params
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<HashSet<_>>();
                collect_local_names(body, &mut locals);
                validate_backend_reference(body, &locals, &symbols, lib_symbols, file, tokens)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_codegen_program(
    items: &[Node],
    tt: &TypeTable,
    lib_symbols: Option<&HashSet<String>>,
    file: &str,
    tokens: &[Token],
) -> CompileResult<()> {
    validate_backend_symbols(items, lib_symbols, file, tokens)?;
    for item in items {
        match item.inner() {
            Node::Decl { specs, init, .. } => {
                if specs.storage != Storage::Typedef {
                    validate_codegen_type(&specs.ty, item, tt, file, tokens)?;
                }
                if let Some(init) = init {
                    if struct_has_bitfields(tt, &specs.ty) {
                        return codegen_error(
                            item,
                            file,
                            tokens,
                            "位域结构体的初始化器暂不支持（C89 位域布局由实现定义）",
                        );
                    }
                    validate_codegen_initializer(init, None, tt, file, tokens)?;
                }
            }
            Node::FuncDecl { ret, params, .. } => {
                validate_codegen_type(ret, item, tt, file, tokens)?;
                for parameter in params {
                    validate_codegen_type(&parameter.ty, item, tt, file, tokens)?;
                }
            }
            Node::FuncDef {
                ret,
                params,
                is_variadic,
                body,
                ..
            } => {
                validate_codegen_type(ret, item, tt, file, tokens)?;
                for parameter in params {
                    validate_codegen_type(&parameter.ty, item, tt, file, tokens)?;
                }
                let mut automatic_objects = params
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<HashSet<_>>();
                collect_automatic_objects(body, &mut automatic_objects);
                let context = BuiltinContext {
                    is_variadic: *is_variadic,
                    fixed_params: params,
                    automatic_objects,
                };
                validate_codegen_stmt(body, &context, tt, file, tokens)?;
            }
            Node::NullStmt => {}
            other => {
                return codegen_error(
                    item,
                    file,
                    tokens,
                    format!("代码生成器无法处理顶层节点: {:?}", other),
                );
            }
        }
    }
    Ok(())
}

/// 便捷入口：AST → 汇编文本（tt：解析器收集的 struct/enum 定义）。
pub fn generate_asm_checked(items: &[Node], tt: &TypeTable, file: &str) -> CompileResult<String> {
    generate_asm_checked_with_tokens(items, tt, file, &[])
}

pub fn generate_asm_checked_with_tokens(
    items: &[Node],
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
) -> CompileResult<String> {
    generate_asm_checked_with_tokens_lib(items, tt, file, tokens, None)
}

/// 库链接模式的代码生成入口。
///
/// `lib_symbols` 为 Some(标准库公开符号集合) 时表示链接 c89lib：
/// 声明但未在本翻译单元定义的符号若属于该集合则放行（由拼接进来的库代码提供），
/// 且 main 的尾声改为 ret（返回给 crt0 的 `_start`）。
pub fn generate_asm_checked_with_tokens_lib(
    items: &[Node],
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
    lib_symbols: Option<&HashSet<String>>,
) -> CompileResult<String> {
    generate_asm_checked_with_tokens_lib_prefix(items, tt, file, tokens, lib_symbols, "")
}

/// 带标签前缀的库链接模式入口：多个翻译单元拼接时，前缀保证局部标签
/// （.L<n> / .Lstr<n> / .Lfloat<n> / .Lstatic_...）不互相冲突。
pub fn generate_asm_checked_with_tokens_lib_prefix(
    items: &[Node],
    tt: &TypeTable,
    file: &str,
    tokens: &[Token],
    lib_symbols: Option<&HashSet<String>>,
    label_prefix: &str,
) -> CompileResult<String> {
    validate_codegen_program(items, tt, lib_symbols, file, tokens)?;
    let stripped: Vec<Node> = items.iter().cloned().map(Node::without_spans).collect();
    let mut codegen = Codegen::new();
    codegen.tt = tt.clone();
    codegen.with_crt0 = lib_symbols.is_some();
    codegen.label_prefix = label_prefix.to_string();
    let asm = codegen.generate(&stripped);
    if let Some(message) = codegen.internal_error {
        let span = items
            .first()
            .and_then(Node::span)
            .cloned()
            .unwrap_or_else(|| Span::new(file, 1, 1));
        return Err(CompileError::codegen(span, message));
    }
    Ok(asm)
}

/// 无 token 上下文的结构化代码生成入口。
pub fn generate_asm(items: &[Node], tt: &TypeTable) -> CompileResult<String> {
    generate_asm_checked(items, tt, "<unknown>")
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    fn compile_error(source: &str) -> CompileError {
        crate::pipeline::compile_source_to_asm(source).unwrap_err()
    }

    fn assert_codegen_error(source: &str, line: u32, column: u32, message: &str) {
        let error = compile_error(source);
        assert_eq!(error.stage, crate::diagnostics::DiagnosticStage::Codegen);
        assert_eq!(error.span.file, "<source>");
        assert_eq!(error.span.line, line);
        assert_eq!(error.span.column, column);
        assert!(error.message.contains(message), "{}", error);
    }

    #[test]
    fn collects_string_literals_from_control_flow_and_aggregate_initializers() {
        let source = r#"
int first(char *text) { return text[0]; }
int main(void) {
    char *words[2] = {"foo", "bar"};
    if (first("A")) return words[0][0];
    return 0;
}
"#;
        let asm = crate::pipeline::compile_source_to_asm(source).unwrap();
        assert!(asm.contains(r#".asciiz "foo""#));
        assert!(asm.contains(r#".asciiz "bar""#));
        assert!(asm.contains(r#".asciiz "A""#));
    }

    #[test]
    fn rejects_unsupported_va_arg_type_without_returning_assembly() {
        assert_codegen_error(
            "int first(int count, ...) {\n    char *ap;\n    va_start(ap, count);\n    return va_arg(ap, char);\n}",
            4,
            23,
            "仅支持 int 和 double",
        );
    }

    #[test]
    fn unsupported_ast_node_is_a_codegen_error() {
        let node = Node::ExprStmt(Box::new(Node::Num(1))).with_span(Span::new("ast.c", 4, 9));
        let error = generate_asm_checked(&[node], &TypeTable::new(), "ast.c").unwrap_err();
        assert_eq!(error.stage, crate::diagnostics::DiagnosticStage::Codegen);
        assert_eq!(error.span, Span::new("ast.c", 4, 9));
        assert!(error.message.contains("顶层节点"));
    }

    #[test]
    fn rejects_undefined_external_function_without_returning_assembly() {
        assert_codegen_error(
            "int helper(int value);\nint main(void) { return helper(1); }",
            2,
            25,
            "没有链接器",
        );
    }

    #[test]
    fn treats_printf_as_an_ordinary_external_function() {
        assert_codegen_error(
            "int printf(const char *format, ...);\nint main(void) { return printf(\"%d\", 1); }",
            2,
            25,
            "外部符号 'printf'",
        );
    }

    #[test]
    fn rejects_undefined_external_object_without_returning_assembly() {
        assert_codegen_error(
            "extern int value;\nint main(void) { return value; }",
            2,
            25,
            "没有链接器",
        );
    }

    #[test]
    fn checked_codegen_accepts_break() {
        let node = Node::FuncDef {
            ret: Type::Int,
            name: "main".into(),
            storage: Storage::None,
            params: Vec::new(),
            is_variadic: false,
            has_prototype: true,
            body: Box::new(Node::Block(vec![Node::While(
                Box::new(Node::Num(1)),
                Box::new(Node::Break),
            )])),
        };
        let asm = generate_asm_checked(&[node], &TypeTable::new(), "break.c").unwrap();
        assert!(!asm.contains("break 未实现"));
    }
}
