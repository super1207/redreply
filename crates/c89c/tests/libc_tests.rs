//! c89lib 端到端测试：C 源码（带标准库头文件）→ c89c -l 链接编译 → rv-core 执行。
//!
//! 每个测试在临时目录写入 .c 文件（预处理 #include <...> 需要真实文件），
//! 然后用 compile_file_to_asm_with_lib 编译，再在 rv-core 上直接执行。

use rv_core::assembler;
use rv_core::cpu::Cpu;

/// 在 64MB 栈的线程上编译：debug 构建的递归下降解析器栈帧很大，
/// 深层嵌套用例（如 5000 层括号的回归测试）在测试线程默认栈上会溢出。
fn compile_on_big_stack(file: String) -> Result<String, c89c::diagnostics::CompileError> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || c89c::pipeline::compile_file_to_asm_with_lib(&file))
        .expect("无法创建编译线程")
        .join()
        .expect("编译线程异常退出")
}

fn compile_and_run(src: &str) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "c89lib_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.c");
    std::fs::write(&file, src).unwrap();
    let asm = compile_on_big_stack(file.to_string_lossy().into_owned())
        .expect("c89c 编译失败");
    std::fs::remove_dir_all(&dir).ok();

    let program = assembler::assemble(&asm).expect("汇编失败");
    let mut cpu = Cpu::new();
    cpu.load_program(&program).unwrap();
    let result = cpu.run(&program.instructions, 1_000_000).unwrap();
    (cpu.exit_code, result.syscall_output)
}

fn expect_exit(src: &str, expected: i32) {
    let (code, _) = compile_and_run(src);
    assert_eq!(code, expected, "退出码不符：源码\n{}", src);
}

