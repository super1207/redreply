/* c89lib setjmp.h：非局部跳转（C89 7.6） */
#ifndef C89_SETJMP_H
#define C89_SETJMP_H

/* 环境：本实现保存 sp、s0（帧指针）、ra 三个槽，8 槽留余量 */
typedef int jmp_buf[8];

/* setjmp 必须是宏（C89 7.6.1）；自引用展开后调用函数 setjmp */
#define setjmp(env) setjmp(env)

int setjmp(jmp_buf env);
void longjmp(jmp_buf env, int val);

#endif /* C89_SETJMP_H */
