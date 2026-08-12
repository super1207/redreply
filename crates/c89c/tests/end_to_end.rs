//! 端到端测试：C 源码 → c89c 编译 → rv-core 模拟器执行

use c89c::codegen::generate_asm;
use c89c::lexer::TokKind;
use c89c::parser::Parser;
use c89c::preprocess::Preprocessor;
use rv_core::assembler::assemble;
use rv_core::cpu::Cpu;

/// 编译并运行 C 源码，返回退出码
fn run_c(src: &str) -> i32 {
    // 1. 预处理
    let toks = Preprocessor::new().run(src).unwrap();
    let toks: Vec<_> = toks
        .into_iter()
        .filter(|t| t.kind != TokKind::Eof)
        .collect();
    // 2. 语法分析
    let mut p = Parser::new(toks);
    let items = p.parse_program().unwrap();
    // 3. 代码生成
    let asm = generate_asm(&items, &p.tt).unwrap();
    // 4. rv-core 汇编
    let prog = assemble(&asm).unwrap();
    // 5. 模拟器执行
    let mut cpu = Cpu::new();
    cpu.load_program(&prog).unwrap();
    cpu.run(&prog.instructions, 10_000_000).unwrap();
    cpu.exit_code
}

#[test]
fn test_return_42() {
    assert_eq!(run_c("int main(void) { return 42; }"), 42);
}

#[test]
fn test_add_function() {
    assert_eq!(
        run_c("int add(int a, int b) { return a + b; } int main(void) { return add(3, 4); }"),
        7
    );
}

#[test]
fn test_local_vars() {
    assert_eq!(
        run_c("int main(void) { int x = 3; int y = 4; return x * y; }"),
        12
    );
}

#[test]
fn test_if_else() {
    assert_eq!(
        run_c("int main(void) { int x = 5; if (x > 3) return 1; else return 0; }"),
        1
    );
    assert_eq!(
        run_c("int main(void) { int x = 2; if (x > 3) return 1; else return 0; }"),
        0
    );
}

#[test]
fn test_while_sum() {
    assert_eq!(
        run_c("int main(void) { int i = 0; int s = 0; while (i < 10) { s = s + i; i = i + 1; } return s; }"),
        45
    );
}

#[test]
fn test_for_sum() {
    assert_eq!(
        run_c("int main(void) { int s = 0; int i; for (i = 1; i <= 5; i = i + 1) s = s + i; return s; }"),
        15
    );
}

#[test]
fn test_fibonacci() {
    assert_eq!(
        run_c("int fib(int n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); } int main(void) { return fib(10); }"),
        55
    );
}

#[test]
fn test_global_var() {
    assert_eq!(run_c("int g; int main(void) { g = 7; return g; }"), 7);
}

#[test]
fn test_cond_expr() {
    assert_eq!(run_c("int main(void) { return 1 ? 11 : 22; }"), 11);
    assert_eq!(run_c("int main(void) { return 0 ? 11 : 22; }"), 22);
}

#[test]
fn test_comparisons() {
    assert_eq!(run_c("int main(void) { return 3 == 3; }"), 1);
    assert_eq!(run_c("int main(void) { return 3 != 3; }"), 0);
    assert_eq!(run_c("int main(void) { return 5 <= 5; }"), 1);
    assert_eq!(run_c("int main(void) { return 5 >= 6; }"), 0);
}

#[test]
fn test_compound_assign() {
    assert_eq!(run_c("int main(void) { int x = 1; x += 4; return x; }"), 5);
}

#[test]
fn test_do_while() {
    assert_eq!(
        run_c("int main(void) { int i = 0; do { i = i + 1; } while (i < 3); return i; }"),
        3
    );
}

#[test]
fn test_nested_calls() {
    assert_eq!(
        run_c("int sq(int x) { return x * x; } int main(void) { return sq(sq(2)) + 1; }"),
        17
    );
}

// ---- P4-3 指针测试 ----

#[test]
fn test_pointer_deref() {
    assert_eq!(
        run_c("int main(void) { int x = 42; int *p = &x; return *p; }"),
        42
    );
}

#[test]
fn test_pointer_store() {
    assert_eq!(
        run_c("int main(void) { int x = 10; int *p = &x; *p = 7; return x; }"),
        7
    );
}

#[test]
fn test_pointer_param() {
    assert_eq!(
        run_c("int get(int *p) { return *p; } int main(void) { int x = 99; return get(&x); }"),
        99
    );
}

#[test]
fn test_pointer_arith() {
    // 局部变量布局：x 在 s0+0，y 在 s0+4 → p+1 指向 y
    assert_eq!(
        run_c("int main(void) { int x = 3; int y = 4; int *p = &x; return *(p + 1); }"),
        4
    );
}

#[test]
fn test_pointer_swap() {
    assert_eq!(
        run_c("int main(void) { int a = 1; int b = 2; int *p = &a; int *q = &b; int t = *p; *p = *q; *q = t; return a * 10 + b; }"),
        21
    );
}

#[test]
fn test_pointer_to_pointer() {
    assert_eq!(
        run_c("int main(void) { int x = 5; int *p = &x; int **pp = &p; return **pp; }"),
        5
    );
}

// ---- P4-4 数组测试 ----

#[test]
fn test_array_basic() {
    assert_eq!(
        run_c(
            "int main(void) { int a[3]; a[0] = 1; a[1] = 2; a[2] = 4; return a[0] + a[1] + a[2]; }"
        ),
        7
    );
}

#[test]
fn test_array_init() {
    assert_eq!(
        run_c("int main(void) { int a[3] = {1, 2, 3}; return a[0] + a[2]; }"),
        4
    );
}

