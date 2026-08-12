//! 局部变量遮蔽（shadowing）支持。
//!
//! 语义检查与代码生成都用「扁平符号表」解析变量（同名后声明覆盖先声明），
//! 无法区分不同块作用域的同名变量。本 pass 在语义检查前把被遮蔽的局部变量
//! 重写为唯一名（`name#<n>`，`#` 不在 C 标识符字符集内，词法安全），
//! 使扁平符号表自然正确，语义层与代码生成层零改动。
//!
//! 只重命名「块作用域局部变量」；全局/函数名、未声明标识符、成员名、
//! 标签名保持原名。

use crate::ast::{Node, Storage};
use crate::diagnostics::{CompileError, CompileResult, Span};
use std::collections::HashSet;

/// 作用域栈：每层为「原名 → 当前可见名」映射。index 0 为函数参数层。
type Scope = Vec<(String, String)>;

/// 解包 `Located` 包装，返回最内层节点（可修改）。
fn inner_mut(node: &mut Node) -> &mut Node {
    let mut current = node;
    loop {
        match current {
            Node::Located(inner, _) => current = inner,
            _ => return current,
        }
    }
}

/// 在 AST 中把所有被遮蔽的局部变量重写为唯一名。
///
/// 同时实施两条作用域约束检查：
/// - 同一（最内层）作用域内重复声明同名普通局部变量 → 报「重复声明」；
/// - 块内声明出现在语句之后（C89 约束）→ 报「块内声明必须位于所有语句之前」。
pub fn rename_shadowed_locals(items: &mut [Node]) -> CompileResult<()> {
    for item in items {
        let Node::FuncDef {
            params, body, ..
        } = inner_mut(item)
        else {
            continue;
        };
        let mut scopes: Vec<Scope> = Vec::new();
        scopes.push(
            params
                .iter()
                .map(|param| (param.name.clone(), param.name.clone()))
                .collect(),
        );
        // 函数级已用名集合：扁平符号表是函数级共享的，无论嵌套还是兄弟作用域，
        // 同名局部变量都会互相覆盖，因此只要函数内出现过该名字就一律改唯一名。
        let mut used_names: HashSet<String> =
            params.iter().map(|param| param.name.clone()).collect();
        let mut counter = 0usize;
        rewrite(body, &mut scopes, &mut used_names, &mut counter)?;
    }
    Ok(())
}

