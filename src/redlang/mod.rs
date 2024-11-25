use std::{any::Any, cell::RefCell, collections::{BTreeMap, HashMap, HashSet, VecDeque}, error, ffi::{c_char, c_int, CStr, CString}, fmt, rc::Rc, sync::Arc, thread, time::SystemTime, vec};
use encoding::Encoding;
use exfun::get_raw_data;
use image::{ImageBuffer, Rgba};

use crate::{cqapi::cq_add_log_w, cqevent::do_script, pkg_can_run, G_CONST_MAP, G_LOCK, G_SINGAL_ARR, G_TEMP_CONST_MAP, REDLANG_UUID};
use libloading::Symbol;

pub mod exfun;
pub(crate) mod cqexfun;
pub(crate) mod webexfun;

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

struct RedLangVarType {
    show_str:Rc<String>,
    dat:Box<dyn Any>
}

fn set_const_val(bin_pool:&mut HashMap<String, RedLangBinPoolVarType>,pkg_name:&str,val_name:&str,val:String) -> Result<(), Box<dyn std::error::Error>> {
    let mut g_map = G_CONST_MAP.write()?;
    let val_map = g_map.get_mut(pkg_name);
    let uid = REDLANG_UUID.to_string();
    let val_t;
    if val.starts_with(&(uid.clone() + "B96ad849c-8e7e-7886-7742-e4e896cc5b86")) { // 特殊字节集类型
        let b = RedLang::parse_bin(bin_pool,&val)?;
        val_t = RedLang::build_bin_with_uid(&uid, b);
    }else {
        val_t = val;
    }
    if val_map.is_none() {
        let mut mp = HashMap::new();
        mp.insert(val_name.to_owned(), val_t);
        g_map.insert(pkg_name.to_owned(), mp);
    }else {
        val_map.unwrap().insert(val_name.to_owned(), val_t);
    }
    Ok(())
}

fn get_const_val(pkg_name:&str,val_name:&str) -> Result<String, Box<dyn std::error::Error>> {
    match G_CONST_MAP.read()?.get(pkg_name) {
        Some(var_map) => 
            match var_map.get(val_name) {
                Some(val) => Ok(val.to_owned()),
                None => Ok("".to_string())
            }
        None => Ok("".to_string())
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

fn set_temp_const_val(bin_pool:&mut HashMap<String, RedLangBinPoolVarType>,pkg_name:&str,val_name:&str,val:String,expire_time:u128) -> Result<(), Box<dyn std::error::Error>> {
    clear_temp_const_val()?; // 清除过期的key
    let mut g_map = G_TEMP_CONST_MAP.write()?;
    let val_map = g_map.get_mut(pkg_name);
    let uid = REDLANG_UUID.to_string();
    let val_t;
    if val.starts_with(&(uid.clone() + "B96ad849c-8e7e-7886-7742-e4e896cc5b86")) { // 特殊字节集类型
        let b = RedLang::parse_bin(bin_pool,&val)?;
        val_t = RedLang::build_bin_with_uid(&uid, b);
    }else {
        val_t = val;
    }
    if val_map.is_none() {
        let mut mp = HashMap::new();
        mp.insert(val_name.to_owned(), (val_t,expire_time));
        g_map.insert(pkg_name.to_owned(), mp);
    }else {
        val_map.unwrap().insert(val_name.to_owned(), (val_t,expire_time));
    }
    Ok(())
}

fn get_temp_const_val(pkg_name:&str,val_name:&str) -> Result<String, Box<dyn std::error::Error>> {
    match G_TEMP_CONST_MAP.read()?.get(pkg_name) {
        Some(var_map) => 
            match var_map.get(val_name) {
                Some(val) => {
                    let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
                    if val.1 < tm {
                        Ok("".to_string())
                    }else {
                        Ok(val.0.to_owned())
                    }
                }
                None => Ok("".to_string())
            }
        None => Ok("".to_string())
    }
}

impl RedLangVarType {
    pub fn new() -> RedLangVarType {
        RedLangVarType {
            show_str:Rc::new(String::new()),
            dat:Box::new(RefCell::new(String::new()))
        }
    }
    pub fn get_string(
        &mut self,
    ) -> Rc<String> {
        if self.show_str.is_empty() {
            if self.dat.is::<String>() {
                let dat_ref = self.dat.downcast_ref::<String>().unwrap();
                self.show_str = Rc::new(dat_ref.to_owned());
            }else if self.dat.is::<Vec<char>>() {
                let dat_ref = self.dat.downcast_ref::<Vec<char>>().unwrap();
                self.show_str = Rc::new(dat_ref.iter().collect::<String>());
            }else if self.dat.is::<Vec<String>>() {
                let dat_ref = self.dat.downcast_ref::<Vec<String>>().unwrap();
                let dat_ref_t = dat_ref.iter().map(|x|x.as_str()).collect::<Vec<&str>>();
                self.show_str = Rc::new(RedLang::build_arr_with_uid(&crate::REDLANG_UUID.to_string(),dat_ref_t));
            }
            else if self.dat.is::<BTreeMap<String,String>>() {
                let dat_ref = self.dat.downcast_ref::<BTreeMap<String,String>>().unwrap();
                self.show_str = Rc::new(RedLang::build_obj_with_uid(&crate::REDLANG_UUID.to_string(),dat_ref.to_owned()));
            }else if self.dat.is::<Vec<u8>>() {
                let dat_ref = self.dat.downcast_ref::<Vec<u8>>().unwrap();
                self.show_str = Rc::new(RedLang::build_bin_with_uid(&crate::REDLANG_UUID.to_string(),dat_ref.to_owned()));
            }else {
                let k:Option<i32> = None;
                k.ok_or("RedLangVarType:get_string中发发生未知错误").unwrap();
            }
        }
        return self.show_str.clone();
    }
    pub fn set_string(&mut self,bin_pool:&mut HashMap<String, RedLangBinPoolVarType>,dat_str:String) -> Result<(), Box<dyn std::error::Error>>{
        let uid = crate::REDLANG_UUID.to_string();
        if dat_str.starts_with(&(uid.clone() + "A")) {
            let t = RedLang::parse_arr(&dat_str)?;
            let mut v:Vec<String> = vec![];
            for it in t {
                v.push(it.to_owned());
            }
            self.dat = Box::new(v);
            self.show_str = Rc::new(dat_str);
            
        }else if dat_str.starts_with(&(uid.clone() + "O")) {
            self.dat = Box::new(RedLang::parse_obj(&dat_str)?);
            self.show_str = Rc::new(dat_str);
        }else if dat_str.starts_with(&(uid.clone() + "B")) {
            if dat_str.starts_with(&(uid.clone() + "B96ad849c-8e7e-7886-7742-e4e896cc5b86")) {
                self.dat = Box::new(RedLang::parse_img_bin(bin_pool,&dat_str)?);
            } else {
                self.dat = Box::new(RedLang::parse_bin(bin_pool,&dat_str)?);
            }
            self.show_str = Rc::new(dat_str);
        }else if dat_str.starts_with(&(uid + "F")) {
            self.dat = Box::new(dat_str.clone());
            self.show_str = Rc::new(dat_str);
        }else {
            let chs = dat_str.chars().collect::<Vec<char>>();
            self.dat = Box::new(chs);
            self.show_str = Rc::new(dat_str);
        }
        Ok(())
    }

    pub fn get_type(&self) -> &'static str {
        if self.dat.is::<String>() {
            return "函数";
        }else if self.dat.is::<Vec<char>>() {
            return "文本";
        }else if self.dat.is::<Vec<String>>() {
            return "数组";
        }else if self.dat.is::<BTreeMap<String,String>>() {
            return "对象";
        }else if self.dat.is::<Vec<u8>>() {
            return "字节集";
        }else {
            let k:Option<i32> = None;
            k.ok_or("RedLangVarType:get_type中发发生未知错误").unwrap();
            return "";
        }
    }
    pub fn add_str(&mut self,s:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "文本" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            for it in s.chars() {
                v.push(it);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("文本增加元素失败,类型不是文本"))
    }
    pub fn add_bin(&mut self,s:Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "字节集" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            for it in s {
                v.push(it);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("字节集增加元素失败,类型不是字节集"))
    }
    pub fn add_arr(&mut self,s:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "数组" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            v.push(s.to_owned());
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("数组增加元素失败,类型不是数组"))
    }
    pub fn add_obj(&mut self,key:String,val:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "对象" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.insert(key, val);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("对象增加元素失败,类型不是对象"))
    }
    pub fn rep_obj(&mut self,key:String,val:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "对象" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.insert(key, val);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("对象替换元素失败,类型不是对象"))
    }
    pub fn rep_arr(&mut self,index:usize,s:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "数组" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            let el = v.get_mut(index).ok_or("替换数组元素时越界")?;
            (*el) = s;
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("数组替换元素失败,类型不是数组"))
    }
    pub fn rep_bin(&mut self,index:usize,s:u8) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "字节集" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            let el = v.get_mut(index).ok_or("替换字节集元素时越界")?;
            (*el) = s;
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("字节集替换元素失败,类型不是字节集"))
    }
    pub fn rep_str(&mut self,index:usize,s:char) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "文本" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            let el = v.get_mut(index).ok_or("替换文本元素时越界")?;
            (*el) = s.to_owned();
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("文本替换元素失败,类型不是文本"))
    }
    pub fn rv_str(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "文本" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("文本删除元素失败,类型不是文本"))
    }
    pub fn rv_bin(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "字节集" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("字节集删除元素失败,类型不是字节集"))
    }
    pub fn rv_arr(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "数组" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("数组删除元素失败,类型不是数组"))
    }
    pub fn rv_obj(&mut self,key:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "对象" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.remove(key);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("对象删除元素失败,类型不是对象"))
    }
    pub fn get_obj(&mut self,key:&str) -> String {
        if self.get_type() == "对象" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            match v.get(key){
                Some(s) => return s.to_owned(),
                None => return "".to_string(),
            }
        }
        return "".to_string()
    }
    pub fn get_arr(&mut self,index:usize) -> String {
        if self.get_type() == "数组" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            match v.get(index){
                Some(s) => return s.to_owned(),
                None => return "".to_string(),
            }
        }
        return "".to_string()
    }
    pub fn get_str(&mut self,index:usize) -> String {
        if self.get_type() == "文本" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            match v.get(index){
                Some(s) => return s.to_string(),
                None => return "".to_string(),
            }
        }
        return "".to_string()
    }
    pub fn get_bin(&mut self,index:usize) -> String {
        if self.get_type() == "字节集" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            match v.get(index){
                Some(s) => return RedLang::build_bin_with_uid(&crate::REDLANG_UUID, vec![*s]),
                None => return RedLang::build_bin_with_uid(&crate::REDLANG_UUID, vec![]),
            }
        }
        return RedLang::build_bin_with_uid(&crate::REDLANG_UUID, vec![]);
    }

}


