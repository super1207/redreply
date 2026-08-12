/* c89lib stdio.h：标准输入输出（C89 7.9：控制台逐字符/逐行 + FILE 流文件读写） */
#ifndef C89_STDIO_H
#define C89_STDIO_H

#include <stddef.h>
#include <stdarg.h>

#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

/* 缓冲模式（setvbuf 用） */
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2

/* 临时文件名长度上限（tmpnam 用） */
#define L_tmpnam 40

typedef long fpos_t;

typedef struct {
    int fd;
    int error;
    int eof;
    /* ungetc 回推缓冲区：has_pushback 非 0 时 fgetc 先返回 pushback
       （用标志区分“回推了 '\0'”与“无回推”） */
    int pushback;
    int has_pushback;
    /* tmpfile 创建的临时文件：fclose 时删除 path 指向的宿主文件 */
    int is_tmp;
    char path[L_tmpnam];
    /* setbuf/setvbuf 配置（当前仅记录，不做实际缓冲） */
    int buf_mode;
} FILE;

#define stdin ((FILE *)0)
#define stdout ((FILE *)1)
#define stderr ((FILE *)2)

int printf(const char *format, ...);
int fprintf(FILE *stream, const char *format, ...);
int fscanf(FILE *stream, const char *format, ...);
int sprintf(char *buf, const char *format, ...);
int sscanf(const char *input, const char *format, ...);
int vprintf(const char *format, va_list arg);
int vfprintf(FILE *stream, const char *format, va_list arg);
int vsprintf(char *buf, const char *format, va_list arg);
int putchar(int c);
int puts(const char *s);
int getchar(void);
int getc(FILE *stream);
int putc(int c, FILE *stream);

FILE *fopen(const char *path, const char *mode);
int remove(const char *path);
int fclose(FILE *stream);
int fflush(FILE *stream);
int fgetc(FILE *stream);
int fputc(int c, FILE *stream);
char *fgets(char *s, int n, FILE *stream);
int fputs(const char *s, FILE *stream);
size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);
int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
int feof(FILE *stream);
int ferror(FILE *stream);
void perror(const char *s);
int rename(const char *oldpath, const char *newpath);
FILE *tmpfile(void);
char *tmpnam(char *s);
FILE *freopen(const char *path, const char *mode, FILE *stream);
void setbuf(FILE *stream, char *buf);
int setvbuf(FILE *stream, char *buf, int mode, size_t size);
int ungetc(int c, FILE *stream);
int fgetpos(FILE *stream, fpos_t *pos);
int fsetpos(FILE *stream, const fpos_t *pos);
void rewind(FILE *stream);
void clearerr(FILE *stream);

#endif