#[test]
fn test_array_size_infer() {
    // int a[] = {5,6,7} → 自动推断大小 3
    assert_eq!(
        run_c("int main(void) { int a[] = {5, 6, 7}; return a[2]; }"),
        7
    );
}

#[test]
fn test_array_loop_sum() {
    assert_eq!(
        run_c("int main(void) { int a[5] = {1,2,3,4,5}; int s = 0; int i; for (i = 0; i < 5; i = i + 1) s = s + a[i]; return s; }"),
        15
    );
}

#[test]
fn test_global_array() {
    assert_eq!(
        run_c("int g[3]; int main(void) { g[1] = 9; return g[1]; }"),
        9
    );
}

#[test]
fn test_array_and_pointer() {
    // 数组名退化为指针
    assert_eq!(
        run_c("int main(void) { int a[2] = {3, 4}; int *p = a; return *(p + 1); }"),
        4
    );
}

// ---- P4-5 结构体测试 ----

#[test]
fn test_struct_basic() {
    assert_eq!(
        run_c("struct Point { int x; int y; }; int main(void) { struct Point p; p.x = 3; p.y = 4; return p.x + p.y; }"),
        7
    );
}

#[test]
fn test_struct_pointer_arrow() {
    assert_eq!(
        run_c("struct P { int a; int b; }; int main(void) { struct P p; struct P *q = &p; q->a = 5; q->b = 6; return q->a * 10 + q->b; }"),
        56
    );
}

#[test]
fn test_struct_init_member() {
    // struct 按值传参暂不支持（TODO），用指针传参
    assert_eq!(
        run_c("struct R { int w; int h; }; int area(struct R *r) { return r->w * r->h; } int main(void) { struct R r; r.w = 3; r.h = 5; return area(&r); }"),
        15
    );
}

#[test]
fn test_struct_in_struct() {
    assert_eq!(
        run_c("struct Inner { int v; }; struct Outer { struct Inner i; int tag; }; int main(void) { struct Outer o; o.i.v = 8; o.tag = 2; return o.i.v + o.tag; }"),
        10
    );
}

#[test]
fn test_struct_array_member() {
    assert_eq!(
        run_c("struct S { int data[3]; }; int main(void) { struct S s; s.data[0] = 1; s.data[1] = 2; s.data[2] = 3; return s.data[0] + s.data[1] + s.data[2]; }"),
        6
    );
}

#[test]
fn test_union_basic() {
    assert_eq!(
        run_c("union U { char c; int i; }; int main(void) { union U u; u.i = 65; return u.i; }"),
        65
    );
}

// ---- P4-6 枚举/cast/sizeof 测试 ----

#[test]
fn test_enum_basic() {
    assert_eq!(
        run_c("enum Color { RED, GREEN, BLUE }; int main(void) { return GREEN; }"),
        1
    );
    assert_eq!(
        run_c("enum Color { RED, GREEN, BLUE }; int main(void) { return BLUE + RED; }"),
        2
    );
}

#[test]
fn test_enum_explicit_values() {
    assert_eq!(
        run_c("enum C { A = 10, B, D = 20 }; int main(void) { return B + D; }"),
        31
    );
}

#[test]
fn test_cast_char() {
    // (char)300 → 300 & 0xFF = 44
    assert_eq!(run_c("int main(void) { return (char)300; }"), 44);
}

#[test]
fn test_cast_pointer() {
    assert_eq!(
        run_c("int main(void) { int x = 7; int *p = &x; char *c = (char*)p; return *c; }"),
        7
    );
}

#[test]
fn test_sizeof_int() {
    assert_eq!(run_c("int main(void) { return sizeof(int); }"), 4);
    assert_eq!(run_c("int main(void) { return sizeof(char); }"), 1);
    assert_eq!(run_c("int main(void) { return sizeof(double); }"), 8);
}

#[test]
fn test_sizeof_struct() {
    assert_eq!(
        run_c("struct P { int x; int y; }; int main(void) { return sizeof(struct P); }"),
        8
    );
}

#[test]
fn test_sizeof_array() {
    assert_eq!(run_c("int main(void) { int a[5]; return sizeof(a); }"), 20);
}

#[test]
fn test_sizeof_expr() {
    assert_eq!(run_c("int main(void) { int x; return sizeof x; }"), 4);
}

// ---- P7 浮点字面量测试 ----

#[test]
fn test_float_literal_decl() {
    // 浮点字面量可存可读
    assert_eq!(
        run_c("int main(void) { float f = 3.14; return (int)f; }"),
        3
    );
}

#[test]
fn test_float_add() {
    // float 加法：1.5 + 2.25 = 3.75 → (int)3
    assert_eq!(
        run_c("int main(void) { float x = 1.5; float y = 2.25; float z = x + y; return (int)z; }"),
        3
    );
}

#[test]
fn test_float_mul() {
    assert_eq!(
        run_c("int main(void) { float f = 1.5; return (int)(f * 2.0); }"),
        3
    );
}

#[test]
fn test_float_div() {
    assert_eq!(run_c("int main(void) { return (int)(7.0 / 2.0); }"), 3);
}

#[test]
fn test_float_neg() {
    assert_eq!(
        run_c("int main(void) { float f = 3.0; return (int)(0.0 - f); }"),
        -3
    );
}

#[test]
fn test_float_mixed_with_int() {
    // float 与 int 混合：int 转 float 后运算（简化：int 按位模式当 float？）
    // 1.5 + 2 = 3.5 → 3
    assert_eq!(
        run_c("int main(void) { float x = 1.5; int n = 2; return (int)(x + n); }"),
        3
    );
}

// ---- 变参测试 ----