pub struct RedLangBinPoolVarType {
    pub type_t:String,
    pub dat:Box<dyn Any>
}

pub struct RedLang {
    var_vec: Vec<HashMap<String,  Rc<RefCell<RedLangVarType>>>>, //变量栈
    xh_vec: Vec<[bool; 2]>,                // 循环控制栈
    params_vec: Vec<Vec<String>>,          // 函数参数栈
    fun_ret_vec: Vec<(bool,usize)>,                // 记录函数是否返回,循环深度
    pub exmap:Rc<RefCell<HashMap<String, Arc<String>>>>, // 用于记录平台相关数据
    xuhao: HashMap<String, usize>,
    pub type_uuid:String,
    pub pkg_name:String,
    pub script_name:String,
    pub lock_vec:HashSet<String>,
    pub req_tx:Option<tokio::sync::mpsc::Sender<bool>>,
    pub req_rx:Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    pub can_wrong:bool,
    stack:VecDeque<String>,
    scriptcallstackdeep:Rc::<RefCell<usize>>, // 记录脚本调用栈的深度
    pub bin_pool:HashMap<String, RedLangBinPoolVarType>,
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
    getrandom::getrandom(&mut rand_buf)?;
    let mut num = 0usize;
    for i in 0..std::mem::size_of::<usize>() {
        num = (num << 8) + (rand_buf[i] as usize);
    }
    Ok(num)
}


