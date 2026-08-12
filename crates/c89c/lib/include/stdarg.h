/* c89lib stdarg.h：va_* 由编译器作为内置操作处理 */
#ifndef C89_STDARG_H
#define C89_STDARG_H

typedef char *va_list;

/* 自引用宏展开后保留 va_* 标识符，由 c89c 后端识别。 */
#define va_start(ap, last) va_start(ap, last)
#define va_arg(ap, type) va_arg(ap, type)
#define va_end(ap) va_end(ap)

#endif
