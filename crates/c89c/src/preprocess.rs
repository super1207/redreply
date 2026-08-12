//! 预处理阶段 4：预处理指令与宏展开
//!
//! 对照 C89 标准 §3.8：
//! - §3.8.3 Macro replacement：对象宏 + 函数宏
//! - §3.8.3.1 Argument substitution：参数替换（参数先展开）
//! - §3.8.3.2 The # operator：字符串化
//! - §3.8.3.3 The ## operator：记号粘贴
//! - §3.8.3.4 Rescanning and further replacement：重扫描 + 递归抑制
//! - §3.8.3.5 Scope of macro definitions：#undef

use crate::expr_eval::eval_expr;
use crate::lexer::{tokenize, TokKind, Token};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 条件编译栈的一层（#if/#ifdef/#ifndef）
#[derive(Clone)]
struct CondState {
    /// 本层是否输出
    taking: bool,
    /// 本层是否已有分支为真（#elif/#else 用）
    ever_true: bool,
}

/// 宏定义
#[derive(Clone)]
enum Macro {
    /// 对象宏：body 是 token 序列
    Object(Vec<Token>),
    /// 函数宏：参数列表 + 宏体
    Function {
        params: Vec<String>,
        body: Vec<Token>,
    },
}

/// 预处理器
pub struct Preprocessor {
    macros: HashMap<String, Macro>,
    /// 尖括号 <xxx.h> 的搜索目录
    include_dirs: Vec<PathBuf>,
    /// 正在处理的文件栈（循环包含检测）
    include_stack: Vec<PathBuf>,
    /// 内嵌头文件（基名 → 内容）：文件系统搜索不到时的兜底
    embedded_headers: Vec<(&'static str, &'static str)>,
    /// 内嵌头文件的包含栈（循环检测）
    embedded_stack: Vec<String>,
    /// 条件编译栈
    cond_stack: Vec<CondState>,
    /// 最近一次失败的结构化诊断；递归 include 会保留最深层文件位置。
    last_error: Option<crate::diagnostics::CompileError>,
}

impl Preprocessor {
    pub fn new() -> Self {
        Preprocessor {
            macros: HashMap::new(),
            include_dirs: Vec::new(),
            include_stack: Vec::new(),
            embedded_headers: Vec::new(),
            embedded_stack: Vec::new(),
            cond_stack: Vec::new(),
            last_error: None,
        }
    }

    /// 设置内嵌头文件（编译期嵌入的库头文件，基名 → 内容）
    pub fn set_embedded_headers(&mut self, files: &'static [(&'static str, &'static str)]) {
        self.embedded_headers = files.to_vec();
    }

    /// 添加尖括号 include 搜索目录
    pub fn add_include_dir(&mut self, dir: &str) {
        self.include_dirs.push(PathBuf::from(dir));
    }

    /// 预处理入口（无 include 场景，简单使用/测试）
    pub fn run(&mut self, source: &str) -> Result<Vec<Token>, String> {
        self.last_error = None;
        self.cond_stack.clear();
        self.run_source_named(source, Path::new(""), "<source>")
    }

    pub fn run_diagnostic(
        &mut self,
        source: &str,
    ) -> crate::diagnostics::CompileResult<Vec<Token>> {
        match self.run(source) {
            Ok(tokens) => Ok(tokens),
            Err(message) => Err(self.last_error.clone().unwrap_or_else(|| {
                let span = crate::diagnostics::legacy_span("<source>", &message);
                crate::diagnostics::CompileError::preprocess(span, message)
            })),
        }
    }

    /// 预处理命名内存源码（如内嵌的库源文件），name 用于诊断与 include 定位。
    pub fn run_named_diagnostic(
        &mut self,
        name: &str,
        source: &str,
    ) -> crate::diagnostics::CompileResult<Vec<Token>> {
        self.last_error = None;
        match self.run_source_named(source, Path::new(""), name) {
            Ok(tokens) => Ok(tokens),
            Err(message) => Err(self.last_error.clone().unwrap_or_else(|| {
                let span = crate::diagnostics::legacy_span(name, &message);
                crate::diagnostics::CompileError::preprocess(span, message)
            })),
        }
    }

    /// 预处理入口（支持 #include）：从文件开始
    pub fn run_file(&mut self, path: &str) -> Result<Vec<Token>, String> {
        self.last_error = None;
        let p = PathBuf::from(path);
        self.include_stack.clear();
        self.cond_stack.clear();
        let src =
            std::fs::read_to_string(&p).map_err(|e| format!("无法读取文件 {}: {}", path, e))?;
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        self.run_source_named(&src, &dir, path)
    }

    pub fn run_file_diagnostic(
        &mut self,
        path: &str,
    ) -> crate::diagnostics::CompileResult<Vec<Token>> {
        match self.run_file(path) {
            Ok(tokens) => Ok(tokens),
            Err(message) => Err(self.last_error.clone().unwrap_or_else(|| {
                let span = crate::diagnostics::legacy_span(path, &message);
                crate::diagnostics::CompileError::io(span, message)
            })),
        }
    }

    /// 处理一段源码（dir 为当前目录，用于解析相对 #include）
    fn run_source_named(
        &mut self,
        source: &str,
        dir: &Path,
        file: &str,
    ) -> Result<Vec<Token>, String> {
        match self.run_source_named_inner(source, dir, file) {
            Ok(tokens) => Ok(tokens),
            Err(message) => {
                if self.last_error.is_none() {
                    let span = crate::diagnostics::legacy_span(file, &message);
                    self.last_error =
                        Some(crate::diagnostics::CompileError::preprocess(span, &message));
                }
                Err(message)
            }
        }
    }

