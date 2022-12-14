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
                k.ok_or("RedLangVarType:get_string????????????????????????").unwrap();
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
            return "??????";
        }else if self.dat.is::<Vec<char>>() {
            return "??????";
        }else if self.dat.is::<Vec<String>>() {
            return "??????";
        }else if self.dat.is::<BTreeMap<String,String>>() {
            return "??????";
        }else if self.dat.is::<Vec<u8>>() {
            return "?????????";
        }else {
            let k:Option<i32> = None;
            k.ok_or("RedLangVarType:get_type????????????????????????").unwrap();
            return "";
        }
    }
    pub fn add_str(&mut self,s:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            for it in s.chars() {
                v.push(it);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn add_bin(&mut self,s:Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "?????????" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            for it in s {
                v.push(it);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("???????????????????????????,?????????????????????"))
    }
    pub fn add_arr(&mut self,s:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            v.push(s.to_owned());
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn add_obj(&mut self,key:String,val:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.insert(key, val);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rep_obj(&mut self,key:String,val:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.insert(key, val);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rep_arr(&mut self,index:usize,s:String) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            let el = v.get_mut(index).ok_or("???????????????????????????")?;
            (*el) = s;
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rep_bin(&mut self,index:usize,s:u8) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "?????????" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            let el = v.get_mut(index).ok_or("??????????????????????????????")?;
            (*el) = s;
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("???????????????????????????,?????????????????????"))
    }
    pub fn rep_str(&mut self,index:usize,s:char) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            let el = v.get_mut(index).ok_or("???????????????????????????")?;
            (*el) = s.to_owned();
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rv_str(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<char>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rv_bin(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "?????????" {
            let v = self.dat.downcast_mut::<Vec<u8>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("???????????????????????????,?????????????????????"))
    }
    pub fn rv_arr(&mut self,index:usize) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<Vec<String>>().unwrap();
            if index < v.len() {
                v.remove(index);
            }
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }
    pub fn rv_obj(&mut self,key:&str) -> Result<(), Box<dyn std::error::Error>> {
        if self.get_type() == "??????" {
            let v = self.dat.downcast_mut::<BTreeMap<String,String>>().unwrap();
            v.remove(key);
            self.show_str = Rc::new(String::new());
            return Ok(())
        }
        Err(RedLang::make_err("????????????????????????,??????????????????"))
    }

}

pub struct RedLang {
    var_vec: Vec<HashMap<String,  Rc<RefCell<RedLangVarType>>>>, //?????????
    xh_vec: Vec<[bool; 2]>,                // ???????????????
    params_vec: Vec<Vec<String>>,          // ???????????????
    fun_ret_vec: Vec<bool>,                // ??????????????????????????????
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
        if tp == "??????" {
            let arr_parse_out = RedLang::parse_arr(&data)?;
            ret = arr_parse_out.len();
        } else if tp == "??????" {
            let map_parse_out = RedLang::parse_obj(&data)?;
            ret = map_parse_out.len();
        }else if tp == "??????" {
            let v_chs =data.chars().collect::<Vec<char>>();
            ret = v_chs.len();
        }else if tp == "?????????" {
            let l = (data.len() - 37) / 2;
            ret = l;
        }else{
            return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp)));
        }
        return Ok(ret);
    }
    fn call_fun(&mut self,params: &[String],is_xh:bool) -> Result<String, Box<dyn std::error::Error>> {
        // ????????????
        let func_t= self.get_param(params, 0)?;

        let tp = self.get_type(&func_t)?;
        let func:String;

        // ?????????????????????????????????????????????
        if tp == "??????" {
            let err = "????????????????????????????????????";
            func = get_const_val(&self.pkg_name, &func_t)?;
            if func == "" {
                return Err(RedLang::make_err(err));
            }
        }else {
            func = func_t;
        }
        let tp = self.get_type(&func)?;
        if tp != "??????"{
            return Err(RedLang::make_err(&format!("???????????????????????????{}??????????????????",tp)));
        }
        let func = func.get(37..).ok_or("??????????????????????????????????????????")?;

        // ??????????????????
        let fun_params = &params[1..];
        let mut fun_params_t: Vec<String> = vec![];
        for i in fun_params {
            if is_xh {
                // ??????????????????????????????????????????????????????
                fun_params_t.push(i.to_string());
            }else{
                let p = self.parse(i)?;
                fun_params_t.push(p);
            }
        }

        // ???????????????
        self.params_vec.push(fun_params_t);

        // ???????????????
        self.var_vec.push(std::collections::HashMap::new());

        self.fun_ret_vec.push(false);

        // ????????????
        let ret_str = self.parse(&func)?;

        // ???????????????????????????
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

        // ?????????????????????
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
                    // ????????????
                    let func = fun;

                    // ??????????????????
                    let fun_params = &params[0..];
                    let mut fun_params_t: Vec<String> = vec![];
                    for i in fun_params {
                        let p = self.parse(i)?;
                        fun_params_t.push(p);
                    }

                    // ???????????????
                    self.params_vec.push(fun_params_t);

                    // ????????????
                    ret_str = self.parse(&func)?;

                    // ???????????????
                    self.params_vec.pop();

                    // ????????????????????????
                    is_cmd_ret = true;
                }
                _ => {

                }
            }
        }
        if is_cmd_ret {
            return Ok(ret_str);
        }

        // ??????????????????
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
        } else if cmd == "??????" {
            ret_str = String::from("\n");
        } else if cmd == "??????" {
            ret_str = String::from("\r");
        } else if cmd == "??????" {
            ret_str = String::from(" ");
        } else if cmd == "??????" {
            let out = self.get_param(params, 0)?;
            self.set_coremap("??????", &out)?;
        } else if cmd == "??????" {
            ret_str = self.get_coremap("??????")?.to_string();
        } else if cmd == "????????????" {
            let k = self.get_param(params, 0)?;
            let v = self.get_param(params, 1)?;
            let var_vec_len = self.var_vec.len();
            let mp = &mut self.var_vec[var_vec_len - 1];
            let mut var = RedLangVarType::new();
            var.set_string(v)?;
            mp.insert(k, Rc::new(RefCell::new(var)));
        } else if cmd == "??????" {
            let k = self.get_param(params, 0)?;
            let var_ref = self.get_var_ref(&k);
            if let Some(v) = var_ref {
                let mut k = (*v).borrow_mut();
                ret_str = (*k.get_string()).clone();
            }else {
                ret_str = "".to_string();
            }
        } else if cmd == "????????????" {
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
        } else if cmd == "??????" {
            let k1 = self.get_param(params, 0)?;
            let k2 = self.get_param(params, 1)?;
            if k1 != k2 {
                ret_str = self.get_param(params, 2)?;
            } else {
                ret_str = self.get_param(params, 3)?;
            }
        } else if cmd == "??????" {
            let data = self.get_param(params, 0)?;
            let len = self.get_len(&data)?;
            if len == 0 {
                ret_str = self.get_param(params, 1)?
            }else{
                ret_str = data;
            }
        }else if cmd == "??????" {
            let k1 = self.get_param(params, 0)?;
            let k1_tp = self.get_type(&k1)?;
            if k1_tp == "??????"{
                let k1 = self.get_param(params, 0)?;
                let tms = k1.parse::<usize>()?;
                self.xh_vec.push([false, false]);
                for _i in 0..tms {
                    let v = self.get_param(params, 1)?;
                    ret_str.push_str(&v);
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }
            else if k1_tp == "??????" {
                let arr_str = k1;
                let fun = params.get(1).ok_or("???????????????????????????????????????")?.to_string();
                let arr = RedLang::parse_arr(&arr_str)?;
                let tms = arr.len();
                self.xh_vec.push([false, false]);
                let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
                fun_params[0] = fun;
                for i in 0..tms {
                    fun_params[1] = i.to_string();
                    fun_params[2] = arr[i].to_owned();
                    let fun_ret = self.call_fun(&fun_params,true)?;
                    ret_str.push_str(&fun_ret);
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }else if k1_tp == "??????" {
                let obj_str = k1;
                let fun = params.get(1).ok_or("???????????????????????????????????????")?.to_string();
                let obj = RedLang::parse_obj(&obj_str)?;
                self.xh_vec.push([false, false]);
                let mut fun_params:Vec<String> = vec!["".to_string(),"".to_string(),"".to_string()];
                fun_params[0] = fun;
                for (k,v) in obj {
                    fun_params[1] = k;
                    fun_params[2] = v;
                    let fun_ret = self.call_fun(&fun_params,true)?;
                    ret_str.push_str(&fun_ret);
                    if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                        break;
                    }
                }
                self.xh_vec.pop();
            }
            
        } else if cmd == "??????" {
            self.xh_vec.push([false, false]);
            while self.get_param(params, 0)? == "???" {
                let v = self.get_param(params, 1)?;
                ret_str.push_str(&v);
                if self.xh_vec[self.xh_vec.len() - 1][1] == true {
                    break;
                }
            }
            self.xh_vec.pop();
        } else if cmd == "??????" {
            // self.xh_vec[self.xh_vec.len() - 1][1] = true;
            let xh_vec_len = self.xh_vec.len();
            self.xh_vec[xh_vec_len - 1][1] = true;
        } else if cmd == "??????" {
            let xh_vec_len = self.xh_vec.len();
            self.xh_vec[xh_vec_len - 1][0] = true;
        } else if cmd == "????????????" {
            let func = params.get(0).ok_or("????????????:??????????????????")?;
            let fun = self.parse_fun(&func)?;
            let func_t = format!("{}F{}",self.type_uuid,fun);
            ret_str = func_t;
        }else if cmd == "????????????" {
            let func_name = self.get_param(params, 0)?;
            let func = params.get(1).ok_or("????????????:??????????????????")?;
            let fun = self.parse_fun(&func)?;
            let mut w = crate::G_CMD_MAP.write()?;
            w.insert(func_name, fun);
        }else if cmd == "????????????" || cmd == "????????????" {
            ret_str = self.call_fun(params,false)?;
        } else if cmd == "??????" {
            let k1 = self.get_param(params, 0)?;
            let tms = k1.parse::<usize>()? - 1;
            let params_vec_len = self.params_vec.len();
            ret_str = self.params_vec[params_vec_len - 1]
                .get(tms).unwrap_or(&"".to_string()).to_string();
        } else if cmd == "????????????" {
            let params_vec_len = self.params_vec.len();
            ret_str = self.params_vec[params_vec_len - 1].len().to_string();
        }else if cmd == "??????" {
            let fun_ret_vec_len = self.fun_ret_vec.len();
            self.fun_ret_vec[fun_ret_vec_len - 1] = true;
        } else if cmd == "??????" {
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
                                "???".to_string()
                            }else {
                                "???".to_string()
                            }
                        },
                        _ => "".to_string()
                    };
                    if ret == "" {
                        RedLang::make_err("????????????");
                    }
                    Ok(ret)
                })?;
                ret_str = r;
        }else if cmd == "??????" {
            let arr_len = params.len();
            let mut temp_ret:Vec<String> = vec![];
            for i in 0..arr_len {
                let s = self.get_param(params, i)?;
                temp_ret.push(s);
            }
            ret_str = self.build_arr(temp_ret);
        }
        else if cmd == "??????" {
            let params_len = params.len();
            if params_len % 2 != 0 {
                return Err(RedLang::make_err("?????????????????????????????????"));
            }
            let mut temp_ret:BTreeMap<String,String> = BTreeMap::new();
            for i in 0..(params_len/2) {
                let k = self.get_param(params, i*2)?;
                let v = self.get_param(params, i*2 + 1)?;
                temp_ret.insert(k, v);
            }
            ret_str = self.build_obj(temp_ret);
        } 
        else if cmd == "?????????" {
            let data = self.get_param(params, 0)?;
            ret_str = self.get_len(&data)?.to_string(); 
        }
        else if cmd == "?????????" {
            let data = self.get_param(params, 0)?;
            let tp = self.get_type(&data)?;
            fn obj_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
                let mut ret_str = String::new();
                ret_str.push('{');
                let mut vec_t:Vec<String>  = vec![];
                let obj = RedLang::parse_obj(&data)?;
                for (k,v) in obj{
                    let tp_k = self_t.get_type(&k)?;
                    if tp_k != "??????" {
                        return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp_k)));
                    }
                    let mut temp_str = String::new();
                    temp_str.push_str(&str_to_text(&k)?);
                    temp_str.push(':');
                    let tp_v = self_t.get_type(&v)?;
                    if tp_v == "??????" {
                        temp_str.push_str(&str_to_text(&v)?);
                    }
                    else if tp_v == "??????" {
                        temp_str.push_str(&arr_to_text(self_t,&v,params)?);
                    }
                    else if tp_v == "?????????" {
                        temp_str.push_str(&bin_to_text(self_t,&v,params)?);
                    }
                    else if tp_v == "??????" {
                        temp_str.push_str(&obj_to_text(self_t,&v,params)?);
                    }
                    else {
                        return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp_v)));
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
                    if tp_v == "??????" {
                        vec_t.push(str_to_text(&v)?);
                    }
                    else if tp_v == "??????" {
                        vec_t.push(arr_to_text(self_t,&v,params)?);
                    }
                    else if tp_v == "?????????" {
                        vec_t.push(bin_to_text(self_t,&v,params)?);
                    }
                    else if tp_v == "??????" {
                        vec_t.push(obj_to_text(self_t,&v,params)?);
                    }
                    else {
                        return Err(RedLang::make_err(&("?????????????????????????????????:".to_owned()+&tp_v)));
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
                    return Err(RedLang::make_err(&("??????????????????:".to_owned()+&code_t)));
                }
                Ok(ret_str)
            }
            if tp == "?????????" {
                ret_str = bin_to_text(self,&data,params)?;
            }else if tp == "??????" {
                ret_str = str_to_text(&data)?;
            }else if tp == "??????" {
                ret_str = arr_to_text(self,&data,params)?;
            }else if tp == "??????" {
                ret_str = obj_to_text(self,&data,params)?;
            }
            else{
                return Err(RedLang::make_err(&("???????????????????????????:".to_owned()+&tp)));
            }
        }
        else if cmd == "????????????" {
            // ????????????
            let var_name = self.get_param(params, 0)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
                data = v;
            }else {
                return Err(RedLang::make_err(&format!("??????`{}`?????????",var_name)));
            }
            // ??????????????????
            let tp =(*data).borrow().get_type();
            let el_len;
            if tp == "??????" {
                el_len = (params.len() -1) / 2;
            }else {
                el_len = params.len() -1;
            }
            //  ????????????
            for i in 0..el_len {
                if tp == "??????" {
                    let el = self.get_param(params, i + 1)?;
                    let mut v = (*data).borrow_mut();
                    v.add_arr(&el)?;
                }else if tp == "??????" {
                    let elk = self.get_param(params, i * 2 + 1)?;
                    let elv = self.get_param(params, i * 2 + 2)?;

                    let mut v = (*data).borrow_mut();
                    v.add_obj(elk,elv)?;
                    
                }else if tp == "??????" { 
                    let el = self.get_param(params, i + 1)?;
                    let mut v = (*data).borrow_mut();
                    v.add_str(&el)?;

                }else if tp == "?????????" {
                    let el_t = self.get_param(params, i + 1)?;
                    let el = RedLang::parse_bin(&el_t)?;
                    let mut  v = (*data).borrow_mut();
                    v.add_bin(el)?;
                }else{
                    return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp)));
                }
            }
        }else if cmd == "????????????" {
            // ????????????
            let var_name = self.get_param(params, 0)?;
            let k_name = self.get_param(params, 1)?;
            let v_name = self.get_param(params, 2)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
                data = v;
            }else {
                return Err(RedLang::make_err(&format!("??????`{}`?????????",var_name)));
            }
            // ??????????????????
            let tp =(*data).borrow().get_type();
            if tp == "??????" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rep_arr(index, v_name)?;
            }else if tp == "??????" {
                let mut v = (*data).borrow_mut();
                v.rep_obj(k_name, v_name)?;
                
            }else if tp == "??????" { 
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                let v_chs = v_name.chars().collect::<Vec<char>>();
                if v_chs.len() != 1 {
                    return Err(RedLang::make_err("???????????????????????????????????????1"));
                }
                v.rep_str(index, v_chs[0])?;

            }else if tp == "?????????" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                let bt = RedLang::parse_bin(&v_name)?;
                if bt.len() != 1 {
                    return Err(RedLang::make_err("??????????????????????????????????????????1"));
                }
                v.rep_bin(index, bt[0])?;
            }else{
                return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp)));
            }
        }else if cmd == "????????????" {
            // ????????????
            let var_name = self.get_param(params, 0)?;
            let k_name = self.get_param(params, 1)?;
            let data:Rc<RefCell<RedLangVarType>>;
            if let Some(v) = self.get_var_ref(&var_name) {
                data = v;
            }else {
                return Err(RedLang::make_err(&format!("??????`{}`?????????",var_name)));
            }
            // ??????????????????
            let tp =(*data).borrow().get_type();
            if tp == "??????" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_arr(index)?;
            }else if tp == "??????" {
                let mut v = (*data).borrow_mut();
                v.rv_obj(&k_name)?;
                
            }else if tp == "??????" { 
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_str(index)?;

            }else if tp == "?????????" {
                let index = k_name.parse::<usize>()?;
                let mut v = (*data).borrow_mut();
                v.rv_bin(index)?;
            }else{
                return Err(RedLang::make_err(&("??????????????????????????????:".to_owned()+&tp)));
            }
        }
        else if cmd == "?????????" {
            let nums = params.len();
            let df = String::new();
            let mut param_data = self.get_param(params, 0)?;
            for i in 1..nums {
                let tp = self.get_type(&param_data)?;
                if tp == "??????" {
                    let index = self.get_param(params, i)?.parse::<usize>()?;
                    let mp = RedLang::parse_arr(&param_data)?;
                    let v_opt = mp.get(index);
                    if let Some(v) = v_opt {
                        param_data = v.to_string();
                    }else{
                        param_data = df;
                        break;
                    }
                }else if tp == "??????" { 
                    let index = self.get_param(params, i)?;
                    let mp = RedLang::parse_obj(&param_data)?;
                    let v_opt = mp.get(&index);
                    if let Some(v) = v_opt {
                        param_data = v.to_string();
                    }else{
                        param_data = df;
                        break;
                    }
                }else if tp == "??????" {
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
                    return Err(RedLang::make_err(&("???????????????????????????:".to_owned()+&tp)));
                }
            }
            ret_str = param_data;
        }else if cmd.to_lowercase() == "?????????key" {
            let param_data = self.get_param(params, 0)?;
            let tp = self.get_type(&param_data)?;
            if tp != "??????" {
                return Err(RedLang::make_err(&("???????????????????????????key:".to_owned()+&tp)));
            }
            let parse_ret = RedLang::parse_obj(&param_data)?;
            let mut arr:Vec<String> = vec![];
            for key in parse_ret.keys() {
                arr.push(key.to_string());
            }
            ret_str = self.build_arr(arr);
        }
        else if cmd == "?????????" {
            let param_data = self.get_param(params, 0)?;
            ret_str = self.get_type(&param_data)?;
        }
        else if cmd == "????????????" {
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
                return Err(RedLang::make_err("?????????????????????,???????????????????????????????????????????????????????????????"));
            }
            let rand_num = get_random()?;
            let num = num2 + 1 - num1;
            let ret_num = (rand_num %  num) + num1;
            ret_str = ret_num.to_string();
        }else if cmd == "????????????" {
            let text = self.get_param(params, 0)?;
            let from = self.get_param(params, 1)?;
            let to = self.get_param(params, 2)?;
            ret_str = text.replace(&from, &to);
        }else if cmd == "????????????" {
            let mut rl = RedLang::new();
            rl.exmap = self.exmap.clone(); // ?????????????????????????????????
            rl.pkg_name = self.pkg_name.clone();
            rl.script_name = self.script_name.clone();
            let code = self.get_param(params, 0)?;
            ret_str = rl.parse(&code)?;
            // ??????????????????
            if let Some(pos) = ret_str.rfind(CLEAR_UUID.as_str()) {
                ret_str = ret_str.get((pos + 36)..).unwrap().to_owned();
            }
        }else {
            return Err(RedLang::make_err(&format!("???????????????:{}", cmd)));
        }
        Ok(ret_str)
    }

    fn get_type(&self, param_data:&str) -> Result<String, Box<dyn std::error::Error>> {
        let ret_str:String;
        if !param_data.starts_with(&self.type_uuid) {
            ret_str = "??????".to_string();
        }else{
            let tp = param_data.get(36..37).ok_or("??????????????????,???????????????")?;
            if tp == "A" {
                ret_str = "??????".to_string();
            }else if tp == "O" {
                ret_str = "??????".to_string();
            }else if tp == "B" {
                ret_str = "?????????".to_string();
            }else if tp == "F" {
                ret_str = "??????".to_string();
            }else {
                return Err(RedLang::make_err(&format!("?????????????????????:`{}`",tp)));
            }
        }
        Ok(ret_str)
    }
    fn parse_bin(bin_data: & str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let err_str = "???????????????????????????";
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
        let err_str = "????????????????????????";
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
        let err_str = "????????????????????????";
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

        // ???????????????????????????????????????
        let v: Vec<HashMap<String, Rc<RefCell<RedLangVarType>>>> = vec![HashMap::new()];

        // ?????????????????????????????????
        let v2: Vec<Vec<String>> = vec![vec![]];

        let v3 = vec![false];

        // ??????????????????
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
                Err(self.make_err_push(e,&("?????????????????????".to_owned() + p)))
            }
        }
    }

    fn parse_params(&mut self, input: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // ????????????(?????????)
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
            } else if ch == '???' {
                if params.len() == 0 { 
                    // ???????????????????????????????????????????????????????????????????????????
                    params.push(cq_code.iter().collect::<String>());
                    cq_code.clear();
                }
                cq_code.push(ch);
                cq_n += 1;
            } else if ch == '???' {
                if cq_n == 0 {
                    return Err(RedLang::make_err("too much ??? in code"));
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
            return Err(RedLang::make_err("too much ??? in code"));
        }
        params.push(cq_code.iter().collect::<String>());
        Ok(params)
    }

    fn parsecq(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let params = self.parse_params(input)?;

        // ???cmd????????????cq???
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

    pub fn parse(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        // ??????utf8????????????
        let chs = input.chars().collect::<Vec<char>>();

        // ??????
        let mut chs_out: Vec<char> = vec![];

        // ??????cq?????????
        let mut cq_code: Vec<char> = vec![];
        let mut cq_n = 0usize;

        // ??????????????????
        let mut i = 0usize;

        // ?????????????????? 0 normal ??? 1 cqmode
        let mut status = 0;

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
                // ??????????????????
                if self.xh_vec[xh_vec_len - 1][0] == true {
                    self.xh_vec[xh_vec_len - 1][0] = false; // ??????????????????
                                                            // ????????????????????????
                    break;
                }
                if self.xh_vec[xh_vec_len - 1][1] == true {
                    // ?????????????????????
                    // ????????????????????????
                    break;
                }
            }

            if i >= chs.len() {
                break;
            }
            let ch = chs[i];
            i += 1;
            if status == 0 {
                if ch == '???' {
                    status = 1;
                    cq_code.clear();
                    cq_code.push('???');
                    cq_n = 1;
                } else {
                    if ch == '\\' {
                        let c = chs.get(i).ok_or("\\ in the last position of code")?;
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
                } else if ch == '???' {
                    cq_n += 1;
                    cq_code.push(ch);
                } else if ch == '???' {
                    cq_n -= 1;
                    cq_code.push(ch);
                } else {
                    cq_code.push(ch);
                }
                if cq_n == 0 {
                    let s = cq_code.iter().collect::<String>();
                    let cqout = self.parsecq(&s)?;
                    for c in cqout.chars() {
                        chs_out.push(c);
                    }
                    cq_code.clear();
                    cq_n = 0;
                    status = 0;
                }
            }
        }
        Ok(chs_out.iter().collect::<String>())
    }

    fn parse_r(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut ret = String::new();
        for i in input.chars() {
            if i == '\\' || i == '@' || i == '???' || i == '???' {
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
        // ??????utf8????????????
        let chs = input.chars().collect::<Vec<char>>();

        // ??????
        let mut chs_out: Vec<char> = vec![];

        // ??????cq?????????
        let mut cq_code: Vec<char> = vec![];
        let mut cq_n = 0usize;

        // ??????????????????
        let mut i = 0usize;

        // ?????????????????? 0 normal ??? 1 cqmode
        let mut status = 0;

        loop {
            if i >= chs.len() {
                break;
            }
            let ch = chs[i];
            i += 1;
            if status == 0 {
                if ch == '???' {
                    status = 1;
                    cq_code.clear();
                    cq_code.push('???');
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
                } else if ch == '???' {
                    cq_n += 1;
                    cq_code.push(ch);
                } else if ch == '???' {
                    cq_n -= 1;
                    cq_code.push(ch);
                } else {
                    cq_code.push(ch);
                }
                if cq_n == 0 {
                    let s = cq_code.iter().collect::<String>();
                    let params = self.parse_params(&s)?;
                    let cmd = self.parse(&params[0])?;
                    if cmd == "??????" {
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
