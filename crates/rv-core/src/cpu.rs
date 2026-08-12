//! CPU 核心：寄存器、内存、指令执行、ecall 系统调用
//!
//! ecall 采用 RARS 风格约定（a7 = 功能号）：
//!   1    = print int（打印 a0 的有符号十进制）
//!   4    = print string（打印 a0 指向的 \0 结尾字符串）
//!   9    = sbrk（从堆顶分配 a0 字节，返回地址放 a0）
//!   10   = exit（a0 = 退出码）
//!   11   = print char（打印 a0 低 8 位字符）
//!   30   = time（秒级时间戳，a0 = 1970-01-01 起的秒数）
//!   31   = clock（毫秒，a0 = 模拟器启动起的毫秒数）
//!   32   = tz_offset（a0 = 宿主 UTC 偏移秒，东为正）
//!   33   = tz_dst（a0 = 1 表示宿主处于夏令时）
//!   34   = tz_name（a0 = 缓冲区，写入 NUL 结尾的时区名）
//!   35   = print hex（打印 a0 的十六进制）
//!   36   = print unsigned（打印 a0 的无符号十进制）
//!   62   = close（a0 = fd，成功返回 0，失败返回 -1）
//!   63   = read（a0 = fd, a1 = 缓冲区, a2 = 最大字节数 → a0 = 读取字节数或 -1）
//!   64   = write（a0 = fd, a1 = 缓冲区, a2 = 字节数 → a0 = 写入字节数或 -1；fd 1/2 输出到控制台）
//!   66   = lseek（a0 = fd, a1 = 偏移, a2 = whence(0/1/2) → a0 = 新偏移或 -1）
//!   1000 = system（a0 = 命令字符串 → a0 = 命令退出码或 -1）
//!   1001 = remove（a0 = 路径 → a0 = 0 或 -1）
//!   1024 = open（a0 = 路径, a1 位标志：读/写/创建/截断/追加 → a0 = fd 或 -1）
//!   fd 0/1/2 保留给 stdin/stdout/stderr

use crate::assembler::{Program, DATA_BASE, TEXT_BASE};
use crate::instr::{Instruction, Op};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::Command;

/// 内存大小（8 MiB，地址空间 0x00000000 ~ 0x007FFFFF）
pub const MEM_SIZE: usize = 8 * 1024 * 1024;
/// 栈顶初始值（向下增长）
pub const STACK_TOP: u32 = 0x007F_F000;
/// 堆初始位置：max(align16(data_end), DATA_BASE + 1MB)（由 load_program 设置）
pub const HEAP_INIT: u32 = DATA_BASE + 0x10_0000;

/// 将地址向上取整到 16 字节边界（malloc/sbrk 对齐用）
fn align16(addr: u32) -> u32 {
    addr.wrapping_add(15) & !15
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;

    let mut process = Command::new("cmd.exe");
    process.args(["/D", "/S", "/C"]);
    process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    process.raw_arg(command);
    process
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> Command {
    let mut process = Command::new("sh");
    process.args(["-c", command]);
    process
}

fn run_system(command: &str) -> (Vec<u8>, i32) {
    match shell_command(command).output() {
        Ok(result) => {
            let mut data = result.stdout;
            data.extend_from_slice(&result.stderr);
            #[cfg(windows)]
            let data = normalize_windows_shell_output(data);
            (data, result.status.code().unwrap_or(-1))
        }
        Err(_) => (Vec::new(), -1),
    }
}

/// Windows：cmd 重定向输出使用 OEM 代码页而非控制台代码页。
/// 输出不是合法 UTF-8 时，用系统实际代码页解码（不猜测）。
#[cfg(windows)]
fn normalize_windows_shell_output(data: Vec<u8>) -> Vec<u8> {
    if std::str::from_utf8(&data).is_ok() {
        return data;
    }
    unsafe {
        let cp = GetOEMCP();
        if cp == 0 || cp == 65001 {
            return data;
        }
        let count = MultiByteToWideChar(
            cp,
            0,
            data.as_ptr(),
            data.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if count <= 0 {
            return data;
        }
        let mut buf = vec![0u16; count as usize];
        let written = MultiByteToWideChar(
            cp,
            0,
            data.as_ptr(),
            data.len() as i32,
            buf.as_mut_ptr(),
            count,
        );
        if written <= 0 {
            return data;
        }
        String::from_utf16_lossy(&buf[..written as usize]).into_bytes()
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetOEMCP() -> u32;
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        mb_str: *const u8,
        mb_count: i32,
        wide_str: *mut u16,
        wide_count: i32,
    ) -> i32;
}

/// 检测宿主时区：返回 (UTC 偏移秒东为正, 是否夏令时, 时区名)。
#[cfg(windows)]
fn detect_timezone() -> (i32, bool, String) {
    // 免依赖 FFI：GetTimeZoneInformation（kernel32）
    #[repr(C)]
    struct SysTimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u16; 8],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u16; 8],
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTimeZoneInformation(tzi: *mut SysTimeZoneInformation) -> u32;
    }
    unsafe {
        let mut tzi = SysTimeZoneInformation {
            bias: 0,
            standard_name: [0; 32],
            standard_date: [0; 8],
            standard_bias: 0,
            daylight_name: [0; 32],
            daylight_date: [0; 8],
            daylight_bias: 0,
        };
        let result = GetTimeZoneInformation(&mut tzi);
        // 0 = 标准时间, 1 = 夏令时, 2 = 未知, 0xFFFFFFFF = 失败
        if result == u32::MAX {
            return (0, false, "UTC".to_string());
        }
        let dst = result == 1;
        let total_bias = tzi.bias + if dst { tzi.daylight_bias } else { 0 };
        let offset = -total_bias * 60;
        let raw = if dst {
            tzi.daylight_name
        } else {
            tzi.standard_name
        };
        let name = String::from_utf16_lossy(&raw);
        let name = name.trim_end_matches('\0').to_string();
        let name = if name.is_empty() {
            "UTC".to_string()
        } else {
            name
        };
        (offset, dst, name)
    }
}

/// 检测宿主时区（Unix：libc localtime_r 的 tm_gmtoff/tm_zone）。
#[cfg(not(windows))]
fn detect_timezone() -> (i32, bool, String) {
    unsafe {
        let mut t: libc::time_t = 0;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let offset = -tm.tm_gmtoff;
        let dst = tm.tm_isdst > 0;
        let name = if tm.tm_zone.is_null() {
            "UTC".to_string()
        } else {
            std::ffi::CStr::from_ptr(tm.tm_zone)
                .to_string_lossy()
                .into_owned()
        };
        (offset as i32, dst, name)
    }
}

pub struct Cpu {
    pub regs: [u32; 32],
    /// 浮点寄存器堆 f0~f31（64 位宽；F 扩展用低 32 位，D 扩展用全 64 位）
    pub fr: [u64; 32],
    pub pc: u32,
    pub mem: Vec<u8>,
    /// 堆顶游标（sbrk 从这里切内存）
    heap_cur: u32,
    /// 程序是否已通过 exit 退出
    pub exited: bool,
    pub exit_code: i32,
    /// ebreak 触发（供 REPL 暂停）
    pub break_hit: bool,
    /// 宿主文件描述符表（fd 0/1/2 保留给 stdin/stdout/stderr）
    files: HashMap<u32, std::fs::File>,
    /// 下一个可分配的 fd
    next_fd: u32,
    /// start instant for ecall 31 (clock)
    start_instant: std::time::Instant,
    /// 宿主时区：UTC 偏移秒（东为正），仅带符号的 i32
    tz_offset_sec: i32,
    /// 宿主时区当前是否处于夏令时
    tz_dst: bool,
    /// 宿主时区名（如 "China Standard Time" / "CST"）
    tz_name: String,
    /// fd 1/2 逐字节输出时尚未组成完整 UTF-8 字符的字节。
    pending_output: Vec<u8>,
}

/// 单步执行的结果：用于展示
/// 连续执行的结果。遇到 exit 或 ebreak 时正常返回。
#[derive(Debug)]
pub struct RunInfo {
    /// 实际执行的指令数。
    pub steps: usize,
    /// 执行期间所有 ecall 输出。
    pub syscall_output: String,
    /// 是否因 ebreak 暂停。
    pub break_hit: bool,
}

#[derive(Debug)]
pub struct StepInfo {
    pub addr: u32,
    pub instr: Instruction,
    /// (寄存器号, 旧值, 新值)
    pub changed: Vec<(u8, u32, u32)>,
    /// 浮点寄存器变化 (f号, 旧值, 新值)
    pub fr_changed: Vec<(u8, u64, u64)>,
    /// 是否发生了跳转（pc 未 +4）
    #[allow(dead_code)]
    pub jumped: bool,
    /// ecall 输出（若有）
    pub syscall_output: String,
    /// ebreak 触发（供 REPL 暂停）
    pub break_hit: bool,
}

impl Cpu {
    pub fn new() -> Self {
        let (tz_off, tz_dst, tz_name_buf) = detect_timezone();
        let mut regs = [0u32; 32];
        regs[2] = STACK_TOP; // sp
        Cpu {
            regs,
            fr: [0u64; 32],
            pc: 0,
            mem: vec![0u8; MEM_SIZE],
            heap_cur: HEAP_INIT,
            exited: false,
            exit_code: 0,
            break_hit: false,
            files: HashMap::new(),
            next_fd: 3,
            start_instant: std::time::Instant::now(),
            tz_offset_sec: tz_off,
            tz_dst,
            tz_name: tz_name_buf,
            pending_output: Vec::new(),
        }
    }

    /// 将 CPU 恢复到刚创建时的状态，并清空全部内存。
    pub fn reset(&mut self) {
        self.regs = [0; 32];
        self.regs[2] = STACK_TOP;
        self.fr = [0; 32];
        self.pc = 0;
        self.mem.fill(0);
        self.heap_cur = HEAP_INIT;
        self.exited = false;
        self.exit_code = 0;
        self.break_hit = false;
        self.files.clear();
        self.next_fd = 3;
        self.start_instant = std::time::Instant::now();
        self.pending_output.clear();
    }