    fn run_source_named_inner(
        &mut self,
        source: &str,
        dir: &Path,
        file: &str,
    ) -> Result<Vec<Token>, String> {
        let tokens = match crate::lexer::tokenize_with_file(source, file) {
            Ok(tokens) => tokens,
            Err(message) => {
                let span = crate::lexer::lex_error_span(source, file, &message);
                self.last_error = Some(crate::diagnostics::CompileError::lex(span, &message));
                return Err(message);
            }
        };
        let mut out = Vec::new();
        let mut i = 0usize;
        // #line 偏移是本文件处理流程的局部状态：每个文件从 0 开始，
        // include 的进出由独立的 run_source_named 调用天然隔离，互不串扰。
        let mut line_shift: i64 = 0;
        // 待宏展开的普通 token 缓冲：宏展开在整个预处理记号流上进行
        // （C89 §3.8），换行只是空白——跨行的函数宏调用（F(1,\n2)）必须
        // 正常展开。指令（# 开头）是展开边界，保证 #define 等按位置生效。
        let mut pending: Vec<Token> = Vec::new();
        // 进入本文件时的条件编译深度：本文件新增的 #if 必须在本文件闭合。
        let cond_depth_at_entry = self.cond_stack.len();

        while i < tokens.len() {
            let t = &tokens[i];
            // 跳过 Newline 与 Eof：Eof 统一在末尾追加一个（此处若跳过会导致死循环）
            if t.kind == TokKind::Newline || t.kind == TokKind::Eof {
                i += 1;
                continue;
            }
            if t.kind == TokKind::Punct && t.text == "#" {
                // 指令边界：先冲刷此前积累的普通 token
                self.flush_pending(&mut pending, &mut out, line_shift)?;
                i += 1;
                // 条件编译指令需要特殊处理（即使在跳过状态也要维护结构）
                if let Some(dt) = tokens.get(i) {
                    match dt.text.as_str() {
                        "if" | "ifdef" | "ifndef" => {
                            i = self.handle_if(&tokens, i, line_shift)?;
                            continue;
                        }
                        "elif" => {
                            i = self.handle_elif(&tokens, i, line_shift)?;
                            continue;
                        }
                        "else" => {
                            i = self.handle_else(&tokens, i)?;
                            continue;
                        }
                        "endif" => {
                            i = self.handle_endif(&tokens, i)?;
                            continue;
                        }
                        _ => {
                            if self.is_active() {
                                let (next, mut inserted) =
                                    self.handle_directive(&tokens, i, dir, &mut line_shift)?;
                                i = next;
                                // include 内容插入当前位置；被包含文件末尾的 Eof 会泄漏
                                // 进主流，Parser 见到 Eof 会提前结束 → 静默截断后续源码
                                inserted.retain(|t| t.kind != TokKind::Eof);
                                out.append(&mut inserted);
                            } else {
                                // 跳过状态下忽略其他指令
                                while i < tokens.len() && tokens[i].kind != TokKind::Newline {
                                    i += 1;
                                }
                            }
                            continue;
                        }
                    }
                }
            }
            // 普通 token：进待展开缓冲（激活状态才收集）
            if self.is_active() {
                pending.push(tokens[i].clone());
            }
            i += 1;
        }

        // 冲刷剩余待展开 token
        self.flush_pending(&mut pending, &mut out, line_shift)?;
        // 本文件新增的 #if 未闭合 → 诊断（C89 §3.8.1 约束）
        if self.cond_stack.len() > cond_depth_at_entry {
            let last_line = tokens.last().map(|t| t.line).unwrap_or(1);
            return Err(format!("第 {} 行: #if 缺少匹配的 #endif", last_line));
        }

        let last_line = tokens.last().map(|t| t.line).unwrap_or(1);
        let mut eof = Token::new(TokKind::Eof, String::new(), last_line, false);
        eof.set_file(file);
        out.push(eof);
        Ok(out)
    }

    /// 宏展开并输出待处理的普通 token 流（跨行进行）。
    fn flush_pending(
        &self,
        pending: &mut Vec<Token>,
        out: &mut Vec<Token>,
        line_shift: i64,
    ) -> Result<(), String> {
        if pending.is_empty() {
            return Ok(());
        }
        let mut expanded = self.expand_with_shift(pending, line_shift)?;
        if line_shift != 0 {
            for t in &mut expanded {
                t.line = (t.line as i64 + line_shift).max(0) as u32;
            }
        }
        out.append(&mut expanded);
        pending.clear();
        Ok(())
    }

    /// 当前是否处于输出状态（所有条件层都激活）
    fn is_active(&self) -> bool {
        self.cond_stack.iter().all(|c| c.taking)
    }

    /// 宏是否"已定义"（含预定义宏 __FILE__ / __LINE__）
    fn is_macro_defined(&self, name: &str) -> bool {
        name == "__FILE__" || name == "__LINE__" || self.macros.contains_key(name)
    }

    /// 父层是否激活（栈去掉最后一层）
    fn parent_active(&self) -> bool {
        let n = self.cond_stack.len();
        self.cond_stack
            .iter()
            .take(n.saturating_sub(1))
            .all(|c| c.taking)
    }

