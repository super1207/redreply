use std::{collections::{HashMap, BTreeMap}, fmt, error, vec, rc::Rc, cell::RefCell, any::Any};
use encoding::Encoding;

use crate::{G_CONST_MAP, CLEAR_UUID};
pub mod exfun;
pub(crate) mod cqexfun;

struct RedLangVarType {
    show_str:Rc<String>,
    dat:Box<dyn Any>
}

fn set_const_val(pkg_name:&str,val_name:&str,val:String) -> Result<(), Box<dyn std::error::Error>> {
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
                self.show_str = Rc::new(RedLang::build_arr_with_uid(&crate::REDLANG_UUID.to_string(),dat_ref.to_owned()));
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
    pub fn set_string(&mut self,dat_str:String) -> Result<(), Box<dyn std::error::Error>>{
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
            self.dat = Box::new(RedLang::parse_bin(&dat_str)?);
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
        Err(RedLang::make_err("对象替换元素失败,类型不是对象"))
    }

}

pub struct RedLang {
    var_vec: Vec<HashMap<String,  Rc<RefCell<RedLangVarType>>>>, //变量栈
    xh_vec: Vec<[bool; 2]>,                // 循环控制栈
    params_vec: Vec<Vec<String>>,          // 函数参数栈
    fun_ret_vec: Vec<bool>,                // 记录函数是否返回的栈
    lua : rlua::Lua,
    exmap:Rc<RefCell<HashMap<String, Rc<String>>>>,
    coremap:HashMap<String, String>,
    pub type_uuid:String,
    xuhao: HashMap<String, usize>,
    pkg_name:String,
    pub script_name:String
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

impl RedLang {
    pub fn get_exmap(
        &self,
        key: &str,
    ) -> Rc<String>{
        let v = (*self.exmap).borrow();
        let ret = v.get(key);
        if let Some(v) = ret{
            return v.to_owned();
        }
        return Rc::new("".to_string());
    }
    #[allow(dead_code)]
    pub fn set_exmap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let k = &*self.exmap;
        k.borrow_mut().insert(key.to_owned(), Rc::new(val.to_string()));
        Ok(())
    }
    pub fn get_coremap(
        &self,
        key: &str,
    ) -> Result<&str, Box<dyn std::error::Error>> {
        let ret = self.coremap.get(key);
        if let Some(v) = ret{
            return Ok(v);
        }
        return Ok("");
    }
    #[allow(dead_code)]
    pub fn set_coremap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.coremap.insert(key.to_owned(), val.to_owned());
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
            let l = (data.len() - 37) / 2;
            ret = l;
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

        // 修改参数栈
        self.params_vec.push(fun_params_t);

        // 修改变量栈
        self.var_vec.push(std::collections::HashMap::new());

