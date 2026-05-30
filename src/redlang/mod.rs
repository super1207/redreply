use std::{cell::RefCell, collections::{BTreeMap, HashMap, HashSet, VecDeque}, error, fmt, rc::Rc, sync::Arc, thread, time::SystemTime, vec};
use encoding::Encoding;
use image::{ImageBuffer, Rgba};

use crate::{cqapi::cq_add_log_w, cqevent::do_script, pkg_can_run, G_CONST_MAP, G_LOCK, G_SINGAL_ARR, G_TEMP_CONST_MAP};

pub mod exfun;
pub(crate) mod cqexfun;
pub(crate) mod webexfun;
pub(crate) mod aifun;
pub(crate) mod astparser;
pub(crate) mod persistent_value_codec;

/// RedLang 运行时值类型（写时拷贝）
///
/// 所有值统一用 `Rc<RedValue>` 表示。
/// 共享时 `Rc::clone()` 仅增加引用计数（O(1)）。
/// 修改时通过 `Rc::make_mut()` 实现 CoW：若引用计数 > 1 则先 clone 出独立副本再修改。
#[derive(Clone, Debug)]
pub enum RedValue {
    /// 纯文本
    Text(Rc<String>),
    /// 数组，元素为 Rc<RedValue>
    Array(Vec<Rc<RedValue>>),
    /// 有序对象（key 为 String，value 为 Rc<RedValue>）
    Object(BTreeMap<String, Rc<RedValue>>),
    /// 字节集（二进制数据）
    Bin(Rc<Vec<u8>>),
    /// 函数体（已解析 AST）
    Fun(astparser::Ast),
}

#[derive(Clone, Debug)]
pub enum RedValueData {
    Text(String),
    Array(Vec<RedValueData>),
    Object(BTreeMap<String, RedValueData>),
    Bin(Vec<u8>),
    Fun(RedAstData),
}

#[derive(Clone, Debug)]
pub struct RedAstData(pub Vec<RedAstNodeData>);

#[derive(Clone, Debug)]
pub enum RedAstNodeData {
    Text(String),
    Command {
        name: String,
        args: Vec<RedAstData>,
    },
}


impl RedValue {
    pub fn is_true(&self) -> bool {
        match self {
            RedValue::Text(s) => s.as_str() == "真",
            _ => false,
        }
    }

    /// 获取类型名称（中文）
    pub fn get_type_name(&self) -> &'static str {
        match self {
            RedValue::Text(_) => "文本",
            RedValue::Array(_) => "数组",
            RedValue::Object(_) => "对象",
            RedValue::Bin(_) => "字节集",
            RedValue::Fun(_) => "函数",
        }
    }

    /// 尝试获取文本引用
    pub fn as_text(&self) -> Option<&str> {
        match self {
            RedValue::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 获取文本，非文本类型返回空字符串
    pub fn text_or_empty(&self) -> &str {
        match self {
            RedValue::Text(s) => s.as_str(),
            _ => "",
        }
    }

    /// 尝试获取数组引用
    pub fn as_array(&self) -> Option<&Vec<Rc<RedValue>>> {
        match self {
            RedValue::Array(v) => Some(v),
            _ => None,
        }
    }

    /// 尝试获取对象引用
    pub fn as_object(&self) -> Option<&BTreeMap<String, Rc<RedValue>>> {
        match self {
            RedValue::Object(m) => Some(m),
            _ => None,
        }
    }

    /// 尝试获取字节集引用
    pub fn as_bin(&self) -> Option<&Vec<u8>> {
        match self {
            RedValue::Bin(b) => Some(b.as_ref()),
            _ => None,
        }
    }

    /// 尝试获取函数体 AST 引用
    pub fn as_fun(&self) -> Option<&astparser::Ast> {
        match self {
            RedValue::Fun(s) => Some(s),
            _ => None,
        }
    }

    pub fn expect_text_value(&self) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            RedValue::Text(s) => Ok(s.as_ref().clone()),
            other => Err(RedLang::make_err(&format!("不是文本类型，当前类型:{}", other.get_type_name()))),
        }
    }

    pub fn expect_array_value(&self) -> Result<Vec<Rc<RedValue>>, Box<dyn std::error::Error>> {
        match self {
            RedValue::Array(arr) => Ok(arr.clone()),
            other => Err(RedLang::make_err(&format!("不是数组类型，当前类型:{}", other.get_type_name()))),
        }
    }

    pub fn expect_object_value(&self) -> Result<BTreeMap<String, Rc<RedValue>>, Box<dyn std::error::Error>> {
        match self {
            RedValue::Object(obj) => Ok(obj.clone()),
            other => Err(RedLang::make_err(&format!("不是对象类型，当前类型:{}", other.get_type_name()))),
        }
    }

    pub fn expect_bin_value(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        match self {
            RedValue::Bin(bin) => Ok(bin.as_ref().clone()),
            other => Err(RedLang::make_err(&format!("不是字节集类型，当前类型:{}", other.get_type_name()))),
        }
    }

}

impl RedValueData {
    pub fn from_red_value(value: &RedValue) -> Result<RedValueData, Box<dyn std::error::Error>> {
        match value {
            RedValue::Text(s) => Ok(RedValueData::Text(s.as_ref().clone())),
            RedValue::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    out.push(RedValueData::from_red_value(item)?);
                }
                Ok(RedValueData::Array(out))
            }
            RedValue::Object(obj) => {
                let mut out = BTreeMap::new();
                for (k, v) in obj {
                    out.insert(k.clone(), RedValueData::from_red_value(v)?);
                }
                Ok(RedValueData::Object(out))
            }
            RedValue::Bin(bin) => Ok(RedValueData::Bin(bin.as_ref().clone())),
            RedValue::Fun(ast) => Ok(RedValueData::Fun(RedAstData::from_ast(ast))),
        }
    }

    pub fn into_rc_value(self) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        match self {
            RedValueData::Text(s) => Ok(rv_text(s)),
            RedValueData::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    out.push(item.into_rc_value()?);
                }
                Ok(rv_array(out))
            }
            RedValueData::Object(obj) => {
                let mut out = BTreeMap::new();
                for (k, v) in obj {
                    out.insert(k, v.into_rc_value()?);
                }
                Ok(Rc::new(RedValue::Object(out)))
            }
            RedValueData::Bin(bin) => Ok(rv_bin(bin)),
            RedValueData::Fun(ast) => Ok(Rc::new(RedValue::Fun(ast.into_ast()))),
        }
    }
}

impl RedAstData {
    pub fn from_ast(ast: &astparser::Ast) -> RedAstData {
        RedAstData(ast.iter().map(RedAstNodeData::from_ast_node).collect())
    }

    pub fn into_ast(self) -> astparser::Ast {
        self.0.into_iter().map(RedAstNodeData::into_ast_node).collect()
    }
}

impl RedAstNodeData {
    fn from_ast_node(node: &astparser::AstNode) -> RedAstNodeData {
        match node {
            astparser::AstNode::Text(text) => RedAstNodeData::Text(text.as_ref().clone()),
            astparser::AstNode::Command(cmd) => RedAstNodeData::Command {
                name: cmd.name.as_ref().clone(),
                args: cmd.args.iter().map(RedAstData::from_ast).collect(),
            },
        }
    }

    fn into_ast_node(self) -> astparser::AstNode {
        match self {
            RedAstNodeData::Text(text) => astparser::AstNode::Text(Rc::new(text)),
            RedAstNodeData::Command { name, args } => astparser::AstNode::Command(astparser::AstCommand {
                name: Rc::new(name),
                args: args.into_iter().map(RedAstData::into_ast).collect(),
            }),
        }
    }
}

impl PartialEq for RedValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RedValue::Text(a), RedValue::Text(b)) => a == b,
            (RedValue::Array(a), RedValue::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| **a == **b)
            }
            (RedValue::Object(a), RedValue::Object(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(k, a_value)| {
                        b.get(k).map(|b_value| **a_value == **b_value).unwrap_or(false)
                    })
            }
            (RedValue::Bin(a), RedValue::Bin(b)) => a == b,
            (RedValue::Fun(a), RedValue::Fun(b)) => astparser::ast_to_string(a) == astparser::ast_to_string(b),
            _ => false,
        }
    }
}

/// 便捷函数：将 String 包装为 Rc<RedValue::Text>
#[inline]
pub fn rv_text(s: String) -> Rc<RedValue> {
    Rc::new(RedValue::Text(Rc::new(s)))
}

/// 便捷函数：将 Vec<u8> 包装为 Rc<RedValue::Bin>
#[inline]
pub fn rv_bin(b: Vec<u8>) -> Rc<RedValue> {
    Rc::new(RedValue::Bin(Rc::new(b)))
}

/// 便捷函数：将 Vec<Rc<RedValue>> 包装为 Rc<RedValue::Array>
#[inline]
pub fn rv_array(a: Vec<Rc<RedValue>>) -> Rc<RedValue> {
    Rc::new(RedValue::Array(a))
}

/// 便捷函数：将有序映射包装为 Rc<RedValue::Object>
#[inline]
pub fn rv_object(o: BTreeMap<String, Rc<RedValue>>) -> Rc<RedValue> {
    Rc::new(RedValue::Object(o))
}

/// 便捷函数：空文本 Rc<RedValue::Text("")>
#[inline]
pub fn rv_empty() -> Rc<RedValue> {
    Rc::new(RedValue::Text(Rc::new(String::new())))
}

lazy_static! {
    static ref OOP_MAP:HashMap<String,i32> = {
        let mut oop_map:HashMap<String,i32> = HashMap::new();    
        oop_map.insert("||".to_owned(), 1);
        oop_map.insert("&&".to_owned(), 2);
        oop_map.insert("==".to_owned(), 3);
        oop_map.insert("<".to_owned(), 3);
        oop_map.insert(">".to_owned(), 3);
        oop_map.insert(">=".to_owned(), 3);
        oop_map.insert("<=".to_owned(), 3);
        oop_map.insert("!=".to_owned(), 3);
        oop_map.insert("+".to_owned(), 5);
        oop_map.insert("-".to_owned(), 5);
        oop_map.insert("*".to_owned(), 6);
        oop_map.insert("/".to_owned(), 6);
        oop_map.insert("//".to_owned(), 6);
        oop_map.insert("%".to_owned(), 6);
        oop_map.insert("--".to_owned(), 7);
        oop_map.insert("!".to_owned(), 7);
        oop_map.insert("^".to_owned(), 8);
        oop_map
    };
    
}


fn set_const_val(pkg_name:&str,val_name:&str,val:RedValueData) -> Result<(), Box<dyn std::error::Error>> {
    let mut g_map = G_CONST_MAP.write()?;
    let val_map = g_map.get_mut(pkg_name);
    if val_map.is_none() {
        let mut mp = HashMap::new();
        mp.insert(val_name.to_owned(), val);
        g_map.insert(pkg_name.to_owned(), mp);
    }else {
        val_map.unwrap().insert(val_name.to_owned(), val);
    }
    Ok(())
}

fn get_const_val(pkg_name:&str,val_name:&str) -> Result<Option<RedValueData>, Box<dyn std::error::Error>> {
    match G_CONST_MAP.read()?.get(pkg_name) {
        Some(var_map) => 
            match var_map.get(val_name) {
                Some(val) => Ok(Some(val.clone())),
                None => Ok(None)
            }
        None => Ok(None)
    }
}