    /// #if / #ifdef / #ifndef（i 指向指令名后的第一个 token）
    fn handle_if(&mut self, toks: &[Token], mut i: usize, line_shift: i64) -> Result<usize, String> {
        let name = toks.get(i).map(|t| t.text.clone()).unwrap_or_default();
        i += 1;
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        let parent = self.is_active();
        let cond;

        if name == "ifdef" || name == "ifndef" {
            let mt = toks
                .get(i)
                .ok_or_else(|| format!("第 {} 行: #{} 后缺少宏名", line, name))?;
            if mt.kind != TokKind::Ident {
                return Err(format!("第 {} 行: #{} 后应为标识符", line, name));
            }
            let defined = self.is_macro_defined(&mt.text);
            cond = if name == "ifdef" { defined } else { !defined };
            i += 1;
        } else {
            // #if 常量表达式
            let mut expr_toks = Vec::new();
            while i < toks.len() && toks[i].kind != TokKind::Newline {
                expr_toks.push(toks[i].clone());
                i += 1;
            }
            if parent {
                let expr_toks = self.replace_defined(&expr_toks)?;
                let expr_toks = self.expand_with_shift(&expr_toks, line_shift)?;
                cond = eval_expr(&expr_toks)
                    .map_err(|e| format!("第 {} 行: #if 表达式错误: {}", line, e))?
                    != 0;
            } else {
                // 跳过组内的 #if 控制表达式不得求值（C89 §3.8.1，
                // 与 #elif 的 need_eval 保护一致）：不报除零/移位越界等错误
                cond = false;
            }
        }
        // 消耗行尾
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            i += 1;
        }
        self.cond_stack.push(CondState {
            taking: parent && cond,
            ever_true: cond,
        });
        Ok(i)
    }

    /// #elif
    fn handle_elif(&mut self, toks: &[Token], mut i: usize, line_shift: i64) -> Result<usize, String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        i += 1;
        if self.cond_stack.is_empty() {
            return Err(format!("第 {} 行: #elif 没有匹配的 #if", line));
        }
        let parent = self.parent_active();
        // 收集 elif 表达式
        let mut expr_toks = Vec::new();
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            expr_toks.push(toks[i].clone());
            i += 1;
        }
        // 消耗行尾
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            i += 1;
        }
        // 先求值（避免 cond_stack 借用冲突）
        let need_eval = parent && !self.cond_stack.last().map(|s| s.ever_true).unwrap_or(true);
        let mut cond = false;
        if need_eval {
            let expr_toks = self.replace_defined(&expr_toks)?;
            let expr_toks = self.expand_with_shift(&expr_toks, line_shift)?;
            cond = eval_expr(&expr_toks)
                .map_err(|e| format!("第 {} 行: #elif 表达式错误: {}", line, e))?
                != 0;
        }
        let st = self.cond_stack.last_mut().unwrap();
        if need_eval {
            st.taking = cond;
            if cond {
                st.ever_true = true;
            }
        } else {
            st.taking = false;
        }
        Ok(i)
    }

    /// #else
    fn handle_else(&mut self, toks: &[Token], mut i: usize) -> Result<usize, String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        i += 1;
        if self.cond_stack.is_empty() {
            return Err(format!("第 {} 行: #else 没有匹配的 #if", line));
        }
        let parent = self.parent_active();
        let st = self.cond_stack.last_mut().unwrap();
        if parent && !st.ever_true {
            st.taking = true;
            st.ever_true = true;
        } else {
            st.taking = false;
        }
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            i += 1;
        }
        Ok(i)
    }

    /// #endif
    fn handle_endif(&mut self, toks: &[Token], mut i: usize) -> Result<usize, String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        i += 1;
        if self.cond_stack.is_empty() {
            return Err(format!("第 {} 行: #endif 没有匹配的 #if", line));
        }
        self.cond_stack.pop();
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            i += 1;
        }
        Ok(i)
    }

    /// 把 #if 表达式里的 defined(X) / defined X 替换为 1/0
    fn replace_defined(&self, toks: &[Token]) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            if toks[i].kind == TokKind::Ident && toks[i].text == "defined" {
                let mut name: Option<String> = None;
                let mut next = i + 1;
                if let Some(t) = toks.get(next) {
                    if t.kind == TokKind::Punct && t.text == "(" {
                        next += 1;
                        if let Some(t2) = toks.get(next) {
                            if t2.kind == TokKind::Ident {
                                name = Some(t2.text.clone());
                                next += 1;
                                if let Some(t3) = toks.get(next) {
                                    if t3.kind == TokKind::Punct && t3.text == ")" {
                                        next += 1;
                                    }
                                }
                            }
                        }
                    } else if t.kind == TokKind::Ident {
                        name = Some(t.text.clone());
                        next += 1;
                    }
                }
                let name =
                    name.ok_or_else(|| format!("第 {} 行: defined 用法错误", toks[i].line))?;
                let v = if self.is_macro_defined(&name) { 1 } else { 0 };
                let mut tok = Token::new(TokKind::Number, v.to_string(), toks[i].line, false);
                tok.set_file(toks[i].file.clone());
                out.push(tok);
                i = next;
            } else {
                out.push(toks[i].clone());
                i += 1;
            }
        }
        Ok(out)
    }

    /// 处理预处理指令（i 指向指令名），返回 (下一个 token 下标, 需要插入输出的 token)
    fn handle_directive(
        &mut self,
        toks: &[Token],
        mut i: usize,
        dir: &Path,
        line_shift: &mut i64,
    ) -> Result<(usize, Vec<Token>), String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        // 空指令 #（§3.8.7）：# 后直接换行或 EOF
        if toks
            .get(i)
            .map(|t| t.kind == TokKind::Newline)
            .unwrap_or(true)
        {
            return Ok((i, Vec::new()));
        }
        let name = toks.get(i).map(|t| t.text.clone()).unwrap_or_default();
        match name.as_str() {
            "define" => {
                i += 1;
                let next = self.parse_define(toks, i)?;
                Ok((next, Vec::new()))
            }
            "undef" => {
                i += 1;
                if let Some(t) = toks.get(i) {
                    if t.kind == TokKind::Ident {
                        self.macros.remove(&t.text);
                    }
                }
                while i < toks.len() && toks[i].kind != TokKind::Newline {
                    i += 1;
                }
                Ok((i, Vec::new()))
            }
            "include" => {
                i += 1;
                self.handle_include(toks, i, dir)
            }
            "error" => {
                i += 1;
                let mut msg = Vec::new();
                while i < toks.len() && toks[i].kind != TokKind::Newline {
                    msg.push(toks[i].text.clone());
                    i += 1;
                }
                Err(format!("第 {} 行: #error: {}", line, msg.join(" ")))
            }
            "pragma" => {
                // 未识别的 pragma 忽略（标准行为）
                while i < toks.len() && toks[i].kind != TokKind::Newline {
                    i += 1;
                }
                Ok((i, Vec::new()))
            }
            "line" => {
                i += 1;
                if let Some(t) = toks.get(i) {
                    if t.kind == TokKind::Number {
                        if let Ok(n) = t.text.parse::<u32>() {
                            // 当前物理行是 t.line，下一行开始应为 n
                            *line_shift = n as i64 - (t.line as i64 + 1);
                        }
                        i += 1;
                        // 可选文件名
                        if let Some(t2) = toks.get(i) {
                            if t2.kind == TokKind::String {
                                i += 1;
                            }
                        }
                    }
                }
                while i < toks.len() && toks[i].kind != TokKind::Newline {
                    i += 1;
                }
                Ok((i, Vec::new()))
            }
            "" => {
                // 空指令 #（§3.8.7）
                while i < toks.len() && toks[i].kind != TokKind::Newline {
                    i += 1;
                }
                Ok((i, Vec::new()))
            }
            other => Err(format!(
                "第 {} 行: 预处理指令 #{} 尚未实现（当前支持 #define / #undef / #include）",
                line, other
            )),
        }
    }

    /// 处理 #include，返回 (下一个 token 下标, 被包含文件展开后的 token)
    fn handle_include(
        &mut self,
        toks: &[Token],
        mut i: usize,
        dir: &Path,
    ) -> Result<(usize, Vec<Token>), String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        let t = toks
            .get(i)
            .ok_or_else(|| format!("第 {} 行: #include 后缺少文件名", line))?;

        let fname: String;
        let mut is_angle = false;
        if t.kind == TokKind::String {
            // #include "foo.h"
            fname = t.text.trim_matches('"').to_string();
            i += 1;
        } else if t.kind == TokKind::Punct && t.text == "<" {
            // #include <foo.h>：收集到 > 为止
            is_angle = true;
            let mut j = i + 1; // 跳过 < 本身
            let mut parts = Vec::new();
            let mut closed = false;
            while let Some(t2) = toks.get(j) {
                if t2.kind == TokKind::Punct && t2.text == ">" {
                    closed = true;
                    break;
                }
                if t2.kind == TokKind::Newline {
                    break;
                }
                parts.push(t2.text.clone());
                j += 1;
            }
            if !closed {
                return Err(format!("第 {} 行: #include <...> 缺少右尖括号", line));
            }
            fname = parts.join("");
            i = j + 1;
        } else {
            return Err(format!(
                "第 {} 行: #include 后应为 \"文件名\" 或 <文件名>",
                line
            ));
        }

        // 定位文件：双引号先当前目录，再搜索 include_dirs；尖括号只搜 include_dirs
        let mut found: Option<PathBuf> = None;
        if !is_angle {
            let p = dir.join(&fname);
            if p.is_file() {
                found = Some(p);
            }
        }
        if found.is_none() {
            for d in &self.include_dirs {
                let p = d.join(&fname);
                if p.is_file() {
                    found = Some(p);
                    break;
                }
            }
        }
        if let Some(path) = found {
            // 循环包含检测
            if self.include_stack.len() > 64 {
                return Err(format!("include 嵌套过深（疑似循环包含）: {}", fname));
            }
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let src = std::fs::read_to_string(&path)
                .map_err(|e| format!("无法读取头文件 {}: {}", fname, e))?;
            if self.include_stack.contains(&canonical) {
                // 互含头文件 + include guard：守卫宏已定义时本次包含是
                // 空操作（标准头文件惯用法，C89 §3.8.2 无栈内去重要求）
                let guarded = detect_include_guard(&src)
                    .map(|guard| self.macros.contains_key(&guard))
                    .unwrap_or(false);
                if guarded {
                    return Ok((i, Vec::new()));
                }
                return Err(format!("循环包含: {}", fname));
            }
            // 递归处理被包含文件（从阶段 1 到阶段 4）
            let sub_dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
            self.include_stack.push(canonical);
            let sub_tokens = self.run_source_named(&src, &sub_dir, &path.to_string_lossy())?;
            self.include_stack.pop();
            return Ok((i, sub_tokens));
        }

        // 文件系统找不到 → 内嵌头文件兜底（编译期嵌入的库头文件）
        for (name, content) in &self.embedded_headers {
            if *name == fname {
                if self.embedded_stack.len() > 64 {
                    return Err(format!("include 嵌套过深（疑似循环包含）: {}", fname));
                }
                if self.embedded_stack.contains(&fname) {
                    let guarded = detect_include_guard(content)
                        .map(|guard| self.macros.contains_key(&guard))
                        .unwrap_or(false);
                    if guarded {
                        return Ok((i, Vec::new()));
                    }
                    return Err(format!("循环包含: {}", fname));
                }
                self.embedded_stack.push(fname.clone());
                let sub_tokens = self.run_source_named(content, Path::new(""), &fname)?;
                self.embedded_stack.pop();
                return Ok((i, sub_tokens));
            }
        }

        Err(format!("第 {} 行: 找不到头文件 {}", line, fname))
    }

    /// 解析 #define（i 指向宏名）
    fn parse_define(&mut self, toks: &[Token], mut i: usize) -> Result<usize, String> {
        let line = toks.get(i).map(|t| t.line).unwrap_or(0);
        let name_tok = toks
            .get(i)
            .ok_or_else(|| format!("第 {} 行: #define 后缺少宏名", line))?;
        if name_tok.kind != TokKind::Ident {
            return Err(format!("第 {} 行: 宏名必须是标识符", line));
        }
        let name = name_tok.text.clone();
        i += 1;

        // 函数宏：宏名后紧跟 "(" 且无空白
        if let Some(t) = toks.get(i) {
            if t.kind == TokKind::Punct && t.text == "(" && !t.has_space {
                i += 1; // 跳过 (
                let mut params = Vec::new();
                // 空参数 ()
                if let Some(t) = toks.get(i) {
                    if t.kind == TokKind::Punct && t.text == ")" {
                        i += 1;
                        return self.finish_define(name, params, toks, i);
                    }
                }
                loop {
                    let p = toks.get(i).ok_or("宏参数列表未闭合")?;
                    if p.kind != TokKind::Ident {
                        return Err(format!("第 {} 行: 宏参数必须是标识符", p.line));
                    }
                    params.push(p.text.clone());
                    i += 1;
                    let sep = toks.get(i).ok_or("宏参数列表未闭合")?;
                    if sep.kind == TokKind::Punct && sep.text == "," {
                        i += 1;
                        continue;
                    }
                    if sep.kind == TokKind::Punct && sep.text == ")" {
                        i += 1;
                        break;
                    }
                    return Err(format!(
                        "第 {} 行: 宏参数列表格式错误（应为 , 或 )）",
                        sep.line
                    ));
                }
                return self.finish_define(name, params, toks, i);
            }
        }
        // 对象宏
        let mut body = Vec::new();
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            body.push(toks[i].clone());
            i += 1;
        }
        self.macros.insert(name, Macro::Object(body));
        Ok(i)
    }

    /// 函数宏：读宏体并登记
    fn finish_define(
        &mut self,
        name: String,
        params: Vec<String>,
        toks: &[Token],
        mut i: usize,
    ) -> Result<usize, String> {
        let mut body = Vec::new();
        while i < toks.len() && toks[i].kind != TokKind::Newline {
            body.push(toks[i].clone());
            i += 1;
        }
        self.macros.insert(name, Macro::Function { params, body });
        Ok(i)
    }

    /// 宏展开入口（无 #line 偏移）
    pub fn expand(&self, toks: &[Token]) -> Result<Vec<Token>, String> {
        self.expand_with_shift(toks, 0)
    }

    /// 带 #line 偏移的展开：__LINE__ 的取值需叠加偏移
    fn expand_with_shift(&self, toks: &[Token], line_shift: i64) -> Result<Vec<Token>, String> {
        let mut disabling = Vec::new();
        self.expand_inner(toks, &mut disabling, None, line_shift)
    }

    /// 展开核心：对象宏 + 函数宏 + 递归抑制
    ///
    /// `loc_override` 用于预定义宏 `__FILE__`/`__LINE__`：
    /// 宏体 token 自带定义处的行列，但标准要求它们展开为"调用处"的位置，
    /// 因此宏展开时把最外层调用点位置传入并覆盖。
    fn expand_inner(
        &self,
        toks: &[Token],
        disabling: &mut Vec<String>,
        loc_override: Option<(&str, u32)>,
        line_shift: i64,
    ) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let t = &toks[i];

            if t.kind == TokKind::Ident {
                // 预定义宏 __FILE__ / __LINE__（未出现在宏表中时）
                if !self.macros.contains_key(&t.text) {
                    if t.text == "__LINE__" {
                        let (file, raw_line) = loc_override.unwrap_or((t.file.as_str(), t.line));
                        // #line 偏移对 __LINE__ 的值生效（token.line 字段留给外层统一偏移）
                        let value = (raw_line as i64 + line_shift).max(0) as u32;
                        let mut tok = Token::new(
                            TokKind::Number,
                            value.to_string(),
                            t.line,
                            t.has_space,
                        );
                        tok.set_file(file);
                        out.push(tok);
                        i += 1;
                        continue;
                    }
                    if t.text == "__FILE__" {
                        let (file, _) = loc_override.unwrap_or((t.file.as_str(), t.line));
                        let escaped = file.replace('\\', "\\\\").replace('"', "\\\"");
                        let mut tok = Token::new(
                            TokKind::String,
                            format!("\"{}\"", escaped),
                            t.line,
                            t.has_space,
                        );
                        tok.set_file(file);
                        out.push(tok);
                        i += 1;
                        continue;
                    }
                }
                // 宏名后紧跟 ( → 函数宏调用（C89 §3.8.3：调用点允许空白，
                // "无空白"要求只适用于 #define 定义行）
                let is_func_call = toks
                    .get(i + 1)
                    .map(|n| n.kind == TokKind::Punct && n.text == "(")
                    .unwrap_or(false);

                if let Some(m) = self.macros.get(&t.text) {
                    if !disabling.contains(&t.text) {
                        match m {
                            Macro::Object(body) => {
                                // 宏体里的 __FILE__/__LINE__ 指向调用点（已有 override 则保持最外层调用点）
                                let call_loc = loc_override.or(Some((t.file.as_str(), t.line)));
                                disabling.push(t.text.clone());
                                let expanded = self.expand_inner(body, disabling, call_loc, line_shift)?;
                                disabling.pop();
                                out.extend(expanded);
                                i += 1;
                                continue;
                            }
                            Macro::Function { params, body } => {
                                if is_func_call {
                                    let (mut args, next) = collect_args(toks, i + 1)?;
                                    // F() 对零参数宏视为无实参（C89 §3.8.3）
                                    if args.len() == 1 && args[0].is_empty() && params.is_empty() {
                                        args.clear();
                                    }
                                    if args.len() != params.len() {
                                        return Err(format!(
                                            "第 {} 行: 宏 {} 参数数量不匹配（需要 {}，给了 {}）",
                                            t.line,
                                            t.text,
                                            params.len(),
                                            args.len()
                                        ));
                                    }
                                    // 函数宏调用点位置：宏体里的 __FILE__/__LINE__ 指向这里
                                    // （嵌套宏展开时保持最外层调用点，与 GCC 行为一致）
                                    let call_loc = loc_override.or(Some((t.file.as_str(), t.line)));
                                    disabling.push(t.text.clone());
                                    let mut new_body = Vec::new();
                                    // 参数替换（§3.8.3.1 参数先展开；# 不展开；## 相邻不展开）
                                    let mut j = 0;
                                    while j < body.len() {
                                        let bt = &body[j];
                                        // # 运算符：字符串化（§3.8.3.2）
                                        if bt.kind == TokKind::Punct && bt.text == "#" {
                                            if let Some(next_tok) = body.get(j + 1) {
                                                if next_tok.kind == TokKind::Ident {
                                                    if let Some(pi) = params
                                                        .iter()
                                                        .position(|p| p == &next_tok.text)
                                                    {
                                                        let s = stringize(&args[pi]);
                                                        new_body.push(Token::new(
                                                            TokKind::String,
                                                            s,
                                                            bt.line,
                                                            false,
                                                        ));
                                                        j += 2; // 跳过 # 和参数名
                                                        continue;
                                                    }
                                                }
                                            }
                                            new_body.push(bt.clone());
                                            j += 1;
                                            continue;
                                        }
                                        if bt.kind == TokKind::Ident {
                                            if let Some(pi) =
                                                params.iter().position(|p| p == &bt.text)
                                            {
                                                // 与 ## 相邻 → 不展开参数（等粘贴）
                                                let left_paste = j > 0
                                                    && body[j - 1].kind == TokKind::Punct
                                                    && body[j - 1].text == "##";
                                                let right_paste = body
                                                    .get(j + 1)
                                                    .map(|x| {
                                                        x.kind == TokKind::Punct && x.text == "##"
                                                    })
                                                    .unwrap_or(false);
                                                if left_paste || right_paste {
                                                    new_body.extend(args[pi].clone());
                                                } else {
                                                    // 参数先展开
                                                    new_body.extend(self.expand_inner(
                                                        &args[pi], disabling, call_loc, line_shift,
                                                    )?);
                                                }
                                                j += 1;
                                                continue;
                                            }
                                        }
                                        new_body.push(bt.clone());
                                        j += 1;
                                    }
                                    // ## 粘贴（§3.8.3.3）
                                    let new_body = self.apply_paste(new_body)?;
                                    let expanded =
                                        self.expand_inner(&new_body, disabling, call_loc, line_shift)?;
                                    disabling.pop();
                                    out.extend(expanded);
                                    i = next;
                                    continue;
                                }
                                // 函数宏但没有 ( → 不展开（标准行为）
                            }
                        }
                    }
                }
            }
            out.push(t.clone());
            i += 1;
        }
        Ok(out)
    }

    /// ## 记号粘贴：左右 token 文本拼接后重新 tokenize（§3.8.3.3）。
    /// 空操作数（CAT(x, )）时粘贴结果即另一侧操作数（标准行为）。
    fn apply_paste(&self, toks: Vec<Token>) -> Result<Vec<Token>, String> {
        let mut out: Vec<Token> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            if toks[i].kind == TokKind::Punct && toks[i].text == "##" {
                let left = out.pop();
                let right = toks.get(i + 1);
                match (left, right) {
                    (Some(left), Some(right)) => {
                        let joined = format!("{}{}", left.text, right.text);
                        let new_toks = tokenize(&joined)
                            .map_err(|e| format!("## 粘贴结果无效: {} ({})", joined, e))?;
                        let mut real: Vec<Token> = new_toks
                            .into_iter()
                            .filter(|t| t.kind != TokKind::Newline && t.kind != TokKind::Eof)
                            .collect();
                        if real.len() != 1 {
                            // 宽松 pp-number：主流编译器允许粘贴结果以符号结尾（如 1 ## + → 1+）
                            if !is_lenient_pp_number(&joined) {
                                return Err(format!("## 粘贴必须产生单个记号: {}", joined));
                            }
                            let mut merged =
                                Token::new(TokKind::Number, joined, left.line, left.has_space);
                            merged.column = left.column;
                            out.push(merged);
                            i += 2;
                            continue;
                        }
                        let mut merged = real.remove(0);
                        merged.line = left.line;
                        out.push(merged);
                        i += 2;
                    }
                    (Some(left), None) => {
                        // CAT(x, )：右操作数为空 → 粘贴结果即 x
                        out.push(left);
                        i += 1;
                    }
                    (None, Some(right)) => {
                        // CAT(, y)：左操作数为空 → 粘贴结果即 y
                        out.push(right.clone());
                        i += 2;
                    }
                    (None, None) => {
                        // CAT(, )：两侧皆空 → 结果为空
                        i += 1;
                    }
                }
            } else {
                out.push(toks[i].clone());
                i += 1;
            }
        }
        Ok(out)
    }
}