/// 重写一个节点及其子树，维护作用域栈。
fn rewrite(
    node: &mut Node,
    scopes: &mut Vec<Scope>,
    used_names: &mut HashSet<String>,
    counter: &mut usize,
) -> CompileResult<()> {
    let entry_span = node.span().cloned().unwrap_or_else(Span::unknown);
    let node = inner_mut(node);
    match node {
        Node::Ident(name) => {
            // 从最内层作用域向外查：当前层优先（正确遮蔽）
            for scope in scopes.iter().rev() {
                if let Some((_, visible)) = scope.iter().rev().find(|(orig, _)| orig == name) {
                    *name = visible.clone();
                    break;
                }
            }
        }
        Node::Decl {
            specs,
            name,
            init,
        } => {
            // 只重命名普通局部变量；static/extern/typedef 不参与块遮蔽
            if !matches!(
                specs.storage,
                Storage::Static | Storage::Extern | Storage::Typedef
            ) {
                // 同一（最内层）作用域已有同名声明 → 重复声明（C89 约束违规），
                // 而不是把它改名成遮蔽变量。
                if let Some(scope) = scopes.last() {
                    if scope.iter().any(|(orig, _)| orig == name) {
                        return Err(CompileError::semantic(
                            entry_span.clone(),
                            format!("重复声明 '{name}'"),
                        ));
                    }
                }
                // 函数内已出现过该局部名（嵌套或兄弟）→ 改唯一名
                let shadowed = used_names.contains(name);
                if shadowed {
                    let original = name.clone();
                    let unique = format!("{}#{}", original, *counter);
                    *counter += 1;
                    *name = unique.clone();
                    used_names.insert(original.clone());
                    scopes
                        .last_mut()
                        .expect("decl 必然在某个块作用域内")
                        .push((original, unique));
                } else {
                    used_names.insert(name.clone());
                    scopes
                        .last_mut()
                        .expect("decl 必然在某个块作用域内")
                        .push((name.clone(), name.clone()));
                }
            }
            if let Some(init) = init {
                rewrite(init, scopes, used_names, counter)?;
            }
        }
        Node::Block(stmts) => {
            scopes.push(Vec::new());
            // C89 约束：块内声明必须位于所有语句之前。
            // 注意：多声明符声明（int a = 1, b = 2;）会被解析器展开成
            // 全由 Decl 组成的 Block，它本身仍是声明，不算是语句。
            let mut seen_statement = false;
            for statement in stmts.iter_mut() {
                let is_decl_like = matches!(statement.inner(), Node::Decl { .. })
                    || matches!(
                        statement.inner(),
                        Node::Block(children)
                            if children
                                .iter()
                                .all(|child| matches!(child.inner(), Node::Decl { .. }))
                    );
                if is_decl_like && seen_statement {
                    return Err(CompileError::semantic(
                        statement
                            .span()
                            .cloned()
                            .unwrap_or_else(Span::unknown),
                        "块内声明必须位于所有语句之前",
                    ));
                }
                rewrite(statement, scopes, used_names, counter)?;
                if !is_decl_like {
                    seen_statement = true;
                }
            }
            scopes.pop();
        }
        Node::If(cond, yes, no) => {
            rewrite(cond, scopes, used_names, counter)?;
            rewrite(yes, scopes, used_names, counter)?;
            if let Some(no) = no {
                rewrite(no, scopes, used_names, counter)?;
            }
        }
        Node::While(cond, body) | Node::Switch(cond, body) | Node::Case(cond, body) => {
            rewrite(cond, scopes, used_names, counter)?;
            rewrite(body, scopes, used_names, counter)?;
        }
        Node::DoWhile(body, cond) => {
            rewrite(body, scopes, used_names, counter)?;
            rewrite(cond, scopes, used_names, counter)?;
        }
        Node::For(init, cond, post, body) => {
            // for 语句自成一个作用域：init 声明与其 body 可见
            scopes.push(Vec::new());
            if let Some(init) = init {
                rewrite(init, scopes, used_names, counter)?;
            }
            if let Some(cond) = cond {
                rewrite(cond, scopes, used_names, counter)?;
            }
            if let Some(post) = post {
                rewrite(post, scopes, used_names, counter)?;
            }
            rewrite(body, scopes, used_names, counter)?;
            scopes.pop();
        }
        Node::Default(statement) | Node::Label(_, statement) => {
            rewrite(statement, scopes, used_names, counter)?
        }
        Node::Return(Some(expr)) | Node::ExprStmt(expr) | Node::SizeofExpr(expr) => {
            rewrite(expr, scopes, used_names, counter)?
        }
        Node::Unary(_, expr) | Node::Cast(_, expr) => rewrite(expr, scopes, used_names, counter)?,
        Node::Comma(left, right) => {
            rewrite(left, scopes, used_names, counter)?;
            rewrite(right, scopes, used_names, counter)?;
        }
        Node::Binary(_, left, right) | Node::Assign(_, left, right) => {
            rewrite(left, scopes, used_names, counter)?;
            rewrite(right, scopes, used_names, counter)?;
        }
        Node::Cond(cond, yes, no) => {
            rewrite(cond, scopes, used_names, counter)?;
            rewrite(yes, scopes, used_names, counter)?;
            rewrite(no, scopes, used_names, counter)?;
        }
        Node::Call(callee, args) => {
            rewrite(callee, scopes, used_names, counter)?;
            for argument in args {
                rewrite(argument, scopes, used_names, counter)?;
            }
        }
        Node::Subscript(array, index) => {
            rewrite(array, scopes, used_names, counter)?;
            rewrite(index, scopes, used_names, counter)?;
        }
        Node::Member(object, _, _) => rewrite(object, scopes, used_names, counter)?,
        Node::InitList(items) => {
            for item in items {
                rewrite(item, scopes, used_names, counter)?;
            }
        }
        Node::Break
        | Node::Continue
        | Node::NullStmt
        | Node::Goto(_)
        | Node::Num(_)
        | Node::FNum(_)
        | Node::Str(_)
        | Node::Return(None)
        | Node::FuncDef { .. }
        | Node::FuncDecl { .. } => {}
        Node::Located(..) => unreachable!("Located 已被 inner_mut 解包"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rename_shadowed_locals;
    use crate::pipeline::parse_source;

    #[test]
    fn shadowed_declaration_gets_unique_name() {
        let program = parse_source(
            r#"
	int main(void) {
	    int v;
	    v = 1;
	    {
	        char v[8];
	        v[0] = 'A';
	    }
	    v = 2;
	    return v;
	}
"#,
        )
        .unwrap();
        let mut items = program.items.clone();
        rename_shadowed_locals(&mut items).unwrap();
        // 外层 v 保留原名，内层 v 改为 v#0
        let src = format!("{items:?}");
        assert!(src.contains("v#0"), "内层遮蔽变量应被重写: {src}");
    }

    #[test]
    fn same_scope_duplicate_declaration_is_rejected() {
        let error = match parse_source("int main(void) { int x = 1; int x = 2; return x; }") {
            Err(error) => error,
            Ok(_) => panic!("同作用域重复声明应报错"),
        };
        assert!(error.message.contains("重复声明"), "{}", error);
        assert_eq!(
            error.stage,
            crate::diagnostics::DiagnosticStage::Semantic
        );
    }

    #[test]
    fn nested_scope_same_name_is_allowed() {
        let program = parse_source(
            "int main(void) { int x = 1; { int x = 2; if (x != 2) return 1; } return x; }",
        )
        .expect("嵌套作用域同名变量合法");
        let src = format!("{:?}", program.items);
        assert!(src.contains("x#0"), "嵌套同名变量应被重写: {src}");
    }

    #[test]
    fn declaration_after_statement_is_rejected() {
        let error = match parse_source("int main(void) { int x; x = 5; int y; return y; }") {
            Err(error) => error,
            Ok(_) => panic!("语句之后的声明应报错"),
        };
        assert!(
            error.message.contains("块内声明必须位于所有语句之前"),
            "{}",
            error
        );
    }

    #[test]
    fn declaration_at_block_start_is_allowed() {
        parse_source("int main(void) { int x = 1; int y = 2; x = x + y; return x; }")
            .expect("块首声明合法");
    }
}