#[test]
fn test_variadic_sum() {
    assert_eq!(
        run_c("int sum(int count, ...) { char *ap; int total = 0; int i; va_start(ap, count); for (i = 0; i < count; i = i + 1) { total = total + va_arg(ap, int); } va_end(ap); return total; } int main(void) { return sum(4, 10, 20, 30, 40); }"),
        100
    );
}

#[test]
fn test_variadic_zero() {
    assert_eq!(
        run_c("int sum(int count, ...) { char *ap; int total = 0; int i; va_start(ap, count); for (i = 0; i < count; i = i + 1) { total = total + va_arg(ap, int); } va_end(ap); return total; } int main(void) { return sum(0); }"),
        0
    );
}

#[test]
fn test_variadic_double() {
    // 变参 double 求和（取整）
    assert_eq!(
        run_c("double dsum(int count, ...) { char *ap; double total = 0.0; int i; va_start(ap, count); for (i = 0; i < count; i = i + 1) { total = total + va_arg(ap, double); } va_end(ap); return total; } int main(void) { return (int)dsum(3, 1.5, 2.25, 3.25); }"),
        7
    );
}

// ---- struct 按值传参测试 ----

#[test]
fn test_struct_by_value() {
    assert_eq!(
        run_c("struct Point { int x; int y; }; int area(struct Point p) { return p.x * p.y; } int main(void) { struct Point q; q.x = 3; q.y = 5; return area(q); }"),
        15
    );
}

#[test]
fn test_struct_by_value_copy() {
    // 按值语义：被调方修改副本不影响调用方
    assert_eq!(
        run_c("struct P { int a; }; int f(struct P p) { p.a = 100; return p.a; } int main(void) { struct P q; q.a = 7; int r = f(q); return r * 10 + q.a; }"),
        1007
    );
}

#[test]
fn test_struct_by_value_two_params() {
    // 两个 struct 参数
    assert_eq!(
        run_c("struct P { int v; }; int add(struct P a, struct P b) { return a.v + b.v; } int main(void) { struct P x; struct P y; x.v = 11; y.v = 22; return add(x, y); }"),
        33
    );
}

#[test]
fn test_break_and_continue() {
    assert_eq!(
        run_c(
            "int main(void) { int i = 0; while (1) { i = i + 1; if (i == 3) break; } return i; }"
        ),
        3
    );
    assert_eq!(run_c("int main(void) { int i = 0; int sum = 0; for (i = 0; i < 6; i = i + 1) { if (i == 2) continue; sum += i; } return sum; }"), 13);
}

#[test]
fn test_do_while_continue() {
    assert_eq!(run_c("int main(void) { int i = 0; do { i = i + 1; if (i < 3) continue; } while (i < 4); return i; }"), 4);
}

#[test]
fn test_goto_and_label() {
    assert_eq!(
        run_c("int main(void) { int x = 0; goto done; x = 99; done: x = 7; return x; }"),
        7
    );
}

#[test]
fn test_switch_fallthrough_and_break() {
    assert_eq!(run_c("int main(void) { int x = 2; int y = 0; switch (x) { case 1: y = 10; break; case 2: y = 20; case 3: y += 3; break; default: y = 99; } return y; }"), 23);
    assert_eq!(
        run_c("int main(void) { int x = 8; switch (x) { case 1: return 1; default: return 9; } }"),
        9
    );
}

#[test]
fn test_nested_loop_break_targets_innermost_loop() {
    assert_eq!(run_c("int main(void) { int i = 0; int j = 0; int n = 0; while (i < 3) { j = 0; while (j < 5) { if (j == 2) break; n += 1; j = j + 1; } i = i + 1; } return n; }"), 6);
}

#[test]
fn test_while_continue_targets_condition() {
    assert_eq!(run_c("int main(void) { int i = 0; int n = 0; while (i < 5) { i = i + 1; if (i == 3) continue; n += i; } return n; }"), 12);
}

#[test]
fn test_backward_goto() {
    assert_eq!(
        run_c("int main(void) { int i = 0; again: i = i + 1; if (i < 4) goto again; return i; }"),
        4
    );
}

#[test]
fn test_switch_without_matching_case_or_default() {
    assert_eq!(
        run_c("int main(void) { int y = 7; switch (9) { case 1: y = 1; } return y; }"),
        7
    );
}

#[test]
fn test_nested_switches_have_independent_cases() {
    assert_eq!(run_c("int main(void) { int y = 0; switch (1) { case 1: switch (2) { case 1: y = 10; break; case 2: y = 20; break; } y += 3; break; default: y = 99; } return y; }"), 23);
}

#[test]
fn test_switch_break_only_exits_switch() {
    assert_eq!(run_c("int main(void) { int i = 0; int n = 0; while (i < 3) { switch (i) { case 1: break; default: n += 2; } n += 1; i = i + 1; } return n; }"), 7);
}

#[test]
fn test_continue_inside_switch_targets_enclosing_loop() {
    assert_eq!(run_c("int main(void) { int i = 0; int n = 0; while (i < 4) { i = i + 1; switch (i) { case 2: continue; default: n += i; } } return n; }"), 8);
}

#[test]
fn test_pre_and_post_increment_values_and_writeback() {
    assert_eq!(
        run_c(
            "int main(void) { int x = 3; int a = x++; int b = ++x; return a * 100 + b * 10 + x; }"
        ),
        355
    );
    assert_eq!(
        run_c(
            "int main(void) { int x = 5; int a = x--; int b = --x; return a * 100 + b * 10 + x; }"
        ),
        533
    );
}

