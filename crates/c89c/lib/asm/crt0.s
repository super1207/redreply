# crt0锛歝89lib 鍚姩鍣紙-l 妯″紡涓嬪叆鍙ｏ級
# main 鐢辩紪璇戝櫒鐢熸垚灏惧０ ret锛岄€€鍑虹爜淇濇寔浜?a0锛沞xit 璐熻矗鎵ц atexit 鍥炶皟
# main 调用约定：argc 在 a0、argv 在 a1（C89 5.1.2.2.1）
.data
__argv0: .asciiz "c89program"
    .align 2
__argv: .word __argv0
    .word 0
.text
.globl _start
_start:
    li a0, 1
    la a1, __argv
    call main
    call exit

# sys_sbrk(int size)锛氬垎閰?size 瀛楄妭锛?6 瀵归綈锛夛紝杩斿洖鏃у爢椤?.globl sys_sbrk
sys_sbrk:
    li a7, 9
    ecall
    ret

# sys_exit(int status)锛氱珛鍗抽€€鍑烘ā鎷熷櫒
.globl sys_exit
sys_exit:
    li a7, 10
    ecall

# sys_putchar(int c)锛氳緭鍑哄崟涓瓧绗︼紙ecall 11锛?.globl sys_putchar
sys_putchar:
    li a7, 11
    ecall
    ret

# sys_read_stdin(char *buf, int count)锛氫粠 stdin 璇诲彇锛岃繑鍥炲疄闄呰鍒扮殑瀛楄妭鏁帮紙ecall 63锛?.globl sys_read_stdin
sys_read_stdin:
    li a7, 63
    mv a2, a1
    li a0, 0
    ecall
    ret

# sys_open(const char *path, int flags)锛歠lags 浣?0..4 = 璇?鍐?鍒涘缓/鎴柇/杩藉姞
.globl sys_open
sys_open:
    li a7, 1024
    ecall
    ret

# sys_close(int fd)锛氬叧闂枃浠讹紝杩斿洖 0锛堝け璐?-1锛夛紙ecall 62锛?.globl sys_close
sys_close:
    li a7, 62
    ecall
    ret

# sys_read(int fd, void *buf, int count)锛氳鍙栵紝杩斿洖瀹為檯瀛楄妭鏁帮紙澶辫触 -1锛夛紙ecall 63锛?.globl sys_read
sys_read:
    li a7, 63
    ecall
    ret

# sys_write(int fd, const void *buf, int count)锛氬啓鍏ワ紝杩斿洖瀹為檯瀛楄妭鏁帮紙澶辫触 -1锛夛紙ecall 64锛?.globl sys_write
sys_write:
    li a7, 64
    ecall
    ret

# sys_lseek(int fd, int offset, int whence)锛氬畾浣嶏紝杩斿洖鏂颁綅缃紙澶辫触 -1锛夛紙ecall 66锛?.globl sys_lseek
sys_lseek:
    li a7, 66
    ecall
    ret

# sys_system(const char *command)锛氶€氳繃瀹夸富鍛戒护澶勭悊鍣ㄦ墽琛屽懡浠?.globl sys_system
sys_system:
    li a7, 1000
    ecall
    ret

# sys_remove(const char *path)锛氬垹闄ゆ枃浠讹紝鎴愬姛 0锛堝け璐?-1锛?.globl sys_remove
sys_remove:
    li a7, 1001
    ecall
    ret

# sys_time(): seconds since 1970-01-01 00:00:00 UTC (u32)
.globl sys_time
sys_time:
    li a7, 30
    ecall
    ret

# sys_clock(): milliseconds since emulator start (u32)
.globl sys_clock
sys_clock:
    li a7, 31
    ecall
    ret

# sys_tz_offset(): host UTC offset seconds (east positive)
.globl sys_tz_offset
sys_tz_offset:
    li a7, 32
    ecall
    ret

# sys_tz_dst(): 1 if DST active else 0
.globl sys_tz_dst
sys_tz_dst:
    li a7, 33
    ecall
    ret

# sys_tz_name(char *buf): copy NUL-terminated TZ name
.globl sys_tz_name
sys_tz_name:
    li a7, 34
    ecall
    ret

# sys_rename(const char *old, const char *new): rename/move file, 0 on success, -1 on failure
.globl sys_rename
sys_rename:
    li a7, 1002
    ecall
    ret

# sys_getenv(const char *name, char *buf, int cap): write env value to buf,
# returns byte count (excluding NUL), -1 if not found
.globl sys_getenv
sys_getenv:
    li a7, 1003
    ecall
    ret