fn expect_compile_error(src: &str, needle: &str) {
    let dir = std::env::temp_dir().join(format!(
        "c89lib_err_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.c");
    std::fs::write(&file, src).unwrap();
    let result = compile_on_big_stack(file.to_string_lossy().into_owned());
    std::fs::remove_dir_all(&dir).ok();
    let error = result.expect_err("应当编译失败");
    assert!(
        error.to_string().contains(needle),
        "错误信息 {} 应包含 {}",
        error,
        needle
    );
}

#[test]
fn lib_bitfields_basic() {
    expect_exit(
        r#"#include <stdio.h>
struct Flags {
    int a : 3;
    unsigned b : 5;
    int : 0;
    unsigned c : 4;
};
struct Mixed {
    int x;
    unsigned flag : 1;
};
int main(void) {
    struct Flags f;
    struct Mixed m;
    f.a = 2;
    f.b = 31;
    f.c = 9;
    if (f.a != 2) { return 1; }
    if (f.b != 31) { return 2; }
    if (f.c != 9) { return 3; }
    f.a = 7;
    if (f.a != -1) { return 4; }
    f.a = 9;
    if (f.a != 1) { return 5; }
    f.b += 2;
    if (f.b != 1) { return 6; }
    f.c++;
    if (f.c != 10) { return 7; }
    if (sizeof(struct Flags) != 8) { return 8; }
    if (sizeof(struct Mixed) != 8) { return 9; }
    m.x = 42;
    m.flag = 1;
    if (m.x != 42 || m.flag != 1) { return 10; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_bitfields_through_pointer() {
    expect_exit(
        r#"#include <stdio.h>
struct Flags {
    int a : 3;
    unsigned b : 5;
    int : 0;
    unsigned c : 4;
};
int main(void) {
    struct Flags f;
    struct Flags *p = &f;
    p->a = -3;
    p->b = 17;
    p->c = 15;
    if (f.a != -3) { return 1; }
    if (f.b != 17) { return 2; }
    if (f.c != 15) { return 3; }
    if (p->a != -3 || p->b != 17 || p->c != 15) { return 4; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_bitfields_invalid_usage_is_rejected() {
    expect_compile_error(
        r#"#include <stdio.h>
struct F { int a : 3; };
int main(void) {
    struct F f;
    int *p = &f.a;
    return 0;
}
"#,
        "不能对位域取地址",
    );
    expect_compile_error(
        r#"#include <stdio.h>
struct F { char c : 3; };
int main(void) { return 0; }
"#,
        "位域类型必须是 int/signed int/unsigned int",
    );
    expect_compile_error(
        r#"#include <stdio.h>
struct F { int a : 3; };
struct F g = { 1 };
int main(void) { return 0; }
"#,
        "初始化器暂不支持",
    );
}

#[test]
fn lib_string_basic() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[64];
    char *p;
    int ok = 0;
    strcpy(buf, "hello");
    ok += (strlen(buf) == 5);
    ok += (strcmp(buf, "hello") == 0);
    strcat(buf, " world");
    ok += (strcmp(buf, "hello world") == 0);
    p = strchr(buf, 'w');
    ok += (p != 0 && *p == 'w');
    p = strstr(buf, "lo wo");
    ok += (p != 0);
    ok += (strncmp(buf, "hello", 5) == 0);
    if (ok == 6) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_string_extra() {
    expect_exit(
        r#"#include <string.h>
int main(void) {
    char buf[64];
    char *p;
    int ok = 0;
    strcpy(buf, "one,two;three");
    p = strtok(buf, ",;");
    ok += (p != 0 && strcmp(p, "one") == 0);
    p = strtok(0, ",;");
    ok += (p != 0 && strcmp(p, "two") == 0);
    p = strtok(0, ",;");
    ok += (p != 0 && strcmp(p, "three") == 0);
    p = strtok(0, ",;");
    ok += (p == 0);
    strncpy(buf, "abcdef", 3);
    ok += (buf[2] == 'c' && buf[3] == '\0');
    ok += (strcmp(strerror(33), "Domain error") == 0);
    if (ok == 6) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_memory_ops() {
    expect_exit(
        r#"#include <string.h>
int main(void) {
    char a[16];
    char b[16];
    int ok = 0;
    memset(a, 'x', 10);
    ok += (a[0] == 'x' && a[9] == 'x');
    memcpy(b, a, 10);
    ok += (b[1] == 'x');
    ok += (memcmp(a, b, 10) == 0);
    ok += (memcmp("abc", "abd", 3) < 0);
    memmove(a + 2, a, 8);
    ok += (a[2] == 'x' && a[9] == 'x');
    if (ok == 5) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_ctype_basic() {
    expect_exit(
        r#"#include <ctype.h>
int main(void) {
    int ok = 0;
    ok += isdigit('7');
    ok += isalpha('a');
    ok += isalnum('9');
    ok += isspace(' ');
    ok += ispunct('.');
    ok += isupper('A');
    ok += islower('z');
    ok += isxdigit('f');
    ok += iscntrl('\n');
    ok += isgraph('#');
    ok += (toupper('q') == 'Q');
    ok += (tolower('Z') == 'z');
    if (ok == 12) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_errno_offsetof_limits() {
    expect_exit(
        r#"#include <stddef.h>
#include <errno.h>
#include <limits.h>
#include <float.h>
struct Pair { int a; char b; double c; };
int main(void) {
    int ok = 0;
    errno = EDOM;
    ok += (errno == 33);
    ok += (NULL == 0);
    ok += (offsetof(struct Pair, a) == 0);
    ok += (offsetof(struct Pair, b) == 4);
    ok += (INT_MAX == 2147483647);
    ok += (UINT_MAX == 4294967295U);
    ok += (CHAR_BIT == 8);
    ok += (DBL_MANT_DIG == 53);
    if (ok == 8) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_printf_output() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char name[16];
    strcpy(name, "lib");
    printf("hello %s\n", name);
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "hello lib\n");
}

#[test]
fn lib_printf_supports_long_integer_formats() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
int main(void) {
    long a = 1;
    long b = 1;
    long next;
    char *format = "F(21) = %ld\n";
    int i;
    for (i = 3; i <= 21; ++i) {
        next = a + b;
        a = b;
        b = next;
    }
    printf(format, b);
    printf("%li|%lu|%lx\n", -42L, 4000000000UL, 0xabcdefUL);
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "F(21) = 10946\n-42|4000000000|abcdef\n");
}

#[test]
fn lib_perror_writes_errno_to_stderr() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <errno.h>
int main(void) {
    errno = EDOM;
    perror("open");
    errno = ERANGE;
    perror("");
    errno = 0;
    perror(0);
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "open: Domain error\nRange error\nUnknown error 0\n");
}

#[test]
fn lib_system_executes_command_and_captures_output() {
    let (code, output) = compile_and_run(
        r#"#include <stdlib.h>
int main(void) {
    int status;
    if (system(0) == 0) { return 1; }
    status = system("echo system-ok");
    if (status != 0) { return 2; }
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert!(output.contains("system-ok"), "output: {}", output);
}

#[test]
fn lib_system_returns_command_exit_code() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    return system("exit 7");
}
"#,
        7,
    );
}

#[test]
fn lib_stdlib_basic() {
    expect_exit(
        r#"#include <stdlib.h>
#include <string.h>
static int cmp_int(const void *a, const void *b) {
    int x = *(const int *)a;
    int y = *(const int *)b;
    return (x > y) - (x < y);
}
int main(void) {
    char *p;
    int ok = 0;
    p = (char *)malloc(100);
    ok += (p != 0);
    strcpy(p, "heap");
    ok += (strcmp(p, "heap") == 0);
    free(p);
    p = (char *)calloc(10, 4);
    ok += (p != 0);
    ok += (p[0] == 0 && p[39] == 0);
    free(p);
    p = (char *)malloc(8);
    ok += (p != 0);
    p = (char *)realloc(p, 64);
    ok += (p != 0);
    free(p);
    ok += (atoi("  -123x") == -123);
    ok += (atol("42") == 42);
    ok += (strtol("ff", 0, 16) == 255);
    ok += (strtol("0x1A", 0, 0) == 26);
    ok += (strtol("077", 0, 0) == 63);
    ok += (strtol("101", 0, 2) == 5);
    ok += (abs(-7) == 7);
    ok += (labs(-900000L) == 900000L);
    srand(1);
    ok += (rand() == 16838);
    ok += (rand() >= 0);
    {
        div_t d = div(17, 5);
        ok += (d.quot == 3 && d.rem == 2);
    }
    {
        int arr[5];
        arr[0] = 4; arr[1] = 1; arr[2] = 3; arr[3] = 5; arr[4] = 2;
        qsort(arr, 5, sizeof(int), cmp_int);
        ok += (arr[0] == 1 && arr[4] == 5);
        {
            int key = 3;
            int *found = (int *)bsearch(&key, arr, 5, sizeof(int), cmp_int);
            ok += (found != 0 && *found == 3);
        }
        {
            int key = 9;
            int *missing = (int *)bsearch(&key, arr, 5, sizeof(int), cmp_int);
            ok += (missing == 0);
        }
    }
    if (ok == 20) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_strtod_basic() {
    expect_exit(
        r#"#include <errno.h>
#include <stdlib.h>
int main(void) {
    const char *input;
    char *end;
    double value;
    int ok = 0;
    ok += (EXIT_SUCCESS == 0);
    ok += (EXIT_FAILURE != 0);
    value = strtod("  -12.5e2tail", &end);
    ok += (value == -1250.0 && *end == 't');
    value = strtod(".125", &end);
    ok += (value == 0.125 && *end == '\0');
    value = strtod("1e-3", &end);
    ok += (value > 0.0009 && value < 0.0011 && *end == '\0');
    value = strtod("1e+", &end);
    ok += (value == 1.0 && *end == 'e');
    input = "nope";
    value = strtod(input, &end);
    ok += (value == 0.0 && end == input);
    value = atof("-2.5");
    ok += (value == -2.5);
    errno = 0;
    value = strtod("9e999", &end);
    ok += (errno == ERANGE && value > 1e307);
    errno = 0;
    value = strtod("1e-999", &end);
    ok += (errno == ERANGE && value == 0.0);
    if (ok == 10) { return EXIT_SUCCESS; }
    return EXIT_FAILURE;
}
"#,
        0,
    );
}

#[test]
fn lib_atexit_runs_lifo_after_main_returns() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <stdlib.h>
static void first(void) { putchar('A'); }
static void second(void) { putchar('B'); }
int main(void) {
    if (atexit(first) != 0) { return 1; }
    if (atexit(second) != 0) { return 2; }
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "BA");
}

#[test]
fn lib_atexit_runs_for_explicit_exit() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <stdlib.h>
static void handler(void) { putchar('X'); }
int main(void) {
    atexit(handler);
    exit(7);
    return 1;
}
"#,
    );
    assert_eq!(code, 7);
    assert_eq!(output, "X");
}

#[test]
fn lib_atexit_capacity_is_bounded() {
    expect_exit(
        r#"#include <stdlib.h>
static void handler(void) { }
int main(void) {
    int i;
    for (i = 0; i < 32; i += 1) {
        if (atexit(handler) != 0) { return 1; }
    }
    if (atexit(handler) == 0) { return 2; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_abort_skips_atexit_handlers() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <stdlib.h>
static void handler(void) { putchar('X'); }
int main(void) {
    atexit(handler);
    abort();
    return 0;
}
"#,
    );
    assert_eq!(code, 134);
    assert_eq!(output, "");
}

#[test]
fn lib_stdio_basic() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
int main(void) {
    int ok = 0;
    ok += (putchar('a') == 'a');
    ok += (putchar('b') == 'b');
    ok += (puts("hello stdio") == 0);
    printf("value=%d\n", 42);
    if (ok == 3) { return 0; }
    return ok;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "abhello stdio\nvalue=42\n");
}

#[test]
fn lib_string_literal_concatenation() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[32];
    char *msg = "abc" "def";
    if (strcmp(msg, "abcdef") != 0) { return 1; }
    strcpy(buf, "one" "two" "three");
    if (strcmp(buf, "onetwothree") != 0) { return 2; }
    if (strcmp("x" "y" "z", "xyz") != 0) { return 3; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_hex_and_octal_escapes() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char *s = "\x41\x42" "\x43";
    if (strcmp(s, "ABC") != 0) { return 1; }
    if ('\x41' != 'A') { return 2; }
    if ('\101' != 'A') { return 3; }
    if ('\12' != '\n') { return 4; }
    if (strcmp("\x68\x69", "hi") != 0) { return 5; }
    if ('\0' != 0) { return 6; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_printf_full_format_specifiers() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[256];
    int ok = 0;
    sprintf(buf, "%o|%X|%x", 255, 255, 255);
    ok += (strcmp(buf, "377|FF|ff") == 0);
    sprintf(buf, "%#o|%#x|%#X", 255, 255, 255);
    ok += (strcmp(buf, "0377|0xff|0XFF") == 0);
    sprintf(buf, "%5d|%05d|%-5d|%.2d|%+d|% d", 42, 42, 42, 5, 42, 42);
    ok += (strcmp(buf, "   42|00042|42   |05|+42| 42") == 0);
    sprintf(buf, "%5.2f|%.0f|%08.2f|%f", 3.14159, 3.7, 3.14, 2.999);
    ok += (strcmp(buf, " 3.14|4|00003.14|2.999000") == 0);
    sprintf(buf, "%e|%E|%g|%g|%.3g|%g", 12345.678, 12345.678, 12345.678, 0.000012345, 1234.5, 0.0);
    ok += (strcmp(buf, "1.234568e+04|1.234568E+04|12345.7|1.2345e-05|1.23e+03|0") == 0);
    sprintf(buf, "%*d|%.*f", 5, 42, 2, 3.14159);
    ok += (strcmp(buf, "   42|3.14") == 0);
    sprintf(buf, "%.5s|%c", "abcdef", 'Z');
    ok += (strcmp(buf, "abcde|Z") == 0);
    sprintf(buf, "[%d][%.0d][%d]", 0, 0, 5);
    ok += (strcmp(buf, "[0][][5]") == 0);
    if (ok == 8) { return 0; }
    return ok;
}
"#,
    );
    assert_eq!(code, 0, "格式化失败");
    assert_eq!(output, "");
}

#[test]
fn lib_printf_char_and_string_args() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
int main(void) {
    char c = 'A';
    printf("%c %s\n", 'A', "hi");
    printf("%c %s\n", c, "hi");
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "A hi\nA hi\n");
}

#[test]
fn lib_remove_deletes_file() {
    let path = std::env::temp_dir().join(format!("c89lib_remove_{}.txt", std::process::id()));
    std::fs::write(&path, "x").unwrap();
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *file;
    file = fopen("{escaped}", "r");
    if (file == 0) {{ return 1; }}
    fclose(file);
    if (remove("{escaped}") != 0) {{ return 2; }}
    file = fopen("{escaped}", "r");
    if (file != 0) {{ return 3; }}
    if (remove("{escaped}") == 0) {{ return 4; }}
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_printf_pointer_and_count_specifiers() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[64];
    int n = 0;
    int *p = (int *)0;
    sprintf(buf, "%p", p);
    if (strcmp(buf, "0x0") != 0) { return 1; }
    sprintf(buf, "%p", (void *)0x1234);
    if (strcmp(buf, "0x1234") != 0) { return 2; }
    n = -1;
    sprintf(buf, "ab%ncd", &n);
    if (n != 2 || strcmp(buf, "abcd") != 0) { return 3; }
    sprintf(buf, "%d%n", 42, &n);
    if (n != 2) { return 4; }
    return 0;
}
"#,
    );
    assert_eq!(code, 0, "%p/%n 失败");
    assert_eq!(output, "");
}

#[test]
fn lib_pp_number_paste() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
#define CAT(a,b) a##b
#define STR_(x) #x
#define STR(x) STR_(x)
int main(void) {
    double x = CAT(1.,5);
    char *s = STR(CAT(1,+));
    if (x != 1.5) { return 1; }
    if (strcmp(s, "1+") != 0) { return 2; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_printf_long_string_no_buffer_overflow() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[320];
    int i;
    for (i = 0; i < 300; i += 1) {
        buf[i] = (char)('a' + (i % 26));
    }
    buf[300] = 0;
    printf("[%s]\n", buf);
    printf("[%.10s]\n", buf);
    printf("[%s]\n", buf + 90);
    return 0;
}
"#,
    );
    assert_eq!(code, 0, "长字符串输出失败");
    let expected: String = {
        let mut s = String::from("[");
        for i in 0..300 {
            s.push((b'a' + (i % 26) as u8) as char);
        }
        s.push_str("]\n[abcdefghij]\n[");
        for i in 90..300 {
            s.push((b'a' + (i % 26) as u8) as char);
        }
        s.push_str("]\n");
        s
    };
    assert_eq!(output, expected);
}

#[test]
fn lib_time_basic() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
#include <time.h>
int main(void) {
    time_t t0 = 0;
    time_t t;
    time_t back;
    time_t leap;
    struct tm *g;
    struct tm tmp;
    char buf[64];
    g = gmtime(&t0);
    if (g->tm_year != 70 || g->tm_mon != 0 || g->tm_mday != 1) { return 1; }
    if (g->tm_hour != 0 || g->tm_min != 0 || g->tm_sec != 0) { return 2; }
    if (g->tm_wday != 4 || g->tm_yday != 0) { return 3; }
    if (strcmp(asctime(g), "Thu Jan  1 00:00:00 1970\n") != 0) { return 4; }
    strftime(buf, sizeof(buf), "%Y-%m-%d %H:%M:%S", g);
    if (strcmp(buf, "1970-01-01 00:00:00") != 0) { return 5; }
    leap = 951782400;
    g = gmtime(&leap);
    if (g->tm_year != 100 || g->tm_mon != 1 || g->tm_mday != 29) { return 6; }
    if (g->tm_wday != 2 || g->tm_yday != 59) { return 7; }
    t = (time_t)1234567890;
    tmp = *localtime(&t);
    if (mktime(&tmp) != t) { return 8; }
    if (strftime(buf, sizeof(buf), "%Z", localtime(&t)) == 0) { return 9; }
    if (difftime(1000, 136) != 864.0) { return 10; }
    if (clock() < 0) { return 11; }
    back = time(NULL);
    time(&t);
    if (t != back && t != back + 1) { return 12; }
    if (t < 1500000000) { return 13; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_locale_c_locale() {
    expect_exit(
        r#"#include <locale.h>
#include <limits.h>
#include <string.h>
int main(void) {
    struct lconv *lc;
    if (strcmp(setlocale(LC_ALL, NULL), "C") != 0) { return 1; }
    if (setlocale(LC_NUMERIC, "C") == NULL) { return 2; }
    if (setlocale(LC_TIME, "") == NULL) { return 3; }
    if (setlocale(LC_MONETARY, "fr_FR") != NULL) { return 4; }
    if (setlocale(LC_COLLATE, "C") == NULL) { return 5; }
    if (LC_ALL == LC_COLLATE || LC_ALL == LC_CTYPE) { return 6; }
    if (LC_NUMERIC == LC_TIME || LC_TIME == LC_MONETARY) { return 7; }
    lc = localeconv();
    if (strcmp(lc->decimal_point, ".") != 0) { return 8; }
    if (lc->thousands_sep[0] != '\0') { return 9; }
    if (lc->grouping[0] != '\0') { return 10; }
    if (lc->int_curr_symbol[0] != '\0') { return 11; }
    if (lc->currency_symbol[0] != '\0') { return 12; }
    if (lc->mon_decimal_point[0] != '\0') { return 13; }
    if (lc->positive_sign[0] != '\0') { return 14; }
    if (lc->negative_sign[0] != '\0') { return 15; }
    if (lc->frac_digits != CHAR_MAX) { return 16; }
    if (lc->int_frac_digits != CHAR_MAX) { return 17; }
    if (lc->p_cs_precedes != CHAR_MAX) { return 18; }
    if (lc->p_sep_by_space != CHAR_MAX) { return 19; }
    if (lc->n_cs_precedes != CHAR_MAX) { return 20; }
    if (lc->n_sep_by_space != CHAR_MAX) { return 21; }
    if (lc->p_sign_posn != CHAR_MAX) { return 22; }
    if (lc->n_sign_posn != CHAR_MAX) { return 23; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_setjmp_longjmp() {
    expect_exit(
        r#"#include <setjmp.h>
#include <stdio.h>
static jmp_buf env;
static void inner(int depth) {
    if (depth > 0) {
        inner(depth - 1);
        return;
    }
    longjmp(env, 42);
}
int main(void) {
    int r;
    int i = 0;
    r = setjmp(env);
    if (r != i) { return 1; }
    if (i < 5) {
        i += 1;
        longjmp(env, i);
    }
    if (i != 5) { return 2; }
    r = setjmp(env);
    if (r == 0) {
        inner(50);
        return 3;
    }
    if (r != 42) { return 4; }
    r = setjmp(env);
    if (r == 0) {
        longjmp(env, 0);
        return 5;
    }
    if (r != 1) { return 6; }
    {
        int hits = 0;
        if (setjmp(env) == 0) {
            hits += 1;
            longjmp(env, 1);
            return 7;
        } else {
            hits += 1;
        }
        if (hits != 2) { return 8; }
    }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_system_output_handles_chinese_paths() {
    let dir = std::env::temp_dir().join(format!("c89lib_中文目录_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("测试文件.txt"), "x").unwrap();
    let dir_c = dir.to_string_lossy().replace('\\', "\\\\");
    #[cfg(windows)]
    let command = format!("dir \"{}\"", dir_c);
    #[cfg(not(windows))]
    let command = format!("ls -1 \"{}\"", dir_c);
    let command_literal = command.replace('"', "\\\"");
    let src = format!(
        r#"#include <stdio.h>
#include <stdlib.h>
int main(void) {{
    int status = system("{command_literal}");
    return status == 0 ? 0 : 1;
}}
"#
    );
    let (code, output) = compile_and_run(&src);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(code, 0, "system 执行失败");
    assert!(
        output.contains("测试文件"),
        "输出应包含中文文件名：{}",
        output
    );
}

#[test]
fn lib_char_array_string_initializer_decodes_escapes() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
char g[14] = "全局\"引号";
int main(void) {
    char c[11] = "你\"好";
    char h[8] = "a\x42";
    printf("[%s]\n", c);
    printf("[%s]\n", g);
    printf("[%s]\n", h);
    return 0;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "[你\"好]\n[全局\"引号]\n[aB]\n");
}

#[test]
fn lib_sprintf_basic() {
    let (code, output) = compile_and_run(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[128];
    int ok = 0;
    int n = sprintf(buf, "%d|%i|%u|%x|%c|%s|%%|%f", -123, 42, -1, -1, 'Z', "hi", 3.14159);
    ok += (strcmp(buf, "-123|42|4294967295|ffffffff|Z|hi|%|3.141590") == 0);
    ok += (n == 43);
    sprintf(buf, "%f", 2.999);
    ok += (strcmp(buf, "2.999000") == 0);
    sprintf(buf, "%f", -1.5);
    ok += (strcmp(buf, "-1.500000") == 0);
    sprintf(buf, "%ld", 1000000L);
    ok += (strcmp(buf, "1000000") == 0);
    sprintf(buf, "%x", 0xDEAD);
    ok += (strcmp(buf, "dead") == 0);
    if (ok == 6) { printf("sprintf ok\n"); return 0; }
    printf("sprintf fail %d\n", ok);
    return ok;
}
"#,
    );
    assert_eq!(code, 0, "退出码不符");
    assert_eq!(output, "sprintf ok\n");
}

#[test]
fn lib_variadic_formatting_basic() {
    let (code, output) = compile_and_run(
        r#"#include <stdarg.h>
#include <stdio.h>
#include <string.h>
static int call_vsprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vsprintf(buf, fmt, ap);
    va_end(ap);
    return result;
}
static int call_vprintf(const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vprintf(fmt, ap);
    va_end(ap);
    return result;
}
static int call_vfprintf(FILE *stream, const char *fmt, ...) {
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vfprintf(stream, fmt, ap);
    va_end(ap);
    return result;
}
int main(void) {
    char buf[64];
    int ok = 0;
    ok += (call_vsprintf(buf, "%d|%s|%f", 12, "ok", 1.5) == 14);
    ok += (strcmp(buf, "12|ok|1.500000") == 0);
    ok += (fprintf(stdout, "fprintf=%x\n", 255) == 11);
    ok += (call_vprintf("vprintf=%u\n", 7) == 10);
    ok += (call_vfprintf(stdout, "vfprintf=%d\n", -3) == 12);
    return ok == 5 ? 0 : 1;
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(output, "fprintf=ff\nvprintf=7\nvfprintf=-3\n");
}

#[test]
fn lib_fprintf_file_output() {
    let path = std::env::temp_dir().join(format!("c89lib_fprintf_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdarg.h>
#include <stdio.h>
static int write_vfprintf(FILE *stream, const char *fmt, ...) {{
    va_list ap;
    int result;
    va_start(ap, fmt);
    result = vfprintf(stream, fmt, ap);
    va_end(ap);
    return result;
}}
int main(void) {{
    FILE *file = fopen("{escaped}", "wb");
    if (file == 0) {{ return 1; }}
    if (fprintf(file, "%s:%d\n", "item", 42) != 8) {{ return 2; }}
    if (write_vfprintf(file, "%f\n", 2.5) != 9) {{ return 3; }}
    if (fclose(file) != 0) {{ return 4; }}
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
    assert_eq!(contents, "item:42\n2.500000\n");
}

#[test]
fn lib_sscanf_basic() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char word[16];
    char letter;
    int decimal;
    int automatic1;
    int automatic2;
    int untouched = 99;
    unsigned int hex;
    long long_decimal;
    unsigned long long_unsigned;
    float single;
    double wide;
    int count;
    count = sscanf(" -42 0xff word Z 1.25 2.5 %", "%d %x %s %c %f %lf %%",
                   &decimal, &hex, word, &letter, &single, &wide);
    if (count != 6) { return 1; }
    if (decimal != -42 || hex != 255 || strcmp(word, "word") != 0 || letter != 'Z') { return 2; }
    if (single < 1.24 || single > 1.26 || wide != 2.5) { return 3; }
    count = sscanf("077 0x10", "%i %i", &automatic1, &automatic2);
    if (count != 2 || automatic1 != 63 || automatic2 != 16) { return 4; }
    count = sscanf("12 nope", "%d %d", &decimal, &untouched);
    if (count != 1 || decimal != 12 || untouched != 99) { return 5; }
    if (sscanf("nope", "%d", &decimal) != 0) { return 6; }
    count = sscanf("-123 456", "%ld %lu", &long_decimal, &long_unsigned);
    if (count != 2 || long_decimal != -123 || long_unsigned != 456) { return 7; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_fscanf_file_input_without_overread() {
    let path = std::env::temp_dir().join(format!("c89lib_fscanf_{}.txt", std::process::id()));
    std::fs::write(&path, "12,34 word 2.5\n99 tail").unwrap();
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {{
    FILE *file;
    char word[16];
    char tail[16];
    int first;
    int second;
    int last;
    int count;
    double real;
    file = fopen("{escaped}", "rb");
    if (file == 0) {{ return 1; }}
    count = fscanf(file, "%d", &first);
    if (count != 1 || first != 12) {{ return 2; }}
    if (fgetc(file) != ',') {{ return 3; }}
    count = fscanf(file, "%d %s %lf", &second, word, &real);
    if (count != 3 || second != 34 || strcmp(word, "word") != 0 || real != 2.5) {{ return 4; }}
    count = fscanf(file, "%d %s", &last, tail);
    if (count != 2 || last != 99 || strcmp(tail, "tail") != 0) {{ return 5; }}
    if (fscanf(file, "%d", &first) != EOF) {{ return 6; }}
    if (fclose(file) != 0) {{ return 7; }}
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_malloc_splits_free_blocks() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    char *large;
    char *first;
    char *second;
    large = (char *)malloc(128);
    if (large == 0) { return 1; }
    free(large);
    first = (char *)malloc(16);
    second = (char *)malloc(32);
    if (first != large) { return 2; }
    if (second != first + 32) { return 3; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_free_coalesces_with_previous_block() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    char *first;
    char *second;
    char *guard;
    char *joined;
    first = (char *)malloc(16);
    second = (char *)malloc(16);
    guard = (char *)malloc(16);
    if (first == 0 || second == 0 || guard == 0) { return 1; }
    free(first);
    free(second);
    joined = (char *)malloc(40);
    if (joined != first) { return 2; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_free_coalesces_with_next_block() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    char *first;
    char *second;
    char *guard;
    char *joined;
    first = (char *)malloc(16);
    second = (char *)malloc(16);
    guard = (char *)malloc(16);
    if (first == 0 || second == 0 || guard == 0) { return 1; }
    free(second);
    free(first);
    joined = (char *)malloc(40);
    if (joined != first) { return 2; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_realloc_reuses_adjacent_space() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    char *first;
    char *second;
    char *resized;
    first = (char *)malloc(16);
    second = (char *)malloc(64);
    if (first == 0 || second == 0) { return 1; }
    first[0] = 'A';
    first[15] = 'Z';
    free(second);
    resized = (char *)realloc(first, 64);
    if (resized != first) { return 2; }
    if (resized[0] != 'A' || resized[15] != 'Z') { return 3; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_realloc_shrink_splits_tail() {
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    char *large;
    char *resized;
    char *tail;
    large = (char *)malloc(128);
    if (large == 0) { return 1; }
    large[0] = 'A';
    resized = (char *)realloc(large, 16);
    if (resized != large || resized[0] != 'A') { return 2; }
    tail = (char *)malloc(64);
    if (tail != resized + 32) { return 3; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_math_basic() {
    expect_exit(
        r#"#include <errno.h>
#include <math.h>
int main(void) {
    int e;
    int ok = 0;
    double m;
    ok += ((int)(sqrt(2.0) * 100.0) == 141);
    ok += ((int)(pow(2.0, 10.0)) == 1024);
    ok += ((int)(pow(-2.0, 3.0)) == -8);
    ok += ((int)(fabs(-3.5) * 10.0) == 35);
    ok += ((int)(floor(3.7)) == 3);
    ok += ((int)(ceil(3.7)) == 4);
    ok += ((int)(fmod(7.5, 2.0) * 10.0) == 15);
    ok += ((int)(ldexp(1.0, 10)) == 1024);
    ok += ((int)(exp(1.0) * 100.0) == 271);
    ok += ((int)(sin(0.5) * 100.0) == 47);
    ok += ((int)(cos(0.5) * 100.0) == 87);
    ok += ((int)(pow(-2.0, 3.0) * 100.0) == -800);
    ok += (log(1.0) > -1e-10 && log(1.0) < 1e-10);
    ok += (log10(1000.0) > 2.9 && log10(1000.0) < 3.1);
    ok += (atan2(1.0, 1.0) > 0.78 && atan2(1.0, 1.0) < 0.79);
    ok += (asin(0.5) > 0.52 && asin(0.5) < 0.53);
    ok += (acos(0.5) > 1.04 && acos(0.5) < 1.05);
    ok += (tanh(1.0) > 0.76 && tanh(1.0) < 0.77);
    errno = 0;
    sqrt(-1.0);
    ok += (errno == EDOM);
    errno = 0;
    log(-1.0);
    ok += (errno == EDOM);
    errno = 0;
    log(0.0);
    ok += (errno == ERANGE);
    errno = 0;
    pow(-2.0, 0.5);
    ok += (errno == EDOM);
    errno = 0;
    fmod(1.0, 0.0);
    ok += (errno == EDOM);
    m = frexp(1000.0, &e);
    ok += (e == 10);
    ok += (m > 0.97 && m < 0.98);
    m = frexp(0.75, &e);
    ok += (e == 0);
    ok += (m == 0.75);
    if (ok == 27) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_assert_stdarg_string_basic() {
    expect_exit(
        r#"#include <assert.h>
#include <stdarg.h>
#include <string.h>
int sum(int count, ...) {
    va_list ap;
    int i;
    int total = 0;
    va_start(ap, count);
    for (i = 0; i < count; i = i + 1) {
        total = total + va_arg(ap, int);
    }
    va_end(ap);
    return total;
}
int main(void) {
    char transformed[16];
    int ok = 0;
    assert(1);
    ok += (strcoll("abc", "abd") < 0);
    ok += (strcoll("same", "same") == 0);
    ok += (strxfrm(transformed, "hello", sizeof(transformed)) == 5);
    ok += (strcmp(transformed, "hello") == 0);
    ok += (strxfrm(transformed, "hello", 0) == 5);
    ok += (strxfrm(transformed, "hello", 3) == 5);
    ok += (transformed[0] == 'h' && transformed[1] == 'e' && transformed[2] == '\0');
    ok += (sum(3, 10, 20, 30) == 60);
    if (ok == 8) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_assert_ndebug() {
    expect_exit(
        r#"#define NDEBUG
#include <assert.h>
int main(void) {
    int value = 0;
    assert(value = 1);
    return value;
}
"#,
        0,
    );
}

#[test]
fn lib_file_io() {
    let path = std::env::temp_dir().join(format!("c89lib_e2e_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {{
    FILE *f;
    char buf[64];
    int ok = 0;
    f = fopen("{escaped}", "wb");
    ok += (f != 0);
    ok += (fwrite("hello file", 1, 10, f) == 10);
    ok += (fclose(f) == 0);
    f = fopen("{escaped}", "rb");
    ok += (f != 0);
    ok += (fread(buf, 1, 10, f) == 10);
    buf[10] = 0;
    ok += (strcmp(buf, "hello file") == 0);
    ok += (fgetc(f) == EOF);
    ok += (feof(f) != 0);
    ok += (fseek(f, 6, 0) == 0);
    ok += (fgetc(f) == 'f');
    ok += (fclose(f) == 0);
    if (ok == 11) {{ return 0; }}
    return ok;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_stdio_seek_macros() {
    let path = std::env::temp_dir().join(format!("c89lib_seek_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *file;
    file = fopen("{escaped}", "w+");
    if (file == 0) {{ return 1; }}
    if (fputs("abcdef", file) != 0) {{ return 2; }}
    if (fseek(file, -2, SEEK_END) != 0) {{ return 3; }}
    if (fgetc(file) != 'e') {{ return 4; }}
    if (fseek(file, -2, SEEK_CUR) != 0) {{ return 5; }}
    if (fgetc(file) != 'd') {{ return 6; }}
    if (fseek(file, 1, SEEK_SET) != 0) {{ return 7; }}
    if (fgetc(file) != 'b') {{ return 8; }}
    return fclose(file) == 0 ? 0 : 9;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_fopen_accepts_c89_modes_and_rejects_invalid_modes() {
    let path = std::env::temp_dir().join(format!("c89lib_modes_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
static int opens(const char *path, const char *mode) {{
    FILE *file = fopen(path, mode);
    if (file == 0) {{ return 0; }}
    fclose(file);
    return 1;
}}
int main(void) {{
    FILE *file;
    const char *path = "{escaped}";
    file = fopen(path, "w");
    if (file == 0) {{ return 1; }}
    fputs("seed", file);
    fclose(file);
    if (!opens(path, "r") || !opens(path, "rb")) {{ return 2; }}
    if (!opens(path, "r+") || !opens(path, "r+b") || !opens(path, "rb+")) {{ return 3; }}
    if (!opens(path, "w") || !opens(path, "wb")) {{ return 4; }}
    if (!opens(path, "w+") || !opens(path, "w+b") || !opens(path, "wb+")) {{ return 5; }}
    if (!opens(path, "a") || !opens(path, "ab")) {{ return 6; }}
    if (!opens(path, "a+") || !opens(path, "a+b") || !opens(path, "ab+")) {{ return 7; }}
    if (opens(path, "") || opens(path, "x") || opens(path, "rw")) {{ return 8; }}
    if (opens(path, "r++") || opens(path, "rbb") || opens(path, "rbx")) {{ return 9; }}
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_fopen_update_modes_have_c89_file_semantics() {
    let existing_path =
        std::env::temp_dir().join(format!("c89lib_update_{}.txt", std::process::id()));
    let missing_path =
        std::env::temp_dir().join(format!("c89lib_missing_{}.txt", std::process::id()));
    std::fs::write(&existing_path, "abc").unwrap();
    std::fs::remove_file(&missing_path).ok();
    let existing = existing_path.to_string_lossy().replace('\\', "\\\\");
    let missing = missing_path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *file;
    char buf[8];
    file = fopen("{missing}", "r+");
    if (file != 0) {{ return 1; }}
    file = fopen("{existing}", "r+");
    if (file == 0 || fgetc(file) != 'a') {{ return 2; }}
    if (fseek(file, 0, SEEK_END) != 0 || fputc('d', file) != 'd') {{ return 3; }}
    fclose(file);
    file = fopen("{existing}", "w+");
    if (file == 0 || ftell(file) != 0) {{ return 4; }}
    if (fputc('x', file) != 'x' || fseek(file, 0, SEEK_SET) != 0) {{ return 5; }}
    if (fgetc(file) != 'x' || fgetc(file) != EOF) {{ return 6; }}
    fclose(file);
    file = fopen("{existing}", "a+");
    if (file == 0) {{ return 7; }}
    if (fseek(file, 0, SEEK_SET) != 0 || fgetc(file) != 'x') {{ return 8; }}
    if (fseek(file, 0, SEEK_SET) != 0 || fputc('y', file) != 'y') {{ return 9; }}
    if (fseek(file, 0, SEEK_SET) != 0) {{ return 10; }}
    if (fgets(buf, sizeof(buf), file) == 0) {{ return 11; }}
    if (buf[0] != 'x' || buf[1] != 'y' || buf[2] != 0) {{ return 12; }}
    fclose(file);
    file = fopen("{missing}", "a+");
    if (file == 0) {{ return 13; }}
    fclose(file);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&existing_path).ok();
    std::fs::remove_file(&missing_path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_rename_moves_file() {
    let old_path = std::env::temp_dir().join(format!("c89lib_old_{}.txt", std::process::id()));
    let new_path = std::env::temp_dir().join(format!("c89lib_new_{}.txt", std::process::id()));
    let old_esc = old_path.to_string_lossy().replace('\\', "\\\\");
    let new_esc = new_path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *f;
    f = fopen("{old_esc}", "w");
    if (f == 0) {{ return 1; }}
    fputs("data", f);
    if (fclose(f) != 0) {{ return 2; }}
    if (rename("{old_esc}", "{new_esc}") != 0) {{ return 3; }}
    f = fopen("{new_esc}", "r");
    if (f == 0) {{ return 4; }}
    if (fgetc(f) != 'd') {{ return 5; }}
    fclose(f);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&old_path).ok();
    std::fs::remove_file(&new_path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_memchr_finds_byte() {
    let src = r#"#include <string.h>
int main(void) {
    const char *s = "hello world";
    char *p;
    p = (char *)memchr(s, 'w', 11);
    if (p == 0 || *p != 'w') { return 1; }
    if ((char *)memchr(s, 'z', 11) != 0) { return 2; }
    if ((char *)memchr(s, 'o', 4) != 0) { return 3; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_getenv_reads_host_variable() {
    // getenv 测试修改进程环境变量，并行时会互相干扰，串行化
    static GETENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = GETENV_LOCK.lock().unwrap();
    let src = r#"#include <stdlib.h>
#include <string.h>
int main(void) {
    char *v;
    if (getenv("C89_GETENV_MISSING_VAR") != 0) { return 1; }
    v = getenv("C89_GETENV_TEST_VAR");
    if (v == 0) { return 2; }
    if (strcmp(v, "hello-env") != 0) { return 3; }
    return 0;
}
"#;
    // 测试进程内设置环境变量，供模拟器 getenv ecall 读取
    std::env::set_var("C89_GETENV_TEST_VAR", "hello-env");
    std::env::remove_var("C89_GETENV_MISSING_VAR");
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_getenv_long_variable_dynamic_allocation() {
    static GETENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = GETENV_LOCK.lock().unwrap();
    // 超过 256 字节静态缓冲的环境变量：验证动态分配路径
    let long_value = "x".repeat(1200);
    let src = r#"#include <stdlib.h>
#include <string.h>
int main(void) {
    char *v;
    v = getenv("C89_GETENV_LONG_VAR");
    if (v == 0) { return 2; }
    if (strlen(v) != 1200) { return 3; }
    return 0;
}
"#;
    std::env::set_var("C89_GETENV_LONG_VAR", &long_value);
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_ungetc_pushback_and_clearerr() {
    let path = std::env::temp_dir().join(format!("c89lib_ungetc_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *f;
    int c;
    f = fopen("{escaped}", "w+");
    if (f == 0) {{ return 1; }}
    fputs("ab", f);
    rewind(f);
    if (fgetc(f) != 'a') {{ return 2; }}
    if (ungetc('X', f) == EOF) {{ return 3; }}
    if (fgetc(f) != 'X') {{ return 4; }}
    if (fgetc(f) != 'b') {{ return 5; }}
    if (fgetc(f) != EOF) {{ return 6; }}
    if (feof(f) == 0) {{ return 7; }}
    clearerr(f);
    if (feof(f) != 0) {{ return 8; }}
    rewind(f);
    if (fgetc(f) != 'a') {{ return 9; }}
    fclose(f);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_fgetpos_fsetpos_rewind() {
    let path = std::env::temp_dir().join(format!("c89lib_pos_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *f;
    fpos_t pos;
    f = fopen("{escaped}", "w+");
    if (f == 0) {{ return 1; }}
    fputs("abcdef", f);
    rewind(f);
    if (fgetc(f) != 'a') {{ return 2; }}
    if (fgetpos(f, &pos) != 0) {{ return 3; }}
    if (fgetc(f) != 'b') {{ return 4; }}
    if (fsetpos(f, &pos) != 0) {{ return 5; }}
    if (fgetc(f) != 'b') {{ return 6; }}
    fclose(f);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_tmpnam_tmpfile_freopen_setvbuf() {
    let path = std::env::temp_dir().join(format!("c89lib_freopen_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {{
    char name[L_tmpnam];
    FILE *tf;
    FILE *f;
    if (tmpnam(name) == 0) {{ return 1; }}
    if (strlen(name) == 0) {{ return 2; }}
    tf = tmpfile();
    if (tf == 0) {{ return 3; }}
    if (fputs("tmp", tf) == EOF) {{ return 4; }}
    fclose(tf);
    if (setvbuf(stdout, 0, _IONBF, 0) != 0) {{ return 5; }}
    f = fopen("{escaped}", "w");
    if (f == 0) {{ return 6; }}
    fputs("one", f);
    f = freopen("{escaped}", "a", f);
    if (f == 0) {{ return 7; }}
    fputs("two", f);
    fclose(f);
    f = fopen("{escaped}", "r");
    if (f == 0) {{ return 8; }}
    if (fgetc(f) != 'o') {{ return 9; }}
    if (fgetc(f) != 'n') {{ return 10; }}
    fclose(f);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_variable_shadowing_semantic_and_codegen() {
    // 复现 A：内层数组遮蔽外层 int，外层赋值不被误报为数组赋值
    let src = r#"#include <stdio.h>
int main(void) {
    int v;
    v = 100;
    {
        char v[8];
        v[0] = 'A';
        v[1] = 0;
        printf("inner=%s\n", v);
    }
    printf("outer=%d\n", v);
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
    assert!(output.contains("inner=A"), "输出不符: {output}");
    assert!(output.contains("outer=100"), "输出不符: {output}");
}

#[test]
fn lib_shadowed_array_runs_without_crash() {
    // 复现 B：内层数组 x 遮蔽外层 int x=5，数组循环写入不再错乱
    let src = r#"#include <stdio.h>
int main(void) {
    int x = 5;
    {
        char x[8];
        int i;
        for (i = 0; i < 8; i++) x[i] = (char)(i + 1);
        if (x[0] != 1) { return 1; }
        if (x[7] != 8) { return 2; }
    }
    if (x != 5) { return 3; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_parameter_shadowed_by_inner_variable() {
    let src = r#"#include <stdio.h>
int twice(int n) {
    {
        int n;
        n = 10;
    }
    return n * 2;
}
int main(void) {
    if (twice(5) != 10) { return 1; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_for_loop_variable_shadows_outer() {
    let src = r#"#include <stdio.h>
int main(void) {
    int i = 99;
    int total = 0;
    for (i = 0; i < 3; i++) {
        int i = 5;   /* 遮蔽外层循环变量 */
        total += i;
    }
    if (total != 15) { return 1; }
    if (i != 3) { return 2; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_shadowing_restores_outer_after_block() {
    let src = r#"#include <stdio.h>
int main(void) {
    int x = 7;
    {
        int x = 100;
        if (x != 100) { return 1; }
    }
    if (x != 7) { return 2; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_sibling_blocks_same_name_different_types() {
    // 兄弟块声明同名不同类型变量：扁平符号表共享，必须都改唯一名
    let src = r#"#include <stdio.h>
int main(void) {
    { int v; v = 111; if (v != 111) { return 1; } }
    { char v[8]; v[0] = 'A'; v[1] = 0; if (v[0] != 'A') { return 2; } }
    { int v; v = 222; if (v != 222) { return 3; } }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_if_else_branches_same_name_different_types() {
    // if/else 分支声明同名不同类型变量
    let src = r#"#include <stdio.h>
int main(void) {
    int flag = 1;
    if (flag) {
        int x;
        x = 10;
        if (x != 10) { return 1; }
    } else {
        char x[4];
        x[0] = 'B';
        if (x[0] != 'B') { return 2; }
    }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_calloc_rejects_overflow() {
    // 32 位 size_t 回绕：count * size 超过 UINT_MAX 必须返回 NULL
    expect_exit(
        r#"#include <stdlib.h>
int main(void) {
    if (calloc(2147483649u, 4u) != 0) { return 1; }
    if (calloc(0xFFFFFFFFu, 2u) != 0) { return 2; }
    if (calloc(4294967295u, 4294967295u) != 0) { return 3; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_strtol_strtoul_erange_on_overflow() {
    expect_exit(
        r#"#include <errno.h>
#include <limits.h>
#include <stdlib.h>
int main(void) {
    char *end;
    long v;
    unsigned long u;
    errno = 0;
    v = strtol("3000000000", 0, 10);
    if (v != LONG_MAX || errno != ERANGE) { return 1; }
    errno = 0;
    v = strtol("-3000000000", 0, 10);
    if (v != LONG_MIN || errno != ERANGE) { return 2; }
    errno = 0;
    u = strtoul("4294967296", 0, 10);
    if (u != ULONG_MAX || errno != ERANGE) { return 3; }
    errno = 0;
    v = strtol("0x80000000", 0, 0);
    if (errno != ERANGE) { return 4; }
    errno = 0;
    v = strtol("12345", &end, 10);
    if (v != 12345 || errno != 0) { return 5; }
    errno = 0;
    u = strtoul("12345", &end, 10);
    if (u != 12345UL || errno != 0) { return 6; }
    errno = 0;
    u = strtoul("4294967295", 0, 10);
    if (u != ULONG_MAX || errno != 0) { return 7; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_printf_f_large_doubles() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[400];
    int ok = 0;
    sprintf(buf, "%f", 1.0e10);
    ok += (strcmp(buf, "10000000000.000000") == 0);
    sprintf(buf, "%f", 4294967297.0);
    ok += (strcmp(buf, "4294967297.000000") == 0);
    sprintf(buf, "%f", 4294967296.0);
    ok += (strcmp(buf, "4294967296.000000") == 0);
    sprintf(buf, "%f", -1.0e15);
    ok += (strcmp(buf, "-1000000000000000.000000") == 0);
    sprintf(buf, "%f", 2.5);
    ok += (strcmp(buf, "2.500000") == 0);
    sprintf(buf, "%f", 3.14159);
    ok += (strcmp(buf, "3.141590") == 0);
    sprintf(buf, "%g", 1.0e10);
    ok += (strcmp(buf, "1e+10") == 0);
    if (ok == 7) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_math_floor_ceil_large_magnitudes() {
    expect_exit(
        r#"#include <math.h>
int main(void) {
    int ok = 0;
    double ip;
    ok += (floor(1.0e10) == 1.0e10);
    ok += (floor(-1.0e10) == -1.0e10);
    ok += (floor(2147483648.5) == 2147483648.0);
    ok += (floor(-2147483648.5) == -2147483649.0);
    ok += (ceil(2147483648.5) == 2147483649.0);
    ok += (ceil(-2147483648.5) == -2147483648.0);
    ok += (ceil(1.0e10) == 1.0e10);
    ok += (ceil(-1.0e10) == -1.0e10);
    ok += (fmod(1.0e10 + 0.5, 1.0) == 0.5);
    ok += (fmod(1.0e10 + 0.5, 2.0) == 0.5);
    ok += (fmod(1.0e10 - 0.5, 1.0) == 0.5);
    ok += (fmod(-1.0e10 - 0.5, 1.0) == -0.5);
    ip = -1.0;
    ok += (modf(2147483648.5, &ip) == 0.5);
    ok += (ip == 2147483648.0);
    ip = -1.0;
    ok += (modf(-2147483648.5, &ip) == -0.5);
    ok += (ip == -2147483648.0);
    ok += (floor(2.5) == 2.0);
    ok += (ceil(2.5) == 3.0);
    ok += (floor(-2.5) == -3.0);
    ok += (floor(2147483647.5) == 2147483647.0);
    ok += (floor(-2147483647.5) == -2147483648.0);
    ok += (pow(-2.0, 1.0e10) > 0.0);
    ok += (pow(-2.0, 1.0e10 + 1.0) < 0.0);
    ok += (pow(-2.0, 3.0) == -8.0);
    if (ok == 23) { return 0; }
    return ok;
}
"#,
        0,
    );
}

#[test]
fn lib_atan_large_arguments() {
    expect_exit(
        r#"#include <math.h>
int main(void) {
    double v;
    double w;
    double u;
    double s;
    v = atan(1.0e200);
    w = atan(-1.0e200);
    u = atan2(1.0e200, 1.0);
    s = sqrt(1.0e308);
    if (!(v - 1.570796 < 1e-3 && v - 1.570796 > -1e-3)) { return 1; }
    if (!(w + 1.570796 < 1e-3 && w + 1.570796 > -1e-3)) { return 2; }
    if (!(u - 1.570796 < 1e-3 && u - 1.570796 > -1e-3)) { return 3; }
    if (s <= 0.0) { return 4; }
    if (atan(0.5) < 0.46 || atan(0.5) > 0.47) { return 5; }
    if (atan(-0.5) > -0.46 || atan(-0.5) < -0.47) { return 6; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_fseek_pseudo_streams_rejected() {
    expect_exit(
        r#"#include <stdio.h>
int main(void) {
    if (fseek(stdin, 0, SEEK_SET) != -1) { return 1; }
    if (fseek(stdout, 0, SEEK_CUR) != -1) { return 2; }
    if (fseek(stderr, 0, SEEK_END) != -1) { return 3; }
    if (ftell(stdin) != -1) { return 4; }
    if (feof(stdin) != 0 || ferror(stdin) != 0) { return 5; }
    if (putchar('z') != 'z') { return 6; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_ungetc_eof_and_nul_pushback() {
    let path = std::env::temp_dir().join(format!("c89lib_ungetc0_{}.txt", std::process::id()));
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        r#"#include <stdio.h>
int main(void) {{
    FILE *f;
    int c;
    f = fopen("{escaped}", "w+");
    if (f == 0) {{ return 1; }}
    fputs("ab", f);
    rewind(f);
    if (fgetc(f) != 'a') {{ return 2; }}
    if (ungetc(EOF, f) != EOF) {{ return 3; }}
    if (fgetc(f) != 'b') {{ return 4; }}
    if (ungetc('\0', f) == EOF) {{ return 5; }}
    c = fgetc(f);
    if (c != 0) {{ return 6; }}
    if (fgetc(f) != EOF) {{ return 7; }}
    if (ungetc('Q', f) == EOF) {{ return 8; }}
    if (fgetc(f) != 'Q') {{ return 9; }}
    fclose(f);
    return 0;
}}
"#
    );
    let (code, _) = compile_and_run(&src);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_sscanf_extra_conversions() {
    expect_exit(
        r#"#include <stdio.h>
#include <string.h>
int main(void) {
    int a = 0;
    int b = 0;
    int n = -1;
    int count;
    unsigned int o = 0;
    unsigned int x = 0;
    char word[16];
    double e = 0.0;
    double g = 0.0;
    float f = 0.0f;
    count = sscanf("077 ff 1.25e2 2.5 3.75", "%o %x %le %lg %f", &o, &x, &e, &g, &f);
    if (count != 5) { return 1; }
    if (o != 63 || x != 255) { return 2; }
    if (e != 125.0) { return 3; }
    if (g != 2.5) { return 4; }
    if (!(f > 3.7 && f < 3.8)) { return 5; }
    count = sscanf("123456", "%3d%d", &a, &b);
    if (count != 2 || a != 123 || b != 456) { return 6; }
    count = sscanf("abcdef", "%3s", word);
    if (count != 1 || strcmp(word, "abc") != 0) { return 7; }
    count = sscanf("abc123def", "%[a-z]", word);
    if (count != 1 || strcmp(word, "abc") != 0) { return 8; }
    count = sscanf("abc123", "%[^0-9]", word);
    if (count != 1 || strcmp(word, "abc") != 0) { return 9; }
    count = sscanf("12xy", "%d%n", &a, &n);
    if (count != 1 || a != 12 || n != 2) { return 10; }
    count = sscanf(" 7", "%d%n", &a, &n);
    if (count != 1 || a != 7 || n != 2) { return 11; }
    count = sscanf("ab cd", "%2c", word);
    if (count != 1 || word[0] != 'a' || word[1] != 'b') { return 12; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_tmpfile_fclose_removes_host_file() {
    // 宿主侧断言：先推高计数器避开其他测试争用的 c89tmp1/c89tmp2，
    // tmpfile 创建的 c89tmp101..103 在 fclose 后必须从宿主目录消失。
    let src = r#"#include <stdio.h>
int main(void) {
    char name[L_tmpnam];
    FILE *f;
    int i;
    for (i = 0; i < 100; i += 1) {
        tmpnam(name);
    }
    for (i = 0; i < 3; i += 1) {
        f = tmpfile();
        if (f == 0) { return 1; }
        if (fputs("tmp", f) == EOF) { return 2; }
        if (fclose(f) != 0) { return 3; }
    }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
    for name in ["c89tmp101", "c89tmp102", "c89tmp103"] {
        assert!(
            !std::path::Path::new(name).exists(),
            "tmpfile 关闭后宿主文件仍残留: {}",
            name
        );
    }
}

#[test]
fn lib_main_receives_argc_argv() {
    expect_exit(
        r#"int main(int argc, char **argv) {
    if (argc != 1) { return 1; }
    if (argv == 0) { return 2; }
    if (argv[0] == 0) { return 3; }
    if (argv[0][0] == 0) { return 4; }
    if (argv[1] != 0) { return 5; }
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_malloc_aligned_after_large_data() {
    // 大全局数组把数据段推过 1MB（HEAP_INIT），堆基址取自数据段末尾，
    // 修复后 malloc 返回值必须 8 字节对齐（可安全解引用 double）。
    expect_exit(
        r#"#include <stdlib.h>
char big[1100001];
int main(void) {
    char *p;
    double *dp;
    p = (char *)malloc(64);
    if (p == 0) { return 1; }
    if (((unsigned int)p % 8) != 0) { return 2; }
    dp = (double *)p;
    *dp = 1.5;
    if (*dp != 1.5) { return 3; }
    big[0] = 'A';
    big[1100000] = 'Z';
    return 0;
}
"#,
        0,
    );
}

#[test]
fn lib_fscanf_stdin_keeps_unmatched_chars() {
    // stdin 无 lseek：子进程重入本测试二进制，通过管道喂 "12x"，
    // scanf 未匹配的 'x' 必须回推保留，随后 getchar 读到它。
    let exe = std::env::current_exe().expect("无法取得测试二进制路径");
    let mut child = std::process::Command::new(&exe)
        .arg("--exact")
        .arg("lib_fscanf_stdin_pushback_child")
        .env("C89_FSCANF_STDIN_CHILD", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("无法启动 stdin 子测试进程");
    {
        use std::io::Write;
        let mut handle = child.stdin.take().unwrap();
        handle.write_all(b"12x").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdin 子测试失败: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lib_fscanf_stdin_pushback_child() {
    // 仅由父测试通过 --exact 在子进程中运行（stdin 接管道）
    if std::env::var("C89_FSCANF_STDIN_CHILD").is_err() {
        return;
    }
    let src = r#"#include <stdio.h>
int main(void) {
    int a = 0;
    int b = 0;
    int c;
    int count;
    count = fscanf(stdin, "%d%d", &a, &b);
    if (count != 1) { return 1; }
    if (a != 12) { return 2; }
    c = getchar();
    if (c != 'x') { return 3; }
    return 0;
}
"#;
    let (code, _) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符：源码\n{}", src);
}

#[test]
fn lib_math_frexp_neg_inf_and_log_inf() {
    // 回归：frexp(-inf) 曾死循环（撞执行步数上限）；log(inf) 曾返回 NaN
    let src = r#"#include <stdio.h>
#include <math.h>
int main(void) {
    double ninf = -1e308 * 10.0;
    double pinf = 1e308 * 10.0;
    int e = 0;
    double r = frexp(ninf, &e);
    if (!(r < -1e300)) { return 1; }  /* r 应为 -inf */
    if (e != 0) { return 2; }
    if (!(log(pinf) > 1e300)) { return 3; }  /* log(+inf) = +inf */
    printf("ok\n");
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "退出码不符，输出: {}", output);
    assert_eq!(output, "ok\n");
}

// ---- 审计修复回归：codegen / 前端 / 运行库 ----

#[test]
fn lib_float_assignment_narrows_expression_value() {
    let src = r#"int main(void) {
    float x;
    double y;
    int r1, r2;
    y = (x = 1.0000001);
    if (!(y == (double)(float)1.0000001)) { return 1; }
    r1 = ((x = 1.0000001) == 1.0000001);
    if (r1 != 0) { return 2; }
    x = 1.0000001;
    x += 0.1;
    if (!(x == (float)(1.0000001 + 0.1))) { return 3; }
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "float 赋值表达式未收敛，输出: {}", output);
}

#[test]
fn lib_deref_array_loads_element_not_word() {
    let src = r#"int main(void) {
    char a[3] = "AB";
    double d[2] = {1.5, 2.5};
    if (*a != 'A') { return 1; }
    if (a[1] != 'B') { return 2; }
    if (*d != 1.5) { return 3; }
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "*数组名 解引用结果错误，输出: {}", output);
}

#[test]
fn lib_sizeof_string_escape_arrays() {
    let src = r#"int main(void) {
    char s[] = "\x41\x42";
    char t[] = "\n";
    if (sizeof s != 3) { return 1; }
    if (s[0] != 'A' || s[1] != 'B' || s[2] != 0) { return 2; }
    if (sizeof t != 2) { return 3; }
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "转义字符串数组长度错误，输出: {}", output);
}

#[test]
fn lib_macro_rescan_across_boundary() {
    let src = r#"
#define A F
#define F(x) (x)
#define g(x) f
#define f(x) x
int main(void) {
    if (A(3) != 3) { return 1; }
    if (g(1)(2) != 2) { return 2; }
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "宏重扫描失败，输出: {}", output);
}

#[test]
fn lib_if_unsigned_semantics() {
    let src = r#"
#if -1 < 0u
int bad = 1;
#else
int bad = 0;
#endif
int main(void) { return bad; }
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "#if 无符号语义错误，输出: {}", output);
}

#[test]
fn lib_brace_elision_initializers() {
    let src = r#"
int a[2][2] = {1,2,3,4};
int main(void) {
    int b[2][2] = {5,6,7,8};
    if (a[0][0] != 1 || a[1][1] != 4) { return 1; }
    if (b[0][0] != 5 || b[1][1] != 8) { return 2; }
    return 0;
}
"#;
    let (code, output) = compile_and_run(src);
    assert_eq!(code, 0, "花括号省略初始化被拒绝或结果错误，输出: {}", output);
}

#[test]
fn lib_enum_implicit_value_overflow_is_error() {
    expect_compile_error("enum { A = (1 << 63) - 1, B };", "枚举");
}

#[test]
fn lib_hex_escape_overflow_is_error() {
    expect_compile_error(
        r#"int main(void) { int x = '\xFFFFFFFFFFFFFFFF'; return 0; }"#,
        "溢出",
    );
}

#[test]
fn lib_tag_redefinition_is_error() {
    expect_compile_error(
        "struct S { int x; }; struct S { int y; }; int main(void) { return 0; }",
        "重复定义",
    );
    expect_compile_error(
        "enum E { A }; enum E { B }; int main(void) { return 0; }",
        "重复定义",
    );
}

#[test]
fn lib_duplicate_struct_member_is_error() {
    expect_compile_error(
        "struct S { int x; int x; }; int main(void) { return 0; }",
        "重复定义",
    );
}

#[test]
fn lib_duplicate_kr_param_is_error() {
    expect_compile_error("int f(a, a) int a; { return a; }", "重复");
}

#[test]
fn lib_use_after_scope_end_is_error() {
    expect_compile_error("int main(void) { { int x = 1; } return x; }", "未声明");
}

#[test]
fn lib_member_validation() {
    expect_compile_error(
        "struct S { int x; }; int main(void) { struct S s; return s.y; }",
        "没有成员",
    );
    expect_compile_error("int main(void) { return (1).x; }", "结构体");
}

#[test]
fn lib_addrof_non_lvalue_is_error() {
    expect_compile_error("int main(void) { int x; &(x + 1); return 0; }", "左值");
}

#[test]
fn lib_block_init_elem_type_is_error() {
    expect_compile_error(
        r#"int main(void) { int a[2] = {"x", 1}; return 0; }"#,
        "元素类型",
    );
}

#[test]
fn lib_file_scope_struct_init_excess_is_error() {
    expect_compile_error(
        "struct S { int x; }; struct S s = {1, 2}; int main(void) { return 0; }",
        "初始化器数量",
    );
}

#[test]
fn lib_invalid_int_suffix_is_error() {
    expect_compile_error("int main(void) { int x = 42ll; return 0; }", "后缀");
}

#[test]
fn lib_defined_missing_paren_is_error() {
    expect_compile_error("#if defined(X + 1)\n#endif\n", "defined");
}

#[test]
fn lib_prototype_pointee_qualifier_conflict_is_error() {
    expect_compile_error("int f(int *); int f(const int *);", "不兼容");
}

#[test]
fn lib_incomplete_struct_object_is_error() {
    expect_compile_error("struct S; struct S s;", "不完整");
}

#[test]
fn lib_deep_paren_nesting_is_error_not_crash() {
    let mut src = String::from("int main(void) { return ");
    for _ in 0..5000 {
        src.push('(');
    }
    src.push('1');
    for _ in 0..5000 {
        src.push(')');
    }
    src.push_str("; }");
    expect_compile_error(&src, "嵌套过深");
}