#[test]
fn test_increment_writes_back_complex_lvalues_once() {
    assert_eq!(run_c("int main(void) { int a[3]; int i = 0; int old; a[0] = 4; old = a[i++]++; return old * 100 + a[0] * 10 + i; }"), 451);
    assert_eq!(
        run_c("int main(void) { int x = 8; int old; int *p = &x; old = (*p)--; return old * 10 + x; }"),
        87
    );
    assert_eq!(run_c("struct S { int x; }; int main(void) { struct S s; s.x = 6; ++s.x; return s.x * 10 + s.x; }"), 77);
}

#[test]
fn test_pointer_increment_uses_pointee_size() {
    assert_eq!(
        run_c("int main(void) { int a[3]; int *p = a; a[0] = 2; a[1] = 7; p++; return *p; }"),
        7
    );
    assert_eq!(
        run_c("int main(void) { double a[2]; double *p = a; a[1] = 9.0; ++p; return (int)*p; }"),
        9
    );
}

#[test]
fn test_pointer_addition_and_difference_semantics() {
    assert_eq!(
        run_c("int main(void) { int a[5]; int *p = 2 + a; *p = 7; return a[2]; }"),
        7
    );
    assert_eq!(
        run_c("int main(void) { int a[5]; int *p = a + 4; int *q = a + 1; return p - q; }"),
        3
    );
}

#[test]
fn test_logical_operators_short_circuit_side_effects() {
    assert_eq!(run_c("int main(void) { int x = 0; int a = 0 && (x = 3); int b = 1 || (x = 5); return a * 100 + b * 10 + x; }"), 10);
    assert_eq!(run_c("int mark(int *p) { *p += 1; return 1; } int main(void) { int x = 0; if (0 && mark(&x)) x = 9; if (1 || mark(&x)) x += 2; return x; }"), 2);
}

#[test]
fn test_logical_results_are_normalized() {
    assert_eq!(
        run_c("int main(void) { return (3 && 4) * 10 + (0 || 7); }"),
        11
    );
    assert_eq!(
        run_c("int main(void) { double x = 0.0; double y = 2.5; return (!x) * 10 + (!!y); }"),
        11
    );
}

#[test]
fn test_conditional_evaluates_only_selected_branch() {
    assert_eq!(
        run_c("int main(void) { int x = 0; int y = 1 ? (x = 4) : (x = 8); return y * 10 + x; }"),
        44
    );
    assert_eq!(
        run_c("int main(void) { int x = 0; int y = 0 ? (x = 4) : (x = 8); return y * 10 + x; }"),
        88
    );
}

#[test]
fn test_conditional_applies_common_arithmetic_type() {
    assert_eq!(run_c("int main(void) { double x = 1 ? 3 : 4.5; double y = 0 ? 3 : 4.5; return (int)(x * 10.0 + y); }"), 34);
}

#[test]
fn test_compound_assignment_evaluates_lhs_once() {
    assert_eq!(
        run_c(
            "int main(void) { int a[2]; int i = 0; a[0] = 4; a[i++] += 3; return a[0] * 10 + i; }"
        ),
        71
    );
}

#[test]
fn test_pointer_compound_assignment_scales_offset() {
    assert_eq!(
        run_c("int main(void) { int a[4]; int *p = a; a[2] = 9; p += 2; return *p; }"),
        9
    );
    assert_eq!(
        run_c("int main(void) { int a[4]; int *p = a + 3; a[1] = 6; p -= 2; return *p; }"),
        6
    );
}

#[test]
fn test_float_lvalues_and_conditions() {
    assert_eq!(run_c("struct S { double x; }; int main(void) { struct S s; s.x = 2.0; s.x++; return (int)s.x; }"), 3);
    assert_eq!(
        run_c("int main(void) { double a[2]; a[0] = 1.5; a[0] += 2.5; return (int)a[0]; }"),
        4
    );
    assert_eq!(
        run_c("int main(void) { double x = 0.5; if (x) return 7; return 3; }"),
        7
    );
}

#[test]
fn test_multiple_declarators_keep_independent_types_and_initializers() {
    assert_eq!(run_c("int main(void) { int a = 1, b = 2, *p = &b, xs[3] = {3, 4, 5}; return a + *p + xs[2]; }"), 8);
}

#[test]
fn test_global_multiple_declarators() {
    assert_eq!(
        run_c("int a = 3, b = 4, xs[2] = {5, 6}; int main(void) { return a + b + xs[1]; }"),
        13
    );
}

#[test]
fn test_forward_double_function_signature_prescan() {
    assert_eq!(
        run_c(
            "int main(void) { return (int)later(2.0); } double later(double x) { return x + 3.5; }"
        ),
        5
    );
}

#[test]
fn test_global_scalar_initializers() {
    assert_eq!(run_c("int gi = 7; double gd = 2.5; char gc = 3; int main(void) { return gi + (int)gd + gc; }"), 12);
}

#[test]
fn test_global_partial_array_initializer() {
    assert_eq!(run_c("int xs[4] = {2, 3}; double ds[3] = {1.5, 2.5}; int main(void) { return xs[0] * 10 + xs[2] + (int)(ds[0] + ds[1] + ds[2]); }"), 24);
}

#[test]
fn test_global_struct_initializer() {
    assert_eq!(run_c("struct Pair { int x; int y; }; struct Pair p = {3, 4}; int main(void) { return p.x * 10 + p.y; }"), 34);
}

#[test]
fn test_local_double_array_initializer() {
    assert_eq!(run_c("int main(void) { double ds[3] = {1.0, 2.5}; return (int)(ds[0] * 10.0 + ds[1] + ds[2]); }"), 12);
}

#[test]
fn test_local_struct_and_partial_zero_initializer() {
    assert_eq!(run_c("struct Pair { int x; int y; }; int main(void) { struct Pair p = {3}; int xs[3] = {4}; return p.x * 100 + p.y * 10 + xs[0] + xs[2]; }"), 304);
}

