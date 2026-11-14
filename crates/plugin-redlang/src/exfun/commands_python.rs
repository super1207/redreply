use super::*;

pub(super) fn register() {
    add_fun(vec!["运行PY"], |self_t, params| {
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint



def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode('utf-8')
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode('utf-8')).decode('utf-8')
    red_print(en)
"#;
        let code1 = self_t.get_param_text(params, 0)?;
        let input_value = self_t.get_param(params, 1)?;
        let input = red_to_python_wire_string(&input_value)?;
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;

        let tmp_dir = get_tmp_dir().map_err(|x| format!("get_tmp_dir err:{:?}", x))?;
        let python_id = crate::python_env::get_local_python_uid()?;

        // 末尾有分隔符
        let python_dir = format!(
            "{}pymain_{}_{}{}",
            tmp_dir,
            self_t.pkg_name,
            python_id,
            std::path::MAIN_SEPARATOR
        );

        fs::create_dir_all(&python_dir)?;

        let python_env_is_create = Path::new(&python_dir).join("redpymainok").is_file();

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        if !python_env_is_create {
            #[cfg(windows)]
            let python_env_status = std::process::Command::new(get_python_cmd_name()?)
                .creation_flags(0x08000000)
                .current_dir(python_dir.clone())
                .arg("-m")
                .arg("venv")
                .arg("pymain")
                .status();

            #[cfg(not(windows))]
            let python_env_status = std::process::Command::new(get_python_cmd_name()?)
                .current_dir(python_dir.clone())
                .arg("-m")
                .arg("venv")
                .arg("pymain")
                .status();

            match python_env_status {
                Err(err) => {
                    return Err(RedLang::make_err(&format!("python环境创建失败:{err:?}")));
                }
                Ok(f) => {
                    let is_ok = f.clone().success();
                    if !is_ok {
                        return Err(format!("python环境创建异常:{:?}", f).into());
                    } else {
                        let mut f = fs::File::create(Path::new(&python_dir).join("redpymainok"))?;
                        std::io::Write::write_all(&mut f, &[])?;
                    }
                }
            }
        }

        let curr_env = std::env::var("PATH").unwrap_or_default();

        let new_env = if cfg!(target_os = "windows") {
            format!("{}pymain/Scripts;{}", python_dir, curr_env)
        } else {
            format!("{}pymain/bin:{}", python_dir, curr_env)
        };
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = RED_PY_DECODE.to_owned();

        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .creation_flags(0x08000000)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .env("PATH", new_env)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .env("PATH", new_env)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        if let Some(mut stdin) = p.stdin.take() {
            stdin.write_all(input_b64.as_bytes())?;
        } else {
            p.kill()?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if !err.is_empty() {
            let _ = cq_add_log_w(&format!("python中的警告或错误:{}", err));
        }
        Ok(Some(python_base64_wire_output_to_red(&out)?))
    });
    add_fun(vec!["运行本地PY"], |self_t, params| {
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode()
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode()).decode()
    red_print(en)
"#;

        let code1 = self_t.get_param_text(params, 0)?;
        let input_value = self_t.get_param(params, 1)?;
        let input = red_to_python_wire_string(&input_value)?;
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = RED_PY_DECODE.to_owned();

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .creation_flags(0x08000000)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        if let Some(mut stdin) = p.stdin.take() {
            stdin.write_all(input_b64.as_bytes())?;
        } else {
            p.kill()?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if !err.is_empty() {
            let _ = cq_add_log_w(&format!("python中的警告或错误:{}", err));
        }
        Ok(Some(python_base64_wire_output_to_red(&out)?))
    });
    add_fun(vec!["运行特殊PY"], |self_t, params| {
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode()
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode()).decode()
    red_print(en)
"#;

        let mut to_add_path = self_t.get_param_text(params, 0)?;
        let code1 = self_t.get_param_text(params, 1)?;
        let input_value = self_t.get_param(params, 2)?;
        let input = red_to_python_wire_string(&input_value)?;
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = RED_PY_DECODE.to_owned();

        let curr_env = std::env::var("PATH").unwrap_or_default();
        let new_env = if !to_add_path.is_empty() {
            if cfg!(target_os = "windows") {
                if !to_add_path.ends_with(";") {
                    to_add_path.push(';');
                }
                format!("{}{}", to_add_path, curr_env)
            } else {
                if !to_add_path.ends_with(":") {
                    to_add_path.push(':');
                }
                format!("{}{}", to_add_path, curr_env)
            }
        } else {
            curr_env
        };

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .env("PATH", new_env)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name()?)
            .creation_flags(0x08000000)
            .stdin(pip_in)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(app_dir)
            .env("PATH", new_env)
            .arg("-c")
            .arg(format!("{red_py_decode}{code}{code1}"))
            .spawn()?;

        if let Some(mut stdin) = p.stdin.take() {
            stdin.write_all(input_b64.as_bytes())?;
        } else {
            p.kill()?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if !err.is_empty() {
            let _ = cq_add_log_w(&format!("python中的警告或错误:{}", err));
        }
        Ok(Some(python_base64_wire_output_to_red(&out)?))
    });
    add_fun(vec!["默认字体"], |self_t, _params| {
        const DEFAULT_FONT_LEN: usize = 10560380;
        if let Some(font) = crate::font::default_font() {
            return Ok(Some(rv_bin(font.clone())));
        }
        let font_file_path = format!(
            "{}default_font.ttf",
            get_tmp_dir().map_err(|err| RedLang::make_err(&err.to_string()))?
        );
        if let Ok(font) = fs::read(&font_file_path) {
            if font.len() == DEFAULT_FONT_LEN {
                crate::font::set_default_font(font.clone());
                return Ok(Some(rv_bin(font)));
            }
        }
        let proxy = self_t.get_coremap("代理");
        let mut timeout_str = self_t.get_coremap("访问超时");
        if timeout_str.is_empty() {
            timeout_str = "60000".to_owned();
        }
        let mut http_header = coremap_value_to_text_map(self_t, "访问头")?;
        if !http_header.contains_key("User-Agent") {
            http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
        }
        let url = "https://fonts.gstatic.com/s/notosanssc/v36/k3kCo84MPvpLmixcA63oeAL7Iqp5IZJF9bmaG9_FnYxNbPzS5HE.ttf";
        let timeout = timeout_str.parse::<u64>()?;
        let content = runtime().block_on(async {
            let ret = tokio::select! {
                val_rst = http_post(url,vec![],&http_header,&proxy,"GET") => {
                    if let Ok(val) = val_rst {
                        val
                    } else {
                        let _ = cq_add_log_w(&format!("{:?}",val_rst));
                        (vec![],BTreeMap::new())
                    }
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                    let _ = cq_add_log_w(&format!("GET访问:`{}`超时",url));
                    (vec![],BTreeMap::new())
                }
            };
            ret
        });
        if content.0.len() == DEFAULT_FONT_LEN {
            if let Err(err) = fs::write(&font_file_path, &content.0) {
                let _ = cq_add_log_w(&format!("默认字体缓存写入失败：{}", err));
            }
            crate::font::set_default_font(content.0.clone());
            Ok(Some(rv_bin(content.0)))
        } else {
            let _ = cq_add_log_w("默认字体下载失败！");
            Ok(Some(rv_bin(vec![])))
        }
    });
    add_fun(vec!["快速运行PY"], |self_t, params| {
        let code = self_t.get_param_text_rc(params, 0)?;
        let input_value = self_t.get_param(params, 1)?;
        let input = red_to_python_wire_string(&input_value)?;
        let ret = call_py_block(code.as_str(), input.as_str());
        Ok(Some(python_wire_string_to_red(&ret)?))
    });
    add_fun(vec!["区间选择"], |self_t, params| {
        let obj_value = self_t.get_param(params, 0)?;
        let select_num = self_t.get_param_text_rc(params, 1)?.parse::<f64>()? + 0.0000001f64;
        let obj = RedLang::red_value_to_text_map(&obj_value, "区间选择参数1")?;
        for (k, v) in &obj {
            let k_t = k.split('~').collect::<Vec<&str>>();
            let num1_str = k_t.first().ok_or("num1 not exist")?;
            let num2_str = k_t.get(1).ok_or("num2 not a exist")?;
            let mut ok1 = false;
            let mut ok2 = false;
            if num1_str.is_empty() || select_num > num1_str.parse::<f64>()? {
                ok1 = true;
            }
            if num2_str.is_empty() || select_num < num2_str.parse::<f64>()? {
                ok2 = true;
            }
            if ok1 && ok2 {
                return Ok(Some(rv_text(v.to_owned())));
            }
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["内存使用"], |_self_t, _params| {
        let s = sysinfo::System::new_all();
        if let Some(process) =
            sysinfo::System::process(&s, sysinfo::Pid::from(std::process::id() as usize))
        {
            let num = sysinfo::Process::memory(process) as f32 / (1024 * 1024) as f32;
            return Ok(Some(rv_text(num.to_string())));
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["正则替换"], |self_t, params| {
        let text = self_t.get_param_text_rc(params, 0)?;
        let re = self_t.get_param_text_rc(params, 1)?;
        let out_text = self_t.get_param_text_rc(params, 2)?;
        let re_obj = fancy_regex::Regex::new(&re)?;
        let out = re_obj.replace_all(&text, &*out_text).to_string();
        Ok(Some(rv_text(out)))
    });

    add_fun(vec!["CPU使用"], |_self_t, _params| {
        let mut s = sysinfo::System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        s.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
        let pid = sysinfo::Pid::from(std::process::id() as usize);
        let Some(process) = sysinfo::System::process(&s, pid) else {
            return Ok(Some(rv_empty()));
        };
        Ok(Some(rv_text(
            (sysinfo::Process::cpu_usage(process) / sysinfo::System::cpus(&s).len() as f32)
                .to_string(),
        )))
    });

    add_fun(vec!["系统信息"], |_self_t, _params| {
        let mut s = sysinfo::System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        s.refresh_all();
        let mut mp = BTreeMap::new();

        let mut cpulist = vec![];
        for cpu in s.cpus() {
            let mut info = BTreeMap::new();
            info.insert("CpuUsage".to_owned(), cpu.cpu_usage().to_string());
            info.insert("Frequency".to_owned(), cpu.frequency().to_string());
            info.insert("Brand".to_owned(), cpu.brand().to_owned());
            info.insert("Name".to_owned(), cpu.name().to_owned());
            info.insert("VendorID".to_owned(), cpu.vendor_id().to_owned());
            cpulist.push(text_map_to_red_object(info));
        }
        mp.insert("CpuList".to_owned(), rv_array(cpulist));

        mp.insert(
            "SystemName".to_owned(),
            rv_text(sysinfo::System::long_os_version().unwrap_or_default()),
        );
        mp.insert(
            "SystemKernelVersion".to_owned(),
            rv_text(sysinfo::System::kernel_version().unwrap_or_default()),
        );
        mp.insert(
            "SystemOSVersion".to_owned(),
            rv_text(sysinfo::System::os_version().unwrap_or_default()),
        );
        mp.insert(
            "SystemHostName".to_owned(),
            rv_text(sysinfo::System::host_name().unwrap_or_default()),
        );

        mp.insert(
            "TotalMemory".to_owned(),
            rv_text(s.total_memory().to_string()),
        );
        mp.insert(
            "UsedMemory".to_owned(),
            rv_text(s.used_memory().to_string()),
        );
        mp.insert("TotalSwap".to_owned(), rv_text(s.total_swap().to_string()));
        mp.insert("UsedSwap".to_owned(), rv_text(s.used_swap().to_string()));

        mp.insert("CPUCount".to_owned(), rv_text(s.cpus().len().to_string()));

        let mut disklist = vec![];
        let disks = sysinfo::Disks::new_with_refreshed_list();
        for disk in &disks {
            let mut info = BTreeMap::new();
            info.insert(
                "MountPoint".to_owned(),
                disk.mount_point().to_string_lossy().to_string(),
            );
            info.insert("Name".to_owned(), disk.name().to_string_lossy().to_string());
            info.insert(
                "FileSystem".to_owned(),
                disk.file_system().to_string_lossy().to_string(),
            );
            info.insert("Kind".to_owned(), disk.kind().to_string());
            info.insert("TotalSpace".to_owned(), disk.total_space().to_string());
            info.insert(
                "AvailableSpace".to_owned(),
                disk.available_space().to_string(),
            );
            disklist.push(text_map_to_red_object(info));
        }
        mp.insert("DiskList".to_owned(), rv_array(disklist));

        let networks = sysinfo::Networks::new_with_refreshed_list();
        let mut netlist = vec![];
        for (interface_name, data) in &networks {
            let mut info = BTreeMap::new();
            info.insert(
                "TotalReceived".to_owned(),
                data.total_received().to_string(),
            );
            info.insert(
                "TotalTransmitted".to_owned(),
                data.total_transmitted().to_string(),
            );
            info.insert("InterfaceName".to_owned(), interface_name.to_owned());
            netlist.push(text_map_to_red_object(info));
        }
        mp.insert("NetList".to_owned(), rv_array(netlist));

        Ok(Some(rv_object(mp)))
    });

    add_fun(vec!["渲染SVG", "SVG渲染"], |self_t, params| {
        let svg_text = self_t.get_param_text(params, 0)?;
        let mut usvg_opt = usvg::Options::default();
        usvg_opt.fontdb_mut().load_system_fonts();
        let tree = resvg::usvg::Tree::from_str(&svg_text, &usvg_opt)?;
        // tree.fontdb().load_system_fonts();
        let width = (tree.size().width() + 0.5) as usize;
        let height = (tree.size().height() + 0.5) as usize;
        check_image_dims(width, height)?;
        let transform = resvg::tiny_skia::Transform::identity();
        let sz = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(resvg::tiny_skia::BYTES_PER_PIXEL))
            .ok_or_else(|| RedLang::make_err("SVG尺寸过大"))?;
        let mut pix = vec![0; sz];
        let mut pixmapmut = resvg::tiny_skia::PixmapMut::from_bytes(
            pix.as_mut_slice(),
            width as u32,
            height as u32,
        )
        .ok_or("create pixmap err")?;
        resvg::render(&tree, transform, &mut pixmapmut);
        Ok(Some(rv_bin(pixmapmut.to_owned().encode_png()?)))
    });

    add_fun(vec!["__MD转HTML"], |self_t, params| {
        let md_text = self_t.get_param_text(params, 0)?;
        let out_html = markdown::to_html(&md_text);
        Ok(Some(rv_text(out_html)))
    });

    add_fun(vec!["转繁体"], |self_t, params| {
        let msg = self_t.get_param_text(params, 0)?;
        let ret = redlang::mytool::str_to_ft(msg.as_str());
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["转简体"], |self_t, params| {
        let msg = self_t.get_param_text(params, 0)?;
        let ret = redlang::mytool::str_to_jt(msg.as_str());
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["github代理"], |_self_t, _params| {
        let port = redlang::config_manager::startup_config()
            .map_err(|_| "无法读取配置文件")?
            .web_port;
        let url = format!(
            "http://127.0.0.1:{}/redlang/api/5350b16b-b5e2-425a-bba1-d33d92813ab4/",
            port
        );
        Ok(Some(rv_text(url)))
    });
    add_fun(vec!["运行lua"], |self_t, params| {
        let code = self_t.get_param_text(params, 0)?;
        let lua = mlua::Lua::new();
        let s_t = RefCell::new(self_t);
        let lua_ret = lua.scope(|scope| {
            let func1 = scope.create_function_mut(|_, red_code: String| {
                let mut s_t = s_t.borrow_mut();
                let ret_str = s_t.parse_to_string(&red_code);
                ret_str.map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            })?;
            let func3 = scope.create_function_mut(|_, muti_params: MultiValue| {
                let params_len = muti_params.len();
                if params_len < 1 {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                }
                let Some(cmd) = muti_params.front() else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let red_cmd = if cmd.is_string() {
                    // Lua 字符串允许含非法 UTF-8 字节，to_str 会失败，
                    // 必须用 to_string_lossy 避免 panic（release 下直接 abort 进程）
                    cmd.as_string()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default()
                } else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let mut s_t = s_t.borrow_mut();
                let mut red_code = "【".to_string() + &red_cmd;
                for it in muti_params.iter().skip(1) {
                    let it_t = if it.is_string() {
                        it.as_string()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default()
                    } else {
                        return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                    };
                    red_code += "@";
                    let it_r = s_t
                        .parse_r_with_black(&it_t)
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    red_code += &it_r;
                }
                red_code += "】";
                let ret_str = s_t.parse_to_string(&red_code);
                ret_str.map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            })?;
            lua.globals().set("call_red", func1)?;
            lua.globals().set("call_redcmd", func3)?;

            let lua_chunk = lua.load(&code);
            lua_chunk.eval::<String>()
        })?;
        Ok(Some(rv_text(lua_ret)))
    });
    add_fun(vec!["运行超级lua"], |self_t, params| {
        let code = self_t.get_param_text(params, 0)?;
        let lua = unsafe { mlua::Lua::unsafe_new() };
        let s_t = RefCell::new(self_t);
        let lua_ret = lua.scope(|scope| {
            let func1 = scope.create_function_mut(|_, red_code: String| {
                let mut s_t = s_t.borrow_mut();
                let ret_str = s_t.parse_to_string(&red_code);
                ret_str.map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            })?;
            let func3 = scope.create_function_mut(|_, muti_params: MultiValue| {
                let params_len = muti_params.len();
                if params_len < 1 {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                }
                let Some(cmd) = muti_params.front() else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let red_cmd = if cmd.is_string() {
                    // 同【运行lua】：Lua 字符串可含非法 UTF-8，用 to_string_lossy 避免 panic
                    cmd.as_string()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default()
                } else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let mut s_t = s_t.borrow_mut();
                let mut red_code = "【".to_string() + &red_cmd;
                for it in muti_params.iter().skip(1) {
                    let it_t = if it.is_string() {
                        it.as_string()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default()
                    } else {
                        return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                    };
                    red_code += "@";
                    let it_r = s_t
                        .parse_r_with_black(&it_t)
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    red_code += &it_r;
                }
                red_code += "】";
                let ret_str = s_t.parse_to_string(&red_code);
                ret_str.map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            })?;
            lua.globals().set("call_red", func1)?;
            lua.globals().set("call_redcmd", func3)?;

            let lua_chunk = lua.load(&code);
            lua_chunk.eval::<String>()
        })?;
        Ok(Some(rv_text(lua_ret)))
    });
    crate::tts_commands::register();
}