fn clear_temp_const_val() -> Result<(), Box<dyn std::error::Error>> {
    let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
    let mut g_map = G_TEMP_CONST_MAP.write()?;
    let mut to_rm = vec![];
    for (pkg_name,mp) in &*g_map {
        for (k,(_v,val1)) in &*mp {
            if *val1 < tm {
                to_rm.push((pkg_name.to_owned(),k.to_owned()));
            }
        }
    }
    for (pkg_name,key) in &to_rm {
        let vv = g_map.get_mut(pkg_name).unwrap();
        vv.remove(key);
    }
    Ok(())
}

fn set_temp_const_val(pkg_name:&str,val_name:&str,val:RedValueData,expire_time:u128) -> Result<(), Box<dyn std::error::Error>> {
    clear_temp_const_val()?; // 清除过期的key
    let mut g_map = G_TEMP_CONST_MAP.write()?;
    let val_map = g_map.get_mut(pkg_name);
    if val_map.is_none() {
        let mut mp = HashMap::new();
        mp.insert(val_name.to_owned(), (val,expire_time));
        g_map.insert(pkg_name.to_owned(), mp);
    }else {
        val_map.unwrap().insert(val_name.to_owned(), (val,expire_time));
    }
    Ok(())
}

fn get_temp_const_val(pkg_name:&str,val_name:&str) -> Result<Option<RedValueData>, Box<dyn std::error::Error>> {
    match G_TEMP_CONST_MAP.read()?.get(pkg_name) {
        Some(var_map) => 
            match var_map.get(val_name) {
                Some(val) => {
                    let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
                    if val.1 < tm {
                        Ok(None)
                    }else {
                        Ok(Some(val.0.clone()))
                    }
                }
                None => Ok(None)
            }
        None => Ok(None)
    }
}



pub struct RedLang {
    var_vec: Vec<HashMap<String,  Rc<RefCell<Rc<RedValue>>>>>, //变量栈（值为 Rc<RedValue>，支持 CoW）
    xh_vec: Vec<[bool; 2]>,                // 循环控制栈
    params_vec: Vec<Vec<Rc<RedValue>>>,    // 函数参数栈
    fun_ret_vec: Vec<(bool,usize)>,                // 记录函数是否返回,循环深度
    pub exmap:Rc<RefCell<HashMap<String, RedValueData>>>, // 用于记录平台相关数据
    xuhao: HashMap<String, usize>,
    pub pkg_name:String,
    pub script_name:String,
    pub lock_vec:HashSet<String>,
    pub req_tx:Option<tokio::sync::mpsc::Sender<bool>>,
    pub req_rx:Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    pub can_wrong:bool,
    stack:VecDeque<Rc<RedValue>>,
    scriptcallstackdeep:Rc::<RefCell<usize>>, // 记录脚本调用栈的深度
}

#[derive(Debug, Clone)]
struct MyStrError {
    err_str: String
}

impl fmt::Display for MyStrError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}",self.err_str)
    }
}

impl MyStrError {
    fn new(err_str: String) -> MyStrError {
        return MyStrError {
            err_str:err_str
        }
    }
}

impl error::Error for MyStrError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

pub fn get_random() -> Result<usize, getrandom::Error> {
    let mut rand_buf = [0u8; std::mem::size_of::<usize>()];
    getrandom::fill(&mut rand_buf)?;
    let mut num = 0usize;
    for i in 0..std::mem::size_of::<usize>() {
        num = (num << 8) + (rand_buf[i] as usize);
    }
    Ok(num)
}


fn get_core_cmd(cmd:&str,pkg_name:&str) -> Option<fn(&mut RedLang, &[astparser::Ast]) -> Result<Option<Rc<RedValue>>, Box<dyn std::error::Error>>> {
    let mut rfun;
    let cmd_t = cmd.to_uppercase();
    let r = crate::G_CMD_FUN_MAP.read().unwrap();

    // 先查看包对应的命令
    let cmd_tt = format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc{cmd_t}");
    rfun = match r.get(&cmd_tt) {
        Some(fun) => Some(fun.clone()),
        None => None,
    };

    // 再查看内置命令
    if rfun.is_none() {
        rfun = match r.get(&cmd_t) {
            Some(fun) => Some(fun.clone()),
            None => None,
        };
    }
    rfun
}



pub fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[astparser::Ast]) -> Result<Option<Rc<RedValue>>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string().to_uppercase();
            let k_t = crate::mytool::str_to_ft(&k);
            if k == k_t {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
            }else {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
                if w.contains_key(&k_t) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k_t)).unwrap();
                }
                w.insert(k_t, fun);
            }
        }
    }