#[test]
fn test_nested_aggregate_initializer() {
    assert_eq!(run_c("struct Pair { int x; int y; }; int main(void) { struct Pair ps[2] = {{1, 2}, {3, 4}}; return ps[0].y * 10 + ps[1].x; }"), 23);
}

#[test]
fn test_local_char_array_string_initializer() {
    assert_eq!(
        run_c("int main(void) { char s[5] = \"abc\"; return s[0] + s[3] + s[4]; }"),
        97
    );
}

#[test]
fn test_odd_sized_struct_parameter_uses_padded_slot() {
    assert_eq!(
        run_c(
            "struct Tiny { char xs[3]; }; int pick(struct Tiny x) { return x.xs[0] * 100 + x.xs[2]; } int main(void) { struct Tiny value; value.xs[0] = 'a'; value.xs[1] = 'b'; value.xs[2] = 'c'; return pick(value); }"
        ),
        9799
    );
}

#[test]
fn test_local_nested_array_initializer_without_inner_braces() {
    assert_eq!(
        run_c(
            "struct Tiny { char xs[3]; }; int main(void) { struct Tiny value = {'a', 'b', 'c'}; return value.xs[0] * 100 + value.xs[2]; }"
        ),
        9799
    );
}

#[test]
fn test_global_nested_array_initializer_without_inner_braces() {
    assert_eq!(
        run_c(
            "struct Tiny { char xs[3]; }; struct Tiny value = {'a', 'b', 'c'}; int main(void) { return value.xs[0] * 100 + value.xs[2]; }"
        ),
        9799
    );
}

#[test]
fn test_omitted_braces_respect_following_struct_member_and_padding() {
    let local = "struct Packet { char xs[3]; int tail; }; int main(void) { struct Packet value = {'a', 'b', 'c', 7}; return value.xs[0] + value.xs[2] + value.tail; }";
    let global = "struct Packet { char xs[3]; int tail; }; struct Packet value = {'a', 'b', 'c', 7}; int main(void) { return value.xs[0] + value.xs[2] + value.tail; }";
    assert_eq!(run_c(local), 203);
    assert_eq!(run_c(global), 203);
}

#[test]
fn test_omitted_braces_fill_array_of_structs_in_subobject_order() {
    assert_eq!(
        run_c(
            "struct Pair { int x; int y; }; int main(void) { struct Pair ps[2] = {1, 2, 3, 4}; return ps[0].y * 10 + ps[1].x; }"
        ),
        23
    );
}

#[test]
fn test_omitted_braces_partial_initializer_zero_fills_nested_objects() {
    let local = "struct Values { int xs[2]; int tail; }; int main(void) { struct Values value = {5}; return value.xs[0] * 100 + value.xs[1] * 10 + value.tail; }";
    let global = "struct Values { int xs[2]; int tail; }; struct Values value = {5}; int main(void) { return value.xs[0] * 100 + value.xs[1] * 10 + value.tail; }";
    assert_eq!(run_c(local), 500);
    assert_eq!(run_c(global), 500);
}

#[test]
fn test_integer_arguments_overflow_to_stack() {
    assert_eq!(
        run_c(
            "int pick(int a0,int a1,int a2,int a3,int a4,int a5,int a6,int a7,int a8,int a9) { return a8 * 10 + a9; } int main(void) { return pick(1,2,3,4,5,6,7,8,9,4); }"
        ),
        94
    );
}

#[test]
fn test_float_arguments_overflow_to_stack() {
    assert_eq!(
        run_c(
            "double pick(double f0,double f1,double f2,double f3,double f4,double f5,double f6,double f7,double f8,double f9,double f10,double f11,double f12,double f13,double f14,double f15,double f16,double f17,double f18,double f19,double f20,double f21,double f22,double f23) { return f22 + f23; } int main(void) { return (int)pick(1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0,19.0,20.0,21.0,22.0,23.0,24.0); }"
        ),
        47
    );
}

#[test]
fn test_mixed_arguments_share_ordered_overflow_stack_slots() {
    assert_eq!(
        run_c(
            "int pick(int a0,double f0,int a1,double f1,int a2,double f2,int a3,double f3,int a4,double f4,int a5,double f5,int a6,double f6,int a7,double f7,double f8,double f9,double f10,double f11,double f12,double f13,double f14,double f15,double f16,double f17,double f18,double f19,double f20,double f21,int a8,double f22,int a9,double f23) { return a8 + a9 + (int)f22 + (int)f23; } int main(void) { return pick(1,1.0,2,2.0,3,3.0,4,4.0,5,5.0,6,6.0,7,7.0,8,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0,19.0,20.0,21.0,22.0,9,23.0,10,24.0); }"
        ),
        66
    );
}

#[test]
fn test_struct_return_uses_hidden_result_pointer() {
    assert_eq!(
        run_c(
            "struct Pair { int x; int y; }; struct Pair make(int x, int y) { struct Pair value = {x, y}; return value; } int main(void) { return make(3, 4).x * 10 + make(5, 6).y; }"
        ),
        36
    );
}

#[test]
fn test_struct_return_shifts_integer_arguments_and_overflows_to_stack() {
    assert_eq!(
        run_c(
            "struct Result { int value; }; struct Result make(int a0,int a1,int a2,int a3,int a4,int a5,int a6,int a7,int a8) { struct Result result = {a7 * 10 + a8}; return result; } int main(void) { return make(1,2,3,4,5,6,7,8,9).value; }"
        ),
        89
    );
}

#[test]
fn test_struct_return_inside_loop_reuses_frame_temporary() {
    assert_eq!(
        run_c(
            "struct Value { int x; }; struct Value make(int x) { struct Value value = {x}; return value; } int main(void) { int i; int sum = 0; for (i = 0; i < 1000; i++) sum += make(1).x; return sum; }"
        ),
        1000
    );
}

