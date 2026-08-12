//! 类型系统：结构体/枚举定义表 + 类型大小计算
//!
//! P4-1：类型基础设施。解析器（P4-2 起）向 TypeTable 注册 struct/enum。

use crate::ast::Type;
use std::collections::HashMap;

/// 结构体成员
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub ty: Type,
    /// 普通成员的字节偏移；位域为其所在 4 字节单元的字节偏移
    pub offset: usize,
    /// Some(w) 表示位域，宽度 w 位
    pub bit_width: Option<usize>,
    /// 位域在单元内的位偏移（0..31，低序位优先）
    pub bit_offset: usize,
}

/// 结构体/联合体定义
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub members: Vec<Member>,
    pub size: usize,
    pub is_union: bool,
}

impl StructDef {
    pub fn member_offset(&self, name: &str) -> Option<usize> {
        self.members
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.offset)
    }
}

/// 枚举定义
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    /// (常量名, 值)
    pub values: Vec<(String, i64)>,
}

impl EnumDef {
    pub fn value(&self, name: &str) -> Option<i64> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

/// 类型表：维护 struct/enum 的 tag → 定义映射
#[derive(Debug, Default, Clone)]
pub struct TypeTable {
    pub structs: HashMap<String, StructDef>,
    pub enums: HashMap<String, EnumDef>,
}

impl TypeTable {
    pub fn new() -> Self {
        TypeTable::default()
    }

    /// 类型大小（字节）
    pub fn size_of(&self, t: &Type) -> Result<usize, String> {
        match t {
            Type::Void => Err("void 类型没有大小".into()),
            Type::Char | Type::UChar => Ok(1),
            Type::Short | Type::UShort => Ok(2),
            Type::Int | Type::UInt | Type::Long | Type::ULong | Type::Float => Ok(4),
            Type::Double => Ok(8),
            Type::Qualified { ty, .. } => self.size_of(ty),
            Type::Ptr(_) => Ok(4),
            Type::Function { .. } => Err("函数类型没有对象大小".into()),
            Type::Array(elem, Some(n)) => Ok(self.size_of(elem)? * n),
            Type::Array(_, None) => Err("不完整数组类型（缺少大小）".into()),
            Type::Struct(tag) => {
                let s = self
                    .structs
                    .get(tag)
                    .ok_or_else(|| format!("未知结构体: {}", tag))?;
                Ok(s.size)
            }
            Type::Enum(_) => Ok(4),
            Type::Typedef(_) => Err("typedef 需要先解析为真实类型".into()),
        }
    }

    /// 类型的自然对齐（教学 ABI 最大按 8 字节对齐）。
    pub fn align_of(&self, ty: &Type) -> Result<usize, String> {
        match ty {
            Type::Void => Err("void 类型没有对齐".into()),
            Type::Char | Type::UChar => Ok(1),
            Type::Short | Type::UShort => Ok(2),
            Type::Int
            | Type::UInt
            | Type::Long
            | Type::ULong
            | Type::Float
            | Type::Ptr(_)
            | Type::Enum(_) => Ok(4),
            Type::Function { .. } => Err("函数类型没有对象对齐".into()),
            Type::Double => Ok(8),
            Type::Qualified { ty, .. } => self.align_of(ty),
            Type::Array(element, _) => self.align_of(element),
            Type::Struct(tag) => {
                let def = self
                    .structs
                    .get(tag)
                    .ok_or_else(|| format!("未知结构体: {}", tag))?;
                Ok(def
                    .members
                    .iter()
                    .filter_map(|member| self.align_of(&member.ty).ok())
                    .max()
                    .unwrap_or(1))
            }
            Type::Typedef(_) => Err("typedef 需要先解析为真实类型".into()),
        }
    }