    /// 加载程序。每次加载都会先完整重置 CPU，避免上一个程序的状态泄漏。
    pub fn load_program(&mut self, prog: &Program) -> Result<(), String> {
        let text_len = prog
            .instructions
            .len()
            .checked_mul(4)
            .ok_or_else(|| "程序文本段过大".to_string())?;
        let entry_offset = prog
            .entry
            .checked_sub(TEXT_BASE)
            .ok_or_else(|| format!("入口地址不在文本段: 0x{:08x}", prog.entry))?;
        if !prog.entry.is_multiple_of(4) {
            return Err(format!("入口地址未按 4 字节对齐: 0x{:08x}", prog.entry));
        }
        if entry_offset as usize >= text_len {
            return Err(format!("入口地址不在文本段: 0x{:08x}", prog.entry));
        }
        if prog.data_end as usize > MEM_SIZE {
            return Err(format!("数据段末尾越界: 0x{:08x}", prog.data_end));
        }
        for (addr, bytes) in &prog.data_segments {
            Self::checked_range(*addr, bytes.len(), "数据段")?;
        }

        self.reset();
        for (addr, bytes) in &prog.data_segments {
            let base = *addr as usize;
            self.mem[base..base + bytes.len()].copy_from_slice(bytes);
        }
        // 数据段末尾不保证 16 对齐，先向上取整再与 HEAP_INIT 取较大者，
        // 否则数据段 >1MB 且末尾非对齐时 malloc 对齐会失守
        self.heap_cur = align16(prog.data_end).max(HEAP_INIT);
        self.pc = prog.entry;
        Ok(())
    }

    fn set_reg(&mut self, rd: u8, val: u32) {
        if rd != 0 {
            self.regs[rd as usize] = val;
        }
    }

    /// 读浮点寄存器为 f32（取低 32 位）
    fn frv(&self, i: u8) -> f32 {
        f32::from_bits(self.fr[i as usize] as u32)
    }
    fn set_fr(&mut self, rd: u8, v: f32) {
        self.fr[rd as usize] = v.to_bits() as u64;
    }
    /// 读浮点寄存器为 f64（全 64 位）
    fn frv64(&self, i: u8) -> f64 {
        f64::from_bits(self.fr[i as usize])
    }
    fn set_fr64(&mut self, rd: u8, v: f64) {
        self.fr[rd as usize] = v.to_bits();
    }

    /// 浮点数值分类（fclass.d）：返回 10 位掩码（64 位版）
    fn fclass64(v: f64) -> u32 {
        let b = v.to_bits();
        let sign = (b >> 63) & 1;
        let exp = (b >> 52) & 0x7FF;
        let man = b & 0xF_FFFF_FFFF_FFFF;
        if exp == 0x7FF {
            if man == 0 {
                if sign == 1 {
                    1
                } else {
                    1 << 7
                }
            } else if man & 0x8_0000_0000_0000 != 0 {
                1 << 9
            } else {
                1 << 8
            }
        } else if exp == 0 {
            if man == 0 {
                if sign == 1 {
                    1 << 3
                } else {
                    1 << 4
                }
            } else {
                if sign == 1 {
                    1 << 2
                } else {
                    1 << 5
                }
            }
        } else {
            if sign == 1 {
                1 << 1
            } else {
                1 << 6
            }
        }
    }

    /// 浮点数值分类（fclass.s）：返回 10 位掩码
    fn fclass(v: f32) -> u32 {
        let b = v.to_bits();
        let sign = (b >> 31) & 1;
        let exp = (b >> 23) & 0xFF;
        let man = b & 0x7F_FFFF;
        if exp == 0xFF {
            if man == 0 {
                if sign == 1 {
                    1
                } else {
                    1 << 7
                }
            }
            // ±无穷
            else if man & 0x40_0000 != 0 {
                1 << 9
            }
            // 安静 NaN
            else {
                1 << 8
            } // 信令 NaN
        } else if exp == 0 {
            if man == 0 {
                if sign == 1 {
                    1 << 3
                } else {
                    1 << 4
                }
            }
            // ±零
            else {
                if sign == 1 {
                    1 << 2
                } else {
                    1 << 5
                }
            } // 非规格化
        } else {
            if sign == 1 {
                1 << 1
            } else {
                1 << 6
            } // 规格化
        }
    }

    // ---- 内存访问（小端，严格自然对齐并带边界检查）----
    fn checked_range(
        addr: u32,
        len: usize,
        operation: &str,
    ) -> Result<std::ops::Range<usize>, String> {
        let start = addr as usize;
        let end = start
            .checked_add(len)
            .ok_or_else(|| format!("{}地址范围溢出 @0x{:08x}", operation, addr))?;
        if end > MEM_SIZE {
            return Err(format!("{}越界 @0x{:08x}（长度 {}）", operation, addr, len));
        }
        Ok(start..end)
    }

    fn check_alignment(addr: u32, alignment: u32, operation: &str) -> Result<(), String> {
        if !addr.is_multiple_of(alignment) {
            return Err(format!(
                "{}地址未按 {} 字节对齐 @0x{:08x}",
                operation, alignment, addr
            ));
        }
        Ok(())
    }

