//! 指令定义：RV32I 全集
//!
//! 每条指令统一表示为 { op, rd, rs1, rs2, imm }，
//! 具体哪些字段有效由 op 决定（R/I/S/B/U/J 六种格式）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    // ---- R 型（算术/逻辑）----
    Add,
    Sub,
    Sll,
    Slt,
    Sltu,
    Xor,
    Srl,
    Sra,
    Or,
    And,
    // ---- M 扩展（乘除法）----
    Mul,
    Mulh,
    Mulhsu,
    Mulhu,
    Div,
    Divu,
    Rem,
    Remu,
    // ---- F 扩展（单精度浮点）----
    FAddS,
    FSubS,
    FMulS,
    FDivS,
    FSqrtS,
    FMinS,
    FMaxS,
    FSignJ,
    FSignJn,
    FSignJx,
    FMAddS,
    FMSubS,
    FNMAddS,
    FNMSUBS,
    FEqS,
    FLtS,
    FLeS,
    FClassS,
    FCvtWS,
    FCvtWUS,
    FCvtSW,
    FCvtSWU,
    FMvXW,
    FMvWX,
    FLw,
    FSw,
    // ---- D 扩展（双精度浮点）----
    FAddD,
    FSubD,
    FMulD,
    FDivD,
    FSqrtD,
    FMinD,
    FMaxD,
    FSignJD,
    FSignJnD,
    FSignJxD,
    FMAddD,
    FMSubD,
    FNMAddD,
    FNMSubD,
    FEqD,
    FLtD,
    FLeD,
    FClassD,
    FCvtDS,
    FCvtSD,
    FCvtWD,
    FCvtWUD,
    FCvtDW,
    FCvtDWU,
    FLd,
    FSd,
    // ---- I 型（立即数算术/移位）----
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
    Slli,
    Srli,
    Srai,
    // ---- 访存 ----
    Lb,
    Lh,
    Lw,
    Lbu,
    Lhu,
    Sb,
    Sh,
    Sw,
    // ---- 分支 ----
    Beq,
    Bne,
    Blt,
    Bge,
    Bltu,
    Bgeu,
    // ---- 跳转 ----
    Jal,
    Jalr,
    // ---- U 型 ----
    Lui,
    Auipc,
    // ---- 系统 ----
    Ecall,
    Ebreak,
    Fence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub op: Op,
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    /// FMA 指令的第三个浮点源操作数（其余指令为 0）
    pub rs3: u8,
    /// 立即数（已按语义存好：分支/jal 是相对偏移；lui/auipc 是 20 位原值）
    pub imm: i32,
}

impl Instruction {
    pub fn new(op: Op) -> Self {
        Instruction {
            op,
            rd: 0,
            rs1: 0,
            rs2: 0,
            rs3: 0,
            imm: 0,
        }
    }
    pub fn r3(op: Op, rd: u8, rs1: u8, rs2: u8) -> Self {
        Instruction {
            op,
            rd,
            rs1,
            rs2,
            rs3: 0,
            imm: 0,
        }
    }
    pub fn r4(op: Op, rd: u8, rs1: u8, rs2: u8, rs3: u8) -> Self {
        // FMA 四操作数
        Instruction {
            op,
            rd,
            rs1,
            rs2,
            rs3,
            imm: 0,
        }
    }
    pub fn r2i(op: Op, rd: u8, rs1: u8, imm: i32) -> Self {
        Instruction {
            op,
            rd,
            rs1,
            rs2: 0,
            rs3: 0,
            imm,
        }
    }
    pub fn r1i(op: Op, rd: u8, imm: i32) -> Self {
        Instruction {
            op,
            rd,
            rs1: 0,
            rs2: 0,
            rs3: 0,
            imm,
        }
    }
    pub fn r2(op: Op, rs1: u8, rs2: u8, imm: i32) -> Self {
        // 分支：imm 为相对偏移
        Instruction {
            op,
            rd: 0,
            rs1,
            rs2,
            rs3: 0,
            imm,
        }
    }