#[test]
fn test_function_pointer_indirect_call() {
    assert_eq!(
        run_c(
            "int add(int a, int b) { return a + b; } int main(void) { int (*fp)(int, int) = add; return fp(20, 22); }"
        ),
        42
    );
}

#[test]
fn test_explicitly_dereferenced_function_pointer_call() {
    assert_eq!(
        run_c(
            "int sub(int a, int b) { return a - b; } int main(void) { int (*fp)(int, int) = sub; return (*fp)(50, 8); }"
        ),
        42
    );
}

#[test]
fn test_function_pointer_float_return_and_stack_arguments() {
    assert_eq!(
        run_c(
            "double sum(int a0,int a1,int a2,int a3,int a4,int a5,int a6,int a7,int a8,double x) { return a8 + x; } int main(void) { double (*fp)(int,int,int,int,int,int,int,int,int,double) = sum; return (int)fp(1,2,3,4,5,6,7,8,9,2.5); }"
        ),
        11
    );
}

#[test]
fn test_function_pointer_struct_return() {
    assert_eq!(
        run_c(
            "struct Pair { int x; int y; }; struct Pair make(int x, int y) { struct Pair value = {x, y}; return value; } int main(void) { struct Pair (*fp)(int, int) = make; return fp(4, 2).x * 10 + fp(1, 7).y; }"
        ),
        47
    );
}

#[test]
fn test_global_function_pointer_initializer() {
    assert_eq!(
        run_c(
            "int mul(int a, int b) { return a * b; } int (*operation)(int, int) = mul; int main(void) { return operation(6, 7); }"
        ),
        42
    );
}

#[test]
fn test_struct_return_initialization_and_assignment() {
    assert_eq!(
        run_c(
            "struct Pair { int x; int y; }; struct Pair make(int x, int y) { struct Pair value = {x, y}; return value; } int main(void) { struct Pair a = make(3, 4); struct Pair b; b = make(5, 6); return a.y * 10 + b.x; }"
        ),
        45
    );
}

#[test]
fn test_struct_return_passed_directly_by_value() {
    assert_eq!(
        run_c(
            "struct Pair { int x; int y; }; struct Pair make(int x, int y) { struct Pair value = {x, y}; return value; } int sum(struct Pair value) { return value.x + value.y; } int main(void) { return sum(make(20, 22)); }"
        ),
        42
    );
}

#[test]
fn test_prototype_drives_argument_conversion_and_register_class() {
    assert_eq!(
        run_c(
            "double add(double a, double b) { return a + b; } int take_int(int value) { return value; } int main(void) { return (int)add(2, 3) * 10 + take_int(4.75); }"
        ),
        54
    );
}

#[test]
fn test_old_style_function_definition_end_to_end() {
    assert_eq!(
        run_c(
            "int add(a, b) int a; int b; { return a + b; } int main(void) { return add(20, 22); }"
        ),
        42
    );
}

#[test]
fn test_variadic_after_fixed_register_overflow() {
    assert_eq!(
        run_c(
            "int sum9(int a0,int a1,int a2,int a3,int a4,int a5,int a6,int a7,int a8, ...) { int ap; int x; va_start(ap, a8); x = va_arg(ap, int); va_end(ap); return a8 * 10 + x; } int main(void) { return sum9(1,2,3,4,5,6,7,8,9,4); }"
        ),
        94
    );
}

#[test]
fn test_variadic_after_fixed_float_register_overflow() {
    assert_eq!(
        run_c(
            "int read(double f0,double f1,double f2,double f3,double f4,double f5,double f6,double f7,double f8,double f9,double f10,double f11,double f12,double f13,double f14,double f15,double f16,double f17,double f18,double f19,double f20,double f21,double f22, ...) { int ap; va_start(ap, f22); return (int)va_arg(ap, double) + (int)f22; } int main(void) { return read(1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0,19.0,20.0,21.0,22.0,23.0, 24.0); }"
        ),
        47
    );
}

#[test]
fn test_function_pointer_callback_parameter() {
    assert_eq!(
        run_c(
            "int apply(int (*callback)(int), int value) { return callback(value); } int twice(int value) { return value * 2; } int main(void) { return apply(twice, 21); }"
        ),
        42
    );
}

#[test]
fn test_unsigned_arithmetic_comparison_and_conversion() {
    assert_eq!(
        run_c("int main(void) { unsigned int x = 0xffffffffU; return x > 1U; }"),
        1
    );
    assert_eq!(
        run_c("int main(void) { unsigned int x = 10U; return x / 3U + x % 3U; }"),
        4
    );
    assert_eq!(
        run_c("int main(void) { unsigned char x = 255U; return x + 1; }"),
        256
    );
}

#[test]
fn test_static_local_storage_duration() {
    assert_eq!(
        run_c(
            "int next(void) { static int n = 0; n = n + 1; return n; } int main(void) { int a = next(); return a * 10 + next(); }"
        ),
        12
    );
}

#[test]
fn test_struct_whole_copy_including_nested_object() {
    assert_eq!(
        run_c(
            "struct Inner { int x; }; struct Outer { struct Inner i; int y; }; int main(void) { struct Outer a = {{3}, 4}; struct Outer b; b = a; return b.i.x + b.y; }"
        ),
        7
    );
}

#[test]
fn test_global_string_pointer_array_initializer() {
    assert_eq!(
        run_c(
            r#"char *words[2] = {"foo", "bar"}; int main(void) { return words[0][0] == 'f' && words[1][0] == 'b'; }"#
        ),
        1
    );
}