    /// 计算成员偏移（struct 顺序布局，union 全 0）。
    /// 位域按 C89 §3.5.2.1 打包进 4 字节单元：低序位优先，放不下则新开单元。
    pub fn layout_struct(
        &self,
        name: &str,
        members: Vec<(String, Type, Option<usize>)>,
        is_union: bool,
    ) -> Result<StructDef, String> {
        let mut offset = 0usize;
        let mut out = Vec::new();
        let mut unit_active = false;
        let mut unit_offset = 0usize;
        let mut bit_cursor = 0usize;
        for (mname, mty, bit_width) in members {
            if let Some(w) = bit_width {
                if w > 32 {
                    return Err(format!(
                        "位域宽度 {} 超过 int 的位数 32（成员 {}）",
                        w, mname
                    ));
                }
                if w == 0 && !mname.is_empty() {
                    return Err(format!("有名位域宽度不能为 0（成员 {}）", mname));
                }
                if is_union {
                    out.push(Member {
                        name: mname,
                        ty: mty,
                        offset: 0,
                        bit_width: Some(w),
                        bit_offset: 0,
                    });
                    continue;
                }
                if w == 0 {
                    // 宽度 0：后续位域不再打入当前单元
                    unit_active = false;
                    continue;
                }
                if !unit_active || bit_cursor + w > 32 {
                    offset = offset.div_ceil(4) * 4;
                    unit_offset = offset;
                    bit_cursor = 0;
                    unit_active = true;
                }
                out.push(Member {
                    name: mname,
                    ty: mty,
                    offset: unit_offset,
                    bit_width: Some(w),
                    bit_offset: bit_cursor,
                });
                bit_cursor += w;
                offset = offset.max(unit_offset + 4);
            } else {
                unit_active = false;
                let size = self.size_of(&mty)?;
                let align = self.align_of(&mty)?.clamp(1, 8);
                let off = if is_union {
                    0
                } else {
                    offset = offset.div_ceil(align) * align;
                    offset
                };
                out.push(Member {
                    name: mname,
                    ty: mty,
                    offset: off,
                    bit_width: None,
                    bit_offset: 0,
                });
                if !is_union {
                    offset += size;
                }
            }
        }
        // 简单对齐：按最大成员大小对齐（教学简化）
        let max_align = out
            .iter()
            .map(|m| self.align_of(&m.ty).unwrap_or(1).min(8))
            .max()
            .unwrap_or(1);
        let size = if is_union {
            out.iter()
                .map(|m| self.size_of(&m.ty).unwrap_or(0))
                .max()
                .unwrap_or(0)
                .div_ceil(max_align)
                * max_align
        } else {
            // 向上取整到对齐
            let align = max_align;
            offset.div_ceil(align) * align
        };
        Ok(StructDef {
            name: name.to_string(),
            members: out,
            size,
            is_union,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn i32() -> Type {
        Type::Int
    }
    fn ptr(t: Type) -> Type {
        Type::Ptr(Box::new(t))
    }

    #[test]
    fn test_size_basic() {
        let tt = TypeTable::new();
        assert_eq!(tt.size_of(&Type::Char).unwrap(), 1);
        assert_eq!(tt.size_of(&Type::Int).unwrap(), 4);
        assert_eq!(tt.size_of(&Type::Double).unwrap(), 8);
        assert_eq!(tt.size_of(&ptr(Type::Int)).unwrap(), 4);
        assert_eq!(
            tt.size_of(&Type::Array(Box::new(Type::Int), Some(10)))
                .unwrap(),
            40
        );
        assert!(tt.size_of(&Type::Array(Box::new(Type::Int), None)).is_err());
        assert!(tt.size_of(&Type::Void).is_err());
    }

    #[test]
    fn test_size_struct() {
        let mut tt = TypeTable::new();
        let s = tt
            .layout_struct(
                "Point",
                vec![("x".into(), Type::Int, None), ("y".into(), Type::Int, None)],
                false,
            )
            .unwrap();
        assert_eq!(s.size, 8);
        assert_eq!(s.member_offset("x"), Some(0));
        assert_eq!(s.member_offset("y"), Some(4));
        tt.structs.insert("Point".into(), s);
        assert_eq!(tt.size_of(&Type::Struct("Point".into())).unwrap(), 8);
    }

    #[test]
    fn test_size_union() {
        let mut tt = TypeTable::new();
        let u = tt
            .layout_struct(
                "U",
                vec![
                    ("c".into(), Type::Char, None),
                    ("i".into(), Type::Int, None),
                ],
                true,
            )
            .unwrap();
        // union：所有成员偏移 0，大小 = 最大成员
        assert_eq!(u.member_offset("c"), Some(0));
        assert_eq!(u.member_offset("i"), Some(0));
        assert_eq!(u.size, 4);
        tt.structs.insert("U".into(), u);
        assert_eq!(tt.size_of(&Type::Struct("U".into())).unwrap(), 4);
    }

    #[test]
    fn test_size_array_of_struct() {
        let mut tt = TypeTable::new();
        let s = tt
            .layout_struct(
                "P",
                vec![("x".into(), Type::Int, None), ("y".into(), Type::Int, None)],
                false,
            )
            .unwrap();
        tt.structs.insert("P".into(), s);
        assert_eq!(
            tt.size_of(&Type::Array(Box::new(Type::Struct("P".into())), Some(3)))
                .unwrap(),
            24
        );
    }

    #[test]
    fn test_enum_value() {
        let mut tt = TypeTable::new();
        let e = EnumDef {
            name: "Color".into(),
            values: vec![("RED".into(), 0), ("GREEN".into(), 5), ("BLUE".into(), 6)],
        };
        assert_eq!(e.value("GREEN"), Some(5));
        assert_eq!(e.value("PURPLE"), None);
        tt.enums.insert("Color".into(), e);
        assert_eq!(tt.size_of(&Type::Enum("Color".into())).unwrap(), 4);
    }

    #[test]
    fn test_layout_bitfields() {
        let tt = TypeTable::new();
        // a: 3 位 + b: 5 位 → 同一单元，size 4
        let s = tt
            .layout_struct(
                "B1",
                vec![
                    ("a".into(), Type::Int, Some(3)),
                    ("b".into(), Type::UInt, Some(5)),
                ],
                false,
            )
            .unwrap();
        assert_eq!(s.size, 4);
        assert_eq!(s.members[0].offset, 0);
        assert_eq!(s.members[0].bit_offset, 0);
        assert_eq!(s.members[1].offset, 0);
        assert_eq!(s.members[1].bit_offset, 3);
        // 放不下 → 新开单元
        let s2 = tt
            .layout_struct(
                "B2",
                vec![
                    ("a".into(), Type::Int, Some(20)),
                    ("b".into(), Type::Int, Some(20)),
                ],
                false,
            )
            .unwrap();
        assert_eq!(s2.size, 8);
        assert_eq!(s2.members[1].offset, 4);
        assert_eq!(s2.members[1].bit_offset, 0);
        // 宽度 0 强制换单元
        let s3 = tt
            .layout_struct(
                "B3",
                vec![
                    ("a".into(), Type::Int, Some(3)),
                    ("".into(), Type::Int, Some(0)),
                    ("b".into(), Type::Int, Some(3)),
                ],
                false,
            )
            .unwrap();
        assert_eq!(s3.members[0].offset, 0);
        assert_eq!(s3.members[1].offset, 4);
        // 位域 + 普通成员
        let s4 = tt
            .layout_struct(
                "B4",
                vec![
                    ("f".into(), Type::Int, Some(4)),
                    ("x".into(), Type::Char, None),
                    ("g".into(), Type::Int, Some(4)),
                ],
                false,
            )
            .unwrap();
        assert_eq!(s4.members[0].offset, 0);
        assert_eq!(s4.members[1].offset, 4);
        assert_eq!(s4.members[2].offset, 8);
        // union 位域：偏移 0
        let u = tt
            .layout_struct("BU", vec![("a".into(), Type::Int, Some(3))], true)
            .unwrap();
        assert_eq!(u.members[0].offset, 0);
        assert_eq!(u.size, 4);
        // 非法宽度
        assert!(tt
            .layout_struct("X", vec![("a".into(), Type::Int, Some(33))], false)
            .is_err());
        assert!(tt
            .layout_struct("X", vec![("a".into(), Type::Int, Some(0))], false)
            .is_err());
    }
}