fn get_core_cmd(cmd:&str,pkg_name:&str) -> Option<fn(&mut RedLang, &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>> {
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


pub fn init_core_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
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
    add_fun(vec!["换行"],|_self_t,_params|{
        return Ok(Some(String::from("\n")));
    });
    add_fun(vec!["回车"],|_self_t,_params|{
        return Ok(Some(String::from("\r")));
    });
    add_fun(vec!["空格"],|_self_t,_params|{
        return Ok(Some(String::from(" ")));
    });
    add_fun(vec!["隐藏"],|self_t,params|{
        let out = self_t.get_param(params, 0)?;
        self_t.set_coremap("隐藏", &out)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["传递"],|self_t,_params|{
        let ret = self_t.get_coremap("隐藏")?;
        return Ok(Some(ret));
    });
    add_fun(vec!["入栈"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        self_t.stack.push_back(text);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["出栈"],|self_t,_params|{
        let ele = self_t.stack.pop_back().unwrap_or_default();
        return Ok(Some(ele));
    });
    add_fun(vec!["定义变量"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        let var_vec_len = self_t.var_vec.len();
        let mp = &mut self_t.var_vec[var_vec_len - 1];
        let mut var = RedLangVarType::new();
        var.set_string(&mut self_t.bin_pool,v)?;
        mp.insert(k, Rc::new(RefCell::new(var)));
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["变量"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let var_ref = self_t.get_var_ref(&k);
        if let Some(v) = var_ref {
            let mut k = (*v).borrow_mut();
            return Ok(Some((*k.get_string()).clone()));
        }else {
            return Ok(Some("".to_string()));
        }
    });
    add_fun(vec!["屏蔽"],|self_t,params|{
        let _k = self_t.get_param(params, 0)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["赋值变量"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let var_vec_len = self_t.var_vec.len();
        let mut is_set = false;
        let vvv = self_t.get_param(params, 1)?;
        let vvv_rc =Rc::new(RefCell::new(vvv));
        for i in 0..var_vec_len {
            let mp = &mut self_t.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get_mut(&k);
            if let Some(val) = v_opt {
                (**val).borrow_mut().set_string(&mut self_t.bin_pool,(*vvv_rc).borrow().to_owned())?;
                is_set = true;
                break;
            }
        }
        if is_set == false {
            let var_vec_len = self_t.var_vec.len();
            let mp = &mut self_t.var_vec[var_vec_len - 1];
            let mut var = RedLangVarType::new();
            var.set_string(&mut self_t.bin_pool,(*vvv_rc).borrow().to_owned())?;
            mp.insert(k, Rc::new(RefCell::new(var)));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["判断","判等"],|self_t,params|{
        let ret_str;
        let k1 = self_t.get_param(params, 0)?;
        let k2 = self_t.get_param(params, 1)?;
        if k1 != k2 {
            ret_str = self_t.get_param(params, 2)?;
        } else {
            ret_str = self_t.get_param(params, 3)?;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["判真"],|self_t,params|{
        let ret_str;
        let k1 = self_t.get_param(params, 0)?;
        if k1 != "真"{
            ret_str = self_t.get_param(params, 1)?;
        }else {
            ret_str = self_t.get_param(params, 2)?;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["判空"],|self_t,params|{
        let ret_str;
        let data = self_t.get_param(params, 0)?;
        let len = self_t.get_len(&data)?;
        if len == 0 {
            ret_str = self_t.get_param(params, 1)?
        }else{
            ret_str = data;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["循环"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        let k1_tp = self_t.get_type(&k1)?;
        let mut ret_str = String::new();
        if k1_tp == "文本"{
            let tms = k1.parse::<usize>()?;
            self_t.xh_vec.push([false, false]);
            let mut last_type = 0u8;
            let xh_len = self_t.xh_vec.len();
            for _i in 0..tms {
                self_t.xh_vec[xh_len - 1][0] = false;
                let v = self_t.get_param(params, 1)?;
                RedLang::conect_arr(&self_t.bin_pool,&mut last_type,&mut ret_str,&v)?;
                if self_t.xh_vec[xh_len - 1][1] == true {
                    break;
                }
            }
            self_t.xh_vec.pop();
        }
        else if k1_tp == "数组" {
            let arr_str = k1;
            let fun = params.get(1).ok_or("数组循环中参数函数获取失败")?.to_string();
            let arr = RedLang::parse_arr(&arr_str)?;
            let tms = arr.len();
            self_t.xh_vec.push([false, false]);
            let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
            fun_params[0] = fun;
            let mut last_type = 0;
            let xh_len = self_t.xh_vec.len();
            for i in 0..tms {
                self_t.xh_vec[xh_len - 1][0] = false;
                fun_params[1] = i.to_string();
                fun_params[2] = arr[i].to_owned();
                let v = self_t.call_fun(&fun_params,true)?;
                RedLang::conect_arr(&self_t.bin_pool,&mut last_type,&mut ret_str,&v)?;
                if self_t.xh_vec[xh_len - 1][1] == true {
                    break;
                }
            }
            self_t.xh_vec.pop();
        }else if k1_tp == "对象" {
            let obj_str = k1;
            let fun = params.get(1).ok_or("对象循环中参数函数获取失败")?.to_string();
            let obj = RedLang::parse_obj(&obj_str)?;
            self_t.xh_vec.push([false, false]);
            let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
            fun_params[0] = fun;
            let mut last_type = 0;
            let xh_len = self_t.xh_vec.len();
            for (k,v) in obj {
                self_t.xh_vec[xh_len - 1][0] = false;
                fun_params[1] = k;
                fun_params[2] = v;
                let v = self_t.call_fun(&fun_params,true)?;
                RedLang::conect_arr(&self_t.bin_pool,&mut last_type,&mut ret_str,&v)?;
                if self_t.xh_vec[xh_len - 1][1] == true {
                    break;
                }
            }
            self_t.xh_vec.pop();
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["判循"],|self_t,params|{
        let mut ret_str = String::new();
        self_t.xh_vec.push([false, false]);
        let mut last_type = 0;
        let xh_len = self_t.xh_vec.len();
        loop {
            self_t.xh_vec[xh_len - 1][0] = false;
            if self_t.get_param(params, 0)? != "真" {
                break;
            }
            let v = self_t.get_param(params, 1)?;
            RedLang::conect_arr(&self_t.bin_pool,&mut last_type,&mut ret_str,&v)?;
            if self_t.xh_vec[xh_len - 1][1] == true {
                break;
            }
        }
        self_t.xh_vec.pop();
        return Ok(Some(ret_str));
    });
    add_fun(vec!["跳出"],|self_t,_params|{
        let xh_vec_len = self_t.xh_vec.len();
        if xh_vec_len != 0 {
            self_t.xh_vec[xh_vec_len - 1][1] = true;
        } else {
            return Err(RedLang::make_err("不在循环中，无法使用`跳出`命令"));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["继续"],|self_t,_params|{
        let xh_vec_len = self_t.xh_vec.len();
        if xh_vec_len == 0 {
            return Err(RedLang::make_err("不在循环中，无法使用`继续`命令"));
        } else {
            self_t.xh_vec[xh_vec_len - 1][0] = true;
        }

        return Ok(Some("".to_string()));
    });
    add_fun(vec!["函数定义"],|self_t,params|{
        let func = params.get(0).ok_or("函数定义:读取参数失败")?;
        let fun = self_t.parse_fun(&func,false)?;
        let func_t = format!("{}F{}",self_t.type_uuid,fun);
        let ret_str = func_t;
        return Ok(Some(ret_str));
    });
    add_fun(vec!["定义命令"],|self_t,params|{
        let func_name = self_t.get_param(params, 0)?;
        let func = params.get(1).ok_or("定义命令:读取参数失败")?;
        let fun = self_t.parse_fun(&func,false)?;
        let mut w = crate::G_CMD_MAP.write()?;
        match w.get_mut(&self_t.pkg_name){
            Some(r) => {
                r.insert(func_name, fun);
            },
            None => {
                let mut r = HashMap::new();
                r.insert(func_name, fun);
                w.insert(self_t.pkg_name.clone(), r);
            },
        };
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["定义二类命令"],|self_t,params|{
        let func_name = self_t.get_param(params, 0)?;
        let func = params.get(1).ok_or("定义命令:读取参数失败")?;
        let fun = format!("1FC0F025-BFE7-63A4-CA66-FC3FD8A55B7B{}",self_t.parse_fun(&func,false)?);
        let mut w = crate::G_CMD_MAP.write()?;
        match w.get_mut(&self_t.pkg_name){
            Some(r) => {
                r.insert(func_name, fun);
            },
            None => {
                let mut r = HashMap::new();
                r.insert(func_name, fun);
                w.insert(self_t.pkg_name.clone(), r);
            },
        };
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["函数调用","调用函数"],|self_t,params|{
        let ret_str = self_t.call_fun(params,false)?;
        return Ok(Some(ret_str));
    });
    add_fun(vec!["参数"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        let tms = k1.parse::<usize>()? - 1;
        let params_vec_len = self_t.params_vec.len();
        let ret_str = self_t.params_vec[params_vec_len - 1].get(tms).unwrap_or(&"".to_string()).to_string();
        return Ok(Some(ret_str));
    });
    add_fun(vec!["二类参数"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        let tms = k1.parse::<usize>()? - 1;
        let params_vec_len = self_t.params_vec.len();
        let ret_str = self_t.params_vec[params_vec_len - 1].get(tms).unwrap_or(&"".to_string()).to_string();
        return Ok(Some(self_t.parse(&ret_str)?));
    });
    add_fun(vec!["参数个数"],|self_t,_params|{
        let params_vec_len = self_t.params_vec.len();
        let ret_str = self_t.params_vec[params_vec_len - 1].len().to_string();
        return Ok(Some(ret_str));
    });
    add_fun(vec!["返回"],|self_t,_params|{
        let fun_ret_vec_len = self_t.fun_ret_vec.len();
        self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["计算"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
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
        return Ok(Some(ret_str));
    });
    add_fun(vec!["数组"],|self_t,params|{
        let arr_len = params.len();
        let mut temp_ret:Vec<String> = vec![];
        for i in 0..arr_len {
            let s = self_t.get_param(params, i)?;
            temp_ret.push(s);
        }
        let ret_str = self_t.build_arr(temp_ret.iter().map(AsRef::as_ref).collect());
        return Ok(Some(ret_str));
    });
    add_fun(vec!["对象"],|self_t,params|{
        let params_len = params.len();
        if params_len % 2 != 0 {
            return Err(RedLang::make_err("请保证对象参数为偶数个"));
        }
        let mut temp_ret:BTreeMap<String,String> = BTreeMap::new();
        for i in 0..(params_len/2) {
            let k = self_t.get_param(params, i*2)?;
            let v = self_t.get_param(params, i*2 + 1)?;
            temp_ret.insert(k, v);
        }
        let ret_str = self_t.build_obj(temp_ret);
        return Ok(Some(ret_str));
    });
    add_fun(vec!["取长度"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        let ret_str = self_t.get_len(&data)?.to_string(); 
        return Ok(Some(ret_str));
    });
    add_fun(vec!["闭包"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        return Ok(Some(data));
    });
    add_fun(vec!["转文本"],|self_t,params|{
        let data = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&data)?;
        fn obj_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
            let mut ret_str = String::new();
            ret_str.push('{');
            let mut vec_t:Vec<String>  = vec![];
            let obj = RedLang::parse_obj(&data)?;
            for (k,v) in obj{
                let tp_k = self_t.get_type(&k)?;
                if tp_k != "文本" {
                    return Err(RedLang::make_err(&("对象的键不支持的类型:".to_owned()+&tp_k)));
                }
                let mut temp_str = String::new();
                temp_str.push_str(&str_to_text(&k)?);
                temp_str.push(':');
                let tp_v = self_t.get_type(&v)?;
                if tp_v == "文本" {
                    temp_str.push_str(&str_to_text(&v)?);
                }
                else if tp_v == "数组" {
                    temp_str.push_str(&arr_to_text(self_t,&v,params)?);
                }
                else if tp_v == "字节集" {
                    temp_str.push_str(&bin_to_text(self_t,&v,params)?);
                }
                else if tp_v == "对象" {
                    temp_str.push_str(&obj_to_text(self_t,&v,params)?);
                }
                else {
                    return Err(RedLang::make_err(&("对象的值不支持的类型:".to_owned()+&tp_v)));
                }
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
        fn arr_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
            let mut vec_t:Vec<String>  = vec![];
            let arr = RedLang::parse_arr(&data)?;
            for v in arr {
                let tp_v = self_t.get_type(&v)?;
                if tp_v == "文本" {
                    vec_t.push(str_to_text(&v)?);
                }
                else if tp_v == "数组" {
                    vec_t.push(arr_to_text(self_t,&v,params)?);
                }
                else if tp_v == "字节集" {
                    vec_t.push(bin_to_text(self_t,&v,params)?);
                }
                else if tp_v == "对象" {
                    vec_t.push(obj_to_text(self_t,&v,params)?);
                }
                else {
                    return Err(RedLang::make_err(&("数组的元素不支持的类型:".to_owned()+&tp_v)));
                }
            }
            return Ok(format!("[{}]",vec_t.join(",")));
        }

        fn bin_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
            let ret_str:String;
            let code_t = self_t.get_param(params, 1)?;
            let code = code_t.to_lowercase();
            let u8_vec = RedLang::parse_bin(&mut self_t.bin_pool,data)?;
            if code == "" || code == "utf8" {
                ret_str = String::from_utf8(u8_vec)?;
            }else if code == "gbk" {
                ret_str = encoding::all::GBK.decode(&u8_vec, encoding::DecoderTrap::Ignore)?;
            }else{
                return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
            }
            Ok(ret_str)
        }
        let ret_str;
        if tp == "字节集" {
            ret_str = bin_to_text(self_t,&data,params)?;
        }else if tp == "文本" {
            ret_str = str_to_text(&data)?;
        }else if tp == "数组" {
            ret_str = arr_to_text(self_t,&data,params)?;
        }else if tp == "对象" {
            ret_str = obj_to_text(self_t,&data,params)?;
        }
        else{
            return Err(RedLang::make_err(&("对应类型不能转文本:".to_owned()+&tp)));
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["增加元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param(params, 0)?;
        let data:Rc<RefCell<RedLangVarType>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp =(*data).borrow().get_type();
        let el_len;
        if tp == "对象" {
            el_len = (params.len() -1) / 2;
        }else {
            el_len = params.len() -1;
        }
        //  增加元素
        for i in 0..el_len {
            if tp == "数组" {
                let el = self_t.get_param(params, i + 1)?;
                let mut v = (*data).borrow_mut();
                v.add_arr(&el)?;
            }else if tp == "对象" {
                let elk = self_t.get_param(params, i * 2 + 1)?;
                let elv = self_t.get_param(params, i * 2 + 2)?;

                let mut v = (*data).borrow_mut();
                v.add_obj(elk,elv)?;
                
            }else if tp == "文本" { 
                let el = self_t.get_param(params, i + 1)?;
                let mut v = (*data).borrow_mut();
                v.add_str(&el)?;

            }else if tp == "字节集" {
                let el_t = self_t.get_param(params, i + 1)?;
                let el = RedLang::parse_bin(&mut self_t.bin_pool,&el_t)?;
                let mut  v = (*data).borrow_mut();
                v.add_bin(el)?;
            }else{
                return Err(RedLang::make_err(&("对应类型不能增加元素:".to_owned()+&tp)));
            }
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["替换元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param(params, 0)?;
        let k_name = self_t.get_param(params, 1)?;
        let v_name = self_t.get_param(params, 2)?;
        let data:Rc<RefCell<RedLangVarType>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp =(*data).borrow().get_type();
        if tp == "数组" {
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            v.rep_arr(index, v_name)?;
        }else if tp == "对象" {
            let mut v = (*data).borrow_mut();
            v.rep_obj(k_name, v_name)?;
            
        }else if tp == "文本" { 
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            let v_chs = v_name.chars().collect::<Vec<char>>();
            if v_chs.len() != 1 {
                return Err(RedLang::make_err("替换文本元素时值的长度不为1"));
            }
            v.rep_str(index, v_chs[0])?;

        }else if tp == "字节集" {
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            let bt = RedLang::parse_bin(&mut self_t.bin_pool,&v_name)?;
            if bt.len() != 1 {
                return Err(RedLang::make_err("替换字节集元素时值的长度不为1"));
            }
            v.rep_bin(index, bt[0])?;
        }else{
            return Err(RedLang::make_err(&("对应类型不能替换元素:".to_owned()+&tp)));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["删除元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param(params, 0)?;
        let k_name = self_t.get_param(params, 1)?;
        let data:Rc<RefCell<RedLangVarType>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        // 获得变量类型
        let tp =(*data).borrow().get_type();
        if tp == "数组" {
            if k_name.starts_with("-"){return Ok(Some("".to_string()));}
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            v.rv_arr(index)?;
        }else if tp == "对象" {
            let mut v = (*data).borrow_mut();
            v.rv_obj(&k_name)?;
            
        }else if tp == "文本" { 
            if k_name.starts_with("-"){return Ok(Some("".to_string()));}
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            v.rv_str(index)?;

        }else if tp == "字节集" {
            if k_name.starts_with("-"){return Ok(Some("".to_string()));}
            let index = k_name.parse::<usize>()?;
            let mut v = (*data).borrow_mut();
            v.rv_bin(index)?;
        }else{
            return Err(RedLang::make_err(&("对应类型不能删除元素:".to_owned()+&tp)));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["取元素"],|self_t,params|{
        let nums = params.len();
        let df = String::new();
        let mut param_data = self_t.get_param(params, 0)?;
        for i in 1..nums {
            let tp = self_t.get_type(&param_data)?;
            if tp == "数组" {
                let index_rst = self_t.get_param(params, i)?.parse::<usize>();
                if index_rst.is_err() {
                    param_data = df;
                    break;
                }
                let index = index_rst.unwrap();
                let mp = RedLang::parse_arr(&param_data)?;
                let v_opt = mp.get(index);
                if let Some(v) = v_opt {
                    param_data = v.to_string();
                }else{
                    param_data = df;
                    break;
                }
            }else if tp == "对象" { 
                let index = self_t.get_param(params, i)?;
                let mp = RedLang::parse_obj(&param_data)?;
                let v_opt = mp.get(&index);
                if let Some(v) = v_opt {
                    param_data = v.to_string();
                }else{
                    param_data = df;
                    break;
                }
            }else if tp == "文本" {
                let index_rst = self_t.get_param(params, i)?.parse::<usize>();
                if index_rst.is_err() {
                    param_data = df;
                    break;
                }
                let index = index_rst.unwrap();
                let v_chs =param_data.chars().collect::<Vec<char>>();
                let v_opt = v_chs.get(index);
                if let Some(v) = v_opt {
                    param_data = v.to_string();
                }else{
                    param_data = df;
                    break;
                }
            }else{
                return Err(RedLang::make_err(&("对应类型不能取元素:".to_owned()+&tp)));
            }
        }
        let ret_str = param_data;
        return Ok(Some(ret_str));
    });
    add_fun(vec!["取变量元素"],|self_t,params|{
        // 获得变量
        let var_name = self_t.get_param(params, 0)?;
        let k_name = self_t.get_param(params, 1)?;
        
        let data:Rc<RefCell<RedLangVarType>>;
        if let Some(v) = self_t.get_var_ref(&var_name) {
            data = v;
        }else {
            return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
        }
        let ret_str;
        // 获得变量类型
        let tp =(*data).borrow().get_type();
        if tp == "数组" {
            let index_rst = k_name.parse::<usize>();
            if index_rst.is_err() {
                ret_str = "".to_owned();
            }else{
                let index = index_rst.unwrap();
                let mut v = (*data).borrow_mut();
                ret_str = v.get_arr(index);
            }
            
        }else if tp == "对象" {
            let mut v = (*data).borrow_mut();
            ret_str = v.get_obj(&k_name);
        }else if tp == "文本" { 
            let index_rst = k_name.parse::<usize>();
            if index_rst.is_err() {
                ret_str = "".to_owned();
            }else {
                let index = index_rst.unwrap();
                let mut v = (*data).borrow_mut();
                ret_str = v.get_str(index);
            }
        }else if tp == "字节集" {
            let index_rst = k_name.parse::<usize>();
            if index_rst.is_err() {
                ret_str = self_t.build_bin(vec![]);
            }else {
                let index = index_rst.unwrap();
                let mut v = (*data).borrow_mut();
                ret_str = v.get_bin(index);
            }
        }else{
            return Err(RedLang::make_err(&("对应类型不能替换元素:".to_owned()+&tp)));
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["取对象KEY"],|self_t,params|{
        let param_data = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&param_data)?;
        if tp != "对象" {
            return Err(RedLang::make_err(&("对应类型不能取对象key:".to_owned()+&tp)));
        }
        let parse_ret = RedLang::parse_obj(&param_data)?;
        let mut arr:Vec<&str> = vec![];
        for key in parse_ret.keys() {
            arr.push(key);
        }
        let ret_str = self_t.build_arr(arr);
        return Ok(Some(ret_str));
    });
    add_fun(vec!["取类型"],|self_t,params|{
        let param_data = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&param_data)?;
        let ret_str;
        if tp == "文本" {
            ret_str = "T".to_string();
        }else if tp == "数组" {
            ret_str = "A".to_string();
        }else if tp == "对象" {
            ret_str = "O".to_string();
        }else if tp == "字节集" {
            ret_str = "B".to_string();
        }else if tp == "函数" {
            ret_str = "F".to_string();
        }else {
            return Err(RedLang::make_err("取类型失败"));
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["取随机数"],|self_t,params|{
        let num1 = self_t.get_param(params, 0)?.parse::<usize>()?;
        let num2 = self_t.get_param(params, 1)?.parse::<usize>()?;
        if num1 > num2 {
            return Err(RedLang::make_err("生成随机数失败,请保证第一个数不大于第二个数，且都为非负数"));
        }
        let rand_num = get_random()?;
        let num = num2 + 1 - num1;
        let ret_num = (rand_num %  num) + num1;
        let ret_str = ret_num.to_string();
        return Ok(Some(ret_str));
    });
    add_fun(vec!["文本替换"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let from = self_t.get_param(params, 1)?;
        let to = self_t.get_param(params, 2)?;
        let ret_str = text.replace(&from, &to);
        return Ok(Some(ret_str));
    });
    add_fun(vec!["运行脚本"],|self_t,params|{
        let mut rl = RedLang::new();
        rl.exmap = self_t.exmap.clone(); // 获得一些拓展相关的变量
        rl.pkg_name = self_t.pkg_name.clone();
        rl.script_name = self_t.script_name.clone();
        rl.can_wrong = self_t.can_wrong;
        let code = self_t.get_param(params, 0)?;
        // 将参数传入新脚本
        let params_len = params.len();
        for i in 1..params_len {
            rl.params_vec[0].push(self_t.get_param(params, i)?);
        }
        let mut ret_str;
        ret_str = rl.parse(&code)?;
        if ret_str.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            ret_str = exfun::get_raw_data(self_t,ret_str)?;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["反射执行"],|self_t,params|{
        let code = self_t.get_param(params, 0)?;
        let ret_str = self_t.parse(&code)?;
        return Ok(Some(ret_str));
    });
    add_fun(vec!["崩溃吧"],|self_t,params|{
        None::<i32>.expect(&self_t.get_param(params, 0)?);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["后台运行脚本"],|self_t,params|{
        let exmap = (*self_t.exmap).borrow().clone();
        let code = self_t.get_param(params, 0)?;
        let pkg_name = self_t.pkg_name.clone();
        let script_name = self_t.script_name.clone();
        let can_wrong = self_t.can_wrong;
        // 获取参数
        let params_len = params.len();
        let mut params_vec: Vec<String> = vec![];
        for i in 1..params_len {
            params_vec.push(self_t.get_param(params, i)?);
        }
        thread::spawn(move ||{
            let mut rl = RedLang::new();
            rl.exmap = Rc::new(RefCell::new(exmap)); // 获得一些拓展相关的变量
            rl.pkg_name = pkg_name;
            rl.script_name = script_name;
            rl.can_wrong = can_wrong;
            // 将参数传入新脚本
            for i in 0..params_vec.len() {
                rl.params_vec[0].push(params_vec[i].clone());
            }
            if let Err(err) = do_script(&mut rl, &code,"normal",false) {
                cq_add_log_w(&format!("{}",err)).unwrap();
            }
        });
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["反转义"],|self_t,params|{
        let mut rl = RedLang::new();
        rl.exmap = self_t.exmap.clone(); // 获得一些拓展相关的变量
        rl.pkg_name = self_t.pkg_name.clone();
        rl.script_name = self_t.script_name.clone();
        let code = self_t.get_param(params, 0)?;
        let ret_str = self_t.parse(&code)?;
        return Ok(Some(ret_str));
    });
    add_fun(vec!["选择"],|self_t,params|{
        let select_num_str = self_t.get_param(params, 0)?;
        let params_len = params.len();
        if params_len == 0 {
            return Ok(Some("".to_string()));
        }
        let select_num;
        if select_num_str == "" { // 空
            let rand_num = get_random()?;
            select_num = rand_num % (params_len - 1) + 1; // 随机取
        }
        else if select_num_str.starts_with("-") { // 可能是负数
            let _foo = select_num_str.parse::<i64>()?;
            select_num = 0; // 越界
        } 
        else {
            select_num = select_num_str.parse::<usize>()? + 1; //解析正整数
        }
        let ret_str;
        if select_num == 0 || select_num > params_len {
            ret_str = "".to_string();
        }else {
            ret_str = self_t.get_param(params, select_num)?;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["当前版本"],|_self_t,_params|{
        return Ok(Some(crate::get_version()));
    });
    add_fun(vec!["加锁"],|self_t,params|{
        let lock_name = self_t.get_param(params, 0)?;
        loop {
            // 当前脚本已经创建了这个锁，则不做任何事
            if self_t.lock_vec.contains(&lock_name) {
                break;
            }
            // 全局已经存在锁，则等待锁消失，再创建锁
            {
                let mut k = crate::G_LOCK.lock()?;
                if !k.contains_key(&self_t.pkg_name) {
                    k.insert(self_t.pkg_name.clone(), HashMap::new());
                }
                if !k[&self_t.pkg_name].contains_key(&lock_name) {
                    k.get_mut(&self_t.pkg_name).unwrap().insert(lock_name.clone(), 0);
                    self_t.lock_vec.insert(lock_name);
                    break;
                }
            }
            let time_struct = core::time::Duration::from_millis(10);
            std::thread::sleep(time_struct);
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["尝试加锁"],|self_t,params|{
        let lock_name = self_t.get_param(params, 0)?;
        // 当前脚本已经创建了这个锁，则不做任何事
        if self_t.lock_vec.contains(&lock_name) {
            return Ok(Some("".to_string()));
        }
        {
            let mut k = crate::G_LOCK.lock()?;
            if !k.contains_key(&self_t.pkg_name) {
                k.insert(self_t.pkg_name.clone(), HashMap::new());
            }
            if !k[&self_t.pkg_name].contains_key(&lock_name) {
                k.get_mut(&self_t.pkg_name).unwrap().insert(lock_name.clone(), 0);
                self_t.lock_vec.insert(lock_name);
                return Ok(Some("".to_string()));
            }
        }
        let ret = self_t.get_param(params, 1)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["解锁"],|self_t,params|{
        let lock_name = self_t.get_param(params, 0)?;
        if self_t.lock_vec.contains(&lock_name) {
            // 当前脚本没有创建这个锁，则不做任何事
            return Ok(Some("".to_string()));
        } else {
            // 否则删除锁
            let mut k = crate::G_LOCK.lock()?;
            k.get_mut(&self_t.pkg_name).unwrap().remove(&lock_name);
            self_t.lock_vec.remove(&lock_name);
            return Ok(Some("".to_string()));
        }
    });
    add_fun(vec!["发送信号"],|self_t,params|{
        let sigal_name = self_t.get_param(params, 0)?; //信号名称
        let mut to_send = self_t.get_param(params, 1)?; //信号名称
        if to_send.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            to_send = get_raw_data(self_t, to_send)?;
        }
        let mut lk = G_SINGAL_ARR.write().unwrap();
        for (_,pkg_name,singal_name_t,data) in  &mut *lk { // 遍历所有信号
            if *pkg_name == self_t.pkg_name && *singal_name_t == sigal_name {
                *data = Some(to_send.to_owned());
            }
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["等待信号"],|self_t,params|{
        let sigal_name = self_t.get_param(params, 0)?; //信号名称
        let tm = self_t.get_param(params, 1)?; 
        let tm = tm.parse::<u64>().unwrap_or(15000); // 超时
        let uid = uuid::Uuid::new_v4().to_string(); // 生成唯一id
        {
            let mut lk_vec = G_SINGAL_ARR.write().unwrap();
            lk_vec.push((uid.to_owned(),self_t.pkg_name.to_owned(),sigal_name,None));
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
            lk_vec.remove(index);
        });
        let mut tm = tm;
        loop {
            {
                let lk = G_SINGAL_ARR.read().unwrap();
                for (uid_t,_,_,data) in  &*lk {
                    if uid == *uid_t && data.is_some() {
                        let dat = data.clone().unwrap();
                        return Ok(Some(dat));
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
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["逻辑选择"],|self_t,params|{
        let loge_arr_str = self_t.get_param(params, 0)?;
        let loge_arr = RedLang::parse_arr(&loge_arr_str)?;
        let mut index = 0;
        for it in loge_arr {
            if it == "真" {
                return Ok(Some(self_t.get_param(params, index + 1)?));
            }
            index += 1;
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["自增"],|self_t,params|{
        let var_name = self_t.get_param(params, 0)?;
        let number_str = self_t.get_param(params, 1)?;
        let number;
        if number_str == "" {
            number = 1;
        } else {
            number = number_str.parse::<i64>()?;
        }
        let var_vec_len = self_t.var_vec.len();
        for i in 0..var_vec_len {
            let mp = &mut self_t.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get_mut(&var_name);
            if let Some(val) = v_opt {
                let mut v = (**val).borrow_mut();
                let v_str: Rc<String> = v.get_string();
                let mut v_num = v_str.parse::<i64>()?;
                v_num += number;
                v.set_string(&mut self_t.bin_pool,v_num.to_string())?;
                break;
            }
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["复制命令"],|self_t,params|{
        let old_cmd = self_t.get_param(params, 0)?;
        let new_cmd = self_t.get_param(params, 1)?;

        let pkg_name = &self_t.pkg_name;
        
        // 如果旧命令不存在，则什么也不做
        let exret = get_core_cmd(&old_cmd, pkg_name);
        if exret == None {
            return Ok(Some("".to_string()));
        }
        // 构造新命令
        let fun = exret.unwrap();
        let k = new_cmd.to_uppercase();
        let k_t: String = crate::mytool::str_to_ft(&k);

        // 添加新命令
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        w.insert(format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc{k}"), fun);
        w.insert(format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc{k_t}"), fun);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["进制转化","进制转换"],|self_t,params|{
        let num_text = self_t.get_param(params, 0)?.to_uppercase();
        let from = self_t.get_param(params, 1)?.parse::<u32>()?;
        let to = self_t.get_param(params, 2)?.parse::<u32>()?;
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
        return Ok(Some(ret));
    });
    add_fun(vec!["当前脚本"],|self_t,_params|{
        Ok(Some(self_t.script_name.to_owned()))
    });
}

impl RedLang {
    pub fn get_exmap(
        &self,
        key: &str,
    ) -> Arc<String>{
        let v = (*self.exmap).borrow();
        let ret = v.get(key);
        if let Some(v) = ret{
            return v.to_owned();
        }
        return Arc::new("".to_string());
    }
    #[allow(dead_code)]
    pub fn set_exmap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
let k = &*self.exmap;
        k.borrow_mut().insert(key.to_owned(), Arc::new(val.to_string()));
        Ok(())
    }
    pub fn get_coremap(
        &mut self,
        key: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {

        let k = format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key);
        let var_ref = self.get_var_ref(&k);
        if let Some(v) = var_ref {
            let mut k = (*v).borrow_mut();
            let val = k.get_string();
            return Ok((*val).clone());
        }else {
            return Ok("".to_string());
        }
    }
    #[allow(dead_code)]
    pub fn set_coremap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let var_vec_len = self.var_vec.len();
        let mp = &mut self.var_vec[var_vec_len - 1];
        let mut var = RedLangVarType::new();
        var.set_string(&mut self.bin_pool,val.to_owned())?;
        mp.insert(format!("{}46631549-6D26-68A5-E192-5EBE9A6EBA61", key), Rc::new(RefCell::new(var)));
        Ok(())
    }
    fn get_len(&self,data:&str) -> Result<usize, Box<dyn std::error::Error>> {
        let tp = self.get_type(&data)?;
        let ret;
        if tp == "数组" {
            let arr_parse_out = RedLang::parse_arr(&data)?;
            ret = arr_parse_out.len();
        } else if tp == "对象" {
            let map_parse_out = RedLang::parse_obj(&data)?;
            ret = map_parse_out.len();
        }else if tp == "文本" {
            let v_chs =data.chars().collect::<Vec<char>>();
            ret = v_chs.len();
        }else if tp == "字节集" {
            let uid = REDLANG_UUID.to_string();
            if data.starts_with(&(uid.clone() + "B96ad849c-8e7e-7886-7742-e4e896cc5b86")) { // 特殊字节集类型
                let raw_bin = RedLang::parse_bin(&self.bin_pool,&data)?;
                ret = raw_bin.len();
            }else{
                let l = (data.len() - 37) / 2;
                ret = l;
            }
        }else{
            return Err(RedLang::make_err(&("对应类型不能获取长度:".to_owned()+&tp)));
        }
        return Ok(ret);
    }
    fn call_fun(&mut self,params: &[String],is_xh:bool) -> Result<String, Box<dyn std::error::Error>> {
        // 获得函数
        let func_t= self.get_param(params, 0)?;

        let tp = self.get_type(&func_t)?;
        let func:String;

        // 尝试通过文本来在常量中获得函数
        if tp == "文本" {
            let err = "无法在常量中找到对应函数";
            func = get_const_val(&self.pkg_name, &func_t)?;
            if func == "" {
                return Err(RedLang::make_err(err));
            }
        }else {
            func = func_t;
        }
        let tp = self.get_type(&func)?;
        if tp != "函数"{
            return Err(RedLang::make_err(&format!("函数调用命令不能对{}类型进行操作",tp)));
        }
        let func = func.get(37..).ok_or("在函数调用命令中获取函数失败")?;

        // 获得函数参数
        let fun_params = &params[1..];
        let mut fun_params_t: Vec<String> = vec![];
        for i in fun_params {
            if is_xh {
                // 来自循环的函数调用参数，无需再次解析
                fun_params_t.push(i.to_string());
            }else{
                let p = self.parse(i)?;
                fun_params_t.push(p);
            }
        }

        // 用于处理参数中的返回
        let fun_ret_vec_len = self.fun_ret_vec.len();
        if self.fun_ret_vec[fun_ret_vec_len - 1].0 == true {
            // 如果参数中已经返回，就收集返回值，然后结束函数调用
            let mut to_ret = String::new();
            for i in fun_params_t {
                to_ret += &i;
            }
            return Ok(to_ret);
        }

        // 修改参数栈
        self.params_vec.push(fun_params_t);

        // 修改变量栈
        self.var_vec.push(std::collections::HashMap::new());

        self.fun_ret_vec.push((false,self.xh_vec.len()));

        // 调用函数
        let ret_str = self.parse(&func)?;

        // 变量栈和参数栈退栈
        self.var_vec.pop();
        self.params_vec.pop();
        self.fun_ret_vec.pop();

        return Ok(ret_str);
    }
    fn do_cmd_fun(
        &mut self,
        cmd: &str,
        params: &[String],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut ret_str: String = String::new();
        let mut is_cmd_ret = false;

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
                    let fun_params = &params[0..];
                    let mut fun_params_t: Vec<String> = vec![];
                    for i in fun_params {
                        if is_cmd2 {
                            let kk = self.parse_fun(i,true)?;
                            fun_params_t.push(kk); // 二类命令不进行参数解析
                        } else {
                            let p = self.parse(i)?;
                            fun_params_t.push(p);
                        }
                    }

                    // 修改参数栈
                    self.params_vec.push(fun_params_t);

                    // 调用命令
                    if is_cmd2 {
                        ret_str = self.parse(&func[36..])?;
                    }else {
                        ret_str = self.parse(&func)?;
                    }
                   
                    // 参数栈退栈
                    self.params_vec.pop();

                    // 指明命令已经执行
                    is_cmd_ret = true;
                }
                _ => {

                }
            }
        }
        if is_cmd_ret {
            return Ok(ret_str);
        }

        // 执行三方命令
        let mut libret: Box<Option<String>> = Box::new(None);
        {
            let mut lib_ptr_opt = None;
            for (_ac,plus) in &*crate::G_LIB_MAP.read().unwrap() {
                if plus.regist_fun.contains(cmd) {
                    lib_ptr_opt = Some(plus.lib.clone());
                    break;
                }
            }
            if let Some(lib_ptr) = lib_ptr_opt {
                
                let call_cmd_fun_rst = unsafe {lib_ptr.get::<Symbol<extern "system" fn(*mut Option<String>,*const c_char,*const c_char,extern "system" fn(*mut Option<String>,*const c_char,c_int))>>(b"redreply_callcmd")};
                if call_cmd_fun_rst.is_ok() {
                    let mut fun_params_t: Vec<String> = vec![];
                    for f in params {
                        let ret = self.parse(f)?;
                        fun_params_t.push(ret);
                    }
                    let bind = fun_params_t.iter().map(|x|x.as_str()).collect();
                    let params_str = self.build_arr(bind);
                    let cmd_cstr = CString::new(cmd)?;
                    let params_cstr = CString::new(params_str)?;
                    extern "system" fn callback(ctx:*mut Option<String>,ret_cstr:*const c_char,retcode:c_int) {
                        let s = unsafe { CStr::from_ptr(ret_cstr) }.to_str().unwrap().to_owned();   
                        if retcode == 0 {
                            let s = unsafe { CStr::from_ptr(ret_cstr) }.to_str().unwrap().to_owned();
                            unsafe {
                                *ctx = Some(s);
                            }
                        } else {
                            unsafe {
                                *ctx = Some("".to_owned());
                            }
                            cq_add_log_w(&format!("err,retcode:{retcode},{s}")).unwrap();
                        }
                    }
                    let call_cmd_fun = call_cmd_fun_rst.unwrap();
                    call_cmd_fun(&mut *libret,cmd_cstr.as_ptr(),params_cstr.as_ptr(),callback);
                   
                }
            }
        }
        if libret.is_some() {
            let ret = (*libret).unwrap();
            return Ok(ret);
        }

        // 执行核心命令与拓展命令
        let exret;
        let rfun = get_core_cmd(cmd,&self.pkg_name);
        
        exret = match rfun {
            Some(fun) => fun(self,params)?,
            None => None,
        };

        if let Some(v) = exret{
            return Ok(v);
        }

        if let Some(v) = exret{
            ret_str = v;
        } else {
            return Err(RedLang::make_err(&format!("未知的命令:{}", cmd)));
        }
        Ok(ret_str)
    }

    pub fn get_type(&self, param_data:&str) -> Result<String, Box<dyn std::error::Error>> {
        let ret_str:String;
        if !param_data.starts_with(&self.type_uuid) {
            ret_str = "文本".to_string();
        }else{
            let tp = param_data.get(36..37).ok_or("类型解析错误,无类型标识")?;
            if tp == "A" {
                ret_str = "数组".to_string();
            }else if tp == "O" {
                ret_str = "对象".to_string();
            }else if tp == "B" {
                ret_str = "字节集".to_string();
            }else if tp == "F" {
                ret_str = "函数".to_string();
            }else {
                return Err(RedLang::make_err(&format!("错误的类型标识:`{}`",tp)));
            }
        }
        Ok(ret_str)
    }
    pub fn parse_img_bin(bin_pool:&mut HashMap<String, RedLangBinPoolVarType>,bin_data: & str) -> Result<(image::ImageFormat,ImageBuffer<Rgba<u8>, Vec<u8>>), Box<dyn std::error::Error>> {
        let err_str = "不能获得字节集类型";
        if !bin_data.starts_with(&crate::REDLANG_UUID.to_string()) {
            return Err(RedLang::make_err(err_str));
        }
        let tp = bin_data.get(36..37).ok_or(err_str)?;
        if tp != "B" {
            return Err(RedLang::make_err(err_str));
        }
        let content_text = bin_data.get(37..).ok_or(err_str)?;
        if content_text.starts_with("96ad849c-8e7e-7886-7742-e4e896cc5b86") { // 图片bin，直接返回
            let bin_pool_key = content_text.get(36..).ok_or(err_str)?;
            let bin_obj = bin_pool.get_mut(bin_pool_key).ok_or(err_str)?;
            if bin_obj.type_t == "img" {
                let mm = bin_obj.dat.downcast_mut::<(image::ImageFormat,ImageBuffer<Rgba<u8>, Vec<u8>>)>().unwrap();
                return Ok(mm.to_owned());
            }
            return Err(RedLang::make_err(err_str));
        } else { // 普通的bin，要转成图片
            let content_text = content_text.as_bytes();
            if content_text.len() % 2 != 0 {
                return Err(RedLang::make_err(err_str));
            }
            let mut content2:Vec<u8> = vec![];
            for pos in 0..(content_text.len() / 2) {
                let mut ch1 = content_text[pos * 2];
                let mut ch2 = content_text[pos * 2 + 1];
                if ch1 < 0x3A {
                    ch1 -= 0x30;
                }else{
                    ch1 -= 0x41;
                    ch1 += 10;
                }
                if ch2 < 0x3A {
                    ch2 -= 0x30;
                }else{
                    ch2 -= 0x41;
                    ch2 += 10;
                }
                content2.push((ch1 << 4) + ch2);
            }
            // 转成图片
            use image::ImageReader;
            let img_t = ImageReader::new(std::io::Cursor::new(content2)).with_guessed_format()?;
            let img_fmt: image::ImageFormat  = img_t.format().ok_or("不能识别的图片格式")?;
            let img = img_t.decode()?.to_rgba8();
            return Ok((img_fmt,img));
        }
    }

    pub fn parse_bin(bin_pool:&HashMap<String, RedLangBinPoolVarType>,bin_data: & str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let err_str = "不能获得字节集类型";
        if !bin_data.starts_with(&crate::REDLANG_UUID.to_string()) {
            return Err(RedLang::make_err(err_str));
        }
        let tp = bin_data.get(36..37).ok_or(err_str)?;
        if tp != "B" {
            return Err(RedLang::make_err(err_str));
        }
        let content_text = bin_data.get(37..).ok_or(err_str)?;
        if content_text.starts_with("96ad849c-8e7e-7886-7742-e4e896cc5b86") { // 图片bin
            let bin_pool_key = content_text.get(36..).ok_or(err_str)?;
            let bin_obj = bin_pool.get(bin_pool_key).ok_or(err_str)?;
            if bin_obj.type_t == "img" {
                let mm = bin_obj.dat.downcast_ref::<(image::ImageFormat,ImageBuffer<Rgba<u8>, Vec<u8>>)>().unwrap();
                let mut bytes: Vec<u8> = Vec::new();
                mm.1.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)?;
                return Ok(bytes);
            }
            return Err(RedLang::make_err(err_str));
        } else { // 普通的bin
            let content_text = content_text.as_bytes();
            if content_text.len() % 2 != 0 {
                return Err(RedLang::make_err(err_str));
            }
            let mut content2:Vec<u8> = vec![];
            for pos in 0..(content_text.len() / 2) {
                let mut ch1 = content_text[pos * 2];
                let mut ch2 = content_text[pos * 2 + 1];
                if ch1 < 0x3A {
                    ch1 -= 0x30;
                }else{
                    ch1 -= 0x41;
                    ch1 += 10;
                }
                if ch2 < 0x3A {
                    ch2 -= 0x30;
                }else{
                    ch2 -= 0x41;
                    ch2 += 10;
                }
                content2.push((ch1 << 4) + ch2);
            }
            return Ok(content2);
        }

    }
    pub fn parse_arr2<'a>(arr_data: &'a str,uuid:&str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
        let err_str = "不能获得数组类型";
        if !arr_data.starts_with(uuid) {
            return Err(RedLang::make_err(err_str));
        }
        let tp = arr_data.get(36..37).ok_or(err_str)?;
        if tp != "A" {
            return Err(RedLang::make_err(err_str));
        }
        let mut ret_arr:Vec<&str> = vec![];
        let mut arr = arr_data.get(37..).ok_or(err_str)?;
        loop {
            let spos_opt = arr.find(",");
            if let None = spos_opt {
                break;
            }
            let spos_num = spos_opt.ok_or(err_str)?;
            let num_opt = arr.get(0..spos_num);
            let num_str = num_opt.ok_or(err_str)?;
            let num = num_str.parse::<usize>()?;
            let str_val = arr.get(spos_num + 1..spos_num + 1 + num).ok_or(err_str)?;
            ret_arr.push(str_val);
            arr = arr.get(spos_num + 1 + num..).ok_or(err_str)?;
        }
        return Ok(ret_arr);
    }
    pub fn parse_arr<'a>(arr_data: &'a str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
        Self::parse_arr2(arr_data,&crate::REDLANG_UUID.to_string())
    }
    pub fn parse_obj(obj_data: &str) -> Result<BTreeMap<String,String>, Box<dyn std::error::Error>> {
        let err_str = "不能获得对象类型";
        if !obj_data.starts_with(&crate::REDLANG_UUID.to_string()) {
            return Err(RedLang::make_err(err_str));
        }
        let tp = obj_data.get(36..37).ok_or(err_str)?;
        if tp != "O" {
            return Err(RedLang::make_err(err_str));
        }
        let mut ret_arr:Vec<&str> = vec![];
        let mut arr = obj_data.get(37..).ok_or(err_str)?;
        loop {
            let spos_opt = arr.find(",");
            if let None = spos_opt {
                break;
            }
            let spos_num = spos_opt.ok_or(err_str)?;
            let num_opt = arr.get(0..spos_num);
            let num_str = num_opt.ok_or(err_str)?;
            let num = num_str.parse::<usize>()?;
            let str_val = arr.get(spos_num + 1..spos_num + 1 + num).ok_or(err_str)?;
            ret_arr.push(str_val);
            arr = arr.get(spos_num + 1 + num..).ok_or(err_str)?;
        }
        if ret_arr.len() % 2 != 0 { 
            return Err(RedLang::make_err(err_str));
        }
        let mut ret_map:BTreeMap<String,String> = BTreeMap::new();
        for i in 0..(ret_arr.len()/2) {
            ret_map.insert(ret_arr[i*2].to_string(), ret_arr[i*2 + 1].to_owned());
        }
        return Ok(ret_map);
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
        let v: Vec<HashMap<String, Rc<RefCell<RedLangVarType>>>> = vec![HashMap::new()];

        // 第一个元素用于全局参数
        let v2: Vec<Vec<String>> = vec![vec![]];

        let v3 = vec![(false,0)];

        // 用于循环控制
        RedLang {
            var_vec: v,
            xh_vec: vec![],
            params_vec: v2,
            fun_ret_vec: v3,
            exmap: Rc::new(RefCell::new(HashMap::new())),
            xuhao:HashMap::new(),
            type_uuid:crate::REDLANG_UUID.to_string(),
            pkg_name:String::new(),
            script_name:String::new(),
            lock_vec:HashSet::new(),
            req_tx:None,
            req_rx:None,
            can_wrong:true,
            stack:VecDeque::new(),
            scriptcallstackdeep: Rc::new(RefCell::new(0)),
            bin_pool:HashMap::new()
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
        params: &[String],
        i: usize,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let s = String::new();
        let p = params.get(i).unwrap_or(&s);
        let ret = self.parse(p);
        return match ret {
            Ok(s) => Ok(s),
            Err(e) => 
            {
                Err(self.make_err_push(e,&("参数解析失败：".to_owned() + p)))
            }
        }
    }

    fn parse_params(&mut self, input: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // 参数数组(字面量)
        let mut params: Vec<String> = vec![];

        let chs = input.chars().collect::<Vec<char>>();

        let mut i = 1usize;

        let mut cq_n = 0usize;
        let mut cq_code: Vec<char> = vec![];

        loop {
            if i >= chs.len() - 1 {
                break;
            }
            let ch = chs[i];
            i += 1;

            if ch == '\\' {
                let c = chs.get(i).ok_or("\\ in the last position of code")?;
                cq_code.push(ch);
                cq_code.push(*c);
                i += 1;
            } else if ch == '【' {
                if params.len() == 0 { 
                    // 命令结束还没有识别到，但已经出现参数，命令强行结束
                    params.push(cq_code.iter().collect::<String>());
                    cq_code.clear();
                }
                if i != chs.len() && chs[i] == '@' {
                    // 处理原始字符串
                    cq_code.push(ch);
                    let mut cn = 1;
                    for it in &chs[i..] {
                        i += 1;
                        cq_code.push(*it);
                        if *it == '【' {
                            cn += 1;
                        } else if *it == '】'{
                            cn -= 1;
                            if cn == 0 {
                                break;
                            }
                        }
                    }
                }else{
                    cq_code.push(ch);
                    cq_n += 1;
                }
            } else if ch == '】' {
                if cq_n == 0 {
                    return Err(RedLang::make_err("too much 】 in code"));
                }
                cq_code.push(ch);
                cq_n -= 1;
            } else if ch == '@' {
                if cq_n != 0 {
                    cq_code.push(ch);
                } else {
                    params.push(cq_code.iter().collect::<String>());
                    cq_code.clear();
                }
            } else {
                cq_code.push(ch);
            }
        }
        if cq_n != 0 {
            return Err(RedLang::make_err("too much 【 in code"));
        }
        params.push(cq_code.iter().collect::<String>());
        Ok(params)
    }

    fn parsecq(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {

        let params = self.parse_params(input)?;

        // 此cmd已经不含cq码
        let cmd = self.parse(&params[0])?;

        let ret = self.do_cmd_fun(cmd.as_str(), &params[1..])?;

        Ok(ret)
    }

    pub fn build_bin(&self,bin:Vec<u8>) ->String {
        return Self::build_bin_with_uid(&self.type_uuid,bin);
    }

    pub fn build_img_bin(&mut self,img:(image::ImageFormat,ImageBuffer<Rgba<u8>, Vec<u8>>)) -> String   {
        let mut ret_str = String::new();
        ret_str.push_str(&self.type_uuid);
        ret_str.push('B');
        ret_str.push_str("96ad849c-8e7e-7886-7742-e4e896cc5b86");
        let img_uuid = uuid::Uuid::new_v4().to_string();
        ret_str.push_str(&img_uuid);
        let bin_pool = &mut self.bin_pool;
        bin_pool.insert(img_uuid, RedLangBinPoolVarType {
            type_t:"img".to_owned(),
            dat:Box::new((img.0,img.1))
        });
        return ret_str;
    }

    pub fn build_bin_with_uid(uid:&str,bin:Vec<u8>) -> String {
        let mut ret_str = String::new();
        ret_str.push_str(uid);
        ret_str.push('B');
        let mut content = String::new();
        for ch in bin {
            content.push_str(&format!("{:02X}",ch));
        }
        ret_str.push_str(&content);
        return ret_str;
    }
    fn build_arr(&self,arr:Vec<&str>) -> String {
        return Self::build_arr_with_uid(&self.type_uuid,arr);
    }
    pub fn build_arr_with_uid(uid:&str,arr:Vec<&str>) -> String {
        let mut ret_str = String::new();
        ret_str.push_str(uid);
        ret_str.push('A');
        for s in arr {
            ret_str.push_str(&s.len().to_string());
            ret_str.push(',');
            ret_str.push_str(&s);
        }
        return ret_str;
    }
    fn build_obj_with_uid(uid:&str,obj:BTreeMap<String,String>) -> String {
        let mut ret_str = String::new();
        ret_str.push_str(uid);
        ret_str.push('O');
        for (k,v) in obj {
            ret_str.push_str(&k.len().to_string());
            ret_str.push(',');
            ret_str.push_str(&k);
            ret_str.push_str(&v.len().to_string());
            ret_str.push(',');
            ret_str.push_str(&v);
        }
        return ret_str;
    }
    pub fn build_obj(&self,obj:BTreeMap<String,String>) -> String {
        return Self::build_obj_with_uid(&self.type_uuid,obj);
    }

    fn conect_arr(bin_pool:&HashMap<String, RedLangBinPoolVarType>,status:&mut u8,chs_out:&mut String,new_str:&str) -> Result<(), Box<dyn std::error::Error>>{
        if new_str.starts_with(&(crate::REDLANG_UUID.to_string() + "A")) {
            if *status == 2 {
                // 这里要进行数组合并，因为之前是数组
                let arr = new_str.get(37..).ok_or("在合并数组时获取新数组失败")?;
                chs_out.push_str(arr);
            } else if *status == 0 { // 之前没有
                chs_out.push_str(&new_str);
            } else { // 之前是其它类型
                return Err(RedLang::make_err(&format!("数组不能与其它类型`{}`直接连接",chs_out)));
            }
            *status = 2;
        } else if new_str.starts_with(&(crate::REDLANG_UUID.to_string() + "O")) {
            if *status == 3 {
                // 这里要进行对象合并，因为之前是对象
                let mut new_obj = RedLang::parse_obj(new_str)?;
                let mut old_obj = RedLang::parse_obj(chs_out)?;
                old_obj.append(&mut new_obj);
                let new_obj_str = RedLang::build_obj_with_uid(&crate::REDLANG_UUID, old_obj);
                chs_out.clear();
                chs_out.push_str(&new_obj_str);
            } else if *status == 0 { // 之前没有
                chs_out.push_str(&new_str);
            } else { // 之前是其它类型
                return Err(RedLang::make_err(&format!("对象不能与其它类型`{}`直接连接",chs_out)));
            }
            *status = 3;
        }else if new_str.starts_with(&(crate::REDLANG_UUID.to_string() + "B")) {
            if *status == 4 {
                // 这里要进行字节集合并，因为之前是字节集
                if chs_out.contains("96ad849c-8e7e-7886-7742-e4e896cc5b86") { // 之前是图片
                    let bin = RedLang::parse_bin(&bin_pool,&chs_out)?;
                    chs_out.clear();
                    chs_out.push_str(&RedLang::build_bin_with_uid(&crate::REDLANG_UUID, bin));
                }
                if new_str.contains("96ad849c-8e7e-7886-7742-e4e896cc5b86") { // 新来的是图片
                    let bin = RedLang::parse_bin(&bin_pool,&new_str)?;
                    let new_s = RedLang::build_bin_with_uid(&crate::REDLANG_UUID, bin);
                    let arr = new_s.get(37..).ok_or("在合并字节集时获取新字节集失败")?;
                    chs_out.push_str(&arr);
                } else { // 新来的不是图片 
                    let arr = new_str.get(37..).ok_or("在合并字节集时获取新字节集失败")?;
                    chs_out.push_str(arr);
                }
            } else if *status == 0 { // 之前没有
                chs_out.push_str(&new_str);
            } else { // 之前是其它类型
                return Err(RedLang::make_err(&format!("字节集不能与其它类型`{}`直接连接",chs_out)));
            }
            *status = 4;
        }
        else if new_str.starts_with(&*crate::CLEAR_UUID) {
            // 处理清空
            chs_out.clear();
            *status = 0;
        } else {
            // 普通文本且长度不为0
            if new_str.len() != 0 {
                if *status == 2 {
                    return Err(RedLang::make_err(&format!("`{}`不能与数组类型直接连接",new_str)));
                }
                if *status == 3 {
                    return Err(RedLang::make_err(&format!("`{}`不能与对象类型直接连接",new_str)));
                }
                if *status == 4 {
                    return Err(RedLang::make_err(&format!("`{}`不能与字节集类型直接连接",new_str)));
                }
                chs_out.push_str(&new_str);
                *status = 1;
            }
        }
        Ok(())
    }

    fn remove_comment(chs:&Vec<char>) -> Result<Vec<char>, Box<dyn std::error::Error>> {
        let mut ret: Vec<char> = vec![];
        let mut i = 0usize;
        let mut mode = 0; // 0:normal 1:comment
        loop {
            if i >= chs.len() {
                break;
            }
            if mode == 0 && chs[i] == '\\' {
                ret.push(chs[i]);
                i += 1;
                ret.push(*chs.get(i).ok_or("\\ in the last position of code")?);

            } else if mode == 0 && chs[i] == '#' &&  i + 1 < chs.len() && chs[i+1] == '#'{
                mode = 1;
            } else if  chs[i] == '\n' || chs[i] == '\r' {
                mode = 0;
                ret.push(chs[i]);
            } else if mode == 0 {
                ret.push(chs[i]);
            }
            i += 1;
        }
        Ok(ret)
    }

    pub fn parse(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {

        let cc = &(*self.scriptcallstackdeep);

        if *cc.borrow() > 200 {
            return Err(RedLang::make_err("too deep call stack"));
        }

        *cc.borrow_mut() += 1;

        let _guard = scopeguard::guard(self.scriptcallstackdeep.clone(), |v| {
            let cc = &(*v);
            *cc.borrow_mut() -= 1;
        });

        // 得到utf8字符数组
        let chs = Self::remove_comment(&input.chars().collect::<Vec<char>>())?;

        // 输出
        let mut chs_out: String = String::new();

        // 用于cq码解析
        let mut cq_code: Vec<char> = vec![];
        let mut cq_n = 0usize;

        // 当前解析位置
        let mut i = 0usize;

        // 当前解析状态 0 normal ， 1 cqmode
        let mut status = 0;

        let mut cur_type_status = 0u8; //0:None 1:text 2:arr 3:object 4:bin

        loop {
            let xh_vec_len = self.xh_vec.len();
            let fun_ret_vec_len = self.fun_ret_vec.len();
            if self.fun_ret_vec[fun_ret_vec_len - 1].0 == true {
                // 跳出当前函数内的所有循环
                for i in self.fun_ret_vec[fun_ret_vec_len - 1].1  .. self.xh_vec.len() {
                    self.xh_vec[i][1] = true;
                }
                // 跳出当前解析
                break;
            }
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

            if i >= chs.len() {
                break;
            }
            let ch = chs[i];
            i += 1;
            if status == 0 {
                if ch == '【' {
                    if i != chs.len() && chs[i] == '@' {
                        let mut cn = 1;
                        // 处理原始字符串
                        for it in &chs[i+1..] {
                            i += 1;
                            if *it == '【' {
                                cn += 1;
                            } else if *it == '】' {
                                cn -= 1;
                                if cn == 0 {
                                    break;
                                }
                            } 
                            chs_out.push(*it);
                        }
                        i += 1;
                        cur_type_status = 1;

                    } else {
                        status = 1;
                        cq_code.clear();
                        cq_code.push('【');
                        cq_n = 1;
                    }
                } else {
                    if ch == '\\' {
                        let c = chs.get(i).ok_or("\\ in the last position of code")?;
                        chs_out.push(*c);
                        cur_type_status = 1;
                        i += 1;
                    } else if self.is_black_char(ch) {
                        // do nothing
                    } else {
                        chs_out.push(ch);
                        cur_type_status = 1;
                    }
                }
            } else if status == 1 {
                if ch == '\\' {
                    let c = chs.get(i).ok_or("\\ in the last position of code")?;
                    cq_code.push('\\');
                    cq_code.push(*c);
                    i += 1;
                } else if ch == '【' {
                    if i != chs.len() && chs[i] == '@' {
                        // 处理原始字符串
                        cq_code.push(ch);
                        let mut cn = 1;
                        for it in &chs[i..] {
                            i += 1;
                            cq_code.push(*it);
                            if *it == '【' {
                                cn += 1;
                            }else if *it == '】' {
                                cn -= 1;
                                if cn == 0 {
                                    break;
                                }
                            }
                        }
                    }else {
                        cq_n += 1;
                        cq_code.push(ch);
                    }
                } else if ch == '】' {
                    cq_n -= 1;
                    cq_code.push(ch);
                } else {
                    cq_code.push(ch);
                }
                if cq_n == 0 {
                    let s = cq_code.iter().collect::<String>();
                    let cqout = self.parsecq(&s)?;
                    RedLang::conect_arr(&self.bin_pool,&mut cur_type_status,&mut chs_out,&cqout)?;
                    cq_code.clear();
                    cq_n = 0;
                    status = 0;
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
    fn get_var_ref(&mut self,var_name:&str) -> Option<Rc<RefCell<RedLangVarType>>> {
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
    fn parse_fun(&mut self, input: &str,is_2_params:bool) -> Result<String, Box<dyn std::error::Error>> {
        // 得到utf8字符数组
        let chs = input.chars().collect::<Vec<char>>();

        // 输出
        let mut chs_out: Vec<char> = vec![];

        // 用于cq码解析
        let mut cq_code: Vec<char> = vec![];
        let mut cq_n = 0usize;

        // 当前解析位置
        let mut i = 0usize;

        // 当前解析状态 0 normal ， 1 cqmode
        let mut status = 0;

        loop {
            if i >= chs.len() {
                break;
            }
            let ch = chs[i];
            i += 1;
            if status == 0 {
                if ch == '【' {
                    status = 1;
                    cq_code.clear();
                    cq_code.push('【');
                    cq_n = 1;
                } else {
                    if ch == '\\' {
                        let c = chs.get(i).ok_or("\\ in the last position of code")?;
                        chs_out.push('\\');
                        chs_out.push(*c);
                        i += 1;
                    }else {
                        chs_out.push(ch);
                    }
                }
            } else if status == 1 {
                if ch == '\\' {
                    let c = chs.get(i).ok_or("\\ in the last position of code")?;
                    cq_code.push('\\');
                    cq_code.push(*c);
                    i += 1;
                } else if ch == '【' {
                    cq_n += 1;
                    cq_code.push(ch);
                } else if ch == '】' {
                    cq_n -= 1;
                    cq_code.push(ch);
                } else {
                    cq_code.push(ch);
                }
                if cq_n == 0 {
                    let s = cq_code.iter().collect::<String>();
                    let params = self.parse_params(&s)?;
                    let cmd = self.parse(&params[0])?;
                    let cmd = crate::mytool::str_to_jt(&cmd);
                    if cmd == "闭包" && is_2_params == false {
                        let cqout = self.get_param(&params, 1)?;
                        let cqout_r = self.parse_r_with_black(&cqout)?;
                        cq_add_log_w(&format!("cqout:{cqout} cqout_r:{cqout_r}")).unwrap();
                        for c in cqout_r.chars() {
                            chs_out.push(c);
                        }
                    } else if is_2_params {
                        if cmd == "二类参数" {
                            let k1 = self.get_param(&params, 1)?.parse::<usize>()?;
                            let ret_str = self.parse(&format!("【二类参数@{k1}】"))?;
                            let cqout_r = self.parse_r_with_black(&ret_str)?;
                            for c in cqout_r.chars() {
                                chs_out.push(c);
                            }
                        } else if cmd == "参数" {
                            let k1 =  self.get_param(&params, 1)?.parse::<usize>()?;
                            
                            let ret_str = self.parse(&format!("【参数@{k1}】"))?;
                            let cqout_r = self.parse_r_with_black(&ret_str)?;
                            for c in cqout_r.chars() {
                                chs_out.push(c);
                            }
                        } else {
                            let mut r_v = vec![];
                            for it in params {
                                if cmd != "函数定义" && cmd != "定义命令" && cmd != "定义二类命令" {
                                    let r = self.parse_fun(&it, is_2_params)?;
                                    r_v.push(r);
                                } else {
                                    let r = self.parse_fun(&it, false)?;
                                    r_v.push(r);
                                }
                            }
                            let rrr = format!("【{}】",r_v.join("@"));
                            for c in rrr.chars() {
                                chs_out.push(c);
                            }
                        }
                    }  
                    else {
                        let mut r_v = vec![];
                        for it in params {
                            let r = self.parse_fun(&it, false)?;
                            r_v.push(r);
                        }
                        let rrr = format!("【{}】",r_v.join("@"));
                        for c in rrr.chars() {
                            chs_out.push(c);
                        }
                    }
                    cq_code.clear();
                    cq_n = 0;
                    status = 0;
                }
            }
        }
        Ok(chs_out.iter().collect::<String>())
    }
}

impl Default for RedLang {
    fn default() -> Self {
        Self::new()
    }
}