    /// 生成人类可读的汇编文本（用于反汇编显示）
    pub fn to_asm(&self) -> String {
        let name = match self.op {
            Op::Add => "add",
            Op::Sub => "sub",
            Op::Sll => "sll",
            Op::Slt => "slt",
            Op::Sltu => "sltu",
            Op::Xor => "xor",
            Op::Srl => "srl",
            Op::Sra => "sra",
            Op::Or => "or",
            Op::And => "and",
            Op::Mul => "mul",
            Op::Mulh => "mulh",
            Op::Mulhsu => "mulhsu",
            Op::Mulhu => "mulhu",
            Op::Div => "div",
            Op::Divu => "divu",
            Op::Rem => "rem",
            Op::Remu => "remu",
            Op::FAddS => "fadd.s",
            Op::FSubS => "fsub.s",
            Op::FMulS => "fmul.s",
            Op::FDivS => "fdiv.s",
            Op::FSqrtS => "fsqrt.s",
            Op::FMinS => "fmin.s",
            Op::FMaxS => "fmax.s",
            Op::FSignJ => "fsgnj.s",
            Op::FSignJn => "fsgnjn.s",
            Op::FSignJx => "fsgnjx.s",
            Op::FMAddS => "fmadd.s",
            Op::FMSubS => "fmsub.s",
            Op::FNMAddS => "fnmadd.s",
            Op::FNMSUBS => "fnmsub.s",
            Op::FEqS => "feq.s",
            Op::FLtS => "flt.s",
            Op::FLeS => "fle.s",
            Op::FClassS => "fclass.s",
            Op::FCvtWS => "fcvt.w.s",
            Op::FCvtWUS => "fcvt.wu.s",
            Op::FCvtSW => "fcvt.s.w",
            Op::FCvtSWU => "fcvt.s.wu",
            Op::FMvXW => "fmv.x.w",
            Op::FMvWX => "fmv.w.x",
            Op::FLw => "flw",
            Op::FSw => "fsw",
            Op::FAddD => "fadd.d",
            Op::FSubD => "fsub.d",
            Op::FMulD => "fmul.d",
            Op::FDivD => "fdiv.d",
            Op::FSqrtD => "fsqrt.d",
            Op::FMinD => "fmin.d",
            Op::FMaxD => "fmax.d",
            Op::FSignJD => "fsgnj.d",
            Op::FSignJnD => "fsgnjn.d",
            Op::FSignJxD => "fsgnjx.d",
            Op::FMAddD => "fmadd.d",
            Op::FMSubD => "fmsub.d",
            Op::FNMAddD => "fnmadd.d",
            Op::FNMSubD => "fnmsub.d",
            Op::FEqD => "feq.d",
            Op::FLtD => "flt.d",
            Op::FLeD => "fle.d",
            Op::FClassD => "fclass.d",
            Op::FCvtDS => "fcvt.d.s",
            Op::FCvtSD => "fcvt.s.d",
            Op::FCvtWD => "fcvt.w.d",
            Op::FCvtWUD => "fcvt.wu.d",
            Op::FCvtDW => "fcvt.d.w",
            Op::FCvtDWU => "fcvt.d.wu",
            Op::FLd => "fld",
            Op::FSd => "fsd",
            Op::Addi => "addi",
            Op::Slti => "slti",
            Op::Sltiu => "sltiu",
            Op::Xori => "xori",
            Op::Ori => "ori",
            Op::Andi => "andi",
            Op::Slli => "slli",
            Op::Srli => "srli",
            Op::Srai => "srai",
            Op::Lb => "lb",
            Op::Lh => "lh",
            Op::Lw => "lw",
            Op::Lbu => "lbu",
            Op::Lhu => "lhu",
            Op::Sb => "sb",
            Op::Sh => "sh",
            Op::Sw => "sw",
            Op::Beq => "beq",
            Op::Bne => "bne",
            Op::Blt => "blt",
            Op::Bge => "bge",
            Op::Bltu => "bltu",
            Op::Bgeu => "bgeu",
            Op::Jal => "jal",
            Op::Jalr => "jalr",
            Op::Lui => "lui",
            Op::Auipc => "auipc",
            Op::Ecall => "ecall",
            Op::Ebreak => "ebreak",
            Op::Fence => "fence",
        };
        use Op::*;
        let rd = reg_name(self.rd);
        let rs1 = reg_name(self.rs1);
        let rs2 = reg_name(self.rs2);
        match self.op {
            Add | Sub | Sll | Slt | Sltu | Xor | Srl | Sra | Or | And | Mul | Mulh | Mulhsu
            | Mulhu | Div | Divu | Rem | Remu => format!("{} {}, {}, {}", name, rd, rs1, rs2),
            Addi | Slti | Sltiu | Xori | Ori | Andi | Slli | Srli | Srai => {
                format!("{} {}, {}, {}", name, rd, rs1, self.imm)
            }
            Lb | Lh | Lw | Lbu | Lhu => format!("{} {}, {}({})", name, rd, self.imm, rs1),
            Sb | Sh | Sw => format!("{} {}, {}({})", name, rs2, self.imm, rs1),
            Beq | Bne | Blt | Bge | Bltu | Bgeu => {
                format!("{} {}, {}, {}", name, rs1, rs2, self.imm)
            }
            Jal => {
                if self.rd == 0 {
                    format!("jal {}", self.imm)
                } else {
                    format!("jal {}, {}", rd, self.imm)
                }
            }
            Jalr => format!("jalr {}, {}({})", rd, self.imm, rs1),
            Lui | Auipc => format!("{} {}, {}", name, rd, self.imm),
            // ---- F 扩展格式 ----
            FAddS | FSubS | FMulS | FDivS | FMinS | FMaxS | FSignJ | FSignJn | FSignJx => format!(
                "{} {}, {}, {}",
                name,
                f_reg_name(self.rd),
                f_reg_name(self.rs1),
                f_reg_name(self.rs2)
            ),
            FSqrtS => format!("{} {}, {}", name, f_reg_name(self.rd), f_reg_name(self.rs1)),
            FMAddS | FMSubS | FNMAddS | FNMSUBS => format!(
                "{} {}, {}, {}, {}",
                name,
                f_reg_name(self.rd),
                f_reg_name(self.rs1),
                f_reg_name(self.rs2),
                f_reg_name(self.rs3)
            ),
            FEqS | FLtS | FLeS => format!(
                "{} {}, {}, {}",
                name,
                reg_name(self.rd),
                f_reg_name(self.rs1),
                f_reg_name(self.rs2)
            ),
            FClassS | FMvXW => {
                format!("{} {}, {}", name, reg_name(self.rd), f_reg_name(self.rs1))
            }
            FCvtWS | FCvtWUS => format!(
                "{} {}, {}{}",
                name,
                reg_name(self.rd),
                f_reg_name(self.rs1),
                rm_suffix(self.rs3)
            ),
            FCvtSW | FCvtSWU => format!(
                "{} {}, {}{}",
                name,
                f_reg_name(self.rd),
                reg_name(self.rs1),
                rm_suffix(self.rs3)
            ),
            FMvWX => format!("{} {}, {}", name, f_reg_name(self.rd), reg_name(self.rs1)),
            FLw => format!(
                "{} {}, {}({})",
                name,
                f_reg_name(self.rd),
                self.imm,
                reg_name(self.rs1)
            ),
            FSw => format!(
                "{} {}, {}({})",
                name,
                f_reg_name(self.rs2),
                self.imm,
                reg_name(self.rs1)
            ),
            // ---- D 扩展格式 ----
            FAddD | FSubD | FMulD | FDivD | FMinD | FMaxD | FSignJD | FSignJnD | FSignJxD => {
                format!(
                    "{} {}, {}, {}",
                    name,
                    f_reg_name(self.rd),
                    f_reg_name(self.rs1),
                    f_reg_name(self.rs2)
                )
            }
            FSqrtD => {
                format!("{} {}, {}", name, f_reg_name(self.rd), f_reg_name(self.rs1))
            }
            FCvtDS | FCvtSD => format!(
                "{} {}, {}{}",
                name,
                f_reg_name(self.rd),
                f_reg_name(self.rs1),
                rm_suffix(self.rs3)
            ),
            FMAddD | FMSubD | FNMAddD | FNMSubD => format!(
                "{} {}, {}, {}, {}",
                name,
                f_reg_name(self.rd),
                f_reg_name(self.rs1),
                f_reg_name(self.rs2),
                f_reg_name(self.rs3)
            ),
            FEqD | FLtD | FLeD => format!(
                "{} {}, {}, {}",
                name,
                reg_name(self.rd),
                f_reg_name(self.rs1),
                f_reg_name(self.rs2)
            ),
            FClassD => {
                format!("{} {}, {}", name, reg_name(self.rd), f_reg_name(self.rs1))
            }
            FCvtWD | FCvtWUD => format!(
                "{} {}, {}{}",
                name,
                reg_name(self.rd),
                f_reg_name(self.rs1),
                rm_suffix(self.rs3)
            ),
            FCvtDW | FCvtDWU => format!(
                "{} {}, {}{}",
                name,
                f_reg_name(self.rd),
                reg_name(self.rs1),
                rm_suffix(self.rs3)
            ),
            FLd => format!(
                "{} {}, {}({})",
                name,
                f_reg_name(self.rd),
                self.imm,
                reg_name(self.rs1)
            ),
            FSd => format!(
                "{} {}, {}({})",
                name,
                f_reg_name(self.rs2),
                self.imm,
                reg_name(self.rs1)
            ),
            Ecall | Ebreak | Fence => name.to_string(),
        }
    }
}

