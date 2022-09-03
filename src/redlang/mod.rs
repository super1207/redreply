use std::{collections::HashMap, fmt, error};
use encoding::Encoding;
extern crate rand;
extern crate hlua;
use hlua::Lua;

pub mod exfun;
mod cqexfun;
use crate::redlang::exfun::exfun;


pub struct RedLang<'a> {
    var_vec: Vec<HashMap<String, String>>, //变量栈
    xh_vec: Vec<[bool; 2]>,                // 循环控制栈
    params_vec: Vec<Vec<String>>,          // 函数参数栈
    fun_ret_vec: Vec<bool>,                // 记录函数是否返回的栈
    lua : Lua<'a>,
    exmap:HashMap<String, String>,
    pub type_uuid:String,
    xuhao: usize
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

impl RedLang<'_> {
    pub fn get_exmap(
        &self,
        key: &str,
    ) -> Result<&str, Box<dyn std::error::Error>> {
        let ret = self.exmap.get(key);
        if let Some(v) = ret{
            return Ok(v);
        }
        return Ok("");
    }
    #[allow(dead_code)]
    pub fn set_exmap(
        &mut self,
        key: &str,
        val: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.exmap.insert(key.to_owned(), val.to_owned());
        Ok(())
    }
    fn do_cmd_fun(
        &mut self,
        cmd: &str,
        params: &[String],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut ret_str: String = String::new();
        let exret = exfun(self,cmd, params)?;
        if let Some(v) = exret{
            ret_str = v;
        } else if cmd == "换行" {
            ret_str = String::from("\n");
        } else if cmd == "空格" {
            ret_str = String::from(" ");
        } else if cmd == "隐藏" {
            let out = self.get_param(params, 0)?;
            self.set_exmap("隐藏", &out)?;
        } else if cmd == "传递" {
            ret_str = self.get_exmap("隐藏")?.to_string();
        } else if cmd == "定义变量" {
            let k = self.get_param(params, 0)?;
            let v = self.get_param(params, 1)?;
            let var_vec_len = self.var_vec.len();
            let mp = &mut self.var_vec[var_vec_len - 1];
            mp.insert(k, v);
        } else if cmd == "变量" {
            let k = self.get_param(params, 0)?;
            let var_ref = self.get_var_ref(&k);
            if let Some(v) = var_ref {
                ret_str = v.to_string();
            }else {
                return Err(self.make_err(&format!("变量`{}`不存在",k)));
            }
        } else if cmd == "赋值变量" {
            let k = self.get_param(params, 0)?;
            let var_vec_len = self.var_vec.len();
            let mut is_set = false;
            let vvv = self.get_param(params, 1)?;
            for i in 0..var_vec_len {
                let mp = &mut self.var_vec[var_vec_len - i - 1];
                let v_opt = mp.get_mut(&k);
                if let Some(val) = v_opt {
                    val.clone_from(&vvv);
                    is_set = true;
                    break;
                }
            }
            if is_set == false {
                let var_vec_len = self.var_vec.len();
                let mp = &mut self.var_vec[var_vec_len - 1];
                mp.insert(k, vvv);
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
            let k1 = self.get_param(params, 0)?;
            if k1 == "" {
                ret_str = self.get_param(params, 1)?
            }else{
                ret_str = k1;
            }
        }else if cmd == "循环" {
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
        } else if cmd == "函数调用" {
            // 获得函数
            let func = self.get_param(params, 0)?;

            let tp = self.get_type(&func)?;
            if tp != "函数"{
                return Err(self.make_err(&format!("函数调用命令不能对{}类型进行操作",tp)));
            }
            let func = func.get(37..).ok_or("在函数调用命令中获取函数失败")?;

            // 获得函数参数
            let fun_params = &params[1..];
            let mut fun_params_t: Vec<String> = vec![];
            for i in fun_params {
                let p = self.parse(i)?;
                fun_params_t.push(p);
            }

            // 修改参数栈
            self.params_vec.push(fun_params_t);

            // 修改变量栈
            self.var_vec.push(std::collections::HashMap::new());

            self.fun_ret_vec.push(false);

            // 调用函数
            ret_str = self.parse(&func)?;

            // 变量栈和参数栈退栈
            self.var_vec.pop();
            self.params_vec.pop();
            self.fun_ret_vec.pop();
        } else if cmd == "参数" {
            let k1 = self.get_param(params, 0)?;
            let tms = k1.parse::<usize>()? - 1;
            let params_vec_len = self.params_vec.len();
            ret_str = self.params_vec[params_vec_len - 1]
                .get(tms)
                .ok_or("获取函数参数失败，越界")?
                .to_string();
        } else if cmd == "返回" {
            let fun_ret_vec_len = self.fun_ret_vec.len();
            self.fun_ret_vec[fun_ret_vec_len - 1] = true;
        } else if cmd == "计算" {
            let mut k1 = self.get_param(params, 0)?;
            // format!("x = ({}) return ({})",k1)
            if k1.contains("=") || k1.contains(">") || k1.contains("<") {
                k1 = k1.replace("!", "~");
                let ret: bool = match self.lua.execute(&format!("return ({})", k1)) {
                    Ok(it) => it,
                    Err(_) => return Err(self.make_err("计算失败")),
                };
                if ret {
                    ret_str = "真".to_string();
                } else {
                    ret_str = "假".to_string();
                }
            } else {
                let ret: String = match self.lua.execute(&format!("return ({})", k1)) {
                    Ok(it) => it,
                    Err(_) => return Err(self.make_err("计算失败")),
                };
                ret_str = ret;
            }
        }else if cmd == "数组" {
            let arr_len = params.len();
            let mut temp_ret = String::new();
            temp_ret.push_str(&self.type_uuid);
            temp_ret.push('A');
            for i in 0..arr_len {
                let s = self.get_param(params, i)?;
                let s_len_str = s.len().to_string();
                temp_ret.push_str(&s_len_str);
                temp_ret.push(',');
                temp_ret.push_str(&s);
            }
            ret_str = temp_ret;
        }
        else if cmd == "对象" {
            let params_len = params.len();
            if params_len % 2 != 0 {
                return Err(self.make_err("请保证对象参数为偶数个"));
            }
            let mut temp_ret = String::new();
            temp_ret.push_str(&self.type_uuid);
            temp_ret.push('O');
            for i in 0..(params_len/2) {
                let k = self.get_param(params, i*2)?;
                let v = self.get_param(params, i*2 + 1)?;
                temp_ret.push_str(&k.len().to_string());
                temp_ret.push(',');
                temp_ret.push_str(&k);
                temp_ret.push_str(&v.len().to_string());
                temp_ret.push(',');
                temp_ret.push_str(&v);
            }
            ret_str = temp_ret;
        } 
        else if cmd == "取长度" {
            let data = self.get_param(params, 0)?;
            let tp = self.get_type(&data)?;
            if tp == "数组" {
                let arr_parse_out = self.parse_arr(&data)?;
                ret_str = arr_parse_out.len().to_string();
            } else if tp == "对象" {
                let map_parse_out = self.parse_obj(&data)?;
                ret_str = map_parse_out.len().to_string();
            }else if tp == "文本" {
                let v_chs =data.chars().collect::<Vec<char>>();
                ret_str = v_chs.len().to_string();
            }else{
                return Err(self.make_err(&("对应类型不能获取长度:".to_owned()+&tp)));
            }
        }else if cmd == "转文本" {
            let data = self.get_param(params, 0)?;
            let tp = self.get_type(&data)?;
            fn obj_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
                let mut ret_str = String::new();
                ret_str.push('{');
                let mut vec_t:Vec<String>  = vec![];
                let obj = self_t.parse_obj(&data)?;
                for (k,v) in obj{
                    let tp_k = self_t.get_type(&k)?;
                    if tp_k != "文本" {
                        return Err(self_t.make_err(&("对象的键不支持的类型:".to_owned()+&tp_k)));
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
                        return Err(self_t.make_err(&("对象的值不支持的类型:".to_owned()+&tp_v)));
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
                let arr = self_t.parse_arr(&data)?;
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
                        return Err(self_t.make_err(&("数组的元素不支持的类型:".to_owned()+&tp_v)));
                    }
                }
                return Ok(format!("[{}]",vec_t.join(",")));
            }

            fn bin_to_text(self_t:&mut RedLang,data:& str,params:&[String]) -> Result<String, Box<dyn std::error::Error>>{
                let ret_str:String;
                let code_t = self_t.get_param(params, 1)?;
                let code = code_t.to_lowercase();
                let b64_str = data.get(37..).ok_or("获取字节集失败")?;
                let u8_vec = base64::decode(b64_str)?;
                if code == "" || code == "utf8" {
                    ret_str = String::from_utf8(u8_vec)?;
                }else if code == "gbk" {
                    ret_str = encoding::all::GBK.decode(&u8_vec, encoding::DecoderTrap::Ignore)?;
                }else{
                    return Err(self_t.make_err(&("不支持的编码:".to_owned()+&code_t)));
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
                return Err(self.make_err(&("对应类型不能转文本:".to_owned()+&tp)));
            }
        }
        else if cmd == "增加元素" {
            let var_name = self.get_param(params, 0)?;
            let el = self.get_param(params, 1)?;
            let tp:String;
            let data:&mut String;
            if let Some(v) = self.get_var_ref(&var_name) {
                let b = v as *mut String;
                unsafe{
                    data = &mut (*b);
                }
                
            }else {
                return Err(self.make_err(&format!("变量`{}`不存在",var_name)));
            }
            tp = self.get_type(data)?; 
            if tp == "数组" {
                data.push_str(&el.len().to_string());
                data.push(',');
                data.push_str(&el);
            }else if tp == "对象" {
                data.push_str(&el.len().to_string());
                data.push(',');
                data.push_str(&el);

                let v = self.get_param(params, 2)?;
                data.push_str(&v.len().to_string());
                data.push(',');
                data.push_str(&v);
            }else if tp == "文本" { 
                data.push_str(&el);
            }else{
                return Err(self.make_err(&("对应类型不能增加元素:".to_owned()+&tp)));
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
                    let mp = self.parse_arr(&param_data)?;
                    let v_opt = mp.get(index);
                    if let Some(v) = v_opt {
                        param_data = v.to_string();
                    }else{
                        param_data = df;
                        break;
                    }
                }else if tp == "对象" { 
                    let index = self.get_param(params, i)?;
                    let mp = self.parse_obj(&param_data)?;
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
                    return Err(self.make_err(&("对应类型不能取元素:".to_owned()+&tp)));
                }
            }
            ret_str = param_data;
        }else if cmd.to_lowercase() == "取对象key" {
            let param_data = self.get_param(params, 0)?;
            let tp = self.get_type(&param_data)?;
            if tp != "对象" {
                return Err(self.make_err(&("对应类型不能取对象key:".to_owned()+&tp)));
            }
            let parse_ret = self.parse_obj(&param_data)?;
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
                return Err(self.make_err("生成随机数失败,请保证第一个数不大于第二个数，且都为非负数"));
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
        }else {
            return Err(self.make_err(&format!("未知的命令:{}", cmd)));
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
                return Err(self.make_err(&format!("错误的类型标识:`{}`",tp)));
            }
        }
        Ok(ret_str)
    }
    
    fn parse_arr<'a>(&self, arr_data: &'a str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
        let err_str = "不能获得数组类型";
        if !arr_data.starts_with(&self.type_uuid) {
            return Err(self.make_err(err_str));
        }
        let tp = arr_data.get(36..37).ok_or(err_str)?;
        if tp != "A" {
            return Err(self.make_err(err_str));
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
    fn parse_obj<'a>(&self, obj_data: &'a str) -> Result<HashMap<String,&'a str>, Box<dyn std::error::Error>> {
        let err_str = "不能获得对象类型";
        if !obj_data.starts_with(&self.type_uuid) {
            return Err(self.make_err(err_str));
        }
        let tp = obj_data.get(36..37).ok_or(err_str)?;
        if tp != "O" {
            return Err(self.make_err(err_str));
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
            return Err(self.make_err(err_str));
        }
        let mut ret_map:HashMap<String,&str> = HashMap::new();
        for i in 0..(ret_arr.len()/2) {
            ret_map.insert(ret_arr[i*2].to_string(), ret_arr[i*2 + 1]);
        }
        return Ok(ret_map);
    }

}