pub fn init_core_fun_map() {
    add_fun(vec!["换行"],|_self_t,_params|{
        return Ok(Some(rv_text(String::from("\n"))));
    });
    add_fun(vec!["回车"],|_self_t,_params|{
        return Ok(Some(rv_text(String::from("\r"))));
    });
    add_fun(vec!["空格"],|_self_t,_params|{
        return Ok(Some(rv_text(String::from(" "))));
    });
    add_fun(vec!["隐藏"],|self_t,params|{
        let out = self_t.get_param(params, 0)?;
        self_t.set_coremap_value("隐藏", out);
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["传递"],|self_t,_params|{
        return Ok(Some(self_t.get_coremap_value("隐藏")));
    });
    add_fun(vec!["入栈"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        self_t.stack.push_back(text);
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["出栈"],|self_t,_params|{
        let ele_opt = self_t.stack.pop_back();
        if let Some(ele) = ele_opt {
            return Ok(Some(ele));
        } else {
            return Ok(Some(rv_empty()));
        }
    });
    add_fun(vec!["栈长度"],|self_t,_params|{
        let len = self_t.stack.len().to_string();
        return Ok(Some(rv_text(len)));
    });
    add_fun(vec!["栈顶"],|self_t,params|{
        let index = self_t.get_param_text_rc(params, 0)?;
        if &*index == "" {
            let ele_opt = self_t.stack.back();
            if let Some(ele) = ele_opt {
                return Ok(Some(ele.clone()));
            } else {
                return Ok(Some(rv_empty()));
            }
        }
        let index_num = index.parse::<usize>()?;
        let stack = &self_t.stack;
        let len = stack.len();
        if len >= 1 + index_num {
            let ele_opt = stack.get(len - 1 - index_num);
            if let Some(ele) = ele_opt {
                return Ok(Some(ele.clone()));
            } else {
                return Ok(Some(rv_empty()));
            }
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["定义变量"],|self_t,params|{
        let k = self_t.get_param_text_rc(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        let var_vec_len = self_t.var_vec.len();
        let mp = &mut self_t.var_vec[var_vec_len - 1];
        mp.insert(k.to_string(), Rc::new(RefCell::new(v)));
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["变量"],|self_t,params|{
        let k = self_t.get_param_text_rc(params, 0)?;
        let var_ref = self_t.get_var_ref(&k);
        if let Some(v) = var_ref {
            return Ok(Some((*v).borrow().clone()));
        }else {
            return Ok(Some(rv_empty()));
        }
    });
    add_fun(vec!["屏蔽"],|self_t,params|{
        let _k = self_t.get_param(params, 0)?;
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["赋值变量"],|self_t,params|{
        let k = self_t.get_param_text_rc(params, 0)?;
        let var_vec_len = self_t.var_vec.len();
        let mut is_set = false;
        let rv = self_t.get_param(params, 1)?;
        for i in 0..var_vec_len {
            let mp = &mut self_t.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get_mut(&k.to_string());
            if let Some(val) = v_opt {
                *(**val).borrow_mut() = rv.clone();
                is_set = true;
                break;
            }
        }
        if is_set == false {
            let var_vec_len = self_t.var_vec.len();
            let mp = &mut self_t.var_vec[var_vec_len - 1];
            mp.insert(k.to_string(), Rc::new(RefCell::new(rv)));
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["判断","判等"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        let k2 = self_t.get_param(params, 1)?;
        if *k1 != *k2 {
            return Ok(Some(self_t.get_param(params, 2)?));
        } else {
            return Ok(Some(self_t.get_param(params, 3)?));
        }
    });
    add_fun(vec!["判真"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        if !k1.is_true() {
            return Ok(Some(self_t.get_param(params, 1)?));
        }else {
            return Ok(Some(self_t.get_param(params, 2)?));
        }
    });
    add_fun(vec!["判空"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        let len = self_t.get_len(&data)?;
        if len == 0 {
            return Ok(Some(self_t.get_param(params, 1)?));
        }else{
            return Ok(Some(data));
        }
    });
    add_fun(vec!["循环"],|self_t,params|{
        let k1_rv = self_t.get_param(params, 0)?;
        let mut ret_rv: Rc<RedValue> = rv_empty();
        match &*k1_rv {
            RedValue::Text(s) => {
                let tms = s.parse::<usize>()?;
                self_t.xh_vec.push([false, false]);
                let xh_len = self_t.xh_vec.len();
                for i in 0..tms {
                    self_t.xh_vec[xh_len - 1][0] = false;
                    let v = self_t.get_param(params, 1)?;
                    self_t.connect_rv_with_context(
                        &mut ret_rv,
                        &v,
                        &format!("拼接`循环`第{}次结果失败", i + 1),
                    )?;
                    if self_t.xh_vec[xh_len - 1][1] == true {
                        break;
                    }
                }
                self_t.xh_vec.pop();
            }
            RedValue::Array(arr) => {
                let fun_ast = params.get(1).ok_or("数组循环中参数函数获取失败")?.clone();
                let arr_clone = arr.clone(); // clone Vec<Rc<RedValue>> (仅引用计数)
                let tms = arr_clone.len();
                self_t.xh_vec.push([false, false]);
                let loop_val_var = format!("__RED_LOOP_VAL_{}", get_random().unwrap_or(0));
                let mut fun_params = vec![
                    fun_ast,
                    astparser::str_to_ast(String::new()),
                    astparser::parse_to_ast(&format!("【变量@{}】", loop_val_var)).map_err(|e| RedLang::make_err(&e))?,
                ];
                let xh_len = self_t.xh_vec.len();
                for i in 0..tms {
                    self_t.xh_vec[xh_len - 1][0] = false;
                    fun_params[1] = astparser::str_to_ast(i.to_string());
                    {
                        let var_vec_len = self_t.var_vec.len();
                        let mp = &mut self_t.var_vec[var_vec_len - 1];
                        mp.insert(loop_val_var.clone(), Rc::new(RefCell::new(arr_clone[i].clone())));
                    }
                    let v = self_t.call_fun(&fun_params)?;
                    self_t.connect_rv_with_context(
                        &mut ret_rv,
                        &v,
                        &format!("拼接`数组循环`第{}次结果失败", i + 1),
                    )?;
                    if self_t.xh_vec[xh_len - 1][1] == true {
                        break;
                    }
                }
                {
                    let var_vec_len = self_t.var_vec.len();
                    let mp = &mut self_t.var_vec[var_vec_len - 1];
                    mp.remove(&loop_val_var);
                }
                self_t.xh_vec.pop();
            }
            RedValue::Object(obj) => {
                let fun_ast = params.get(1).ok_or("对象循环中参数函数获取失败")?.clone();
                let obj_clone = obj.clone(); // clone BTreeMap（仅引用计数）
                self_t.xh_vec.push([false, false]);
                let loop_val_var = format!("__RED_LOOP_VAL_{}", get_random().unwrap_or(0));
                let mut fun_params = vec![
                    fun_ast,
                    astparser::str_to_ast(String::new()),
                    astparser::parse_to_ast(&format!("【变量@{}】", loop_val_var)).map_err(|e| RedLang::make_err(&e))?,
                ];
                let xh_len = self_t.xh_vec.len();
                for (k,v) in &obj_clone {
                    self_t.xh_vec[xh_len - 1][0] = false;
                    fun_params[1] = astparser::str_to_ast(k.clone());
                    {
                        let var_vec_len = self_t.var_vec.len();
                        let mp = &mut self_t.var_vec[var_vec_len - 1];
                        mp.insert(loop_val_var.clone(), Rc::new(RefCell::new(v.clone())));
                    }
                    let v = self_t.call_fun(&fun_params)?;
                    self_t.connect_rv_with_context(
                        &mut ret_rv,
                        &v,
                        &format!("拼接`对象循环`键`{}`的结果失败", k),
                    )?;
                    if self_t.xh_vec[xh_len - 1][1] == true {
                        break;
                    }
                }
                {
                    let var_vec_len = self_t.var_vec.len();
                    let mp = &mut self_t.var_vec[var_vec_len - 1];
                    mp.remove(&loop_val_var);
                }
                self_t.xh_vec.pop();
            }
            _ => {}
        }
        return Ok(Some(ret_rv));
    });
    add_fun(vec!["判循"],|self_t,params|{
        let mut ret_rv: Rc<RedValue> = rv_empty();
        self_t.xh_vec.push([false, false]);
        let xh_len = self_t.xh_vec.len();
        loop {
            self_t.xh_vec[xh_len - 1][0] = false;
            if !self_t.get_param(params, 0)?.is_true() {
                break;
            }
            let v = self_t.get_param(params, 1)?;
            self_t.connect_rv_with_context(
                &mut ret_rv,
                &v,
                "拼接`判循`结果失败",
            )?;
            if self_t.xh_vec[xh_len - 1][1] == true {
                break;
            }
        }
        self_t.xh_vec.pop();
        return Ok(Some(ret_rv));
    });
    add_fun(vec!["跳出"],|self_t,_params|{
        let xh_vec_len = self_t.xh_vec.len();
        if xh_vec_len != 0 {
            self_t.xh_vec[xh_vec_len - 1][1] = true;
        } else {
            return Err(RedLang::make_err("不在循环中，无法使用`跳出`命令"));
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["继续"],|self_t,_params|{
        let xh_vec_len = self_t.xh_vec.len();
        if xh_vec_len == 0 {
            return Err(RedLang::make_err("不在循环中，无法使用`继续`命令"));
        } else {
            self_t.xh_vec[xh_vec_len - 1][0] = true;
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["函数定义"],|self_t,params|{
        let fun = if let Some(func) = params.get(0) {
            self_t.parse_fun_ast(func,false)?
        } else {
            String::new()
        };
        let ast = astparser::parse_to_ast(&fun).map_err(|e| RedLang::make_err(&e))?;
        return Ok(Some(Rc::new(RedValue::Fun(ast))));
    });
    add_fun(vec!["定义命令"],|self_t,params|{
        let func_name = self_t.get_param_text_rc(params, 0)?;
        let fun = if let Some(func) = params.get(1) {
            self_t.parse_fun_ast(func,false)?
        } else {
            String::new()
        };
        let mut w = crate::G_CMD_MAP.write()?;
        match w.get_mut(&self_t.pkg_name){
            Some(r) => {
                r.insert(func_name.to_string(), fun);
            },
            None => {
                let mut r = HashMap::new();
                r.insert(func_name.to_string(), fun);
                w.insert(self_t.pkg_name.clone(), r);
            },
        };
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["定义二类命令"],|self_t,params|{
        let func_name = self_t.get_param_text_rc(params, 0)?;
        let fun_body = if let Some(func) = params.get(1) {
            self_t.parse_fun_ast(func,false)?
        } else {
            String::new()
        };
        let fun = format!("1FC0F025-BFE7-63A4-CA66-FC3FD8A55B7B{}", fun_body);
        let mut w = crate::G_CMD_MAP.write()?;
        match w.get_mut(&self_t.pkg_name){
            Some(r) => {
                r.insert(func_name.to_string(), fun);
            },
            None => {
                let mut r = HashMap::new();
                r.insert(func_name.to_string(), fun);
                w.insert(self_t.pkg_name.clone(), r);
            },
        };
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["函数调用","调用函数"],|self_t,params|{
        let ret = self_t.call_fun(params)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["参数"],|self_t,params|{
        let k1 = self_t.get_param_text_rc(params, 0)?;
        let tms_t = k1.parse::<usize>()?;
        if tms_t == 0 {
            return Ok(Some(rv_empty()));
        }
        let tms = tms_t - 1;
        let params_vec_len = self_t.params_vec.len();
        let ret_val = self_t.params_vec[params_vec_len - 1]
            .get(tms)
            .cloned()
            .unwrap_or_else(rv_empty);
        return Ok(Some(ret_val));
    });
    add_fun(vec!["二类参数"],|self_t,params|{
        let k1 = self_t.get_param_text_rc(params, 0)?;
        let tms = k1.parse::<usize>()? - 1;
        let params_vec_len = self_t.params_vec.len();
        let ret_val = self_t.params_vec[params_vec_len - 1]
            .get(tms)
            .cloned()
            .unwrap_or_else(rv_empty);
        let ret_str = ret_val.expect_text_value()?;
        return Ok(Some(self_t.parse(&ret_str)?));
    });
    add_fun(vec!["参数个数"],|self_t,_params|{
        let params_vec_len = self_t.params_vec.len();
        let ret_str = self_t.params_vec[params_vec_len - 1].len().to_string();
        return Ok(Some(rv_text(ret_str)));
    });
    add_fun(vec!["返回"],|self_t,_params|{
        let fun_ret_vec_len = self_t.fun_ret_vec.len();
        self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["计算"],|self_t,params|{
        let k1 = self_t.get_param_text_rc(params, 0)?;
        let k1 = k1.replace("小恶魔", "0").replace("恶魔妹妹", "0").replace("恶魔", "0");
        fn cala(expre:&str) -> Result<String, Box<dyn std::error::Error>> {
            let expres_t = expre.chars().collect::<Vec<char>>();
            let mut expres:Vec<char> = vec![];
            // 去除空白
            for it in expres_t {
                if it.is_whitespace() {
                    continue;
                } else if it == '（' {
                    expres.push('(');
                } else if it == '）' {
                    expres.push(')');
                } else {
                    expres.push(it);
                }
            }
            fn is_num_char(s:&char) -> bool {
                let num_vec = ['0','1','2','3','4','5','6','7','8','9','.'];
                for ch in num_vec {
                    if &ch == s {
                        return true;
                    }
                }
                return false;
            }
            let expre_len = expres.len();
            let mut token:Vec<String> = vec![];
            let mut temp_str = "".to_owned();
            let mut status = 0; //0 normal 1 num 2 fun
            let mut i = 0usize;
            // 首先要进行分词，分为数字，运算符，括号，逗号
            while i < expre_len {
                let ch = expres[i];
                if status == 0 { //normal
                    if ch == ',' || ch == '+'  || ch == '*' || ch == '^' || ch == '(' || ch == ')' || ch == '%' || ch == '真' || ch == '假' {
                        token.push(ch.to_string());
                    }else if ch == '-' {
                        if i != 0 && (expres[i - 1] == ')' || is_num_char(&expres[i - 1])) {
                            token.push(ch.to_string());
                        } else {
                            token.push("--".to_string());
                        }
                    }else if ch == '/' {
                        let ch1 = expres.get(i+1).ok_or("express error near '/'")?;
                        if ch1 == &'/' {
                            token.push("//".to_string());
                            i += 1;
                        }else{
                            token.push(ch.to_string());
                        }
                    }
                    else if ch == '<' {
                        let ch1 = expres.get(i+1).ok_or("express error near '<'")?;
                        if ch1 == &'=' {
                            token.push("<=".to_string());
                            i += 1;
                        }else{
                            token.push(ch.to_string());
                        }
                    }else if ch == '>' {
                        let ch1 = expres.get(i+1).ok_or("express error near '>'")?;
                        if ch1 == &'=' {
                            token.push(">=".to_string());
                            i += 1;
                        }else{
                            token.push(ch.to_string());
                        }
                    }
                    else if ch == '!' {
                        let ch1 = expres.get(i+1).ok_or("express error near '/'")?;
                        if ch1 == &'=' {
                            token.push("!=".to_string());
                            i += 1;
                        }else{
                            token.push(ch.to_string());
                        }
                    }else if ch == '=' {
                        let ch1 = expres.get(i+1).ok_or("express error near '='")?;
                        if ch1 == &'=' {
                            token.push("==".to_string());
                            i += 1;
                        }else{
                            let k:Option<char> = None;
                            k.ok_or(format!("出现未知字符：`{}`","="))?;
                        }
                    }else if ch == '|' {
                        let ch1 = expres.get(i+1).ok_or("express error near '|'")?;
                        if ch1 == &'|' {
                            token.push("||".to_string());
                            i += 1;
                        }else{
                            let k:Option<char> = None;
                            k.ok_or(format!("出现未知字符：`{}`","|"))?;
                        }
                    }else if ch == '&' {
                        let ch1 = expres.get(i+1).ok_or("express error near '&'")?;
                        if ch1 == &'&' {
                            token.push("&&".to_string());
                            i += 1;
                        }else{
                            let k:Option<char> = None;
                            k.ok_or(format!("出现未知字符：`{}`","&"))?;
                        }
                    }else if is_num_char(&ch) {
                        status = 1;
                        temp_str.push('N');
                        temp_str.push(ch);
                    }else {
                        let k:Option<char> = None;
                        k.ok_or(format!("出现未知字符：`{}`",ch))?;
                    }
                }else if status == 1 { // num
                    if is_num_char(&ch) {
                        temp_str.push(ch);
                    }else {
                        token.push(temp_str.to_owned());
                        temp_str.clear();
                        status = 0;
                        i -= 1
                    }
                }
                i += 1;
            }
            if !temp_str.is_empty() {
                token.push(temp_str.to_owned());
                temp_str.clear();
            }
            // println!("{:?}",token);
            let mut out_vec:Vec<String> = vec![];
            let mut op_stack:Vec<String> = vec![];
            // println!("token:{:?}",token);
            for it in token {
                if it.starts_with("N") || it == "真" || it == "假"{
                    out_vec.push(it);
                }else{
                    if it == "(" {
                        op_stack.push(it);
                    }else if it == ")" {
                        loop {
                            let pop_it = op_stack.pop();
                            if pop_it == None {
                                let k:Option<char> = None;
                                k.ok_or(format!("括号没有成对出现"))?;
                            }
                            let pop_it_t = pop_it.unwrap();
                            if pop_it_t == "(" {
                                break;
                            }
                            out_vec.push(pop_it_t);
        
                        }
                        
                    }else {
                        loop {
                            if op_stack.is_empty() || op_stack[op_stack.len() - 1] == "(" || it == "--" || it == "!" {
                                op_stack.push(it);
                                break;
                            }
                            let pri_it = OOP_MAP.get(&it).ok_or(&format!("未知的运算符:`{}`",it)).unwrap();
                            let up = op_stack[op_stack.len() - 1].clone();
                            let pri_up = OOP_MAP.get(&up).ok_or(&format!("未知的运算符:`{}`",up)).unwrap();
                            if pri_it > pri_up {
                                op_stack.push(it);
                                break;
                            }
                            op_stack.pop();
                            out_vec.push(up);
                        }
                    }
                }
                    
            }
            
            while !op_stack.is_empty() {
                let pop_it = op_stack.pop().unwrap();
                out_vec.push(pop_it);
            }
            // println!("mid express:{:?}",out_vec);
            let mut out_vec2:Vec<String> = vec![];
            for it in out_vec {
                if it.starts_with('N') {
                    out_vec2.push(it.get(1..).unwrap().to_string());
                }if it == "真" || it == "假" {
                    out_vec2.push(it.to_owned());
                }else if it == "^" {
                    let l2 = out_vec2.pop().ok_or("^ err")?;
                    let l1 = out_vec2.pop().ok_or("^ err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1.powf(n2)).to_string());
                }else if it == "!" {
                    let l1 = out_vec2.pop().ok_or("! err")?;
                    if l1 == "真" {
                        out_vec2.push("假".to_string());
                    }else{
                        out_vec2.push("真".to_string());
                    }
                }else if it == "--" {
                    let l1 = out_vec2.pop().ok_or("- err")?;
                    let n1 = l1.parse::<f64>()?;
                    out_vec2.push((-n1).to_string());
                }else if it == "%" {
                    let l2 = out_vec2.pop().ok_or("% err")?;
                    let l1 = out_vec2.pop().ok_or("% err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1 % n2).to_string());
                }else if it == "/" {
                    let l2 = out_vec2.pop().ok_or("/ err")?;
                    let l1 = out_vec2.pop().ok_or("/ err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1 / n2).to_string());
                }else if it == "//" {
                    let l2 = out_vec2.pop().ok_or("// err")?;
                    let l1 = out_vec2.pop().ok_or("// err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push(((n1 / n2) as i64).to_string());
                }else if it == "*" {
                    let l2 = out_vec2.pop().ok_or("* err")?;
                    let l1 = out_vec2.pop().ok_or("* err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1 * n2).to_string());
                }else if it == "+" {
                    let l2 = out_vec2.pop().ok_or("+ err")?;
                    let l1 = out_vec2.pop().ok_or("+ err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1 + n2).to_string());
                }else if it == "-" {
                    let l2 = out_vec2.pop().ok_or("- err")?;
                    let l1 = out_vec2.pop().ok_or("- err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    out_vec2.push((n1 - n2).to_string());
                }else if it == "==" {
                    let l2 = out_vec2.pop().ok_or("== err")?;
                    let l1 = out_vec2.pop().ok_or("== err")?;
                    if l1 == l2{
                        out_vec2.push("真".to_string());
                    }else{
                        if (l1 == "真" && l2 == "假")||(l1 == "假" && l2 == "真"){
                            out_vec2.push("假".to_string());
                        }else{
                            let n1 = l1.parse::<f64>()?;
                            let n2 = l2.parse::<f64>()?;
                            if (n1 - n2).abs() < 0.0000001f64 {
                                out_vec2.push("真".to_string());
                            }else{
                                out_vec2.push("假".to_string());
                            }
                        }
                    }
                }else if it == "!=" {
                    let l2 = out_vec2.pop().ok_or("!= err")?;
                    let l1 = out_vec2.pop().ok_or("!= err")?;
                    if l1 == l2{
                        out_vec2.push("假".to_string());
                    }else{
                        if (l1 == "真" && l2 == "假")||(l1 == "假" && l2 == "真"){
                            out_vec2.push("真".to_string());
                        }else{
                            let n1 = l1.parse::<f64>()?;
                            let n2 = l2.parse::<f64>()?;
                            if (n1 - n2).abs() < 0.0000001f64 {
                                out_vec2.push("假".to_string());
                            }else{
                                out_vec2.push("真".to_string());
                            }
                        }
                    }
                }else if it == ">" {
                    let l2 = out_vec2.pop().ok_or("> err")?;
                    let l1 = out_vec2.pop().ok_or("> err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    if n1 > n2 {
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                    
                }else if it == "<" {
                    let l2 = out_vec2.pop().ok_or("< err")?;
                    let l1 = out_vec2.pop().ok_or("< err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    if n1 < n2 {
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                }else if it == ">=" {
                    let l2 = out_vec2.pop().ok_or(">= err")?;
                    let l1 = out_vec2.pop().ok_or(">= err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    if n1 >= n2 {
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                }else if it == "<=" {
                    let l2 = out_vec2.pop().ok_or("<= err")?;
                    let l1 = out_vec2.pop().ok_or("<= err")?;
                    let n1 = l1.parse::<f64>()?;
                    let n2 = l2.parse::<f64>()?;
                    if n1 <= n2 {
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                }else if it == "||" {
                    let l2 = out_vec2.pop().ok_or("|| err")?;
                    let l1 = out_vec2.pop().ok_or("|| err")?;
                    if l1=="真"  || l2 == "真"{
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                }else if it == "&&" {
                    let l2 = out_vec2.pop().ok_or("&& err")?;
                    let l1 = out_vec2.pop().ok_or("&& err")?;
                    if l1=="真"  && l2 == "真"{
                        out_vec2.push("真".to_owned());
                    }else {
                        out_vec2.push("假".to_owned());
                    }
                }
                
            }
            return Ok(out_vec2.get(0).ok_or("计算失败")?.to_string());
        }
        let ret_str = cala(&k1)?;
        return Ok(Some(rv_text(ret_str)));
    });
    add_fun(vec!["数组"],|self_t,params|{
        let arr_len = params.len();
        let mut temp_ret:Vec<Rc<RedValue>> = vec![];
        for i in 0..arr_len {
            let s = self_t.get_param(params, i)?;
            temp_ret.push(s);
        }
        return Ok(Some(Rc::new(RedValue::Array(temp_ret))));
    });
    add_fun(vec!["对象"],|self_t,params|{
        let params_len = params.len();
        if params_len % 2 != 0 {
            return Err(RedLang::make_err("请保证对象参数为偶数个"));
        }
        let mut temp_ret:BTreeMap<String,Rc<RedValue>> = BTreeMap::new();
        for i in 0..(params_len/2) {
            let k = self_t.get_param_text_rc(params, i*2)?;
            let v = self_t.get_param(params, i*2 + 1)?;
            temp_ret.insert(k.to_string(), v);
        }
        return Ok(Some(Rc::new(RedValue::Object(temp_ret))));
    });
    add_fun(vec!["取长度"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        let len = match &*data {
            RedValue::Array(arr) => arr.len(),
            RedValue::Object(obj) => obj.len(),
            RedValue::Text(s) => s.chars().count(),
            RedValue::Bin(b) => b.len(),
            _ => return Err(RedLang::make_err(&("对应类型不能获取长度:".to_owned()+data.get_type_name()))),
        };
        return Ok(Some(rv_text(len.to_string())));
    });
    add_fun(vec!["闭包"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        return Ok(Some(data));
    });
    add_fun(vec!["转文本"],|self_t,params|{ 
        let data = self_t.get_param(params, 0)?;
        fn obj_to_text(self_t:&mut RedLang,data:&BTreeMap<String, Rc<RedValue>>,params:&[astparser::Ast]) -> Result<String, Box<dyn std::error::Error>>{
            let mut ret_str = String::new();
            ret_str.push('{');
            let mut vec_t:Vec<String>  = vec![];
            for (k,v) in data{
                let mut temp_str = String::new();
                temp_str.push_str(&str_to_text(k)?);
                temp_str.push(':');
                temp_str.push_str(&value_to_text(self_t, v, params)?);
                vec_t.push(temp_str);
            }
            ret_str.push_str(&vec_t.join(","));
            ret_str.push('}');
            Ok(ret_str)
        }
        fn str_to_text(data:&str) -> Result<String, Box<dyn std::error::Error>>{
            let j:serde_json::Value = serde_json::json!(
                data
            );
            return Ok(j.to_string())
        }
        fn arr_to_text(self_t:&mut RedLang,data:&[Rc<RedValue>],params:&[astparser::Ast]) -> Result<String, Box<dyn std::error::Error>>{
            let mut vec_t:Vec<String>  = vec![];
            for v in data {
                vec_t.push(value_to_text(self_t, v, params)?);
            }
            return Ok(format!("[{}]",vec_t.join(",")));
        }

        fn bin_to_text(self_t:&mut RedLang,data:&[u8],params:&[astparser::Ast]) -> Result<String, Box<dyn std::error::Error>>{
            let ret_str:String;
            let code_t = self_t.get_param_text_rc(params, 1)?;
            let code = code_t.to_lowercase();
            if code == "" || code == "utf8" || code == "utf-8" {
                ret_str = String::from_utf8(data.to_vec())?;
            }else if code == "gbk" {
                ret_str = encoding::all::GBK.decode(data, encoding::DecoderTrap::Ignore)?;
            }else{
                return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
            }
            Ok(ret_str)
        }

        fn value_to_text(self_t:&mut RedLang,data:&RedValue,params:&[astparser::Ast]) -> Result<String, Box<dyn std::error::Error>> {
            match data {
                RedValue::Text(s) => str_to_text(s),
                RedValue::Array(arr) => arr_to_text(self_t, arr, params),
                RedValue::Object(obj) => obj_to_text(self_t, obj, params),
                RedValue::Bin(bin) => bin_to_text(self_t, bin, params),
                _ => Err(RedLang::make_err(&("对应类型不能转文本:".to_owned()+data.get_type_name()))),
            }
        }
        let ret_str = value_to_text(self_t, &data, params)?;
        return Ok(Some(rv_text(ret_str)));
    });
    add_fun(vec!["增加元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param_text_rc(params, 0)?;
        let data:Rc<RefCell<Rc<RedValue>>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp = (*data).borrow().get_type_name().to_string();
        let el_len;
        if tp == "对象" {
            el_len = (params.len() -1) / 2;
        }else {
            el_len = params.len() -1;
        }
        //  增加元素 - 直接操作 RedValue，无需序列化/反序列化
        for i in 0..el_len {
            if tp == "数组" {
                let el_rv = self_t.get_param(params, i + 1)?;
                let old_rc = (*data).borrow().clone();
                let mut new_val = (*old_rc).clone();
                if let RedValue::Array(ref mut arr) = new_val {
                    arr.push(el_rv);
                }
                *(*data).borrow_mut() = Rc::new(new_val);
            }else if tp == "对象" {
                let elk = self_t.get_param_text_rc(params, i * 2 + 1)?;
                let elv = self_t.get_param(params, i * 2 + 2)?;
                let old_rc = (*data).borrow().clone();
                let mut new_val = (*old_rc).clone();
                if let RedValue::Object(ref mut obj) = new_val {
                    obj.insert(elk.to_string(), elv);
                }
                *(*data).borrow_mut() = Rc::new(new_val);
            }else if tp == "文本" { 
                let el = self_t.get_param_text_rc(params, i + 1)?;
                let old_rc = (*data).borrow().clone();
                let cur = old_rc.text_or_empty().to_string();
                let new_str = cur + &el;
                *(*data).borrow_mut() = Rc::new(RedValue::Text(Rc::new(new_str)));
            }else if tp == "字节集" {
                let el_rv = self_t.get_param(params, i + 1)?;
                let mut el_bin = match &*el_rv {
                    RedValue::Bin(b) => b.as_ref().clone(),
                    _ => return Err(RedLang::make_err("增加字节集元素时，值不是字节集类型")),
                };
                let old_rc = (*data).borrow().clone();
                let mut new_val = (*old_rc).clone();
                if let RedValue::Bin(ref mut bin) = new_val {
                    Rc::make_mut(bin).append(&mut el_bin);
                }
                *(*data).borrow_mut() = Rc::new(new_val);
            }else{
                return Err(RedLang::make_err(&("对应类型不能增加元素:".to_owned()+&tp)));
            }
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["替换元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param_text_rc(params, 0)?;
        let k_name = self_t.get_param_text_rc(params, 1)?;
        let v_rv = self_t.get_param(params, 2)?;
        let data:Rc<RefCell<Rc<RedValue>>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp = (*data).borrow().get_type_name().to_string();
        if tp == "数组" {
            let index = k_name.parse::<usize>()?;
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Array(ref mut arr) = new_val {
                let el = arr.get_mut(index).ok_or("替换数组元素时越界")?;
                *el = v_rv.clone();
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else if tp == "对象" {
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Object(ref mut obj) = new_val {
                obj.insert(k_name.to_string(), v_rv.clone());
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else if tp == "文本" { 
            let index = k_name.parse::<usize>()?;
            let old_rc = (*data).borrow().clone();
            let cur_str = old_rc.text_or_empty().to_string();
            let mut chs = cur_str.chars().collect::<Vec<char>>();
            let v_str = match &*v_rv {
                RedValue::Text(s) => s.to_string(),
                _ => return Err(RedLang::make_err("替换文本元素时值不是文本类型")),
            };
            let v_chs = v_str.chars().collect::<Vec<char>>();
            if v_chs.len() != 1 {
                return Err(RedLang::make_err("替换文本元素时值的长度不为1"));
            }
            let el = chs.get_mut(index).ok_or("替换文本元素时越界")?;
            *el = v_chs[0];
            *(*data).borrow_mut() = Rc::new(RedValue::Text(Rc::new(chs.iter().collect::<String>())));
        }else if tp == "字节集" {
            let index = k_name.parse::<usize>()?;
            let bt = match &*v_rv {
                RedValue::Bin(b) => b.as_ref().clone(),
                _ => return Err(RedLang::make_err("替换字节集元素时值不是字节集类型")),
            };
            if bt.len() != 1 {
                return Err(RedLang::make_err("替换字节集元素时值的长度不为1"));
            }
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Bin(ref mut bin) = new_val {
                let el = Rc::make_mut(bin).get_mut(index).ok_or("替换字节集元素时越界")?;
                *el = bt[0];
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else{
            return Err(RedLang::make_err(&("对应类型不能替换元素:".to_owned()+&tp)));
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["删除元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param_text_rc(params, 0)?;
        let k_name = self_t.get_param_text_rc(params, 1)?;
        let data:Rc<RefCell<Rc<RedValue>>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp = (*data).borrow().get_type_name().to_string();
        if tp == "数组" {
            if k_name.starts_with("-"){return Ok(Some(rv_empty()));}
            let index = k_name.parse::<usize>()?;
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Array(ref mut arr) = new_val {
                if index < arr.len() {
                    arr.remove(index);
                }
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else if tp == "对象" {
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Object(ref mut obj) = new_val {
                obj.remove(&*k_name);
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else if tp == "文本" { 
            if k_name.starts_with("-"){return Ok(Some(rv_empty()));}
            let index = k_name.parse::<usize>()?;
            let old_rc = (*data).borrow().clone();
            let cur_str = old_rc.text_or_empty().to_string();
            let mut chs = cur_str.chars().collect::<Vec<char>>();
            if index < chs.len() {
                chs.remove(index);
            }
            *(*data).borrow_mut() = Rc::new(RedValue::Text(Rc::new(chs.iter().collect::<String>())));
        }else if tp == "字节集" {
            if k_name.starts_with("-"){return Ok(Some(rv_empty()));}
            let index = k_name.parse::<usize>()?;
            let old_rc = (*data).borrow().clone();
            let mut new_val = (*old_rc).clone();
            if let RedValue::Bin(ref mut bin) = new_val {
                if index < bin.len() {
                    Rc::make_mut(bin).remove(index);
                }
            }
            *(*data).borrow_mut() = Rc::new(new_val);
        }else{
            return Err(RedLang::make_err(&("对应类型不能删除元素:".to_owned()+&tp)));
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["取元素"],|self_t,params|{
        let nums = params.len();
        let mut cur_rv = self_t.get_param(params, 0)?;
        for i in 1..nums {
            let index_str = self_t.get_param_text_rc(params, i)?;
            match &*cur_rv {
                RedValue::Array(arr) => {
                    let index_rst = index_str.parse::<usize>();
                    if index_rst.is_err() {
                        cur_rv = rv_empty();
                        break;
                    }
                    let index = index_rst.unwrap();
                    if let Some(v) = arr.get(index) {
                        cur_rv = v.clone();
                    } else {
                        cur_rv = rv_empty();
                        break;
                    }
                }
                RedValue::Object(obj) => {
                    if let Some(v) = obj.get(&*index_str) {
                        cur_rv = v.clone();
                    } else {
                        cur_rv = rv_empty();
                        break;
                    }
                }
                RedValue::Text(s) => {
                    let index_rst = index_str.parse::<usize>();
                    if index_rst.is_err() {
                        cur_rv = rv_empty();
                        break;
                    }
                    let index = index_rst.unwrap();
                    let v_chs = s.chars().collect::<Vec<char>>();
                    if let Some(v) = v_chs.get(index) {
                        cur_rv = rv_text(v.to_string());
                    } else {
                        cur_rv = rv_empty();
                        break;
                    }
                }
                _ => {
                    return Err(RedLang::make_err(&("对应类型不能取元素:".to_owned()+cur_rv.get_type_name())));
                }
            }
        }
        return Ok(Some(cur_rv));
    });
    add_fun(vec!["取变量元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param_text_rc(params, 0)?;
        let k_name = self_t.get_param_text_rc(params, 1)?;
        
        let data:Rc<RefCell<Rc<RedValue>>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型 - 直接从 RedValue 取元素，无需序列化/反序列化
        let cur_rv = (*data).borrow().clone();
        let ret_rv: Rc<RedValue>;
        match &*cur_rv {
            RedValue::Array(arr) => {
                let index_rst = k_name.parse::<usize>();
                if index_rst.is_err() {
                    ret_rv = rv_empty();
                }else{
                    let index = index_rst.unwrap();
                    ret_rv = match arr.get(index) {
                        Some(s) => s.clone(),
                        None => rv_empty(),
                    };
                }
            }
            RedValue::Object(obj) => {
                ret_rv = match obj.get(&*k_name) {
                    Some(s) => s.clone(),
                    None => rv_empty(),
                };
            }
            RedValue::Text(cur_str) => {
                let index_rst = k_name.parse::<usize>();
                if index_rst.is_err() {
                    ret_rv = rv_empty();
                }else {
                    let index = index_rst.unwrap();
                    let chs = cur_str.chars().collect::<Vec<char>>();
                    ret_rv = match chs.get(index) {
                        Some(s) => rv_text(s.to_string()),
                        None => rv_empty(),
                    };
                }
            }
            RedValue::Bin(bin) => {
                let index_rst = k_name.parse::<usize>();
                if index_rst.is_err() {
                    ret_rv = Rc::new(RedValue::Bin(Rc::new(vec![])));
                }else {
                    let index = index_rst.unwrap();
                    ret_rv = match bin.get(index) {
                        Some(s) => Rc::new(RedValue::Bin(Rc::new(vec![*s]))),
                        None => Rc::new(RedValue::Bin(Rc::new(vec![]))),
                    };
                }
            }
            _ => {
                return Err(RedLang::make_err(&("对应类型不能取元素:".to_owned()+cur_rv.get_type_name())));
            }
        }
        return Ok(Some(ret_rv));
    });
    add_fun(vec!["取对象KEY"],|self_t,params|{
        let param_data = self_t.get_param(params, 0)?;
        match &*param_data {
            RedValue::Object(obj) => {
                let arr: Vec<Rc<RedValue>> = obj.keys().map(|k| rv_text(k.clone())).collect();
                return Ok(Some(Rc::new(RedValue::Array(arr))));
            }
            _ => {
                return Err(RedLang::make_err(&("对应类型不能取对象key:".to_owned()+param_data.get_type_name())));
            }
        }
    });
    add_fun(vec!["取类型"],|self_t,params|{
        let param_data = self_t.get_param(params, 0)?;
        let tp = param_data.get_type_name();
        let ret_str = match tp {
            "文本" => "T",
            "数组" => "A",
            "对象" => "O",
            "字节集" => "B",
            "函数" => "F",
            _ => "T"
        };
        return Ok(Some(rv_text(ret_str.to_string())));
    });
    add_fun(vec!["取随机数"],|self_t,params|{
        let num1 = self_t.get_param_text_rc(params, 0)?.parse::<usize>()?;
        let num2 = self_t.get_param_text_rc(params, 1)?.parse::<usize>()?;
        if num1 > num2 {
            return Err(RedLang::make_err("生成随机数失败,请保证第一个数不大于第二个数，且都为非负数"));
        }
        let rand_num = get_random()?;
        let num = num2 + 1 - num1;
        let ret_num = (rand_num %  num) + num1;
        let ret_str = ret_num.to_string();
        return Ok(Some(rv_text(ret_str)));
    });
    add_fun(vec!["文本替换"],|self_t,params|{
        let text = self_t.get_param_text_rc(params, 0)?;
        let from = self_t.get_param_text_rc(params, 1)?;
        let to = self_t.get_param_text_rc(params, 2)?;
        let ret_str = text.replace(&*from, &to);
        return Ok(Some(rv_text(ret_str)));
    });
    add_fun(vec!["运行脚本"],|self_t,params|{
        let mut rl = RedLang::new();
        rl.exmap = self_t.exmap.clone();
        rl.pkg_name = self_t.pkg_name.clone();
        rl.script_name = self_t.script_name.clone();
        rl.can_wrong = self_t.can_wrong;
        let code = self_t.get_param_text_rc(params, 0)?.to_string();
        let params_len = params.len();
        for i in 1..params_len {
            rl.params_vec[0].push(self_t.get_param(params, i)?);
        }
        let ret = rl.parse(&code)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["反射执行"],|self_t,params|{
        let code = self_t.get_param_text_rc(params, 0)?;
        let ret = self_t.parse(&code)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["崩溃吧"],|self_t,params|{
        None::<i32>.expect(&self_t.get_param_text_rc(params, 0)?);
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["后台运行脚本"],|self_t,params|{
        let exmap = (*self_t.exmap).borrow().clone();
        let code = self_t.get_param_text_rc(params, 0)?.to_string();
        let pkg_name = self_t.pkg_name.clone();
        let script_name = self_t.script_name.clone();
        let can_wrong = self_t.can_wrong;
        let params_len = params.len();
        let mut params_vec: Vec<RedValueData> = vec![];
        for i in 1..params_len {
            let param = self_t.get_param(params, i)?;
            params_vec.push(RedValueData::from_red_value(&param)?);
        }
        thread::spawn(move ||{
            let mut rl = RedLang::new();
            rl.exmap = Rc::new(RefCell::new(exmap));
            rl.pkg_name = pkg_name;
            rl.script_name = script_name;
            rl.can_wrong = can_wrong;
            for item in params_vec {
                match item.into_rc_value() {
                    Ok(v) => rl.params_vec[0].push(v),
                    Err(err) => {
                        cq_add_log_w(&format!("{}",err)).unwrap();
                        rl.params_vec[0].push(rv_empty());
                    }
                }
            }
            if let Err(err) = do_script(&mut rl, &code,"normal",false) {
                cq_add_log_w(&format!("{}",err)).unwrap();
            }
        });
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["选择"],|self_t,params|{
        let select_num_str = self_t.get_param_text_rc(params, 0)?;
        let params_len = params.len();
        if params_len == 0 {
            return Ok(Some(rv_empty()));
        }
        if params_len == 1 {
            return Ok(Some(rv_empty()));
        }
        let select_num;
        if &*select_num_str == "" {
            let rand_num = get_random()?;
            select_num = rand_num % (params_len - 1) + 1;
        }
        else if select_num_str.starts_with("-") {
            let _foo = select_num_str.parse::<i64>()?;
            select_num = 0;
        } 
        else {
            select_num = select_num_str.parse::<usize>()? + 1;
        }
        if select_num == 0 || select_num > params_len {
            return Ok(Some(rv_empty()));
        }else {
            return Ok(Some(self_t.get_param(params, select_num)?));
        }
    });
    add_fun(vec!["当前版本"],|_self_t,_params|{
        return Ok(Some(rv_text(crate::get_version())));
    });
    add_fun(vec!["加锁"],|self_t,params|{
        let lock_name = self_t.get_param_text_rc(params, 0)?;
        loop {
            if self_t.lock_vec.contains(&*lock_name) {
                break;
            }
            {
                let mut k = crate::G_LOCK.lock()?;
                if !k.contains_key(&self_t.pkg_name) {
                    k.insert(self_t.pkg_name.clone(), HashMap::new());
                }
                if !k[&self_t.pkg_name].contains_key(&lock_name.to_string()) {
                    k.get_mut(&self_t.pkg_name).unwrap().insert(lock_name.to_string(), 0);
                    self_t.lock_vec.insert(lock_name.to_string());
                    break;
                }
            }
            let time_struct = core::time::Duration::from_millis(10);
            std::thread::sleep(time_struct);
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["尝试加锁"],|self_t,params|{
        let lock_name = self_t.get_param_text_rc(params, 0)?;
        if self_t.lock_vec.contains(&*lock_name) {
            return Ok(Some(rv_empty()));
        }
        {
            let mut k = crate::G_LOCK.lock()?;
            if !k.contains_key(&self_t.pkg_name) {
                k.insert(self_t.pkg_name.clone(), HashMap::new());
            }
            if !k[&self_t.pkg_name].contains_key(&*lock_name) {
                k.get_mut(&self_t.pkg_name).unwrap().insert(lock_name.to_string(), 0);
                self_t.lock_vec.insert(lock_name.to_string());
                return Ok(Some(rv_empty()));
            }
        }
        let ret = self_t.get_param(params, 1)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["解锁"],|self_t,params|{
        let lock_name = self_t.get_param_text_rc(params, 0)?;
        if !self_t.lock_vec.contains(&*lock_name) {
            return Ok(Some(rv_empty()));
        } else {
            let mut k = crate::G_LOCK.lock()?;
            k.get_mut(&self_t.pkg_name).unwrap().remove(&lock_name.to_string());
            self_t.lock_vec.remove(&lock_name.to_string());
            return Ok(Some(rv_empty()));
        }
    });
    add_fun(vec!["发送信号"],|self_t,params|{
        let sigal_name = self_t.get_param_text_rc(params, 0)?;
        let param = self_t.get_param(params, 1)?;
        let to_send = Arc::new(RedValueData::from_red_value(&param)?);
        let mut lk = G_SINGAL_ARR.write().unwrap();
        for (_,pkg_name,singal_name_t,data) in  &mut *lk {
            if *pkg_name == self_t.pkg_name && *singal_name_t == *sigal_name {
                *data = Some(to_send.clone());
            }
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["等待信号"],|self_t,params|{
        let sigal_name = self_t.get_param_text_rc(params, 0)?;
        let tm = self_t.get_param_text_rc(params, 1)?; 
        let tm = tm.parse::<u64>().unwrap_or(15000);
        let uid = uuid::Uuid::new_v4().to_string();
        {
            let mut lk_vec = G_SINGAL_ARR.write().unwrap();
            lk_vec.push((uid.to_owned(),self_t.pkg_name.to_owned(),sigal_name.to_string(),None));
        }
        let _guard = scopeguard::guard(uid.to_owned(), |uid| {
            let mut lk_vec = G_SINGAL_ARR.write().unwrap();
            let mut index = 0usize;
            for it in &*lk_vec {
                if uid == it.0 {
                    break;
                }
                index += 1;
            }
            if index < lk_vec.len() {
                lk_vec.remove(index);
            }
        });
        let mut tm = tm;
        loop {
            {
                let lk = G_SINGAL_ARR.read().unwrap();
                for (uid_t,_,_,data) in  &*lk {
                    if uid == *uid_t && data.is_some() {
                        let dat = data.clone().unwrap();
                        return Ok(Some((*dat).clone().into_rc_value()?));
                    }
                }
            }
            if pkg_can_run(&self_t.pkg_name,"等待信号") == false {
                return Err("等待信号终止，因用户要求退出".into());
            }
            if tm < 10 {
                break;
            }
            tm -= 10;
            let time_struct = core::time::Duration::from_millis(10);
            std::thread::sleep(time_struct);
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["逻辑选择"],|self_t,params|{
        let loge_arr_rv = self_t.get_param(params, 0)?;
        if let RedValue::Array(arr) = &*loge_arr_rv {
            for (index, it) in arr.iter().enumerate() {
                if it.is_true() {
                    return Ok(Some(self_t.get_param(params, index + 1)?));
                }
            }
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["自增"],|self_t,params|{
        let var_name = self_t.get_param_text_rc(params, 0)?;
        let number_str = self_t.get_param_text_rc(params, 1)?;
        let number;
        if &*number_str == "" {
            number = 1;
        } else {
            number = number_str.parse::<i64>()?;
        }
        let var_vec_len = self_t.var_vec.len();
        for i in 0..var_vec_len {
            let mp = &mut self_t.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get_mut(&*var_name);
            if let Some(val) = v_opt {
                let v_str = (**val).borrow().text_or_empty().to_string();
                let mut v_num = v_str.parse::<i64>()?;
                v_num += number;
                *(**val).borrow_mut() = Rc::new(RedValue::Text(Rc::new(v_num.to_string())));
                break;
            }
        }
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["复制命令"],|self_t,params|{
        let old_cmd = self_t.get_param_text_rc(params, 0)?;
        let new_cmd = self_t.get_param_text_rc(params, 1)?;

        let pkg_name = &self_t.pkg_name;
        
        // 如果旧命令不存在，则什么也不做
        let exret = get_core_cmd(&old_cmd, pkg_name);
        if exret.is_none() {
            return Ok(Some(rv_empty()));
        }
        // 构造新命令
        let fun = exret.unwrap();
        let k = new_cmd.to_uppercase();
        let k_t: String = crate::mytool::str_to_ft(&k);

        // 添加新命令
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        w.insert(format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc{k}"), fun);
        w.insert(format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc{k_t}"), fun);
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["进制转化","进制转换"],|self_t,params|{
        let num_text = self_t.get_param_text_rc(params, 0)?.to_uppercase();
        let from = self_t.get_param_text_rc(params, 1)?.parse::<u32>()?;
        let to = self_t.get_param_text_rc(params, 2)?.parse::<u32>()?;
        // 你好，这是Bing。我可以尝试用Rust语言写一个函数，实现任意进制的转化。请看下面的代码：
        // 定义一个函数，接受一个十进制数和一个目标进制，返回一个字符串表示转换后的结果
        fn convert_base(num: u32, base: u32) -> String {
            // 定义一个字符数组，用于表示不同的数字
            let digits = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
                        'A', 'B', 'C', 'D', 'E', 'F'];
            // 定义一个空字符串，用于存储结果
            let mut result = String::new();
            // 定义一个临时变量，用于存储余数
            let mut remainder;
            // 定义一个副本，用于循环除法
            let mut quotient = num;
            // 如果目标进制不在2到16之间，返回错误信息
            if base < 2 || base > 16 {
                return "Invalid base".to_string();
            }
            // 如果输入的数是0，直接返回0
            if num == 0 {
                return "0".to_string();
            }
            // 循环进行除法，直到商为0
            while quotient > 0 {
                // 计算余数
                remainder = quotient % base;
                // 将余数对应的字符插入到结果字符串的开头
                result.insert(0, digits[remainder as usize]);
                // 计算商
                quotient = quotient / base;
            }
            // 返回结果字符串
            result
        }
        // 我可以用Rust语言写一个函数，实现任意进制转为10进制的功能。请看下面的代码：
        // 定义一个函数，接受一个字符串和一个基数，返回一个十进制数
        fn convert_to_base10(num: &str, base: u32) -> u32 {
            // 定义一个字符数组，用于表示不同的数字
            let digits = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
                        'A', 'B', 'C', 'D', 'E', 'F'];
            // 定义一个哈希表，用于存储字符和数字的对应关系
            let mut map = std::collections::HashMap::new();
            for i in 0..16 {
                map.insert(digits[i], i as u32);
            }
            // 定义一个变量，用于存储结果
            let mut result = 0;
            // 定义一个变量，用于存储当前的次方
            let mut power = 0;
            // 如果基数不在2到16之间，返回0
            if base < 2 || base > 16 {
                return 0;
            }
            // 从最低位开始遍历字符串
            for c in num.chars().rev() {
                // 如果字符不在哈希表中，返回0
                if let Some(d) = map.get(&c) {
                    // 将字符对应的数字乘以基数的相应次方，累加到结果中
                    result += d * base.pow(power);
                    // 增加次方
                    power += 1;
                } else {
                    return 0;
                }
            }
            // 返回结果
            result
        }
        let ret = convert_base(convert_to_base10(&num_text,from),to);
        if ret == "Invalid base" {
            return Err(RedLang::make_err(&ret));
        }
        return Ok(Some(rv_text(ret)));
    });
    add_fun(vec!["当前脚本"],|self_t,_params|{
        Ok(Some(rv_text(self_t.script_name.to_owned())))
    });
}

impl RedLang {
    pub fn get_exmap(
        &self,
        key: &str,
    ) -> Arc<String> {
        let v = (*self.exmap).borrow();
        let ret = v.get(key);
        if let Some(v) = ret{
            if let RedValueData::Text(text) = v {
                return Arc::new(text.clone());
            }
        }
        return Arc::new("".to_string());
    }
    pub fn get_exmap_value(
        &self,
        key: &str,
    ) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        let v = (*self.exmap).borrow();
        if let Some(value) = v.get(key) {
            return value.clone().into_rc_value();
        }
        Ok(rv_empty())
    }
    pub fn set_exmap(
        &mut self,
        key: &str,
        val: &str,
    ) {
        let k = &*self.exmap;
        k.borrow_mut().insert(key.to_owned(), RedValueData::Text(val.to_string()));
    }
    pub fn set_exmap_value(
        &mut self,
        key: &str,
        val: Rc<RedValue>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let k = &*self.exmap;
        k.borrow_mut().insert(key.to_owned(), RedValueData::from_red_value(&val)?);
        Ok(())
    }
    pub fn get_coremap(
        &mut self,
        key: &str,
    ) -> String {

        let k = format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key);
        let var_ref = self.get_var_ref(&k);
        if let Some(v) = var_ref {
            return (*v).borrow().expect_text_value().unwrap_or_default();
        }else {
            return "".to_string();
        }
    }
    pub fn get_coremap_value(
        &mut self,
        key: &str,
    ) -> Rc<RedValue> {
        let k = format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key);
        let var_ref = self.get_var_ref(&k);
        if let Some(v) = var_ref {
            return (*v).borrow().clone();
        }
        rv_empty()
    }
    pub fn red_value_to_text_map(
        value: &RedValue,
        key: &str,
    ) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
        match value {
            RedValue::Object(obj) => {
                let mut out = BTreeMap::new();
                for (k, v) in obj {
                    out.insert(k.clone(), v.expect_text_value()?);
                }
                Ok(out)
            }
            RedValue::Text(s) if s.is_empty() => Ok(BTreeMap::new()),
            other => Err(RedLang::make_err(&format!(
                "{key}必须是对象，当前类型:{}",
                other.get_type_name()
            ))),
        }
    }
    pub fn get_coremap_text_map(
        &mut self,
        key: &str,
    ) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
        let value = self.get_coremap_value(key);
        Self::red_value_to_text_map(&value, key)
    }
    pub fn set_coremap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let var_vec_len = self.var_vec.len();
        let mp = &mut self.var_vec[var_vec_len - 1];
        
        if val == "" {
            mp.remove(&format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key));
        } else {
            mp.insert(format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key), Rc::new(RefCell::new(rv_text(val.to_string()))));
        }
        Ok(())
    }
    pub fn set_coremap_value(
        &mut self,
        key: &str,
        val: Rc<RedValue>,
    ) {
        let var_vec_len = self.var_vec.len();
        let mp = &mut self.var_vec[var_vec_len - 1];
        mp.insert(format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key), Rc::new(RefCell::new(val)));
    }
    pub fn get_gobalmap(
        &mut self,
        key: &str,
    ) -> String {

        let k = format!("{}8bb64e93-143d-4209-8fad-3e3a6a43f191", key);
        let var_ref = self.var_vec[0].get(&k);
        if let Some(v) = var_ref {
            return (*v).borrow().expect_text_value().unwrap_or_default();
        }else {
            return "".to_string();
        }
    }
    pub fn set_gobalmap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mp = &mut self.var_vec[0];
        
        if val == "" {
            mp.remove(&format!("{}8bb64e93-143d-4209-8fad-3e3a6a43f191", key));
        } else {
            mp.insert(format!("{}8bb64e93-143d-4209-8fad-3e3a6a43f191", key), Rc::new(RefCell::new(rv_text(val.to_string()))));
        }
        Ok(())
    }
    fn get_len(&self,data:&RedValue) -> Result<usize, Box<dyn std::error::Error>> {
        let ret = match data {
            RedValue::Array(arr) => arr.len(),
            RedValue::Object(obj) => obj.len(),
            RedValue::Text(s) => s.chars().count(),
            RedValue::Bin(b) => b.len(),
            _ => return Err(RedLang::make_err(&("对应类型不能获取长度:".to_owned()+data.get_type_name()))),
        };
        return Ok(ret);
    }
    fn call_fun(&mut self,params: &[astparser::Ast]) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        // 获得函数
        let func_rv = self.get_param(params, 0)?;
        let load_fun_from_const = |name: &str| -> Result<astparser::Ast, Box<dyn std::error::Error>> {
            let err = "无法在常量中找到对应函数";
            if let Some(value) = get_const_val(&self.pkg_name, name)? {
                match &*value.into_rc_value()? {
                    RedValue::Fun(ast) => Ok(ast.clone()),
                    other => Err(RedLang::make_err(&format!(
                        "函数调用命令不能对{}类型进行操作",
                        other.get_type_name()
                    ))),
                }
            } else {
                Err(RedLang::make_err(err))
            }
        };
        let func_ast = match &*func_rv {
            RedValue::Fun(ast) => ast.clone(),
            RedValue::Text(name) => load_fun_from_const(name)?,
            _ => {
                return Err(RedLang::make_err(&format!(
                    "函数调用命令不能对{}类型进行操作",
                    func_rv.get_type_name()
                )));
            }
        };

        // 获得函数参数（Ast 直接 eval，无论是否来自循环都走 eval_ast）
        let fun_params = &params[1..];
        let mut fun_params_t: Vec<Rc<RedValue>> = vec![];
        for param_ast in fun_params {
            let p = self.eval_ast(param_ast)?;
            fun_params_t.push(p);
        }

        // 用于处理参数中的返回
        let fun_ret_vec_len = self.fun_ret_vec.len();
        if self.fun_ret_vec[fun_ret_vec_len - 1].0 == true {
            // 如果参数中已经返回，就收集返回值，然后结束函数调用
            let mut to_ret = String::new();
            for i in fun_params_t {
                to_ret += &i.expect_text_value()?;
            }
            return Ok(rv_text(to_ret));
        }

        // 修改参数栈
        self.params_vec.push(fun_params_t);

        // 修改变量栈
        self.var_vec.push(std::collections::HashMap::new());

        self.fun_ret_vec.push((false,self.xh_vec.len()));

        // 调用函数
        let ret_str = self.eval_ast_with_stack_guard(&func_ast)?;

        // 变量栈和参数栈退栈
        self.var_vec.pop();
        self.params_vec.pop();
        self.fun_ret_vec.pop();

        return Ok(ret_str);
    }
    fn do_cmd_fun(
        &mut self,
        cmd: &str,
        params: &[astparser::Ast],
    ) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        let mut ret_rv: Option<Rc<RedValue>> = None;

        // 执行自定义命令
        {
            // 获得命令内容
            let fun;
            {
                let r = crate::G_CMD_MAP.read()?;
                if let Some(m) = r.get(&self.pkg_name) {
                    fun = match m.get(cmd){
                        Some(f) => Some(f.clone()),
                        None =>  None
                    }
                }else {
                    fun = None;
                }
            }
            match fun {
                Some(fun) => {
                    // 获得命令
                    let func = fun;

                    // 判断是否为二类命令
                    let is_cmd2 = func.starts_with("1FC0F025-BFE7-63A4-CA66-FC3FD8A55B7B");

                    // 获得命令参数
                    let mut fun_params_t: Vec<Rc<RedValue>> = vec![];
                    for param_ast in params {
                        if is_cmd2 {
                            let kk = self.parse_fun_ast(param_ast,true)?;
                            fun_params_t.push(rv_text(kk)); // 二类命令不进行参数解析
                        } else {
                            let p = self.eval_ast(param_ast)?;
                            fun_params_t.push(p);
                        }
                    }

                    // 修改参数栈
                    self.params_vec.push(fun_params_t);

                    // 调用命令
                    let r;
                    if is_cmd2 {
                        r = self.parse(&func[36..])?;
                    }else {
                        r = self.parse(&func)?;
                    }
                   
                    // 参数栈退栈
                    self.params_vec.pop();

                    ret_rv = Some(r);
                }
                _ => {}
            }
        }
        if let Some(rv) = ret_rv {
            return Ok(rv);
        }

        // 执行核心命令与拓展命令
        let rfun = get_core_cmd(cmd,&self.pkg_name);
        
        let exret = match rfun {
            Some(fun) => fun(self,params)?,
            None => None,
        };

        if let Some(v) = exret{
            return Ok(v);
        }

        return Err(RedLang::make_err(&format!("未知的命令:{}", cmd)));
    }

    pub fn parse_bin_to_img_raw(raw: Vec<u8>) -> Result<(image::ImageFormat,ImageBuffer<Rgba<u8>, Vec<u8>>), Box<dyn std::error::Error>> {
        use image::ImageReader;
        let img_t = ImageReader::new(std::io::Cursor::new(raw)).with_guessed_format()?;
        let img_fmt = img_t.format().ok_or("不能识别的图片格式")?;
        let img = img_t.decode()?.to_rgba8();
        Ok((img_fmt, img))
    }

}

impl Drop for RedLang {
    fn drop(&mut self) {
        let mut lk = G_LOCK.lock().unwrap();
        if lk.contains_key(&self.pkg_name) { 
            for lock_name in &self.lock_vec {
                lk.get_mut(&self.pkg_name).unwrap().remove(lock_name);
            }
        }
    }
}

impl RedLang {
    pub fn new() -> RedLang {

        // 第一个元素用于保持全局变量
        let v: Vec<HashMap<String, Rc<RefCell<Rc<RedValue>>>>> = vec![HashMap::new()];

        // 第一个元素用于全局参数
        let v2: Vec<Vec<Rc<RedValue>>> = vec![vec![]];

        let v3 = vec![(false,0)];

        // 用于循环控制
        RedLang {
            var_vec: v,
            xh_vec: vec![],
            params_vec: v2,
            fun_ret_vec: v3,
            exmap: Rc::new(RefCell::new(HashMap::new())),
            xuhao:HashMap::new(),
            pkg_name:String::new(),
            script_name:String::new(),
            lock_vec:HashSet::new(),
            req_tx:None,
            req_rx:None,
            can_wrong:true,
            stack:VecDeque::new(),
            scriptcallstackdeep: Rc::new(RefCell::new(0)),
        }
    }

    pub fn make_err(err_str: &str) -> Box<dyn std::error::Error> {
        Box::new(MyStrError::new(err_str.to_owned()))
    }
    fn make_err_push(&self, e:Box<dyn std::error::Error> ,err_str: &str) -> Box<dyn std::error::Error> {
        let err = None.unwrap_or(format!("{}:\n{}",err_str,e.to_string()));
        Box::new(MyStrError::new(err.to_owned()))
    }

    fn is_black_char(&self, ch: char) -> bool {
        ch == ' ' || ch == '\r' || ch == '\n' || ch == '\t'
    }

    fn get_param(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        let empty_ast = astparser::str_to_ast(String::new());
        let p = params.get(i).unwrap_or(&empty_ast);
        let ret = self.eval_ast(p);
        return match ret {
            Ok(s) => Ok(s),
            Err(e) => 
            {
                Err(self.make_err_push(e,&format!("参数解析失败：{}", astparser::ast_to_string(p))))
            }
        }
    }

    /// 严格文本取参：Text 直接返回。
    fn get_param_text_rc(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<Rc<String>, Box<dyn std::error::Error>> {
        let rv = self.get_param(params, i)?;
        rv.expect_text_value().map(Rc::new).map_err(|e| {
            RedLang::make_err(&format!("参数{}不是文本类型，{}", i, e))
        })
    }

    /// 兼容旧签名：返回拥有所有权的文本
    fn get_param_text(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.get_param_text_rc(params, i)?.as_ref().clone())
    }

    /// 获取参数并转为数组。
    fn get_param_array(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<Vec<Rc<RedValue>>, Box<dyn std::error::Error>> {
        let rv = self.get_param(params, i)?;
        rv.expect_array_value().map_err(|e| {
            RedLang::make_err(&format!("参数{}不是数组类型，{}", i, e))
        })
    }

    /// 获取参数并转为字节集。
    fn get_param_bin_rc(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<Rc<Vec<u8>>, Box<dyn std::error::Error>> {
        let rv = self.get_param(params, i)?;
        rv.expect_bin_value().map(Rc::new).map_err(|e| {
            RedLang::make_err(&format!("参数{}不是字节集类型，{}", i, e))
        })
    }

    /// 兼容旧签名：返回拥有所有权的字节集
    fn get_param_bin(
        &mut self,
        params: &[astparser::Ast],
        i: usize,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(self.get_param_bin_rc(params, i)?.as_ref().clone())
    }

    fn connect_rv_with_context(
        &self,
        cur: &mut Rc<RedValue>,
        new_val: &Rc<RedValue>,
        context: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        RedLang::conect_rv(cur, new_val).map_err(|e| self.make_err_push(e, context))
    }

    pub fn build_bin_raw_from_img(&self, img: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)?;
        Ok(bytes)
    }

    fn conect_rv(cur: &mut Rc<RedValue>, new_val_t: &Rc<RedValue>) -> Result<(), Box<dyn std::error::Error>> {
        let new_val = new_val_t.clone();

        // 如果当前为空文本，直接替换
        if let RedValue::Text(s) = &**cur {
            if s.is_empty() {
                *cur = new_val.clone();
                return Ok(());
            }
        }

        // 检查 CLEAR_UUID
        if let RedValue::Text(s) = &*new_val {
            if s.starts_with(&*crate::CLEAR_UUID) {
                *cur = rv_empty();
                return Ok(());
            }
        }
        match (&**cur, &*new_val) {
            (RedValue::Text(old), RedValue::Text(new_s)) => {
                if !new_s.is_empty() {
                    *cur = Rc::new(RedValue::Text(Rc::new(old.to_string() + new_s.as_str())));
                }
            }
            (RedValue::Array(old_arr), RedValue::Array(new_arr)) => {
                let mut merged = old_arr.clone();
                merged.extend(new_arr.iter().cloned());
                *cur = Rc::new(RedValue::Array(merged));
            }
            (RedValue::Object(old_obj), RedValue::Object(new_obj)) => {
                let mut merged = old_obj.clone();
                for (k, v) in new_obj {
                    merged.insert(k.clone(), v.clone());
                }
                *cur = Rc::new(RedValue::Object(merged));
            }
            (RedValue::Bin(old_bin), RedValue::Bin(new_bin)) => {
                let mut merged = old_bin.as_ref().clone();
                merged.extend(new_bin.iter().copied());
                *cur = Rc::new(RedValue::Bin(Rc::new(merged)));
            }
            (_, RedValue::Text(s)) if s.is_empty() => {
                // 新值为空文本，不做任何事
            }
            _ => {
                let old_type = cur.get_type_name();
                let new_type = new_val.get_type_name();
                return Err(RedLang::make_err(&format!("`{}`类型不能与`{}`类型直接连接", new_type, old_type)));
            }
        }
        Ok(())
    }
    
    pub fn parse(&mut self, input: &str) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        // 解析为AST（内部会移除注释）
        let ast = astparser::parse_to_ast(input).map_err(|e| RedLang::make_err(&e))?;
        self.eval_ast_with_stack_guard(&ast)
    }

    fn eval_ast_with_stack_guard(&mut self, ast: &astparser::Ast) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        let cc = &(*self.scriptcallstackdeep);

        if *cc.borrow() > 500 {
            return Err(RedLang::make_err("too deep call stack"));
        }

        *cc.borrow_mut() += 1;

        let _guard = scopeguard::guard(self.scriptcallstackdeep.clone(), |v| {
            let cc = &(*v);
            *cc.borrow_mut() -= 1;
        });

        // 执行AST
        self.eval_ast(ast)
    }

    /// 兼容方法：parse 并返回文本。非文本值需要在调用方以结构化值处理。
    pub fn parse_to_string(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let rv = self.parse(input)?;
        rv.expect_text_value()
    }

    fn eval_ast(&mut self, ast: &astparser::Ast) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
        // 输出
        let mut chs_out: Rc<RedValue> = rv_empty();

        for node in ast {
            // 检查函数返回
            let fun_ret_vec_len = self.fun_ret_vec.len();
            if self.fun_ret_vec[fun_ret_vec_len - 1].0 == true {
                // 跳出当前函数内的所有循环
                for i in self.fun_ret_vec[fun_ret_vec_len - 1].1  .. self.xh_vec.len() {
                    self.xh_vec[i][1] = true;
                }
                // 跳出当前解析
                break;
            }

            // 检查循环控制
            let xh_vec_len = self.xh_vec.len();
            if xh_vec_len != 0 {
                // 说明在循环中
                if self.xh_vec[xh_vec_len - 1][0] == true {
                    break;
                }
                if self.xh_vec[xh_vec_len - 1][1] == true {
                    // 没有下次循环了
                    // 这里退出本次循环
                    break;
                }
            }

            match node {
                astparser::AstNode::Text(text) => {
                    let text_rv = Rc::new(RedValue::Text(text.clone()));
                    self.connect_rv_with_context(
                        &mut chs_out,
                        &text_rv,
                        &format!("拼接文本节点失败：{}", astparser::ast_to_string(&vec![node.clone()])),
                    )?;
                }
                astparser::AstNode::Command(cmd) => {
                    // 直接传递参数的 AST，避免重复解析
                    let ret = match self.do_cmd_fun(&cmd.name, &cmd.args) {
                        Ok(ret) => ret,
                        Err(e) => {
                            return Err(self.make_err_push(e, &format!("命令`{}`执行失败", cmd.name)));
                        }
                    };
                    self.connect_rv_with_context(
                        &mut chs_out,
                        &ret,
                        &format!("拼接命令`{}`的返回值失败", cmd.name),
                    )?;
                }
            }
        }
        Ok(chs_out)
    }

    pub fn parse_r_with_black(&self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut ret = String::new();
        for i in input.chars() {
            if i == '\\' || i == '@' || i == '【' || i == '】' || self.is_black_char(i) {
                ret.push('\\');
            }
            ret.push(i);
        }
        Ok(ret)
    }
    fn get_var_ref(&mut self,var_name:&str) -> Option<Rc<RefCell<Rc<RedValue>>>> {
        let var_vec_len = self.var_vec.len();
        for i in 0..var_vec_len {
            let mp = &self.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get(var_name);
            if let Some(v) = v_opt {
                return Some(v.clone());
                
            }
        }
        return None;
    }
    fn parse_fun(&mut self, input: &str, is_2_params: bool) -> Result<String, Box<dyn std::error::Error>> {
        // 使用 AST 解析输入
        let ast = astparser::parse_to_ast(input).map_err(|e| RedLang::make_err(&e))?;
        self.parse_fun_ast(&ast, is_2_params)
    }

    fn parse_fun_ast(&mut self, ast: &astparser::Ast, is_2_params: bool) -> Result<String, Box<dyn std::error::Error>> {
        let mut out = String::new();
        for node in ast {
            match node {
                astparser::AstNode::Text(text) => {
                    // 文本节点：需要转义特殊字符以保持字面量
                    for ch in text.chars() {
                        if ch == '\\' || ch == '@' || ch == '【' || ch == '】' || ch.is_whitespace() {
                            out.push('\\');
                        }
                        out.push(ch);
                    }
                }
                astparser::AstNode::Command(cmd) => {
                    // 求值命令名
                    let cmd_name = self.parse_to_string(&cmd.name)?;
                    let cmd_jt = crate::mytool::str_to_jt(&cmd_name);

                    if cmd_jt == "闭包" && !is_2_params {
                        // 闭包：求值第一个参数并转义后内联
                        let cqout = if !cmd.args.is_empty() {
                            self.eval_ast(&cmd.args[0])?.expect_text_value()?
                        } else {
                            String::new()
                        };
                        let cqout_r = self.parse_r_with_black(&cqout)?;
                        cq_add_log_w(&format!("cqout:{cqout} cqout_r:{cqout_r}")).unwrap();
                        out.push_str(&cqout_r);
                    } else if is_2_params {
                        if cmd_jt == "二类参数" {
                            let k1 = if !cmd.args.is_empty() {
                                self.eval_ast(&cmd.args[0])?.expect_text_value()?.parse::<usize>()?
                            } else {
                                0
                            };
                            let ret_str = self.parse_to_string(&format!("【二类参数@{k1}】"))?;
                            let cqout_r = self.parse_r_with_black(&ret_str)?;
                            out.push_str(&cqout_r);
                        } else if cmd_jt == "参数" {
                            let k1 = if !cmd.args.is_empty() {
                                self.eval_ast(&cmd.args[0])?.expect_text_value()?.parse::<usize>()?
                            } else {
                                0
                            };
                            let ret_str = self.parse_to_string(&format!("【参数@{k1}】"))?;
                            let cqout_r = self.parse_r_with_black(&ret_str)?;
                            out.push_str(&cqout_r);
                        } else {
                            // 其他命令：递归转换命令名和所有参数
                            let mut r_v = Vec::new();
                            let name_str: &str = &cmd.name;
                            if cmd_jt != "函数定义" && cmd_jt != "定义命令" && cmd_jt != "定义二类命令" {
                                r_v.push(self.parse_fun(name_str, is_2_params)?);
                            } else {
                                r_v.push(self.parse_fun(name_str, false)?);
                            }
                            for arg_ast in cmd.args.iter() {
                                if cmd_jt != "函数定义" && cmd_jt != "定义命令" && cmd_jt != "定义二类命令" {
                                    r_v.push(self.parse_fun_ast(arg_ast, is_2_params)?);
                                } else {
                                    r_v.push(self.parse_fun_ast(arg_ast, false)?);
                                }
                            }
                            out.push_str(&format!("【{}】", r_v.join("@")));
                        }
                    } else {
                        // 普通模式：递归转换命令名和所有参数
                        let mut r_v = Vec::new();
                        let name_str: &str = &cmd.name;
                        r_v.push(self.parse_fun(name_str, false)?);
                        for arg_ast in cmd.args.iter() {
                            r_v.push(self.parse_fun_ast(arg_ast, false)?);
                        }
                        out.push_str(&format!("【{}】", r_v.join("@")));
                    }
                }
            }
        }
        Ok(out)
    }
}

impl Default for RedLang {
    fn default() -> Self {
        Self::new()
    }
}