/// 寄存器编号 → RARS 风格名称
pub fn reg_name(r: u8) -> String {
    const NAMES: [&str; 32] = [
        "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
        "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
        "t5", "t6",
    ];
    NAMES[(r & 31) as usize].to_string()
}

/// fcvt 系列指令的舍入模式后缀（rs3 字段，0=RNE 缺省不显示）
fn rm_suffix(rm: u8) -> String {
    match rm {
        1 => ", rtz".to_string(),
        2 => ", rdn".to_string(),
        3 => ", rup".to_string(),
        4 => ", rmm".to_string(),
        7 => ", dyn".to_string(),
        _ => String::new(),
    }
}

/// 浮点寄存器编号 → 名称
pub fn f_reg_name(r: u8) -> String {
    format!("f{}", r & 31)
}

/// 寄存器名称 → 编号（别名全支持）
pub fn parse_reg(s: &str) -> Result<u8, String> {
    let s = s.trim().to_lowercase();
    if let Some(n) = s.strip_prefix('x') {
        if let Ok(v) = n.parse::<u8>() {
            if v <= 31 {
                return Ok(v);
            }
        }
        return Err(format!("寄存器越界: {}", s));
    }
    let r = match s.as_str() {
        "zero" => 0,
        "ra" => 1,
        "sp" => 2,
        "gp" => 3,
        "tp" => 4,
        "t0" => 5,
        "t1" => 6,
        "t2" => 7,
        "s0" | "fp" => 8,
        "s1" => 9,
        "a0" => 10,
        "a1" => 11,
        "a2" => 12,
        "a3" => 13,
        "a4" => 14,
        "a5" => 15,
        "a6" => 16,
        "a7" => 17,
        "s2" => 18,
        "s3" => 19,
        "s4" => 20,
        "s5" => 21,
        "s6" => 22,
        "s7" => 23,
        "s8" => 24,
        "s9" => 25,
        "s10" => 26,
        "s11" => 27,
        "t3" => 28,
        "t4" => 29,
        "t5" => 30,
        "t6" => 31,
        _ => return Err(format!("未知寄存器: {}", s)),
    };
    Ok(r)
}
