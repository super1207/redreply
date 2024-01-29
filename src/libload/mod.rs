use std::{collections::HashSet, ffi::{c_char, c_int, CStr}, fs, sync::Arc};

use crate::{cqapi::{cq_add_log, cq_get_app_directory1}, redlang::RedLang, LibStruct, G_LIB_AC, G_LIB_MAP};

fn gen_lib_ac() -> c_int {
    let mut lk = G_LIB_AC.lock().unwrap();
    *lk += 1;
    *lk
}


pub fn init_lib() -> Result<(), Box<dyn std::error::Error>> {
    let lib_path = cq_get_app_directory1().unwrap() + "lib";
    std::fs::create_dir_all(&lib_path).unwrap();
    let dirs = fs::read_dir(lib_path)?;
    //let mut ret_vec:Vec<String> = vec![];
    let is_win = std::path::MAIN_SEPARATOR == '\\';
    let platform_end;
    if is_win {
        platform_end = ".dll"
    }else{
        platform_end = ".so";
    }
    for dir in dirs {
        let path = dir?.path();
        if path.is_file() {
            let file_name = path.file_name().ok_or("获取文件名失败")?;
            let file_name_str = file_name.to_string_lossy();
            if !file_name_str.to_lowercase().ends_with(platform_end){
                continue;
            }
            let file_path = path.to_str().ok_or("获取目录文件异常")?.to_owned();
            unsafe {
                let lib = Arc::new(libloading::os::windows::Library::new(path)?);
                // 检查版本号
                let api_version_fun_rst = lib.get::<libloading::os::windows::Symbol<unsafe extern "system" fn(ac:c_int) -> c_int>>(b"redreply_api_version");
                if api_version_fun_rst.is_err() {
                    continue;
                }
                let ac = Box::new(gen_lib_ac());
                let api_version_fun = api_version_fun_rst.unwrap();
                let api_version:c_int = api_version_fun(*ac);
                // 当前只支持版本号为1
                if api_version != 1 {
                    continue;
                }

                //执行到这里，说明插件加载成功，应该保存起来了
                {
                    let mut lk = G_LIB_MAP.write().unwrap();
                    lk.insert(*ac,LibStruct{
                        lib:lib.clone(),
                        path: file_path,
                        regist_fun: HashSet::new(),
                        ac:*ac
                    });
                }
                
                // 注册命令
                let regist_fun_rst = lib.get::<libloading::os::windows::Symbol<unsafe extern "system" fn(*const c_int,callback: extern "system" fn (*const c_int,*const c_char))>>(b"redreply_regist_cmd");
                extern "system" fn callback(ac_ptr:*const c_int,cmdarr:*const c_char) {
                    let ac = unsafe { *ac_ptr };
                    let cmdarr_cstr = unsafe { CStr::from_ptr(cmdarr) };
                    let cmdarr_str_rst = cmdarr_cstr.to_str();
                    if cmdarr_str_rst.is_err() {
                        //println!("1");
                        return;
                    }
                    let cmdarr_str = cmdarr_str_rst.unwrap();
                    let mut lk = G_LIB_MAP.write().unwrap();
                    let plus_opt = lk.get_mut(&ac);
                    if plus_opt.is_none() {
                        //println!("2:ac:{ac}");
                        return;
                    }
                    let plus = plus_opt.unwrap();
                    let cmd_arr_rst = RedLang::parse_arr2(cmdarr_str,"12331549-6D26-68A5-E192-5EBE9A6EB998");
                    if cmd_arr_rst.is_err() {
                        //println!("3");
                        return;
                    }
                    let cmd_arr = cmd_arr_rst.unwrap();
                    cq_add_log(&format!("注入三方命令：{cmd_arr:?}")).unwrap();
                    for cmd in cmd_arr {
                        plus.regist_fun.insert(cmd.to_owned());
                    }
                    
                }
                if regist_fun_rst.is_ok() {
                    let regist_fun = regist_fun_rst.unwrap();
                    regist_fun(&*ac,callback);
                }
            };
        }
    }
    Ok(())
}