#[test]
fn test_local_string_pointer_array_initializer_is_aligned() {
    assert_eq!(
        run_c(
            r#"int main(void) { char *words[2] = {"foo", "bar"}; return words[0][0] == 'f' && words[1][0] == 'b'; }"#
        ),
        1
    );
}

#[test]
fn test_string_literal_inside_if_condition() {
    assert_eq!(
        run_c(
            r#"int first(char *text) { return text[0]; } int main(void) { if (first("A")) return 42; return 0; }"#
        ),
        42
    );
}

#[test]
fn test_leetcode_30_substring_with_concatenation() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_30.c")), 42);
}

#[test]
fn test_static_local_char_array_decays_to_pointer() {
    assert_eq!(
        run_c(
            "char *make_digit(int value) { static char result[2]; result[0] = (char)('0' + value); result[1] = '\0'; return result; } int main(void) { char *text = make_digit(7); return text[0]; }"
        ),
        55
    );
}

#[test]
fn test_leetcode_60_permutation_sequence() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_60.c")), 42);
}

#[test]
fn test_leetcode_01_two_sum() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_01.c")), 42);
}

#[test]
fn test_leetcode_02_add_two_numbers() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_02.c")), 42);
}

#[test]
fn test_leetcode_03_longest_substring_without_repeating() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_03.c")), 42);
}

#[test]
fn test_leetcode_04_median_of_two_sorted_arrays() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_04.c")), 42);
}

#[test]
fn test_leetcode_05_longest_palindromic_substring() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_05.c")), 42);
}

#[test]
fn test_leetcode_06_zigzag_conversion() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_06.c")), 42);
}

#[test]
fn test_leetcode_07_reverse_integer() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_07.c")), 42);
}

#[test]
fn test_leetcode_08_string_to_integer() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_08.c")), 42);
}

#[test]
fn test_leetcode_09_palindrome_number() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_09.c")), 42);
}

#[test]
fn test_leetcode_10_regular_expression_matching() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_10.c")), 42);
}

#[test]
fn test_leetcode_11_container_with_most_water() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_11.c")), 42);
}

#[test]
fn test_leetcode_12_integer_to_roman() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_12.c")), 42);
}

#[test]
fn test_leetcode_13_roman_to_integer() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_13.c")), 42);
}

#[test]
fn test_leetcode_14_longest_common_prefix() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_14.c")), 42);
}

#[test]
fn test_leetcode_15_three_sum() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_15.c")), 42);
}

#[test]
fn test_leetcode_16_three_sum_closest() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_16.c")), 42);
}

#[test]
fn test_multidimensional_array_row_decays_to_pointer() {
    assert_eq!(
        run_c(
            "int main(void) { char matrix[4][5]; char *row; matrix[3][0] = 'x'; matrix[3][1] = '\0'; row = matrix[3]; return row[0]; }"
        ),
        120
    );
}

#[test]
fn test_leetcode_17_letter_combinations() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_17.c")), 42);
}

#[test]
fn test_leetcode_18_four_sum() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_18.c")), 42);
}

#[test]
fn test_leetcode_19_remove_nth_node_from_end() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_19.c")), 42);
}

#[test]
fn test_leetcode_20_valid_parentheses() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_20.c")), 42);
}

#[test]
fn test_leetcode_21_merge_two_sorted_lists() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_21.c")), 42);
}

#[test]
fn test_leetcode_22_generate_parentheses() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_22.c")), 42);
}

#[test]
fn test_leetcode_23_merge_k_sorted_lists() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_23.c")), 42);
}

#[test]
fn test_leetcode_24_swap_nodes_in_pairs() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_24.c")), 42);
}

#[test]
fn test_leetcode_25_reverse_nodes_in_k_group() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_25.c")), 42);
}

#[test]
fn test_leetcode_26_remove_duplicates() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_26.c")), 42);
}

#[test]
fn test_leetcode_27_remove_element() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_27.c")), 42);
}

#[test]
fn test_leetcode_28_find_first_occurrence() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_28.c")), 42);
}

#[test]
fn test_leetcode_29_divide_two_integers() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_29.c")), 42);
}

#[test]
fn test_leetcode_31_next_permutation() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_31.c")), 42);
}

#[test]
fn test_leetcode_32_longest_valid_parentheses() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_32.c")), 42);
}

#[test]
fn test_leetcode_33_search_rotated_sorted_array() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_33.c")), 42);
}

#[test]
fn test_leetcode_34_find_first_last_position() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_34.c")), 42);
}

#[test]
fn test_leetcode_35_search_insert_position() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_35.c")), 42);
}

#[test]
fn test_leetcode_36_valid_sudoku() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_36.c")), 42);
}

#[test]
fn test_leetcode_37_sudoku_solver() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_37.c")), 42);
}

#[test]
fn test_leetcode_38_count_and_say() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_38.c")), 42);
}

#[test]
fn test_leetcode_39_combination_sum() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_39.c")), 42);
}

#[test]
fn test_leetcode_40_combination_sum_two() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_40.c")), 42);
}

#[test]
fn test_leetcode_41_first_missing_positive() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_41.c")), 42);
}

#[test]
fn test_leetcode_42_trapping_rain_water() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_42.c")), 42);
}

#[test]
fn test_leetcode_43_multiply_strings() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_43.c")), 42);
}

#[test]
fn test_leetcode_44_wildcard_matching() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_44.c")), 42);
}

#[test]
fn test_leetcode_45_jump_game_two() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_45.c")), 42);
}

#[test]
fn test_leetcode_46_permutations() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_46.c")), 42);
}

#[test]
fn test_leetcode_47_permutations_two() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_47.c")), 42);
}

#[test]
fn test_leetcode_48_rotate_image() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_48.c")), 42);
}