    fn load8(&self, addr: u32) -> Result<u8, String> {
        let range = Self::checked_range(addr, 1, "内存读取")?;
        Ok(self.mem[range.start])
    }
    fn load16(&self, addr: u32) -> Result<u16, String> {
        Self::check_alignment(addr, 2, "内存读取")?;
        let range = Self::checked_range(addr, 2, "内存读取")?;
        Ok(u16::from_le_bytes(self.mem[range].try_into().unwrap()))
    }
    fn load32(&self, addr: u32) -> Result<u32, String> {
        Self::check_alignment(addr, 4, "内存读取")?;
        let range = Self::checked_range(addr, 4, "内存读取")?;
        Ok(u32::from_le_bytes(self.mem[range].try_into().unwrap()))
    }
    fn store8(&mut self, addr: u32, value: u8) -> Result<(), String> {
        let range = Self::checked_range(addr, 1, "内存写入")?;
        self.mem[range.start] = value;
        Ok(())
    }
    fn store16(&mut self, addr: u32, value: u16) -> Result<(), String> {
        Self::check_alignment(addr, 2, "内存写入")?;
        let range = Self::checked_range(addr, 2, "内存写入")?;
        self.mem[range].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }
    fn store32(&mut self, addr: u32, value: u32) -> Result<(), String> {
        Self::check_alignment(addr, 4, "内存写入")?;
        let range = Self::checked_range(addr, 4, "内存写入")?;
        self.mem[range].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }
    fn load64(&self, addr: u32) -> Result<u64, String> {
        Self::check_alignment(addr, 8, "内存读取")?;
        let range = Self::checked_range(addr, 8, "内存读取")?;
        Ok(u64::from_le_bytes(self.mem[range].try_into().unwrap()))
    }
    fn store64(&mut self, addr: u32, value: u64) -> Result<(), String> {
        Self::check_alignment(addr, 8, "内存写入")?;
        let range = Self::checked_range(addr, 8, "内存写入")?;
        self.mem[range].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// 读内存（供 REPL 查看）。该接口允许任意字节地址，不要求对齐。
    pub fn peek(&self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        let range = Self::checked_range(addr, len, "内存查看")?;
        Ok(self.mem[range].to_vec())
    }

    /// 连续执行，直到程序 exit、遇到 ebreak 或达到步数上限。
    pub fn run(
        &mut self,
        instructions: &[Instruction],
        max_steps: usize,
    ) -> Result<RunInfo, String> {
        let mut output = String::new();
        for steps in 1..=max_steps {
            let info = self.step(instructions)?;
            output.push_str(&info.syscall_output);
            if self.exited || info.break_hit {
                return Ok(RunInfo {
                    steps,
                    syscall_output: output,
                    break_hit: info.break_hit,
                });
            }
        }
        Err(format!("执行步数超过上限 {}，程序未退出或暂停", max_steps))
    }

    /// 单步执行一条指令
    pub fn step(&mut self, instructions: &[Instruction]) -> Result<StepInfo, String> {
        if self.exited {
            return Err("程序已退出".to_string());
        }
        // x0 是硬件零寄存器；调试器或嵌入方的误写不能影响指令读取。
        self.regs[0] = 0;
        let addr = self.pc;
        let offset = addr
            .checked_sub(TEXT_BASE)
            .ok_or_else(|| format!("PC 越界: 0x{:08x} 不在文本段", addr))?;
        if !addr.is_multiple_of(4) {
            return Err(format!("PC 未按 4 字节对齐: 0x{:08x}", addr));
        }
        let idx = (offset / 4) as usize;
        let instr = instructions
            .get(idx)
            .ok_or_else(|| format!("PC 越界: 0x{:08x} 不是有效指令地址", addr))?;
        let instr = *instr;
        let old = self.regs;
        let old_fr = self.fr;
        let mut syscall_output = String::new();

        self.break_hit = false;
        let mut jumped = false;

        match instr.op {
            Op::Add => {
                let v = self.regs[instr.rs1 as usize].wrapping_add(self.regs[instr.rs2 as usize]);
                self.set_reg(instr.rd, v);
            }
            Op::Sub => {
                let v = self.regs[instr.rs1 as usize].wrapping_sub(self.regs[instr.rs2 as usize]);
                self.set_reg(instr.rd, v);
            }
            Op::Sll => {
                let v = self.regs[instr.rs1 as usize] << (self.regs[instr.rs2 as usize] & 31);
                self.set_reg(instr.rd, v);
            }
            Op::Slt => {
                let v = ((self.regs[instr.rs1 as usize] as i32)
                    < (self.regs[instr.rs2 as usize] as i32)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Sltu => {
                let v = (self.regs[instr.rs1 as usize] < self.regs[instr.rs2 as usize]) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Xor => {
                let v = self.regs[instr.rs1 as usize] ^ self.regs[instr.rs2 as usize];
                self.set_reg(instr.rd, v);
            }
            Op::Srl => {
                let v = self.regs[instr.rs1 as usize] >> (self.regs[instr.rs2 as usize] & 31);
                self.set_reg(instr.rd, v);
            }
            Op::Sra => {
                let v = ((self.regs[instr.rs1 as usize] as i32)
                    >> (self.regs[instr.rs2 as usize] & 31)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Or => {
                let v = self.regs[instr.rs1 as usize] | self.regs[instr.rs2 as usize];
                self.set_reg(instr.rd, v);
            }
            Op::And => {
                let v = self.regs[instr.rs1 as usize] & self.regs[instr.rs2 as usize];
                self.set_reg(instr.rd, v);
            }
            // ---- M 扩展：乘除法 ----
            Op::Mul => {
                let v = self.regs[instr.rs1 as usize].wrapping_mul(self.regs[instr.rs2 as usize]);
                self.set_reg(instr.rd, v);
            }
            Op::Mulh => {
                let a = self.regs[instr.rs1 as usize] as i32 as i64;
                let b = self.regs[instr.rs2 as usize] as i32 as i64;
                let v = ((a * b) >> 32) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Mulhsu => {
                let a = self.regs[instr.rs1 as usize] as i32 as i64;
                let b = self.regs[instr.rs2 as usize] as i64;
                let v = ((a * b) >> 32) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Mulhu => {
                let a = self.regs[instr.rs1 as usize] as u64;
                let b = self.regs[instr.rs2 as usize] as u64;
                let v = ((a * b) >> 32) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Div => {
                let a = self.regs[instr.rs1 as usize] as i32;
                let b = self.regs[instr.rs2 as usize] as i32;
                // RISC-V 规范：除零 → -1；INT_MIN / -1 溢出 → INT_MIN
                let v = if b == 0 {
                    -1
                } else if a == i32::MIN && b == -1 {
                    i32::MIN
                } else {
                    a / b
                };
                self.set_reg(instr.rd, v as u32);
            }
            Op::Divu => {
                let a = self.regs[instr.rs1 as usize];
                let b = self.regs[instr.rs2 as usize];
                let v = if b == 0 { u32::MAX } else { a / b };
                self.set_reg(instr.rd, v);
            }
            Op::Rem => {
                let a = self.regs[instr.rs1 as usize] as i32;
                let b = self.regs[instr.rs2 as usize] as i32;
                // RISC-V 规范：除零 → 余数 = 被除数；INT_MIN % -1 → 0
                let v = if b == 0 {
                    a
                } else if a == i32::MIN && b == -1 {
                    0
                } else {
                    a % b
                };
                self.set_reg(instr.rd, v as u32);
            }
            Op::Remu => {
                let a = self.regs[instr.rs1 as usize];
                let b = self.regs[instr.rs2 as usize];
                let v = if b == 0 { a } else { a % b };
                self.set_reg(instr.rd, v);
            }
            // ---- F 扩展：单精度浮点（固定 RNE 舍入）----
            Op::FAddS => {
                let v = self.frv(instr.rs1) + self.frv(instr.rs2);
                self.set_fr(instr.rd, v);
            }
            Op::FSubS => {
                let v = self.frv(instr.rs1) - self.frv(instr.rs2);
                self.set_fr(instr.rd, v);
            }
            Op::FMulS => {
                let v = self.frv(instr.rs1) * self.frv(instr.rs2);
                self.set_fr(instr.rd, v);
            }
            Op::FDivS => {
                let v = self.frv(instr.rs1) / self.frv(instr.rs2);
                self.set_fr(instr.rd, v);
            }
            Op::FSqrtS => {
                let v = self.frv(instr.rs1).sqrt();
                self.set_fr(instr.rd, v);
            }
            Op::FMinS => {
                let a = self.frv(instr.rs1);
                let b = self.frv(instr.rs2);
                // RISC-V 20191213：单 NaN 返回另一操作数，双 NaN 返回 canonical NaN
                let v = if a.is_nan() {
                    if b.is_nan() {
                        f32::from_bits(0x7FC0_0000)
                    } else {
                        b
                    }
                } else if b.is_nan() {
                    a
                } else {
                    a.min(b)
                };
                self.set_fr(instr.rd, v);
            }
            Op::FMaxS => {
                let a = self.frv(instr.rs1);
                let b = self.frv(instr.rs2);
                let v = if a.is_nan() {
                    if b.is_nan() {
                        f32::from_bits(0x7FC0_0000)
                    } else {
                        b
                    }
                } else if b.is_nan() {
                    a
                } else {
                    a.max(b)
                };
                self.set_fr(instr.rd, v);
            }
            Op::FSignJ => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF)
                    | (self.fr[instr.rs2 as usize] & 0x8000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FSignJn => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF)
                    | ((self.fr[instr.rs2 as usize] ^ 0x8000_0000) & 0x8000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FSignJx => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF)
                    | ((self.fr[instr.rs1 as usize] ^ self.fr[instr.rs2 as usize]) & 0x8000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FMAddS => {
                let v = self
                    .frv(instr.rs1)
                    .mul_add(self.frv(instr.rs2), self.frv(instr.rs3));
                self.set_fr(instr.rd, v);
            }
            Op::FMSubS => {
                let v = self
                    .frv(instr.rs1)
                    .mul_add(self.frv(instr.rs2), -self.frv(instr.rs3));
                self.set_fr(instr.rd, v);
            }
            Op::FNMAddS => {
                let v = -self
                    .frv(instr.rs1)
                    .mul_add(self.frv(instr.rs2), self.frv(instr.rs3));
                self.set_fr(instr.rd, v);
            }
            Op::FNMSUBS => {
                let v = -self
                    .frv(instr.rs1)
                    .mul_add(self.frv(instr.rs2), -self.frv(instr.rs3));
                self.set_fr(instr.rd, v);
            }
            Op::FEqS => {
                let v = (self.frv(instr.rs1) == self.frv(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FLtS => {
                let v = (self.frv(instr.rs1) < self.frv(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FLeS => {
                let v = (self.frv(instr.rs1) <= self.frv(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FClassS => {
                let v = Self::fclass(self.frv(instr.rs1));
                self.set_reg(instr.rd, v);
            }
            Op::FCvtWS => {
                // 浮点 → 有符号整数（向零截断；按 RISC-V 规范饱和：
                // NaN → 0x7FFFFFFF；+∞ 或 x ≥ 2^31 → 0x7FFFFFFF；
                // -∞ 或 x < -2^31 → 0x80000000）
                let x = self.frv(instr.rs1);
                let v = if x.is_nan() || x >= 2147483648.0 {
                    0x7FFF_FFFFu32
                } else if x < -2147483648.0 {
                    0x8000_0000u32
                } else {
                    (x.trunc() as i32) as u32
                };
                self.set_reg(instr.rd, v);
            }
            Op::FCvtWUS => {
                // 浮点 → 无符号整数（向零截断；NaN → 0xFFFFFFFF；
                // +∞ 或 x ≥ 2^32 → 0xFFFFFFFF；x ≤ -1.0 → 0；
                // 其余截断转 u32，如 -0.5 截断为 -0.0 转 0）
                let x = self.frv(instr.rs1);
                let v = if x.is_nan() || x >= 4294967296.0 {
                    0xFFFF_FFFFu32
                } else if x <= -1.0 {
                    0
                } else {
                    x.trunc() as u32
                };
                self.set_reg(instr.rd, v);
            }
            Op::FCvtSW => {
                let v = self.regs[instr.rs1 as usize] as i32 as f32;
                self.set_fr(instr.rd, v);
            }
            Op::FCvtSWU => {
                let v = self.regs[instr.rs1 as usize] as f32;
                self.set_fr(instr.rd, v);
            }
            Op::FMvXW => {
                self.set_reg(instr.rd, self.fr[instr.rs1 as usize] as u32);
            }
            Op::FMvWX => {
                self.fr[instr.rd as usize] = self.regs[instr.rs1 as usize] as u64;
            }
            Op::FLw => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load32(a)?;
                self.fr[instr.rd as usize] = v as u64;
            }
            Op::FSw => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.store32(a, self.fr[instr.rs2 as usize] as u32)?;
            }
            // ---- D 扩展：双精度浮点 ----
            Op::FAddD => {
                let v = self.frv64(instr.rs1) + self.frv64(instr.rs2);
                self.set_fr64(instr.rd, v);
            }
            Op::FSubD => {
                let v = self.frv64(instr.rs1) - self.frv64(instr.rs2);
                self.set_fr64(instr.rd, v);
            }
            Op::FMulD => {
                let v = self.frv64(instr.rs1) * self.frv64(instr.rs2);
                self.set_fr64(instr.rd, v);
            }
            Op::FDivD => {
                let v = self.frv64(instr.rs1) / self.frv64(instr.rs2);
                self.set_fr64(instr.rd, v);
            }
            Op::FSqrtD => {
                let v = self.frv64(instr.rs1).sqrt();
                self.set_fr64(instr.rd, v);
            }
            Op::FMinD => {
                let a = self.frv64(instr.rs1);
                let b = self.frv64(instr.rs2);
                // RISC-V 20191213：单 NaN 返回另一操作数，双 NaN 返回 canonical NaN
                let v = if a.is_nan() {
                    if b.is_nan() {
                        f64::from_bits(0x7FF8_0000_0000_0000)
                    } else {
                        b
                    }
                } else if b.is_nan() {
                    a
                } else {
                    a.min(b)
                };
                self.set_fr64(instr.rd, v);
            }
            Op::FMaxD => {
                let a = self.frv64(instr.rs1);
                let b = self.frv64(instr.rs2);
                let v = if a.is_nan() {
                    if b.is_nan() {
                        f64::from_bits(0x7FF8_0000_0000_0000)
                    } else {
                        b
                    }
                } else if b.is_nan() {
                    a
                } else {
                    a.max(b)
                };
                self.set_fr64(instr.rd, v);
            }
            Op::FSignJD => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF_FFFF_FFFF)
                    | (self.fr[instr.rs2 as usize] & 0x8000_0000_0000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FSignJnD => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF_FFFF_FFFF)
                    | ((self.fr[instr.rs2 as usize] ^ 0x8000_0000_0000_0000)
                        & 0x8000_0000_0000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FSignJxD => {
                let v = (self.fr[instr.rs1 as usize] & 0x7FFF_FFFF_FFFF_FFFF)
                    | ((self.fr[instr.rs1 as usize] ^ self.fr[instr.rs2 as usize])
                        & 0x8000_0000_0000_0000);
                self.fr[instr.rd as usize] = v;
            }
            Op::FMAddD => {
                let v = self
                    .frv64(instr.rs1)
                    .mul_add(self.frv64(instr.rs2), self.frv64(instr.rs3));
                self.set_fr64(instr.rd, v);
            }
            Op::FMSubD => {
                let v = self
                    .frv64(instr.rs1)
                    .mul_add(self.frv64(instr.rs2), -self.frv64(instr.rs3));
                self.set_fr64(instr.rd, v);
            }
            Op::FNMAddD => {
                let v = -self
                    .frv64(instr.rs1)
                    .mul_add(self.frv64(instr.rs2), self.frv64(instr.rs3));
                self.set_fr64(instr.rd, v);
            }
            Op::FNMSubD => {
                let v = -self
                    .frv64(instr.rs1)
                    .mul_add(self.frv64(instr.rs2), -self.frv64(instr.rs3));
                self.set_fr64(instr.rd, v);
            }
            Op::FEqD => {
                let v = (self.frv64(instr.rs1) == self.frv64(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FLtD => {
                let v = (self.frv64(instr.rs1) < self.frv64(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FLeD => {
                let v = (self.frv64(instr.rs1) <= self.frv64(instr.rs2)) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::FClassD => {
                let v = Self::fclass64(self.frv64(instr.rs1));
                self.set_reg(instr.rd, v);
            }
            Op::FCvtDS => {
                let v = self.frv(instr.rs1) as f64;
                self.set_fr64(instr.rd, v);
            }
            Op::FCvtSD => {
                let v = self.frv64(instr.rs1) as f32;
                self.set_fr(instr.rd, v);
            }
            Op::FCvtWD => {
                let x = self.frv64(instr.rs1);
                let v = if x.is_nan() || x >= 2147483648.0 {
                    0x7FFF_FFFFu32
                } else if x < -2147483648.0 {
                    0x8000_0000u32
                } else {
                    (x.trunc() as i32) as u32
                };
                self.set_reg(instr.rd, v);
            }
            Op::FCvtWUD => {
                let x = self.frv64(instr.rs1);
                let v = if x.is_nan() || x >= 4294967296.0 {
                    0xFFFF_FFFFu32
                } else if x <= -1.0 {
                    0
                } else {
                    x.trunc() as u32
                };
                self.set_reg(instr.rd, v);
            }
            Op::FCvtDW => {
                let v = self.regs[instr.rs1 as usize] as i32 as f64;
                self.set_fr64(instr.rd, v);
            }
            Op::FCvtDWU => {
                let v = self.regs[instr.rs1 as usize] as f64;
                self.set_fr64(instr.rd, v);
            }
            Op::FLd => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load64(a)?;
                self.fr[instr.rd as usize] = v;
            }
            Op::FSd => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.store64(a, self.fr[instr.rs2 as usize])?;
            }
            Op::Addi => {
                let v = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.set_reg(instr.rd, v);
            }
            Op::Slti => {
                let v = ((self.regs[instr.rs1 as usize] as i32) < instr.imm) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Sltiu => {
                let v = (self.regs[instr.rs1 as usize] < instr.imm as u32) as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Xori => {
                let v = self.regs[instr.rs1 as usize] ^ instr.imm as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Ori => {
                let v = self.regs[instr.rs1 as usize] | instr.imm as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Andi => {
                let v = self.regs[instr.rs1 as usize] & instr.imm as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Slli => {
                let v = self.regs[instr.rs1 as usize] << (instr.imm as u32 & 31);
                self.set_reg(instr.rd, v);
            }
            Op::Srli => {
                let v = self.regs[instr.rs1 as usize] >> (instr.imm as u32 & 31);
                self.set_reg(instr.rd, v);
            }
            Op::Srai => {
                let v = ((self.regs[instr.rs1 as usize] as i32) >> (instr.imm as u32 & 31)) as u32;
                self.set_reg(instr.rd, v);
            }

            Op::Lb => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load8(a)? as i8 as i32 as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Lh => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load16(a)? as i16 as i32 as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Lw => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load32(a)?;
                self.set_reg(instr.rd, v);
            }
            Op::Lbu => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load8(a)? as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Lhu => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                let v = self.load16(a)? as u32;
                self.set_reg(instr.rd, v);
            }
            Op::Sb => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.store8(a, self.regs[instr.rs2 as usize] as u8)?;
            }
            Op::Sh => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.store16(a, self.regs[instr.rs2 as usize] as u16)?;
            }
            Op::Sw => {
                let a = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32);
                self.store32(a, self.regs[instr.rs2 as usize])?;
            }

            Op::Beq => {
                if self.regs[instr.rs1 as usize] == self.regs[instr.rs2 as usize] {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }
            Op::Bne => {
                if self.regs[instr.rs1 as usize] != self.regs[instr.rs2 as usize] {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }
            Op::Blt => {
                if (self.regs[instr.rs1 as usize] as i32) < (self.regs[instr.rs2 as usize] as i32) {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }
            Op::Bge => {
                if (self.regs[instr.rs1 as usize] as i32) >= (self.regs[instr.rs2 as usize] as i32)
                {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }
            Op::Bltu => {
                if self.regs[instr.rs1 as usize] < self.regs[instr.rs2 as usize] {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }
            Op::Bgeu => {
                if self.regs[instr.rs1 as usize] >= self.regs[instr.rs2 as usize] {
                    self.pc = self.pc.wrapping_add(instr.imm as u32);
                    jumped = true;
                }
            }

            Op::Jal => {
                self.set_reg(instr.rd, self.pc.wrapping_add(4));
                self.pc = self.pc.wrapping_add(instr.imm as u32);
                jumped = true;
            }
            Op::Jalr => {
                let link = self.pc.wrapping_add(4);
                let target = self.regs[instr.rs1 as usize].wrapping_add(instr.imm as u32) & !1;
                self.set_reg(instr.rd, link);
                self.pc = target;
                jumped = true;
            }
            Op::Lui => {
                self.set_reg(instr.rd, (instr.imm as u32) << 12);
            }
            Op::Auipc => {
                self.set_reg(instr.rd, self.pc.wrapping_add((instr.imm as u32) << 12));
            }
            Op::Ecall => {
                syscall_output = self.ecall()?;
            }
            Op::Ebreak => {
                self.break_hit = true;
            }
            Op::Fence => {}
        }

        if !jumped && !self.exited {
            self.pc = self.pc.wrapping_add(4);
        }

        // 收集变化的寄存器（用于显示）
        let mut changed = Vec::new();
        for (i, (&old_value, &new_value)) in old.iter().zip(self.regs.iter()).enumerate() {
            if new_value != old_value {
                changed.push((i as u8, old_value, new_value));
            }
        }
        let mut fr_changed = Vec::new();
        for (i, (&old_value, &new_value)) in old_fr.iter().zip(self.fr.iter()).enumerate() {
            if new_value != old_value {
                fr_changed.push((i as u8, old_value, new_value));
            }
        }

        // x0 是硬件零寄存器，即使外部调试代码改写也会在执行后恢复。
        self.regs[0] = 0;

        Ok(StepInfo {
            addr,
            instr,
            changed,
            fr_changed,
            jumped,
            syscall_output,
            break_hit: self.break_hit,
        })
    }

    fn flush_pending_output(&mut self) -> String {
        if self.pending_output.is_empty() {
            return String::new();
        }
        let output = String::from_utf8_lossy(&self.pending_output).into_owned();
        self.pending_output.clear();
        output
    }

    fn append_output_byte(&mut self, byte: u8) -> String {
        self.pending_output.push(byte);
        let decoded = std::str::from_utf8(&self.pending_output).map(str::to_owned);
        match decoded {
            Ok(output) => {
                self.pending_output.clear();
                output
            }
            Err(error) if error.error_len().is_none() => String::new(),
            Err(_) => self.flush_pending_output(),
        }
    }

    fn append_output_bytes(&mut self, bytes: &[u8]) -> String {
        let mut output = String::new();
        for &byte in bytes {
            output.push_str(&self.append_output_byte(byte));
        }
        output
    }

    /// ecall 系统调用（RARS 风格）
    fn ecall(&mut self) -> Result<String, String> {
        let a7 = self.regs[17];
        let a0 = self.regs[10];
        let mut output = String::new();
        match a7 {
            1 => {
                output.push_str(&self.flush_pending_output());
                output.push_str(&format!("{}", a0 as i32));
            }
            2 => {
                output.push_str(&self.flush_pending_output());
                output.push_str(&format!("{}", self.frv(10)));
            }
            3 => {
                output.push_str(&self.flush_pending_output());
                output.push_str(&format!("{}", self.frv64(10)));
            }
            4 => {
                // 打印以 \\0 结尾的 UTF-8 字符串。
                output.push_str(&self.flush_pending_output());
                let mut bytes = Vec::new();
                let mut addr = a0;
                loop {
                    let b = self.load8(addr)?;
                    if b == 0 {
                        break;
                    }
                    bytes.push(b);
                    addr = addr.wrapping_add(1);
                }
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
            9 => {
                // sbrk：请求按 16 字节向上取整，堆不得越过初始栈顶。
                let requested = a0 as usize;
                let n = requested
                    .checked_add(15)
                    .map(|value| value & !15)
                    .ok_or_else(|| format!("sbrk 请求大小溢出: {}", a0))?;
                let new_heap = (self.heap_cur as usize)
                    .checked_add(n)
                    .ok_or_else(|| format!("sbrk 请求大小溢出: {}", a0))?;
                if new_heap > STACK_TOP as usize || new_heap > MEM_SIZE {
                    return Err(format!("sbrk 堆空间不足: 请求 {} 字节", a0));
                }
                let ret = self.heap_cur;
                self.heap_cur = new_heap as u32;
                self.regs[10] = ret;
            }
            10 => {
                output.push_str(&self.flush_pending_output());
                self.exited = true;
                self.exit_code = a0 as i32;
            }
            11 => {
                output.push_str(&self.append_output_byte((a0 & 0xFF) as u8));
            }
            30 => {
                // time：1970-01-01 起的秒数（32 位够用到 2106 年）
                let seconds = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as u32)
                    .unwrap_or(0);
                self.regs[10] = seconds;
            }
            31 => {
                self.regs[10] = self.start_instant.elapsed().as_millis() as u32;
            }
            32 => {
                // tz_offset：宿主 UTC 偏移秒（东为正），负值按位模式返回
                self.regs[10] = self.tz_offset_sec as u32;
            }
            33 => {
                // tz_dst：1 表示宿主当前处于夏令时
                self.regs[10] = if self.tz_dst { 1 } else { 0 };
            }
            34 => {
                // tz_name：把 NUL 结尾的时区名写入 a0 指向的缓冲区
                let buf = self.regs[10];
                let name = self.tz_name.as_bytes();
                let len = name.len().min(255);
                let range = Self::checked_range(buf, len + 1, "tz_name")?;
                self.mem[range.start..range.start + len].copy_from_slice(&name[..len]);
                self.mem[range.start + len] = 0;
            }
            62 => {
                // close
                let fd = self.regs[10];
                self.regs[10] = if fd >= 3 {
                    if self.files.remove(&fd).is_some() {
                        0
                    } else {
                        u32::MAX
                    }
                } else {
                    0
                };
            }
            63 => {
                // read
                let fd = self.regs[10];
                let buf = self.regs[11];
                let count = self.regs[12] as usize;
                let range = Self::checked_range(buf, count, "read")?;
                let result = if fd == 0 {
                    let mut chunk = vec![0u8; count];
                    match std::io::stdin().lock().read(&mut chunk) {
                        Ok(n) => {
                            self.mem[range.start..range.start + n].copy_from_slice(&chunk[..n]);
                            n as u32
                        }
                        Err(_) => u32::MAX,
                    }
                } else if fd == 1 || fd == 2 {
                    u32::MAX // 输出流不可读
                } else if let Some(file) = self.files.get_mut(&fd) {
                    let mut chunk = vec![0u8; count];
                    match file.read(&mut chunk) {
                        Ok(n) => {
                            self.mem[range.start..range.start + n].copy_from_slice(&chunk[..n]);
                            n as u32
                        }
                        Err(_) => u32::MAX,
                    }
                } else {
                    u32::MAX // 无效 fd
                };
                self.regs[10] = result;
            }
            64 => {
                // write
                let fd = self.regs[10];
                let buf = self.regs[11];
                let count = self.regs[12] as usize;
                let range = Self::checked_range(buf, count, "write")?;
                if fd == 1 || fd == 2 {
                    let bytes = self.mem[range].to_vec();
                    output.push_str(&self.append_output_bytes(&bytes));
                    self.regs[10] = count as u32;
                } else if let Some(file) = self.files.get_mut(&fd) {
                    match file.write_all(&self.mem[range]) {
                        Ok(()) => self.regs[10] = count as u32,
                        Err(_) => self.regs[10] = u32::MAX,
                    }
                } else {
                    self.regs[10] = u32::MAX;
                }
            }
            66 => {
                // lseek
                let fd = self.regs[10];
                let offset = self.regs[11] as i32 as i64;
                let whence = self.regs[12];
                let result = if fd >= 3 {
                    if let Some(file) = self.files.get_mut(&fd) {
                        let from = match whence {
                            0 => SeekFrom::Start(offset as u64),
                            1 => SeekFrom::Current(offset),
                            2 => SeekFrom::End(offset),
                            _ => {
                                self.regs[10] = u32::MAX;
                                return Ok(output);
                            }
                        };
                        match file.seek(from) {
                            Ok(pos) => pos as u32,
                            Err(_) => u32::MAX,
                        }
                    } else {
                        u32::MAX
                    }
                } else {
                    u32::MAX // stdin/stdout 不支持 seek
                };
                self.regs[10] = result;
            }
            1024 => {
                // open：读取 \0 结尾 UTF-8 路径（与 read_cstr/remove 一致，
                // 用 from_utf8_lossy 解码，不能逐字节 push(b as char) 双重编码）
                let path_addr = self.regs[10];
                let flags = self.regs[11];
                let mut path_bytes = Vec::new();
                let mut addr = path_addr;
                loop {
                    let b = self.load8(addr)?;
                    if b == 0 {
                        break;
                    }
                    path_bytes.push(b);
                    addr = addr.wrapping_add(1);
                }
                let path = String::from_utf8_lossy(&path_bytes);
                let readable = flags & 1 != 0;
                let writable = flags & 2 != 0;
                let create = flags & 4 != 0;
                let truncate = flags & 8 != 0;
                let append = flags & 16 != 0;
                if flags & !31 != 0
                    || (!readable && !writable)
                    || (truncate && (!writable || append))
                    || (append && !writable)
                    || (create && !writable)
                {
                    self.regs[10] = u32::MAX;
                    return Ok(output);
                }
                let mut options = std::fs::OpenOptions::new();
                options
                    .read(readable)
                    .write(writable)
                    .create(create)
                    .truncate(truncate)
                    .append(append);
                match options.open(path.as_ref()) {
                    Ok(file) => {
                        let fd = self.next_fd;
                        self.next_fd += 1;
                        self.files.insert(fd, file);
                        self.regs[10] = fd;
                    }
                    Err(_) => self.regs[10] = u32::MAX,
                }
            }
            1000 => {
                // system：通过宿主命令处理器执行命令并捕获其输出。
                let command_addr = self.regs[10];
                let mut command_bytes = Vec::new();
                let mut addr = command_addr;
                loop {
                    let byte = self.load8(addr)?;
                    if byte == 0 {
                        break;
                    }
                    command_bytes.push(byte);
                    addr = addr.wrapping_add(1);
                }
                let command = String::from_utf8_lossy(&command_bytes);
                let (data, status) = run_system(&command);
                output.push_str(&self.flush_pending_output());
                output.push_str(&String::from_utf8_lossy(&data));
                self.regs[10] = status as u32;
            }
            1001 => {
                // remove：删除宿主文件，成功返回 0，失败返回 -1。
                let path_addr = self.regs[10];
                let mut path_bytes = Vec::new();
                let mut addr = path_addr;
                loop {
                    let byte = self.load8(addr)?;
                    if byte == 0 {
                        break;
                    }
                    path_bytes.push(byte);
                    addr = addr.wrapping_add(1);
                }
                let path = String::from_utf8_lossy(&path_bytes);
                self.regs[10] = match std::fs::remove_file(path.as_ref()) {
                    Ok(()) => 0,
                    Err(_) => u32::MAX,
                };
            }
            1002 => {
                // rename：重命名/移动宿主文件，成功返回 0，失败返回 -1。
                let old_addr = self.regs[10];
                let new_addr = self.regs[11];
                let read_cstr = |addr: u32| -> Result<String, String> {
                    let mut bytes = Vec::new();
                    let mut a = addr;
                    loop {
                        let byte = self.load8(a)?;
                        if byte == 0 {
                            break;
                        }
                        bytes.push(byte);
                        a = a.wrapping_add(1);
                    }
                    Ok(String::from_utf8_lossy(&bytes).into_owned())
                };
                let old_path = read_cstr(old_addr)?;
                let new_path = read_cstr(new_addr)?;
                self.regs[10] = match std::fs::rename(&old_path, &new_path) {
                    Ok(()) => 0,
                    Err(_) => u32::MAX,
                };
            }
            1003 => {
                // getenv：读取宿主环境变量，写入 a0 指向的缓冲区（容量 a1），
                // 返回写入的字节数（不含结尾 NUL），变量不存在返回 -1。
                let name_addr = self.regs[10];
                let buf_addr = self.regs[11];
                let capacity = self.regs[12] as usize;
                let mut name_bytes = Vec::new();
                let mut a = name_addr;
                loop {
                    let byte = self.load8(a)?;
                    if byte == 0 {
                        break;
                    }
                    name_bytes.push(byte);
                    a = a.wrapping_add(1);
                }
                let name = String::from_utf8_lossy(&name_bytes);
                match std::env::var(name.as_ref()) {
                    Ok(value) => {
                        let bytes = value.as_bytes();
                        if capacity == 0 {
                            // 长度探测：只返回字节数不写入
                            self.regs[10] = bytes.len() as u32;
                            return Ok(output);
                        }
                        let n = bytes.len().min(capacity);
                        let range = Self::checked_range(buf_addr, n, "getenv")?;
                        self.mem[range.start..range.start + n].copy_from_slice(&bytes[..n]);
                        self.regs[10] = n as u32;
                    }
                    Err(_) => self.regs[10] = u32::MAX,
                }
            }
            35 => {
                output.push_str(&self.flush_pending_output());
                output.push_str(&format!("0x{:08x}", a0));
            }
            36 => {
                output.push_str(&self.flush_pending_output());
                output.push_str(&format!("{}", a0));
            }
            other => return Err(format!("不支持的 ecall 功能号: {} (a7)", other)),
        }
        Ok(output)
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::approx_constant)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::TEXT_BASE;
    use crate::instr::Op;

    /// 执行二元运算 a op b，返回 rd 结果
    fn binop(op: Op, a: u32, b: u32) -> u32 {
        let mut cpu = Cpu::new();
        cpu.regs[10] = a; // a0
        cpu.regs[11] = b; // a1
        let ins = crate::instr::Instruction::r3(op, 12, 10, 11); // a2 = a0 op a1
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        cpu.regs[12]
    }

    #[test]
    fn test_mul() {
        assert_eq!(binop(Op::Mul, 6, 7), 42);
        assert_eq!(binop(Op::Mul, (-3i32) as u32, 4), (-12i32) as u32);
        // 低 32 位截断：0x80000000 * 2 = 0x100000000 → 0
        assert_eq!(binop(Op::Mul, 0x8000_0000, 2), 0);
    }

    #[test]
    fn test_mulh() {
        // 有符号 × 有符号，取高 32 位
        // (-2^31) * (-2^31) = 2^62 → 高 32 位 0x40000000
        assert_eq!(binop(Op::Mulh, 0x8000_0000, 0x8000_0000), 0x4000_0000);
        // (-2^31) * (2^31 - 1) = -2^62 + 2^31 → 高 32 位 0xC0000000
        assert_eq!(binop(Op::Mulh, 0x8000_0000, 0x7FFF_FFFF), 0xC000_0000);
    }

    #[test]
    fn test_mulhu() {
        // 无符号 × 无符号：2^31 * 2^31 = 2^62 → 高 32 位 0x40000000
        assert_eq!(binop(Op::Mulhu, 0x8000_0000, 0x8000_0000), 0x4000_0000);
        assert_eq!(binop(Op::Mulhu, 0xFFFF_FFFF, 0xFFFF_FFFF), 0xFFFF_FFFE);
    }

    #[test]
    fn test_mulhsu() {
        // 有符号 × 无符号：(-2^31) * 2^31 = -2^62 → 高 32 位 0xC0000000
        assert_eq!(binop(Op::Mulhsu, 0x8000_0000, 0x8000_0000), 0xC000_0000);
    }

    #[test]
    fn test_div() {
        assert_eq!(binop(Op::Div, 10, 3), 3);
        assert_eq!(binop(Op::Div, (-10i32) as u32, 3), (-3i32) as u32);
        assert_eq!(binop(Op::Div, (-10i32) as u32, (-3i32) as u32), 3);
        // 除零 → -1 (0xFFFFFFFF)
        assert_eq!(binop(Op::Div, 10, 0), u32::MAX);
        // INT_MIN / -1 溢出 → INT_MIN
        assert_eq!(binop(Op::Div, 0x8000_0000, u32::MAX), 0x8000_0000);
    }

    #[test]
    fn test_divu() {
        assert_eq!(binop(Op::Divu, 10, 3), 3);
        // 无符号除零 → 0xFFFFFFFF
        assert_eq!(binop(Op::Divu, 10, 0), u32::MAX);
    }

    #[test]
    fn test_rem() {
        assert_eq!(binop(Op::Rem, 10, 3), 1);
        assert_eq!(binop(Op::Rem, (-10i32) as u32, 3), (-1i32) as u32);
        // 除零 → 余数 = 被除数
        assert_eq!(binop(Op::Rem, 10, 0), 10);
        // INT_MIN % -1 → 0
        assert_eq!(binop(Op::Rem, 0x8000_0000, u32::MAX), 0);
    }

    #[test]
    fn test_remu() {
        assert_eq!(binop(Op::Remu, 10, 3), 1);
        assert_eq!(binop(Op::Remu, 10, 0), 10);
    }

    #[test]
    fn test_mul_end_to_end() {
        // 汇编端到端：li t0,6; li t1,7; mul t2,t0,t1 → t2=42
        let src = ".text\nmain:\n    li t0, 6\n    li t1, 7\n    mul t2, t0, t1\n";
        let prog = crate::assembler::assemble(src).unwrap();
        assert_eq!(prog.instructions.len(), 3); // li 6、li 7、mul
        assert_eq!(prog.instructions[2].op, Op::Mul);
        let mut cpu = Cpu::new();
        cpu.load_program(&prog).unwrap();
        for _ in 0..3 {
            cpu.step(&prog.instructions).unwrap();
        }
        assert_eq!(cpu.regs[7], 42); // t2
    }

    // ---- F 扩展测试 ----

    /// 浮点二元运算：f10 op f11 → f12
    fn frun(op: Op, a: f32, b: f32) -> f32 {
        let mut cpu = Cpu::new();
        cpu.fr[10] = a.to_bits() as u64;
        cpu.fr[11] = b.to_bits() as u64;
        let ins = crate::instr::Instruction::r3(op, 12, 10, 11);
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        f32::from_bits(cpu.fr[12] as u32)
    }

    #[test]
    fn test_fadd() {
        assert_eq!(frun(Op::FAddS, 1.5, 2.25), 3.75);
        assert_eq!(frun(Op::FAddS, -1.5, 0.5), -1.0);
    }
    #[test]
    fn test_fmul_div() {
        assert_eq!(frun(Op::FMulS, 3.0, 4.0), 12.0);
        assert_eq!(frun(Op::FDivS, 10.0, 4.0), 2.5);
        assert!(frun(Op::FDivS, 1.0, 0.0).is_infinite());
    }
    #[test]
    fn test_fsqrt() {
        assert_eq!(frun(Op::FSqrtS, 9.0, 0.0), 3.0);
    }
    #[test]
    fn test_fminmax() {
        assert_eq!(frun(Op::FMinS, 1.5, 2.5), 1.5);
        assert_eq!(frun(Op::FMaxS, 1.5, 2.5), 2.5);
        // 单 NaN → 返回非 NaN 操作数（rs1/rs2 两个方向）
        assert_eq!(frun(Op::FMinS, f32::NAN, 1.0), 1.0);
        assert_eq!(frun(Op::FMinS, 1.0, f32::NAN), 1.0);
        assert_eq!(frun(Op::FMaxS, f32::NAN, 2.0), 2.0);
        assert_eq!(frun(Op::FMaxS, 2.0, f32::NAN), 2.0);
        // 双 NaN → canonical NaN 0x7FC00000
        assert_eq!(frun(Op::FMinS, f32::NAN, f32::NAN).to_bits(), 0x7FC0_0000);
        assert_eq!(frun(Op::FMaxS, f32::NAN, f32::NAN).to_bits(), 0x7FC0_0000);
        // -0 vs +0 → -0
        assert_eq!(frun(Op::FMinS, -0.0, 0.0).to_bits(), (-0.0f32).to_bits());
    }
    #[test]
    fn test_fmadd() {
        let mut cpu = Cpu::new();
        cpu.fr[10] = 2.0f32.to_bits() as u64;
        cpu.fr[11] = 3.0f32.to_bits() as u64;
        cpu.fr[12] = 4.0f32.to_bits() as u64;
        let ins = crate::instr::Instruction::r4(Op::FMAddS, 13, 10, 11, 12);
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(f32::from_bits(cpu.fr[13] as u32), 10.0);
    }
    #[test]
    fn test_fcvt() {
        let mut cpu = Cpu::new();
        // fcvt.s.w f12, a0（a0 = 42）
        cpu.regs[10] = 42;
        let ins = crate::instr::Instruction {
            op: Op::FCvtSW,
            rd: 12,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(f32::from_bits(cpu.fr[12] as u32), 42.0);
        // fcvt.w.s a0, f12 → 42
        let ins2 = crate::instr::Instruction {
            op: Op::FCvtWS,
            rd: 10,
            rs1: 12,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins2]).unwrap();
        assert_eq!(cpu.regs[10], 42);
    }
    #[test]
    fn test_fcvt_overflow() {
        // fcvt.w.s：按 RISC-V 规范饱和（NaN/+∞/≥2^31 → 0x7FFFFFFF；
        // -∞/<-2^31 → 0x80000000），越界时不再统一返回 0x80000000
        let mut cpu = Cpu::new();
        let ins = crate::instr::Instruction {
            op: Op::FCvtWS,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        let run = |cpu: &mut Cpu, f: f32| -> u32 {
            cpu.fr[10] = f.to_bits() as u64;
            cpu.pc = TEXT_BASE;
            cpu.step(&[ins]).unwrap();
            cpu.regs[11]
        };
        assert_eq!(run(&mut cpu, f32::NAN), 0x7FFF_FFFF);
        assert_eq!(run(&mut cpu, f32::INFINITY), 0x7FFF_FFFF);
        assert_eq!(run(&mut cpu, 2147483648.0), 0x7FFF_FFFF); // 恰好 2^31
        assert_eq!(run(&mut cpu, f32::NEG_INFINITY), 0x8000_0000);
        assert_eq!(run(&mut cpu, -2147483904.0), 0x8000_0000); // < -2^31
        assert_eq!(run(&mut cpu, -2147483648.0), 0x8000_0000); // 恰好 -2^31，合法
        assert_eq!(run(&mut cpu, 2147483520.0), 2147483520); // 2^31 - 128
        // 向零截断：-3.7 → -3
        assert_eq!(run(&mut cpu, -3.7), (-3i32) as u32);
        assert_eq!(run(&mut cpu, 3.7), 3);
    }
    #[test]
    fn test_fcvt_wus_saturation() {
        // fcvt.wu.s：NaN/+∞/≥2^32 → 0xFFFFFFFF；≤ -1.0 → 0；其余截断
        let mut cpu = Cpu::new();
        let ins = crate::instr::Instruction {
            op: Op::FCvtWUS,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        let run = |cpu: &mut Cpu, f: f32| -> u32 {
            cpu.fr[10] = f.to_bits() as u64;
            cpu.pc = TEXT_BASE;
            cpu.step(&[ins]).unwrap();
            cpu.regs[11]
        };
        assert_eq!(run(&mut cpu, f32::NAN), 0xFFFF_FFFF);
        assert_eq!(run(&mut cpu, f32::INFINITY), 0xFFFF_FFFF);
        assert_eq!(run(&mut cpu, 4294967296.0), 0xFFFF_FFFF); // 恰好 2^32
        assert_eq!(run(&mut cpu, f32::NEG_INFINITY), 0);
        assert_eq!(run(&mut cpu, -1.0), 0);
        assert_eq!(run(&mut cpu, -1.5), 0);
        assert_eq!(run(&mut cpu, -0.5), 0); // trunc → -0.0 → 0
        assert_eq!(run(&mut cpu, 3000000000.0), 3000000000);
        assert_eq!(run(&mut cpu, 42.7), 42);
    }
    #[test]
    fn test_fcvt_wd_wud_saturation() {
        // fcvt.w.d / fcvt.wu.d：双精度版饱和边界（f64 可精确表示整数边界）
        let mut cpu = Cpu::new();
        let ins_w = crate::instr::Instruction {
            op: Op::FCvtWD,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        let run_w = |cpu: &mut Cpu, f: f64| -> u32 {
            cpu.fr[10] = f.to_bits();
            cpu.pc = TEXT_BASE;
            cpu.step(&[ins_w]).unwrap();
            cpu.regs[11]
        };
        assert_eq!(run_w(&mut cpu, f64::NAN), 0x7FFF_FFFF);
        assert_eq!(run_w(&mut cpu, f64::INFINITY), 0x7FFF_FFFF);
        assert_eq!(run_w(&mut cpu, 2147483648.0), 0x7FFF_FFFF);
        assert_eq!(run_w(&mut cpu, 2147483647.0), 0x7FFF_FFFF);
        assert_eq!(run_w(&mut cpu, f64::NEG_INFINITY), 0x8000_0000);
        assert_eq!(run_w(&mut cpu, -2147483649.0), 0x8000_0000);
        assert_eq!(run_w(&mut cpu, -2147483648.0), 0x8000_0000);
        assert_eq!(run_w(&mut cpu, -3.7), (-3i32) as u32);

        let ins_wu = crate::instr::Instruction {
            op: Op::FCvtWUD,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        let run_wu = |cpu: &mut Cpu, f: f64| -> u32 {
            cpu.fr[10] = f.to_bits();
            cpu.pc = TEXT_BASE;
            cpu.step(&[ins_wu]).unwrap();
            cpu.regs[11]
        };
        assert_eq!(run_wu(&mut cpu, f64::NAN), 0xFFFF_FFFF);
        assert_eq!(run_wu(&mut cpu, f64::INFINITY), 0xFFFF_FFFF);
        assert_eq!(run_wu(&mut cpu, 4294967296.0), 0xFFFF_FFFF);
        assert_eq!(run_wu(&mut cpu, 4294967295.9), 0xFFFF_FFFF); // 截断 → 2^32-1
        assert_eq!(run_wu(&mut cpu, 4294967295.0), 0xFFFF_FFFF);
        assert_eq!(run_wu(&mut cpu, f64::NEG_INFINITY), 0);
        assert_eq!(run_wu(&mut cpu, -1.0), 0);
        assert_eq!(run_wu(&mut cpu, -0.5), 0);
        assert_eq!(run_wu(&mut cpu, 0.5), 0);
        assert_eq!(run_wu(&mut cpu, 42.7), 42);
    }
    #[test]
    fn test_fcmp() {
        // feq.s a0, f10, f11
        let mut cpu = Cpu::new();
        cpu.fr[10] = 1.5f32.to_bits() as u64;
        cpu.fr[11] = 1.5f32.to_bits() as u64;
        let ins = crate::instr::Instruction {
            op: Op::FEqS,
            rd: 12,
            rs1: 10,
            rs2: 11,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(cpu.regs[12], 1);
        // NaN 比较 → 0
        cpu.fr[11] = f32::NAN.to_bits() as u64;
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(cpu.regs[12], 0);
    }
    #[test]
    fn test_fclass() {
        assert_eq!(Cpu::fclass(3.0), 1 << 6); // 正规格化
        assert_eq!(Cpu::fclass(-3.0), 1 << 1); // 负规格化
        assert_eq!(Cpu::fclass(0.0), 1 << 4); // 正零
        assert_eq!(Cpu::fclass(-0.0), 1 << 3); // 负零
        assert_eq!(Cpu::fclass(f32::INFINITY), 1 << 7); // 正无穷
        assert_eq!(Cpu::fclass(f32::NAN), 1 << 9); // 安静 NaN
    }
    #[test]
    fn test_fsgnj() {
        // fsgnj.s：用第二个操作数的符号
        let mut cpu = Cpu::new();
        cpu.fr[10] = 3.0f32.to_bits() as u64;
        cpu.fr[11] = (-5.0f32).to_bits() as u64;
        let ins = crate::instr::Instruction::r3(Op::FSignJ, 12, 10, 11);
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(f32::from_bits(cpu.fr[12] as u32), -3.0);
    }
    #[test]
    fn test_flw_fsw() {
        // fsw f10, 0(a0); flw f11, 0(a0)
        let mut cpu = Cpu::new();
        cpu.regs[10] = DATA_BASE;
        cpu.fr[10] = 3.14f32.to_bits() as u64;
        let s = crate::instr::Instruction {
            op: Op::FSw,
            rd: 0,
            rs1: 10,
            rs2: 10,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[s]).unwrap();
        let l = crate::instr::Instruction {
            op: Op::FLw,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[l]).unwrap();
        assert_eq!(f32::from_bits(cpu.fr[11] as u32), 3.14);
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_fmv() {
        // fmv.w.x f10, a0（位拷贝）
        let mut cpu = Cpu::new();
        cpu.regs[10] = 0x40490FDB; // 3.1415926...
        let ins = crate::instr::Instruction {
            op: Op::FMvWX,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(cpu.fr[11], 0x40490FDBu64);
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_fp_end_to_end() {
        // 汇编端到端：flw 加载 1.5 + 2.25 → fadd → 结果
        let src = ".data\nval1: .float 1.5\nval2: .float 2.25\n.text\nmain:\n    la t0, val1\n    flw f0, 0(t0)\n    la t0, val2\n    flw f1, 0(t0)\n    fadd.s f2, f0, f1\n";
        let prog = crate::assembler::assemble(src).unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&prog).unwrap();
        for _ in 0..prog.instructions.len() {
            cpu.step(&prog.instructions).unwrap();
        }
        assert_eq!(f32::from_bits(cpu.fr[2] as u32), 3.75);
    }

    // ---- D 扩展测试 ----

    /// 双精度二元运算：f10 op f11 → f12
    fn drun(op: Op, a: f64, b: f64) -> f64 {
        let mut cpu = Cpu::new();
        cpu.fr[10] = a.to_bits();
        cpu.fr[11] = b.to_bits();
        let ins = crate::instr::Instruction::r3(op, 12, 10, 11);
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        f64::from_bits(cpu.fr[12])
    }

    #[test]
    fn test_dadd() {
        assert_eq!(drun(Op::FAddD, 1.5, 2.25), 3.75);
        // 双精度才有的精度：0.1 + 0.2
        assert_eq!(drun(Op::FAddD, 0.1, 0.2), 0.30000000000000004);
    }
    #[test]
    fn test_dmul_div() {
        assert_eq!(drun(Op::FMulD, 3.0, 4.0), 12.0);
        assert_eq!(drun(Op::FDivD, 10.0, 4.0), 2.5);
        assert!(drun(Op::FDivD, 1.0, 0.0).is_infinite());
    }
    #[test]
    fn test_dsqrt() {
        assert_eq!(drun(Op::FSqrtD, 9.0, 0.0), 3.0);
    }
    #[test]
    fn test_dminmax_nan() {
        assert_eq!(drun(Op::FMinD, 1.5, 2.5), 1.5);
        assert_eq!(drun(Op::FMaxD, 1.5, 2.5), 2.5);
        // 单 NaN → 返回非 NaN 操作数
        assert_eq!(drun(Op::FMinD, f64::NAN, 1.0), 1.0);
        assert_eq!(drun(Op::FMinD, 1.0, f64::NAN), 1.0);
        assert_eq!(drun(Op::FMaxD, f64::NAN, 2.0), 2.0);
        assert_eq!(drun(Op::FMaxD, 2.0, f64::NAN), 2.0);
        // 双 NaN → canonical NaN 0x7FF8000000000000
        assert_eq!(
            drun(Op::FMinD, f64::NAN, f64::NAN).to_bits(),
            0x7FF8_0000_0000_0000
        );
        assert_eq!(
            drun(Op::FMaxD, f64::NAN, f64::NAN).to_bits(),
            0x7FF8_0000_0000_0000
        );
        assert_eq!(drun(Op::FMinD, -0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    }
    #[test]
    fn test_dfcvt() {
        let mut cpu = Cpu::new();
        // fcvt.d.w f12, a0（a0 = 42 → 42.0 double）
        cpu.regs[10] = 42;
        let ins = crate::instr::Instruction {
            op: Op::FCvtDW,
            rd: 12,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(f64::from_bits(cpu.fr[12]), 42.0);
        // fcvt.w.d a0, f12 → 42
        let ins2 = crate::instr::Instruction {
            op: Op::FCvtWD,
            rd: 10,
            rs1: 12,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins2]).unwrap();
        assert_eq!(cpu.regs[10], 42);
        // fcvt.w.d +∞ 溢出 → 饱和到 0x7FFFFFFF（非 0x80000000）
        cpu.fr[12] = f64::INFINITY.to_bits();
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins2]).unwrap();
        assert_eq!(cpu.regs[10], 0x7FFF_FFFF);
    }
    #[test]
    fn test_ds_conversion() {
        let mut cpu = Cpu::new();
        // fcvt.d.s：f32 → f64
        cpu.fr[10] = 1.5f32.to_bits() as u64;
        let ins = crate::instr::Instruction {
            op: Op::FCvtDS,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins]).unwrap();
        assert_eq!(f64::from_bits(cpu.fr[11]), 1.5);
        // fcvt.s.d：f64 → f32
        cpu.fr[10] = 2.75f64.to_bits();
        let ins2 = crate::instr::Instruction {
            op: Op::FCvtSD,
            rd: 12,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[ins2]).unwrap();
        assert_eq!(f32::from_bits(cpu.fr[12] as u32), 2.75);
    }
    #[test]
    fn test_fld_fsd_alignment() {
        // fld 需要 8 字节对齐；未对齐应报错
        let mut cpu = Cpu::new();
        cpu.regs[10] = DATA_BASE + 4; // 未对齐
        let l = crate::instr::Instruction {
            op: Op::FLd,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        assert!(cpu.step(&[l]).is_err()); // 应报对齐错误
                                          // 对齐地址：fsd + fld 往返
        cpu.regs[10] = DATA_BASE;
        cpu.fr[10] = 3.141592653589793f64.to_bits();
        let s = crate::instr::Instruction {
            op: Op::FSd,
            rd: 0,
            rs1: 10,
            rs2: 10,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[s]).unwrap();
        let l2 = crate::instr::Instruction {
            op: Op::FLd,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        cpu.pc = TEXT_BASE;
        cpu.step(&[l2]).unwrap();
        assert_eq!(f64::from_bits(cpu.fr[11]), 3.141592653589793);
    }
    #[test]
    fn test_dclass() {
        assert_eq!(Cpu::fclass64(3.0), 1 << 6);
        assert_eq!(Cpu::fclass64(-3.0), 1 << 1);
        assert_eq!(Cpu::fclass64(0.0), 1 << 4);
        assert_eq!(Cpu::fclass64(f64::INFINITY), 1 << 7);
        assert_eq!(Cpu::fclass64(f64::NAN), 1 << 9);
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_double_end_to_end() {
        // 汇编端到端：.double 数据 + fld + fadd.d + fld 第二个数
        let src = ".data\nval1: .double 1.5\nval2: .double 2.25\n.text\nmain:\n    la t0, val1\n    fld f0, 0(t0)\n    la t0, val2\n    fld f1, 0(t0)\n    fadd.d f2, f0, f1\n";
        let prog = crate::assembler::assemble(src).unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&prog).unwrap();
        for _ in 0..prog.instructions.len() {
            cpu.step(&prog.instructions).unwrap();
        }
        assert_eq!(f64::from_bits(cpu.fr[2]), 3.75);
    }

    #[test]
    fn test_load_program_resets_machine_state() {
        let prog = crate::assembler::assemble(".text\nmain:\n  li a0, 7\n").unwrap();
        let mut cpu = Cpu::new();
        cpu.regs[10] = 99;
        cpu.fr[3] = 0x1234;
        cpu.mem[DATA_BASE as usize] = 0xaa;
        cpu.exited = true;
        cpu.load_program(&prog).unwrap();
        assert_eq!(cpu.regs[2], STACK_TOP);
        assert_eq!(cpu.regs[10], 0);
        assert_eq!(cpu.fr[3], 0);
        assert_eq!(cpu.mem[DATA_BASE as usize], 0);
        assert!(!cpu.exited);
        assert_eq!(cpu.pc, prog.entry);
    }

    #[test]
    fn test_pc_alignment_and_memory_alignment_errors() {
        let mut cpu = Cpu::new();
        cpu.pc = TEXT_BASE + 2;
        let error = cpu.step(&[]).unwrap_err();
        assert!(error.contains("PC 未按 4 字节对齐"));

        let mut cpu = Cpu::new();
        cpu.regs[10] = DATA_BASE + 1;
        let instruction = Instruction {
            op: Op::Lw,
            rd: 11,
            rs1: 10,
            rs2: 0,
            rs3: 0,
            imm: 0,
        };
        let error = cpu.step(&[instruction]).unwrap_err();
        assert!(error.contains("未按 4 字节对齐"));
    }

    #[test]
    fn test_run_has_stable_step_limit() {
        let program = crate::assembler::assemble(".text\nmain:\n  j main\n").unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&program).unwrap();
        let error = cpu.run(&program.instructions, 3).unwrap_err();
        assert!(error.contains("执行步数超过上限 3"));
    }

    #[test]
    fn test_run_collects_output_until_exit() {
        let program = crate::assembler::assemble(
            ".text\nmain:\n  li a0, 65\n  li a7, 11\n  ecall\n  li a0, 0\n  li a7, 10\n  ecall\n",
        )
        .unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&program).unwrap();
        let result = cpu.run(&program.instructions, 20).unwrap();
        assert_eq!(result.syscall_output, "A");
        assert!(cpu.exited);
        assert_eq!(result.steps, 6);
    }

    /// 通过 open/write/read/close/lseek ecall 验证文件 I/O 闭环。
    #[test]
    fn test_file_io_ecalls() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "rvcore_io_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        // 汇编字符串不识别 \U 等转义，先把反斜杠转成 \\（\\ 在汇编字符串里是字面反斜杠）
        let path_str = path.to_str().unwrap().replace('\\', "\\\\");

        let write_src = format!(
            ".text\nmain:\n  la a0, path\n  li a1, 14\n  li a7, 1024\n  ecall\n  mv t0, a0\n  \
             la a1, msg\n  li a2, 11\n  mv a0, t0\n  li a7, 64\n  ecall\n  \
             mv a0, t0\n  li a7, 62\n  ecall\n  \
             li a0, 0\n  li a7, 10\n  ecall\n\
             .data\npath: .asciiz \"{path_str}\"\nmsg: .asciiz \"hello world\"\n"
        );
        let program = crate::assembler::assemble(&write_src).unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&program).unwrap();
        cpu.run(&program.instructions, 100).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");

        let read_src = format!(
            ".text\nmain:\n  la a0, path\n  li a1, 1\n  li a7, 1024\n  ecall\n  mv t0, a0\n  \
             la a1, buf\n  li a2, 32\n  mv a0, t0\n  li a7, 63\n  ecall\n  mv t1, a0\n  \
             mv a0, t1\n  li a7, 1\n  ecall\n  \
             mv a0, t0\n  li a1, 6\n  li a2, 0\n  li a7, 66\n  ecall\n  mv t1, a0\n  \
             mv a0, t1\n  li a7, 1\n  ecall\n  \
             mv a0, t0\n  li a7, 62\n  ecall\n  \
             li a0, 0\n  li a7, 10\n  ecall\n\
             .data\npath: .asciiz \"{path_str}\"\nbuf: .space 32\n"
        );
        let program = crate::assembler::assemble(&read_src).unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&program).unwrap();
        let result = cpu.run(&program.instructions, 100).unwrap();
        // 读回 11 字节，lseek 到位置 6（print int 无换行）
        assert_eq!(result.syscall_output, "116");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_time_ecall() {
        let program = crate::assembler::assemble(
            ".text\nmain:\n  li a7, 30\n  ecall\n  mv t0, a0\n  li a0, 0\n  li a7, 10\n  ecall\n",
        )
        .unwrap();
        let mut cpu = Cpu::new();
        cpu.load_program(&program).unwrap();
        cpu.run(&program.instructions, 20).unwrap();
        assert!(cpu.regs[5] > 1_500_000_000); // 2026 年前后约为 17.8 亿秒
    }

    /// ecall 1024 open：guest 内存中的 UTF-8 路径不能被逐字节双重编码
    #[test]
    fn test_open_utf8_path() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "rvcore_utf8_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        // 非 ASCII 文件名：旧实现把每个 UTF-8 字节映射成码点再编码，
        // 会变成乱码文件名导致 open 失败
        let path = dir.join("开放-тест.txt");
        fs::write(&path, b"ok").unwrap();
        let path_bytes = path.to_str().unwrap().as_bytes();

        let mut cpu = Cpu::new();
        let addr = DATA_BASE;
        cpu.mem[addr as usize..addr as usize + path_bytes.len()].copy_from_slice(path_bytes);
        cpu.mem[addr as usize + path_bytes.len()] = 0;
        cpu.regs[10] = addr;
        cpu.regs[11] = 1; // 只读打开（无 create，旧实现必然打不开）
        cpu.regs[17] = 1024;
        let ecall = Instruction::new(Op::Ecall);
        cpu.pc = TEXT_BASE;
        cpu.step(&[ecall]).unwrap();
        let fd = cpu.regs[10];
        assert_ne!(fd, u32::MAX, "open 非 ASCII UTF-8 路径失败");
        assert!(fd >= 3, "open 未返回有效 fd");

        // 关闭并清理
        cpu.regs[10] = fd;
        cpu.regs[17] = 62;
        cpu.pc = TEXT_BASE;
        cpu.step(&[ecall]).unwrap();
        assert_eq!(cpu.regs[10], 0);
        fs::remove_dir_all(&dir).ok();
    }

    /// 数据段 >1MB 且末尾非 16 对齐时，堆基址应向上取整到 16 字节边界
    #[test]
    fn test_heap_base_16_aligned() {
        // DATA_BASE(0x100000) + 1MiB + 8 = 0x200008，末尾非 16 对齐
        let prog =
            crate::assembler::assemble(".data\nbig: .space 0x100008\n.text\nmain: nop\n").unwrap();
        assert_eq!(prog.data_end, DATA_BASE + 0x100008);
        let mut cpu = Cpu::new();
        cpu.load_program(&prog).unwrap();
        assert_eq!(cpu.heap_cur, DATA_BASE + 0x100010);
        assert_eq!(cpu.heap_cur % 16, 0);

        // 数据段很小时仍取 HEAP_INIT（本身 16 对齐）
        let small = crate::assembler::assemble(".data\nx: .word 1\n.text\nmain: nop\n").unwrap();
        let mut cpu2 = Cpu::new();
        cpu2.load_program(&small).unwrap();
        assert_eq!(cpu2.heap_cur, HEAP_INIT);
        assert_eq!(cpu2.heap_cur % 16, 0);

        // sbrk 从对齐后的堆顶返回 16 对齐地址
        let prog2 = crate::assembler::assemble(
            ".text\nmain:\n  li a0, 1\n  li a7, 9\n  ecall\n  mv t0, a0\n  li a0, 0\n  li a7, 10\n  ecall\n.data\nbig2: .space 0x100008\n",
        )
        .unwrap();
        let mut cpu3 = Cpu::new();
        cpu3.load_program(&prog2).unwrap();
        cpu3.run(&prog2.instructions, 20).unwrap();
        assert_eq!(cpu3.regs[5], DATA_BASE + 0x100010); // t0 = sbrk 返回值
        assert!(cpu3.regs[5] % 16 == 0);
    }
}