impl RedLang<'_> {
    pub fn new() -> RedLang<'static> {
        // 第一个元素用于保持全局变量
        let v: Vec<HashMap<String, String>> = vec![HashMap::new()];

        // 第一个元素用于全局参数
        let v2: Vec<Vec<String>> = vec![vec![]];

        let v3 = vec![false];

        // 用于循环控制
        RedLang {
            var_vec: v,
            xh_vec: vec![],
            params_vec: v2,
            fun_ret_vec: v3,
            lua:Lua::new(),
            exmap: HashMap::new(),
            type_uuid:crate::REDLANG_UUID.to_string(),
            xuhao:0usize
        }
    }

    fn make_err(&self, err_str: &str) -> Box<dyn std::error::Error> {
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
                Err(self.make_err_push(e,"参数解析失败"))
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
                    return Err(self.make_err("too much 】 in code"));
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
            return Err(self.make_err("too much 【 in code"));
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

    fn build_arr(&self,arr:Vec<String>) -> String {
        let mut ret_str = String::new();
        ret_str.push_str(&self.type_uuid);
        ret_str.push('A');
        for s in arr {
            ret_str.push_str(&s.len().to_string());
            ret_str.push(',');
            ret_str.push_str(&s);
        }
        return ret_str;
    }
    fn build_obj(&self,obj:HashMap<String,String>) -> String {
        let mut ret_str = String::new();
        ret_str.push_str(&self.type_uuid);
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

    pub fn parse(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {
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
            if i == '\\' || i == '@' || i == '【' || i == '】' {
                ret.push('\\');
            }
            ret.push(i);
        }
        Ok(ret)
    }
    fn get_var_ref(&mut self,var_name:&str) -> Option<&mut String> {
        let var_vec_len = self.var_vec.len();
        for i in 0..var_vec_len {
            let mp = & mut self.var_vec[var_vec_len - i - 1];
            let v_opt = mp.get_mut(var_name);
            if let Some(v) = v_opt {
                let p =  v as *mut String;
                unsafe{
                    return Some(&mut *p);
                };
                
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