#[test]
fn test_leetcode_49_group_anagrams() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_49.c")), 42);
}

#[test]
fn test_leetcode_50_power() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_50.c")), 42);
}

#[test]
fn test_leetcode_51_n_queens() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_51.c")), 42);
}

#[test]
fn test_leetcode_52_n_queens_two() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_52.c")), 42);
}

#[test]
fn test_leetcode_53_maximum_subarray() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_53.c")), 42);
}

#[test]
fn test_leetcode_54_spiral_matrix() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_54.c")), 42);
}

#[test]
fn test_leetcode_55_jump_game() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_55.c")), 42);
}

#[test]
fn test_leetcode_56_merge_intervals() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_56.c")), 42);
}

#[test]
fn test_leetcode_57_insert_interval() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_57.c")), 42);
}

#[test]
fn test_leetcode_58_length_of_last_word() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_58.c")), 42);
}

#[test]
fn test_leetcode_59_spiral_matrix_two() {
    assert_eq!(run_c(include_str!("../../../examples/leetcode_59.c")), 42);
}

// ---- 缺陷修复回归测试（浮点转换矩阵）----

#[test]
fn test_fix_float_cast_matrix() {
    // int→float/double 由 fcvt.d.w/fcvt.d.wu 处理；以下覆盖浮点 src 的各目标。
    assert_eq!(run_c("int main(void) { return (int)(float)2.5; }"), 2);
    assert_eq!(run_c("int main(void) { return (int)(double)2.5; }"), 2);
    // float 参与运算前窄化回单精度再提升：2.9f * 10 → 29（不窄化会得 28）
    assert_eq!(
        run_c("int main(void) { return (int)((float)(double)2.9 * 10.0); }"),
        29
    );
    assert_eq!(
        run_c("int main(void) { return (int)((double)(float)2.5 * 10.0); }"),
        25
    );
    assert_eq!(run_c("int main(void) { return (char)(double)2.5; }"), 2);
    assert_eq!(run_c("int main(void) { return (int)(unsigned)(float)3.9; }"), 3);
    assert_eq!(run_c("int main(void) { return (unsigned)(float)3.9; }"), 3);
    assert_eq!(run_c("int main(void) { return (int)(double)-2.5; }"), -2);
}

// ---- 缺陷修复回归测试（collect_floats 遍历）----

#[test]
fn test_fix_float_in_subscript_index() {
    // 下标里的浮点字面量必须被 collect_floats 登记（此前越界 panic）
    assert_eq!(
        run_c("int main(void) { int a[3] = {10, 20, 30}; return a[(int)2.5]; }"),
        30
    );
    assert_eq!(
        run_c("int main(void) { double x = 1.5; int a[3] = {10, 20, 30}; return a[(int)(x + 1.0)]; }"),
        30
    );
}

#[test]
fn test_fix_float_in_switch_control() {
    // switch 控制表达式里的浮点字面量必须被登记（此前标签缺失/越界）。
    // 语义层会拒绝非整型 switch（见 cli_diagnostics），此处代码生成必须可用。
    assert_eq!(
        run_c("int main(void) { switch ((int)2.5) { case 2: return 42; default: return 1; } }"),
        42
    );
}

// ---- 缺陷修复回归测试（数组维数常量表达式）----

#[test]
fn test_fix_array_size_enum_constant() {
    assert_eq!(
        run_c("enum { N = 5 }; int a[N]; int main(void) { a[4] = 42; return a[4]; }"),
        42
    );
}

#[test]
fn test_fix_array_size_sizeof() {
    assert_eq!(
        run_c("int a[sizeof(int) * 2]; int main(void) { a[1] = 7; return a[1]; }"),
        7
    );
    assert_eq!(
        run_c("char b[sizeof(char) * 4]; int main(void) { b[3] = 'A'; return b[3]; }"),
        65
    );
    // 嵌套 sizeof：sizeof(sizeof(int)) == sizeof(int) == 4
    assert_eq!(
        run_c("int c[sizeof(sizeof(int))]; int main(void) { c[0] = 9; return c[0]; }"),
        9
    );
    assert_eq!(
        run_c("enum { N = 5 }; int d[sizeof(char) * N]; int main(void) { d[2] = 11; return d[2]; }"),
        11
    );
}

// ---- 缺陷修复回归测试（静态指针地址常量）----

#[test]
fn test_fix_static_pointer_addr_of_subscript() {
    assert_eq!(
        run_c("int arr[4]; int *p = &arr[1]; int main(void) { arr[1] = 7; return *p; }"),
        7
    );
    assert_eq!(
        run_c("int arr[4]; int *p = &arr[1]; int main(void) { return p == arr + 1 ? 42 : 1; }"),
        42
    );
}

#[test]
fn test_fix_static_pointer_string_plus_offset() {
    assert_eq!(
        run_c("char *s = \"abc\" + 1; int main(void) { return s[0] == 'b' && s[1] == 'c' ? 42 : 1; }"),
        42
    );
}

#[test]
fn test_fix_static_pointer_array_plus_offset() {
    assert_eq!(
        run_c("int arr[4] = {1, 2, 3, 4}; int *p = arr + 2; int main(void) { return *p; }"),
        3
    );
}

#[test]
fn test_fix_static_local_pointer_string_plus_offset() {
    assert_eq!(
        run_c("int main(void) { static char *s = \"abc\" + 1; return s[0] == 'b' && s[1] == 'c' ? 42 : 1; }"),
        42
    );
    // 自动变量（运行时初始化）也应正常
    assert_eq!(
        run_c("int main(void) { char *s = \"abc\" + 1; return s[0] == 'b' ? 42 : 1; }"),
        42
    );
}
