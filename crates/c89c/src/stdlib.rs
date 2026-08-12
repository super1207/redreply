//! c89lib 标准库定位与公开符号清单
//!
//! 链接模型（无链接器）：
//! 1. 库头文件在 `-l` 时作为尖括号 include 搜索的兜底；
//! 2. 库源文件（lib/src/ 下的 .c）作为独立翻译单元编译（同样带公开符号白名单）；
//! 3. 库汇编片段（lib/asm/ 下的 .s，crt0、setjmp 等手写片段）原样拼接；
//! 4. 全部汇编文本按顺序拼接成单一汇编文件，由 rv-core 两遍汇编器解析。
//!
//! 全部库文件在编译期用 include_str! 嵌入二进制，运行时无需 lib 目录。
//! 约束：库内所有非公开符号必须带 `c89_` 前缀，避免与用户代码在平铺命名空间冲突。

use std::collections::HashSet;

/// 内嵌库头文件（基名 → 内容）
pub fn embedded_lib_headers() -> &'static [(&'static str, &'static str)] {
    &[
        ("assert.h", include_str!("../lib/include/assert.h")),
        ("ctype.h", include_str!("../lib/include/ctype.h")),
        ("errno.h", include_str!("../lib/include/errno.h")),
        ("float.h", include_str!("../lib/include/float.h")),
        ("limits.h", include_str!("../lib/include/limits.h")),
        ("locale.h", include_str!("../lib/include/locale.h")),
        ("math.h", include_str!("../lib/include/math.h")),
        ("setjmp.h", include_str!("../lib/include/setjmp.h")),
        ("stdarg.h", include_str!("../lib/include/stdarg.h")),
        ("stddef.h", include_str!("../lib/include/stddef.h")),
        ("stdio.h", include_str!("../lib/include/stdio.h")),
        ("stdlib.h", include_str!("../lib/include/stdlib.h")),
        ("string.h", include_str!("../lib/include/string.h")),
        ("time.h", include_str!("../lib/include/time.h")),
    ]
}

/// 内嵌库源文件（虚拟文件名 → 内容，按文件名排序保证拼接顺序稳定）
pub fn embedded_lib_sources() -> &'static [(&'static str, &'static str)] {
    &[
        ("ctype.c", include_str!("../lib/src/ctype.c")),
        ("errno.c", include_str!("../lib/src/errno.c")),
        ("locale.c", include_str!("../lib/src/locale.c")),
        ("math.c", include_str!("../lib/src/math.c")),
        ("stdio.c", include_str!("../lib/src/stdio.c")),
        ("stdlib.c", include_str!("../lib/src/stdlib.c")),
        ("string.c", include_str!("../lib/src/string.c")),
        ("time.c", include_str!("../lib/src/time.c")),
    ]
}

/// 内嵌库汇编片段（虚拟文件名 → 内容）
pub fn embedded_lib_asm() -> &'static [(&'static str, &'static str)] {
    &[
        ("crt0.s", include_str!("../lib/asm/crt0.s")),
        ("setjmp.s", include_str!("../lib/asm/setjmp.s")),
    ]
}

/// 库公开符号集合（头文件声明的全部函数与对象）。
///
/// 维护约定：新增库 API 时同步更新此清单，并补一个调用该函数的 e2e 测试。
pub fn lib_symbol_set() -> HashSet<String> {
    LIB_SYMBOLS.iter().map(|s| s.to_string()).collect()
}

pub const LIB_SYMBOLS: &[&str] = &[
    // errno.h
    "errno", // ctype.h
    "isalnum",
    "isalpha",
    "iscntrl",
    "isdigit",
    "isgraph",
    "islower",
    "isprint",
    "ispunct",
    "isspace",
    "isupper",
    "isxdigit",
    "tolower",
    "toupper", // string.h
    "memcpy",
    "memmove",
    "memcmp",
    "memset",
    "strcat",
    "strncat",
    "strchr",
    "strrchr",
    "strcmp",
    "strcoll",
    "strncmp",
    "strcpy",
    "strncpy",
    "strcspn",
    "strpbrk",
    "strspn",
    "strstr",
    "strlen",
    "strxfrm",
    "strtok",
    "strerror", // stdlib.h
    "abort",
    "atexit",
    "exit",
    "system",
    "abs",
    "labs",
    "div",
    "ldiv",
    "atoi",
    "atol",
    "atof",
    "strtod",
    "strtol",
    "strtoul",
    "rand",
    "srand",
    "malloc",
    "calloc",
    "realloc",
    "free",
    "qsort",
    "bsearch", // stdio.h
    "printf",
    "fprintf",
    "fscanf",
    "sprintf",
    "sscanf",
    "vprintf",
    "vfprintf",
    "vsprintf",
    "putchar",
    "puts",
    "getchar",
    "getc",
    "putc",
    "fopen",
    "remove",
    "fclose",
    "fflush",
    "fgetc",
    "fputc",
    "fgets",
    "fputs",
    "fread",
    "fwrite",
    "fseek",
    "ftell",
    "feof",
    "ferror",
    "perror",
    "rename",
    "tmpfile",
    "tmpnam",
    "freopen",
    "setbuf",
    "setvbuf",
    "ungetc",
    "fgetpos",
    "fsetpos",
    "rewind",
    "clearerr",
    "memchr",
    "getenv", // math.h
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "sinh",
    "cosh",
    "tanh",
    "exp",
    "log",
    "log10",
    "pow",
    "sqrt",
    "fabs",
    "ceil",
    "floor",
    "fmod",
    "ldexp",
    "frexp",
    "modf",
    "time",
    "clock",
    "difftime",
    "mktime",
    "gmtime",
    "localtime",
    "asctime",
    "ctime",
    "strftime", // locale.h
    "setlocale",
    "localeconv", // setjmp.h
    "setjmp",
    "longjmp", // crt0 系统调用包装
    "sys_sbrk",
    "sys_exit",
    "sys_putchar",
    "sys_read_stdin",
    "sys_open",
    "sys_close",
    "sys_read",
    "sys_write",
    "sys_lseek",
    "sys_system",
    "sys_remove",
    "sys_rename",
    "sys_getenv",
    "sys_tz_offset",
    "sys_tz_dst",
    "sys_tz_name",
    "sys_time",
    "sys_clock",
];