        self.fun_ret_vec.push(false);

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
            let fun;
            {
                let r = crate::G_CMD_MAP.read()?;
                fun = match r.get(cmd){
                    Some(f) => Some(f.clone()),
                    None =>  None
                }
            }
            match fun {
                Some(fun) => {
                    // 获得命令
                    let func = fun;

                    // 获得命令参数
                    let fun_params = &params[0..];
                    let mut fun_params_t: Vec<String> = vec![];
                    for i in fun_params {
                        let p = self.parse(i)?;
                        fun_params_t.push(p);
                    }

                    // 修改参数栈
                    self.params_vec.push(fun_params_t);

                    // 调用命令
                    ret_str = self.parse(&func)?;

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

        // 执行拓展命令
        let exret;
        {
            let cmd_t = cmd.to_uppercase();
            let r = crate::G_CMD_FUN_MAP.read()?;
            exret = match r.get(&cmd_t) {
                Some(fun) => fun(self,params)?,
                None => None,
            };
        }

        if let Some(v) = exret{
            ret_str = v;
        } else if cmd == "换行" {
            ret_str = String::from("\n");
        } else if cmd == "回车" {
            ret_str = String::from("\r");
        } else if cmd == "空格" {
            ret_str = String::from(" ");
        } else if cmd == "隐藏" {
            let out = self.get_param(params, 0)?;
            self.set_coremap("隐藏", &out)?;
        } else if cmd == "传递" {
            ret_str = self.get_coremap("隐藏")?.to_string();
        } else if cmd == "定义变量" {
            let k = self.get_param(params, 0)?;
            let v = self.get_param(params, 1)?;
            let var_vec_len = self.var_vec.len();
            let mp = &mut self.var_vec[var_vec_len - 1];
            let mut var = RedLangVarType::new();
            var.set_string(v)?;
            mp.insert(k, Rc::new(RefCell::new(var)));
        } else if cmd == "变量" {
            let k = self.get_param(params, 0)?;
            let var_ref = self.get_var_ref(&k);
            if let Some(v) = var_ref {
                let mut k = (*v).borrow_mut();
                ret_str = (*k.get_string()).clone();
            }else {
                ret_str = "".to_string();
            }
        } else if cmd == "赋值变量" {
            let k = self.get_param(params, 0)?;
            let var_vec_len = self.var_vec.len();
            let mut is_set = false;
            let vvv = self.get_param(params, 1)?;
            let vvv_rc =Rc::new(RefCell::new(vvv));
            for i in 0..var_vec_len {
                let mp = &mut self.var_vec[var_vec_len - i - 1];
                let v_opt = mp.get_mut(&k);
                if let Some(val) = v_opt {
                    (**val).borrow_mut().set_string((*vvv_rc).borrow().to_owned())?;
                    is_set = true;
                    break;
                }
            }
            if is_set == false {
                let var_vec_len = self.var_vec.len();
                let mp = &mut self.var_vec[var_vec_len - 1];
                let mut var = RedLangVarType::new();
                var.set_string((*vvv_rc).borrow().to_owned())?;
                mp.insert(k, Rc::new(RefCell::new(var)));
            }
        } else if cmd == "判断" {
            let k1 = self.get_param(params, 0)?;
            let k2 = self.get_param(params, 1)?;
            if k1 != k2 {
                ret_str = self.get_param(params, 2)?;
            } else {
                ret_str = self.get_param(params, 3)?;
            }
        } else if cmd == "判空" {
            let data = self.get_param(params, 0)?;
            let len = self.get_len(&data)?;
            if len == 0 {
                ret_str = self.get_param(params, 1)?
            }else{
                ret_str = data;
            }
        }else if cmd == "循环" {
            let k1 = self.get_param(params, 0)?;
            let k1_tp = self.get_type(&k1)?;
            if k1_tp == "文本"{
                let k1 = self.get_param(params, 0)?;
                let tms = k1.parse::<usize>()?;
                self.xh_vec.push([false, false]);
                let mut last_type = 0u8;
                for _i in 0..tms {
                    let v = self.get_param(params, 1)?;
                    RedLang::conect_arr(&mut last_type,&mut ret_str,v)?;
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }
            else if k1_tp == "数组" {
                let arr_str = k1;
                let fun = params.get(1).ok_or("数组循环中参数函数获取失败")?.to_string();
                let arr = RedLang::parse_arr(&arr_str)?;
                let tms = arr.len();
                self.xh_vec.push([false, false]);
                let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
                fun_params[0] = fun;
                let mut last_type = 0;
                for i in 0..tms {
                    fun_params[1] = i.to_string();
                    fun_params[2] = arr[i].to_owned();
                    let v = self.call_fun(&fun_params,true)?;
                    RedLang::conect_arr(&mut last_type,&mut ret_str,v)?;
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }else if k1_tp == "对象" {
                let obj_str = k1;
                let fun = params.get(1).ok_or("对象循环中参数函数获取失败")?.to_string();
                let obj = RedLang::parse_obj(&obj_str)?;
                self.xh_vec.push([false, false]);
                let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
                fun_params[0] = fun;
                let mut last_type = 0;
                for (k,v) in obj {
                    fun_params[1] = k;
                    fun_params[2] = v;
                    let v = self.call_fun(&fun_params,true)?;
                    RedLang::conect_arr(&mut last_type,&mut ret_str,v)?;
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }
            
        } else if cmd == "判循" {
            self.xh_vec.push([false, false]);
            while self.get_param(params, 0)? == "真" {
                let v = self.get_param(params, 1)?;
                ret_str.push_str(&v);
                if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                    break;
                }
            }
            self.xh_vec.pop();
        } else if cmd == "跳出" {
            // self.xh_vec[self.xh_vec.len() - 1][1] = true;
            let xh_vec_len = self.xh_vec.len();
            self.xh_vec[xh_vec_len - 1][1] = true;
        } else if cmd == "继续" {
            let xh_vec_len = self.xh_vec.len();
            self.xh_vec[xh_vec_len - 1][0] = true;
        } else if cmd == "函数定义" {
            let func = params.get(0).ok_or("函数定义:读取参数失败")?;
            let fun = self.parse_fun(&func)?;
            let func_t = format!("{}F{}",self.type_uuid,fun);
            ret_str = func_t;
        }else if cmd == "定义命令" {
            let func_name = self.get_param(params, 0)?;
            let func = params.get(1).ok_or("定义命令:读取参数失败")?;
            let fun = self.parse_fun(&func)?;
            let mut w = crate::G_CMD_MAP.write()?;
            w.insert(func_name, fun);
        }else if cmd == "函数调用" || cmd == "调用函数" {
            ret_str = self.call_fun(params,false)?;
        } else if cmd == "参数" {
            let k1 = self.get_param(params, 0)?;
            let tms = k1.parse::<usize>()? - 1;
            let params_vec_len = self.params_vec.len();
            ret_str = self.params_vec[params_vec_len - 1]
                .get(tms).unwrap_or(&"".to_string()).to_string();
        } else if cmd == "参数个数" {
            let params_vec_len = self.params_vec.len();
            ret_str = self.params_vec[params_vec_len - 1].len().to_string();
        }else if cmd == "返回" {
            let fun_ret_vec_len = self.fun_ret_vec.len();
            self.fun_ret_vec[fun_ret_vec_len - 1] = true;
        } else if cmd == "计算" {
                let mut k1 = self.get_param(params, 0)?;
                if k1.contains("=") || k1.contains(">") || k1.contains("<") {
                    k1 = k1.replace("!", "~");
                }
                let r = self.lua.context(|lua_ctx| -> Result<String, Box<dyn std::error::Error>> {
                    let v = lua_ctx.load(&k1).eval::<rlua::Value>()?;
                    let ret:String = match v {
                        rlua::Value::Integer(val) => val.to_string(),
                        rlua::Value::Number(val) => val.to_string(),
                        rlua::Value::Boolean(val) => {
                            if val {
                                "真".to_string()
                            }else {
                                "假".to_string()
                            }
                        },
                        _ => "".to_string()
                    };
                    if ret == "" {
                        RedLang::make_err("计算出错");
                    }
                    Ok(ret)
                })?;
                ret_str = r;
        }else if cmd == "数组" {
            let arr_len = params.len();
            let mut temp_ret:Vec<String> = vec![];
            for i in 0..arr_len {
                let s = self.get_param(params, i)?;
                temp_ret.push(s);
            }
            ret_str = self.build_arr(temp_ret);
        }
        else if cmd == "对象" {
            let params_len = params.len();
            if params_len % 2 != 0 {
                return Err(RedLang::make_err("请保证对象参数为偶数个"));
            }
            let mut temp_ret:BTreeMap<String,String> = BTreeMap::new();
            for i in 0..(params_len/2) {
                let k = self.get_param(params, i*2)?;
                let v = self.get_param(params, i*2 + 1)?;
                temp_ret.insert(k, v);
            }
            ret_str = self.build_obj(temp_ret);
        } 
        else if cmd == "取长度" {
            let data = self.get_param(params, 0)?;
            ret_str = self.get_len(&data)?.to_string(); 
        }
        else if cmd == "转文本" {
            let data = self.get_param(params, 0)?;
            let tp = self.get_type(&data)?;
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
                let u8_vec = RedLang::parse_bin(data)?;
                if code == "" || code == "utf8" {
                    ret_str = String::from_utf8(u8_vec)?;
                }else if code == "gbk" {
                    ret_str = encoding::all::GBK.decode(&u8_vec, encoding::DecoderTrap::Ignore)?;
                }else{
                    return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
                }
                Ok(ret_str)
            }
            if tp == "字节集" {
                ret_str = bin_to_text(self,&data,params)?;
            }else if tp == "文本" {
                ret_str = str_to_text(&data)?;
            }else if tp == "数组" {
                ret_str = arr_to_text(self,&data,params)?;
            }else if tp == "对象" {
                ret_str = obj_to_text(self,&data,params)?;
            }
            else{
                return Err(RedLang::make_err(&("对应类型不能转文本:".to_owned()+&tp)));
            }
        }
        else if cmd == "增加元素" {
            // 获得变量
            let var_name = self.get_param(params, 0)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
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
                    let el = self.get_param(params, i + 1)?;
                    let mut v = (*data).borrow_mut();
                    v.add_arr(&el)?;
                }else if tp == "对象" {
                    let elk = self.get_param(params, i * 2 + 1)?;
                    let elv = self.get_param(params, i * 2 + 2)?;

                    let mut v = (*data).borrow_mut();
                    v.add_obj(elk,elv)?;
                    
                }else if tp == "文本" { 
                    let el = self.get_param(params, i + 1)?;
                    let mut v = (*data).borrow_mut();
                    v.add_str(&el)?;

                }else if tp == "字节集" {
                    let el_t = self.get_param(params, i + 1)?;
                    let el = RedLang::parse_bin(&el_t)?;
                    let mut  v = (*data).borrow_mut();
                    v.add_bin(el)?;
                }else{
                    return Err(RedLang::make_err(&("对应类型不能增加元素:".to_owned()+&tp)));
                }
            }
        }else if cmd == "替换元素" {
            // 获得变量
            let var_name = self.get_param(params, 0)?;
            let k_name = self.get_param(params, 1)?;
            let v_name = self.get_param(params, 2)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
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
                let bt = RedLang::parse_bin(&v_name)?;
                if bt.len() != 1 {
                    return Err(RedLang::make_err("替换字节集元素时值的长度不为1"));
                }
                v.rep_bin(index, bt[0])?;
            }else{
                return Err(RedLang::make_err(&("对应类型不能替换元素:".to_owned()+&tp)));
            }
        }else if cmd == "删除元素" {
            // 获得变量
            let var_name = self.get_param(params, 0)?;
            let k_name = self.get_param(params, 1)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
                data = v;
            }else {
                return Err(RedLang::make_err(&format!("变量`{}`不存在",var_name)));
            }
            // 获得变量类型
            let tp =(*data).borrow().get_type();
            if tp == "数组" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_arr(index)?;
            }else if tp == "对象" {
                let mut v = (*data).borrow_mut();
                v.rv_obj(&k_name)?;
                
            }else if tp == "文本" { 
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_str(index)?;

            }else if tp == "字节集" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_bin(index)?;
            }else{
                return Err(RedLang::make_err(&("对应类型不能删除元素:".to_owned()+&tp)));
            }
        }
        else if cmd == "取元素" {
            let nums = params.len();
            let df = String::new();
            let mut param_data = self.get_param(params, 0)?;
            for i in 1..nums {
                let tp = self.get_type(&param_data)?;
                if tp == "数组" {
                    let index = self.get_param(params, i)?.parse::<usize>()?;
                    let mp = RedLang::parse_arr(&param_data)?;
                    let v_opt = mp.get(index);
                    if let Some(v) = v_opt {
                        param_data = v.to_string();
                    }else{
                        param_data = df;
                        break;
                    }
                }else if tp == "对象" { 
                    let index = self.get_param(params, i)?;
                    let mp = RedLang::parse_obj(&param_data)?;
                    let v_opt = mp.get(&index);
                    if let Some(v) = v_opt {
                        param_data = v.to_string();
                    }else{
                        param_data = df;
                        break;
                    }
                }else if tp == "文本" {
                    let index = self.get_param(params, i)?.parse::<usize>()?;
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
            ret_str = param_data;
        }else if cmd.to_lowercase() == "取对象key" {
            let param_data = self.get_param(params, 0)?;
            let tp = self.get_type(&param_data)?;
            if tp != "对象" {
                return Err(RedLang::make_err(&("对应类型不能取对象key:".to_owned()+&tp)));
            }
            let parse_ret = RedLang::parse_obj(&param_data)?;
            let mut arr:Vec<String> = vec![];
            for key in parse_ret.keys() {
                arr.push(key.to_string());
            }
            ret_str = self.build_arr(arr);
        }
        else if cmd == "取类型" {
            let param_data = self.get_param(params, 0)?;
            ret_str = self.get_type(&param_data)?;
        }
        else if cmd == "取随机数" {
            fn get_random() -> Result<usize, getrandom::Error> {
                let mut rand_buf = [0u8; std::mem::size_of::<usize>()];
                getrandom::getrandom(&mut rand_buf)?;
                let mut num = 0usize;
                for i in 0..std::mem::size_of::<usize>() {
                    num += (num << 8) + (rand_buf[i] as usize);
                }
                Ok(num)
            }
            let num1 = self.get_param(params, 0)?.parse::<usize>()?;
            let num2 = self.get_param(params, 1)?.parse::<usize>()?;
            if num1 > num2 {
                return Err(RedLang::make_err("生成随机数失败,请保证第一个数不大于第二个数，且都为非负数"));
            }
            let rand_num = get_random()?;
            let num = num2 + 1 - num1;
            let ret_num = (rand_num %  num) + num1;
            ret_str = ret_num.to_string();
        }else if cmd == "文本替换" {
            let text = self.get_param(params, 0)?;
            let from = self.get_param(params, 1)?;
            let to = self.get_param(params, 2)?;
            ret_str = text.replace(&from, &to);
        }else if cmd == "运行脚本" {
            let mut rl = RedLang::new();
            rl.exmap = self.exmap.clone(); // 获得一些拓展相关的变量
            rl.pkg_name = self.pkg_name.clone();
            rl.script_name = self.script_name.clone();
            let code = self.get_param(params, 0)?;
            // 将参数传入新脚本
            let params_len = params.len();
            for i in 1..params_len {
                rl.params_vec[0].push(self.get_param(params, i)?);
            }
            ret_str = rl.parse(&code)?;
            // 处理清空指令
            if let Some(pos) = ret_str.rfind(CLEAR_UUID.as_str()) {
                ret_str = ret_str.get((pos + 36)..).unwrap().to_owned();
            }
        }else {
            return Err(RedLang::make_err(&format!("未知的命令:{}", cmd)));
        }
        Ok(ret_str)
    }

    fn get_type(&self, param_data:&str) -> Result<String, Box<dyn std::error::Error>> {
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
    fn parse_bin(bin_data: & str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let err_str = "不能获得字节集类型";
        if !bin_data.starts_with(&crate::REDLANG_UUID.to_string()) {
            return Err(RedLang::make_err(err_str));
        }
        let tp = bin_data.get(36..37).ok_or(err_str)?;
        if tp != "B" {
            return Err(RedLang::make_err(err_str));
        }
        let content_text = bin_data.get(37..).ok_or(err_str)?.as_bytes();
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
    fn parse_arr<'a>(arr_data: &'a str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
        let err_str = "不能获得数组类型";
        if !arr_data.starts_with(&crate::REDLANG_UUID.to_string()) {
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
    fn parse_obj(obj_data: &str) -> Result<BTreeMap<String,String>, Box<dyn std::error::Error>> {
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

impl RedLang {
    pub fn new() -> RedLang {

        // 第一个元素用于保持全局变量
        let v: Vec<HashMap<String, Rc<RefCell<RedLangVarType>>>> = vec![HashMap::new()];

        // 第一个元素用于全局参数
        let v2: Vec<Vec<String>> = vec![vec![]];

        let v3 = vec![false];

        // 用于循环控制
        RedLang {
            var_vec: v,
            xh_vec: vec![],
            params_vec: v2,
            fun_ret_vec: v3,
            lua : rlua::Lua::new(),
            exmap: Rc::new(RefCell::new(HashMap::new())),
            coremap: HashMap::new(),
            type_uuid:crate::REDLANG_UUID.to_string(),
            xuhao:HashMap::new(),
            pkg_name:String::new(),
            script_name:String::new()
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
                cq_code.push(ch);
                cq_n += 1;
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

    fn build_bin(&self,bin:Vec<u8>) ->String {
        return Self::build_bin_with_uid(&self.type_uuid,bin);
    }
    fn build_bin_with_uid(uid:&str,bin:Vec<u8>) -> String {
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
    fn build_arr(&self,arr:Vec<String>) -> String {
        return Self::build_arr_with_uid(&self.type_uuid,arr);
    }
    fn build_arr_with_uid(uid:&str,arr:Vec<String>) -> String {
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
    fn build_obj(&self,obj:BTreeMap<String,String>) -> String {
        return Self::build_obj_with_uid(&self.type_uuid,obj);
    }
    fn conect_arr(status:&mut u8,chs_out:&mut String,new_str:String) -> Result<(), Box<dyn std::error::Error>>{
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
        }else {
            if new_str.len() != 0 {
                if *status == 2 {
                    return Err(RedLang::make_err(&format!("`{}`不能与数组类型直接连接",new_str)));
                }
                chs_out.push_str(&new_str);
                *status = 1;
            }
        }
        Ok(())
    }
    pub fn parse(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        // 得到utf8字符数组
        let chs = input.chars().collect::<Vec<char>>();

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
            if self.fun_ret_vec[fun_ret_vec_len - 1] == true {
                if xh_vec_len != 0 {
                    self.xh_vec[xh_vec_len - 1][1] = true;
                }
                break;
            }
            if xh_vec_len != 0 {
                // 说明在循环中
                if self.xh_vec[xh_vec_len - 1][0] == true {
                    self.xh_vec[xh_vec_len - 1][0] = false; // 准备下次循环
                                                            // 这里退出本次循环
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
                    status = 1;
                    cq_code.clear();
                    cq_code.push('【');
                    cq_n = 1;
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
                    let cqout = self.parsecq(&s)?;
                    RedLang::conect_arr(&mut cur_type_status,&mut chs_out,cqout)?;
                    cq_code.clear();
                    cq_n = 0;
                    status = 0;
                }
            }
        }
        Ok(chs_out)
    }

    fn parse_r(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut ret = String::new();
        for i in input.chars() {
            if i == '\\' || i == '@' || i == '【' || i == '】' {
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
    fn parse_fun(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
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
                    } else if self.is_black_char(ch) {
                        // do nothing
                    } else {
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
                    if cmd == "闭包" {
                        let cqout = self.get_param(&params, 1)?;
                        let cqout_r = self.parse_r(&cqout)?;
                        for c in cqout_r.chars() {
                            chs_out.push(c);
                        }
                    } else {
                        for c in s.chars() {
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