/// 宽松 pp-number 判定：digit 或 .digit 开头，后续允许数字/标识符/./e±/E±，
/// 并且允许以单个 + 或 - 结尾（GCC/Clang 对 ## 粘贴结果的通行处理）。
fn is_lenient_pp_number(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let mut i;
    if chars[0].is_ascii_digit() {
        i = 1;
    } else if chars[0] == '.' && chars.get(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
        i = 2;
    } else {
        return false;
    }
    while i < chars.len() {
        let c = chars[i];
        let ok = c.is_ascii_alphanumeric()
            || c == '_'
            || c == '.'
            || ((c == '+' || c == '-')
                && (chars[i - 1] == 'e' || chars[i - 1] == 'E' || i == chars.len() - 1));
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 收集宏调用的参数（处理嵌套括号），返回 (参数列表, 下一个 token 下标)
fn collect_args(toks: &[Token], open: usize) -> Result<(Vec<Vec<Token>>, usize), String> {
    let mut args: Vec<Vec<Token>> = Vec::new();
    let mut cur: Vec<Token> = Vec::new();
    let mut depth = 0usize;
    let mut i = open;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == TokKind::Punct && t.text == "(" {
            depth += 1;
            if depth > 1 {
                cur.push(t.clone());
            }
        } else if t.kind == TokKind::Punct && t.text == ")" {
            if depth == 0 {
                break;
            }
            depth -= 1;
            if depth == 0 {
                // C89 §3.8.3：F() 是一个空实参（F 为单参函数宏时）；
                // 零参数宏由调用方按空列表处理。
                args.push(cur);
                return Ok((args, i + 1));
            }
            cur.push(t.clone());
        } else if t.kind == TokKind::Punct && t.text == "," && depth == 1 {
            args.push(cur);
            cur = Vec::new();
        } else {
            cur.push(t.clone());
        }
        i += 1;
    }
    Err("宏调用缺少右括号 )".into())
}

/// 检测文件开头的 include guard（#ifndef G 紧跟 #define G），返回守卫宏名。
/// 用于互含头文件的循环包含判定：守卫已定义时重入是空操作。
fn detect_include_guard(src: &str) -> Option<String> {
    let tokens = crate::lexer::tokenize(src).ok()?;
    let mut idx = 0;
    // 第一个指令前的 token 只能是换行（行首空白由词法器吞掉）
    while idx < tokens.len() {
        let t = &tokens[idx];
        if t.kind == TokKind::Newline {
            idx += 1;
            continue;
        }
        if t.kind == TokKind::Punct && t.text == "#" {
            break;
        }
        return None;
    }
    if idx >= tokens.len() {
        return None;
    }
    let mut i = idx + 1; // 跳过 #
    if tokens.get(i).map(|t| t.text.as_str()) != Some("ifndef") {
        return None;
    }
    i += 1;
    let guard = tokens.get(i)?;
    if guard.kind != TokKind::Ident {
        return None;
    }
    let guard = guard.text.clone();
    // 下一行（或同行）必须是 #define guard
    let mut j = i + 1;
    while j < tokens.len() {
        let t = &tokens[j];
        if t.kind == TokKind::Punct && t.text == "#" {
            let name = tokens.get(j + 1).map(|t| t.text.as_str());
            if name != Some("define") {
                return None;
            }
            let defined = tokens.get(j + 2);
            if defined.map(|t| t.kind == TokKind::Ident && t.text == guard).unwrap_or(false) {
                return Some(guard);
            }
            return None;
        }
        j += 1;
    }
    None
}

/// # 字符串化：参数 token → 带引号字符串（§3.8.3.2）
fn stringize(toks: &[Token]) -> String {
    let inner: Vec<String> = toks.iter().map(|t| t.text.clone()).collect();
    let joined = inner.join(" ");
    let escaped = joined.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

impl Default for Preprocessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预处理并返回展开后的文本列表（去掉 Eof）
    fn texts(src: &str) -> Vec<String> {
        let mut pp = Preprocessor::new();
        pp.run(src)
            .unwrap()
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect()
    }

    // ---- 对象宏（回归）----
    #[test]
    fn test_object_macro() {
        assert_eq!(texts("#define X 42\nX"), ["42"]);
    }
    #[test]
    fn test_macro_self_reference() {
        assert_eq!(texts("#define X X\nX"), ["X"]);
    }
    #[test]
    fn test_macro_mutual_reference() {
        assert_eq!(texts("#define A B\n#define B A\nA"), ["A"]);
    }
    #[test]
    fn test_macro_undef() {
        assert_eq!(texts("#define X 42\n#undef X\nX"), ["X"]);
    }
    #[test]
    fn test_macro_in_string() {
        assert_eq!(texts("#define X 42\n\"X\""), ["\"X\""]);
    }

    // ---- 函数宏 ----
    #[test]
    fn test_func_macro_basic() {
        assert_eq!(
            texts("#define SQUARE(x) ((x)*(x))\nSQUARE(3)"),
            ["(", "(", "3", ")", "*", "(", "3", ")", ")"]
        );
    }
    #[test]
    fn test_func_macro_arg_expr() {
        // 参数是表达式：SQUARE(1+2) → ((1+2)*(1+2))
        assert_eq!(
            texts("#define SQUARE(x) ((x)*(x))\nSQUARE(1+2)"),
            ["(", "(", "1", "+", "2", ")", "*", "(", "1", "+", "2", ")", ")"]
        );
    }
    #[test]
    fn test_func_macro_arg_expands_first() {
        // 参数先展开：F(A) 且 A=3 → 3（§3.8.3.1）
        assert_eq!(texts("#define A 3\n#define F(x) x\nF(A)"), ["3"]);
    }
    #[test]
    fn test_func_macro_nested() {
        // 宏展开后重扫描继续展开（§3.8.3.4）：B(1) → A(1) → 1+1
        assert_eq!(
            texts("#define A(x) x+1\n#define B(x) A(x)\nB(1)"),
            ["1", "+", "1"]
        );
        // 自我嵌套被禁用集抑制：F(F(1)) → F(1)+F(1)（标准行为）
        assert_eq!(
            texts("#define F(x) x+x\nF(F(1))"),
            ["F", "(", "1", ")", "+", "F", "(", "1", ")"]
        );
    }
    #[test]
    fn test_func_macro_without_paren() {
        // 函数宏名后无 ( → 不展开
        assert_eq!(texts("#define F(x) x+1\nF"), ["F"]);
    }
    #[test]
    fn test_object_macro_with_space() {
        // #define F (x) x → 对象宏（宏名后有空格）→ F 展开为 ( x ) x
        assert_eq!(texts("#define F (x) x\nF"), ["(", "x", ")", "x"]);
    }
    #[test]
    fn test_stringize() {
        // # 字符串化（§3.8.3.2）
        assert_eq!(texts("#define S(x) #x\nS(hello)"), ["\"hello\""]);
        assert_eq!(texts("#define S(x) #x\nS(1 + 2)"), ["\"1 + 2\""]);
    }
    #[test]
    fn test_paste() {
        // ## 粘贴（§3.8.3.3）
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(ho,me)"), ["home"]);
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(12,34)"), ["1234"]);
        // C89：.5 不是单个 pp-number，1 ## . 产生 1.，随后 5 独立
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(1,.5)"), ["1.", "5"]);
        // C89 下拼出 1.5 的写法：1. ## 5
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(1.,5)"), ["1.5"]);
        // 宽松扩展（GCC/Clang 通行）：1 ## + → 1+
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(1,+)"), ["1+"]);
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(1,-)"), ["1-"]);
        assert_eq!(texts("#define CAT(a,b) a##b\nCAT(0x,1e)"), ["0x1e"]);
        // 仍然拒绝非法拼接
        assert!(Preprocessor::new()
            .run("#define CAT(a,b) a##b\nCAT(1,++)")
            .is_err());
    }
    #[test]
    fn test_paste_macro_arg() {
        // ## 相邻的参数不展开（§3.8.3.3）：CAT(A,2) → A2
        assert_eq!(
            texts("#define A 1\n#define CAT(a,b) a##b\nCAT(A,2)"),
            ["A2"]
        );
    }
    #[test]
    fn test_arg_count_mismatch() {
        let mut pp = Preprocessor::new();
        assert!(pp.run("#define F(x) x\nF(1,2)").is_err());
    }
    #[test]
    fn test_nested_paren_args() {
        // 嵌套括号的参数：F((1+2)) → (1+2)
        assert_eq!(texts("#define F(x) x\nF((1+2))"), ["(", "1", "+", "2", ")"]);
    }
    #[test]
    fn test_empty_args() {
        assert_eq!(texts("#define F() 42\nF()"), ["42"]);
    }
    // ---- #include 测试（需要真实文件）----

    fn inc_dir(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("inc")
            .join(name)
    }

    #[test]
    fn test_include_basic() {
        let dir = inc_dir("basic");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("defs.h"), "#define ANSWER 42\n").unwrap();
        std::fs::write(dir.join("main.c"), "#include \"defs.h\"\nANSWER\n").unwrap();
        let mut pp = Preprocessor::new();
        let toks = pp.run_file(dir.join("main.c").to_str().unwrap()).unwrap();
        let texts: Vec<String> = toks
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(texts, ["42"]);
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("defs.h"));
    }

    #[test]
    fn test_include_nested() {
        // inner.h 再 include defs.h（嵌套包含）
        let dir = inc_dir("nested");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("defs.h"), "#define A 1\n").unwrap();
        std::fs::write(dir.join("inner.h"), "#include \"defs.h\"\n#define B A\n").unwrap();
        std::fs::write(dir.join("main.c"), "#include \"inner.h\"\nB\n").unwrap();
        let mut pp = Preprocessor::new();
        let toks = pp.run_file(dir.join("main.c").to_str().unwrap()).unwrap();
        let texts: Vec<String> = toks
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(texts, ["1"]); // B → A → 1
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("inner.h"));
        let _ = std::fs::remove_file(dir.join("defs.h"));
    }

    #[test]
    fn test_include_angle() {
        // 尖括号 include 搜索 include_dirs
        let dir = inc_dir("angle");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("std.h"), "#define N 7\n").unwrap();
        std::fs::write(dir.join("main.c"), "#include <std.h>\nN\n").unwrap();
        let mut pp = Preprocessor::new();
        pp.add_include_dir(dir.to_str().unwrap());
        let toks = pp.run_file(dir.join("main.c").to_str().unwrap()).unwrap();
        let texts: Vec<String> = toks
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(texts, ["7"]);
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("std.h"));
    }

    #[test]
    fn test_line_shift_is_per_file_state() {
        // #line 100 的文件 include 另一文件：两文件行号互不串扰
        let dir = inc_dir("lineshift");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sub.h"), "SUB_TOKEN
").unwrap();
        std::fs::write(
            dir.join("main.c"),
            "#line 100
#include \"sub.h\"
AFTER_INCLUDE
",
        )
        .unwrap();
        let mut pp = Preprocessor::new();
        let toks = pp.run_file(dir.join("main.c").to_str().unwrap()).unwrap();
        let sub = toks.iter().find(|t| t.text == "SUB_TOKEN").unwrap();
        assert_eq!(sub.line, 1, "被包含文件行号应从 1 开始，不受 #line 串扰");
        let after = toks.iter().find(|t| t.text == "AFTER_INCLUDE").unwrap();
        assert_eq!(after.line, 101, "主文件 include 之后的行号应延续 #line 偏移");
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("sub.h"));
    }

    #[test]
    fn test_line_shift_reset_between_files() {
        // 连续预处理两个文件：第二个从正常行号开始
        let dir = inc_dir("lineshift2");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.c"), "#line 100
FIRST
").unwrap();
        std::fs::write(dir.join("b.c"), "SECOND
").unwrap();
        let mut pp = Preprocessor::new();
        let toks_a = pp.run_file(dir.join("a.c").to_str().unwrap()).unwrap();
        let first = toks_a.iter().find(|t| t.text == "FIRST").unwrap();
        assert_eq!(first.line, 100);
        let toks_b = pp.run_file(dir.join("b.c").to_str().unwrap()).unwrap();
        let second = toks_b.iter().find(|t| t.text == "SECOND").unwrap();
        assert_eq!(second.line, 1, "第二个文件行号应从头开始");
        let _ = std::fs::remove_file(dir.join("a.c"));
        let _ = std::fs::remove_file(dir.join("b.c"));
    }

    #[test]
    fn test_line_expansion_keeps_real_file_name() {
        // __LINE__ 展开出的数字 token 必须携带当前真实文件名（而不是 <source>）
        let dir = inc_dir("linefile");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.c");
        std::fs::write(&path, "int v = __LINE__;
").unwrap();
        let mut pp = Preprocessor::new();
        let toks = pp.run_file(path.to_str().unwrap()).unwrap();
        let num = toks
            .iter()
            .find(|t| t.kind == TokKind::Number && t.text == "1")
            .expect("__LINE__ 应展开为 1");
        assert_eq!(
            num.file,
            path.to_str().unwrap(),
            "__LINE__ 展开 token 应携带真实文件名"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_include_cycle_detected() {
        // a.h ↔ b.h 循环包含 → 报错
        let dir = inc_dir("cycle");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.h"), "#include \"b.h\"\n").unwrap();
        std::fs::write(dir.join("b.h"), "#include \"a.h\"\n").unwrap();
        std::fs::write(dir.join("main.c"), "#include \"a.h\"\nint x;\n").unwrap();
        let mut pp = Preprocessor::new();
        let res = pp.run_file(dir.join("main.c").to_str().unwrap());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("循环包含"));
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("a.h"));
        let _ = std::fs::remove_file(dir.join("b.h"));
    }

    #[test]
    fn test_include_macro_visible_after() {
        // include 文件里的宏在 include 之后可用
        let dir = inc_dir("visible");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.h"), "#define LIB_VAL 99\n").unwrap();
        std::fs::write(dir.join("main.c"), "#include \"lib.h\"\nLIB_VAL + 1\n").unwrap();
        let mut pp = Preprocessor::new();
        let toks = pp.run_file(dir.join("main.c").to_str().unwrap()).unwrap();
        let texts: Vec<String> = toks
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(texts, ["99", "+", "1"]);
        let _ = std::fs::remove_file(dir.join("main.c"));
        let _ = std::fs::remove_file(dir.join("lib.h"));
    }

    // ---- 条件编译测试 ----

    #[test]
    fn test_ifdef() {
        assert_eq!(texts("#define X 1\n#ifdef X\nYES\n#endif"), ["YES"]);
        assert_eq!(texts("#ifdef Y\nYES\n#endif"), Vec::<String>::new()); // Y 未定义
    }
    #[test]
    fn test_ifndef() {
        assert_eq!(texts("#ifndef Z\nNO_Z\n#endif"), ["NO_Z"]);
        assert_eq!(
            texts("#define Z 1\n#ifndef Z\nNO_Z\n#endif"),
            Vec::<String>::new()
        );
    }
    #[test]
    fn test_ifdef_else() {
        assert_eq!(texts("#ifdef X\nA\n#else\nB\n#endif"), ["B"]);
        assert_eq!(texts("#define X 1\n#ifdef X\nA\n#else\nB\n#endif"), ["A"]);
    }
    #[test]
    fn test_if_expr() {
        assert_eq!(texts("#if 1 + 2 * 3 == 7\nOK\n#endif"), ["OK"]);
        assert_eq!(texts("#if 0\nNO\n#endif"), Vec::<String>::new());
        assert_eq!(texts("#if 3 > 5\nNO\n#endif"), Vec::<String>::new());
        assert_eq!(texts("#if (1 || 0) && !0\nOK\n#endif"), ["OK"]);
    }
    #[test]
    fn test_if_defined() {
        assert_eq!(texts("#define A 1\n#if defined(A)\nOK\n#endif"), ["OK"]);
        assert_eq!(texts("#if defined(B)\nNO\n#endif"), Vec::<String>::new());
        assert_eq!(texts("#if defined B\nOK\n#endif"), Vec::<String>::new()); // B 未定义
    }
    #[test]
    fn test_if_macro_in_expr() {
        // #if 表达式里的宏会展开
        assert_eq!(texts("#define N 5\n#if N > 3\nBIG\n#endif"), ["BIG"]);
    }
    #[test]
    fn test_elif() {
        let src = "#if 0\nA\n#elif 1\nB\n#else\nC\n#endif";
        assert_eq!(texts(src), ["B"]);
        // 多级 elif
        let src2 = "#define X 2\n#if X == 1\nA\n#elif X == 2\nB\n#elif X == 3\nC\n#else\nD\n#endif";
        assert_eq!(texts(src2), ["B"]);
    }
    #[test]
    fn test_cond_nested() {
        let src = "#if 1\n#ifdef A\nINNER\n#endif\nOUTER\n#endif";
        assert_eq!(texts(src), ["OUTER"]);
    }
    #[test]
    fn test_skip_define_not_effective() {
        // 跳过区里的 #define 不生效
        let src = "#if 0\n#define X 1\n#endif\nX";
        assert_eq!(texts(src), ["X"]); // X 未定义，保持 X
    }
    #[test]
    fn test_header_guard() {
        // 经典头文件 guard 模式
        let src = "#ifndef MY_H\n#define MY_H\nint guarded;\n#endif\nint after;";
        assert_eq!(texts(src), ["int", "guarded", ";", "int", "after", ";"]);
    }
    #[test]
    fn test_endif_without_if() {
        let mut pp = Preprocessor::new();
        assert!(pp.run("#endif").is_err());
    }
    #[test]
    fn test_char_in_if() {
        assert_eq!(texts("#if 'a' == 97\nOK\n#endif"), ["OK"]);
    }

    // ---- 收尾指令测试 ----

    #[test]
    fn test_error_directive() {
        let mut pp = Preprocessor::new();
        let res = pp.run("#error custom error message");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("custom error message"));
    }

    #[test]
    fn test_pragma_ignored() {
        // 未知 pragma 忽略
        assert_eq!(texts("#pragma once\nint x;"), ["int", "x", ";"]);
    }

    #[test]
    fn test_null_directive() {
        // 单独一个 # 是空指令
        assert_eq!(texts("#\nint x;"), ["int", "x", ";"]);
    }

    #[test]
    fn test_error_in_skipped() {
        // 跳过状态下的 #error 不触发
        let src = "#if 0\n#error should not fire\n#endif\nint x;";
        assert_eq!(texts(src), ["int", "x", ";"]);
    }

    #[test]
    fn test_line_directive() {
        // #line 100 → 后续 token 行号从 100 起
        let src = "int a;\n#line 100\nint b;";
        let toks = Preprocessor::new().run(src).unwrap();
        let b_line = toks.iter().find(|t| t.text == "b").map(|t| t.line);
        assert_eq!(b_line, Some(100));
    }

    #[test]
    fn test_line_with_file() {
        // #line 200 "foo.c"（带文件名）
        let src = "#line 200 \"foo.c\"\nint c;";
        let toks = Preprocessor::new().run(src).unwrap();
        let c_line = toks.iter().find(|t| t.text == "c").map(|t| t.line);
        assert_eq!(c_line, Some(200));
    }

    // ---- 预定义宏 __FILE__ / __LINE__ ----

    #[test]
    fn test_file_line_top_level() {
        // 顶层使用：__LINE__ = 出现行号，__FILE__ = 当前文件名
        let src = "int a;\n__LINE__\n__FILE__";
        let mut pp = Preprocessor::new();
        let toks = pp.run(src).unwrap();
        let texts: Vec<String> = toks
            .iter()
            .filter(|t| t.kind != TokKind::Eof)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(texts, ["int", "a", ";", "2", "\"<source>\""]);
    }

    #[test]
    fn test_file_line_in_macro_body() {
        // 宏体里的 __LINE__/__FILE__ 展开为调用点的位置
        let src = "int a;\n#define HERE __LINE__\nHERE\n#define WHERE __FILE__\nWHERE";
        assert_eq!(texts(src), ["int", "a", ";", "3", "\"<source>\""]);
    }

    #[test]
    fn test_file_line_in_function_macro() {
        // 函数宏：__LINE__ 指向调用行
        let src = "int a;\n#define F(x) __LINE__\nF(1)\n#define G(x) x\nG(__LINE__)";
        assert_eq!(texts(src), ["int", "a", ";", "3", "5"]);
    }

    #[test]
    fn test_file_line_defined_checks() {
        // #ifdef __LINE__ 与 defined(__FILE__) 均为真
        let src = "#ifdef __LINE__\nYES\n#endif\n#if defined(__FILE__)\nALSO\n#endif";
        assert_eq!(texts(src), ["YES", "ALSO"]);
    }

    #[test]
    fn test_line_number_after_line_directive() {
        // #line 改变行号后，__LINE__ 跟随
        let src = "int a;\n#line 100\n__LINE__";
        assert_eq!(texts(src), ["int", "a", ";", "100"]);
    }
}
