use super::*;

pub(super) fn register() {
    add_fun(vec!["文件信息"], |self_t, params| {
        let file_path = self_t.get_param_text(params, 0)?;
        let path = Path::new(&file_path);
        let mut ret_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        if !path.exists() {
            return Ok(Some(rv_object(BTreeMap::new())));
        }
        let file_info = fs::metadata(path)?;
        ret_obj.insert("大小".to_string(), rv_text(file_info.len().to_string()));
        if let Ok(val) = file_info.created() {
            ret_obj.insert(
                "创建时间".to_string(),
                rv_text(
                    val.duration_since(std::time::UNIX_EPOCH)?
                        .as_secs()
                        .to_string(),
                ),
            );
        }
        if let Ok(val) = file_info.modified() {
            ret_obj.insert(
                "修改时间".to_string(),
                rv_text(
                    val.duration_since(std::time::UNIX_EPOCH)?
                        .as_secs()
                        .to_string(),
                ),
            );
        }
        if let Ok(val) = file_info.accessed() {
            ret_obj.insert(
                "访问时间".to_string(),
                rv_text(
                    val.duration_since(std::time::UNIX_EPOCH)?
                        .as_secs()
                        .to_string(),
                ),
            );
        }
        if file_info.is_dir() {
            ret_obj.insert("类型".to_string(), rv_text("目录".to_string()));
        } else {
            ret_obj.insert("类型".to_string(), rv_text("文件".to_string()));
        }
        if file_info.is_symlink() {
            ret_obj.insert("符号链接".to_string(), rv_text("真".to_string()));
        } else {
            ret_obj.insert("符号链接".to_string(), rv_text("假".to_string()));
        }
        Ok(Some(rv_object(ret_obj)))
    });

    add_fun(vec!["判存"], |self_t, params| {
        let file_path = self_t.get_param_text(params, 0)?;
        let path = Path::new(&file_path);
        let ret_str = if !path.exists() {
            self_t.get_param_text(params, 1)?
        } else {
            file_path
        };
        Ok(Some(rv_text(ret_str)))
    });

    add_fun(vec!["删除目录"], |self_t, params| {
        let dir_path = self_t.get_param_text(params, 0)?;
        let path = Path::new(&dir_path);
        let _foo = fs::remove_dir_all(path);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["删除文件"], |self_t, params| {
        let file_path = self_t.get_param_text(params, 0)?;
        let path = Path::new(&file_path);
        let _ = fs::remove_file(path);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["压缩"], |self_t, params| {
        let src_path = self_t.get_param_text(params, 0)?;
        let remote_path = self_t.get_param_text(params, 1)?;
        sevenz_rust2::compress_to_path(src_path, remote_path)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["解压"], |self_t, params| {
        let src_path = self_t.get_param_text(params, 0)?;
        let remote_path = self_t.get_param_text(params, 1)?;
        sevenz_rust2::decompress_file(src_path, remote_path)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["去重"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        let mut arr_out: Vec<Rc<RedValue>> = vec![];
        for txt in arr {
            if !arr_out.iter().any(|x| **x == *txt) {
                arr_out.push(txt);
            }
        }
        Ok(Some(rv_array(arr_out)))
    });
    add_fun(vec!["打乱"], |self_t, params| {
        let mut arr_out = self_t.get_param_array(params, 0)?;
        for i in (1..arr_out.len()).rev() {
            let rand_i = get_random()? % (i + 1);
            if rand_i != i {
                arr_out.swap(i, rand_i);
            }
        }
        Ok(Some(rv_array(arr_out)))
    });
    add_fun(vec!["合并"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        let txt = self_t.get_param_text(params, 1)?;
        let mut str_out = String::new();
        for i in 0..arr.len() {
            if arr[i].get_type_name() != "文本" {
                return Err(RedLang::make_err("只能对元素类型为文本的数组进行合并"));
            }
            str_out.push_str(arr[i].as_text().ok_or("数组文本获取失败")?);
            if i != arr.len() - 1 {
                str_out.push_str(&txt);
            }
        }
        Ok(Some(rv_text(str_out)))
    });

    add_fun(vec!["骰"], |self_t, params| {
        fn tou(input: &str) -> Result<String, Box<dyn std::error::Error>> {
            // 分割tokens
            fn get_token(input: &str) -> Vec<String> {
                let mut ret_vec: Vec<String> = vec![];
                let mut tmp = String::new();
                for ch in input.chars() {
                    if ch.is_ascii_digit() {
                        tmp.push(ch);
                    } else if ch.is_ascii_alphabetic() {
                        // 字母无论前后是否有数字都应成为 token（兼容 d6/3D6 写法），
                        // 统一转小写，此前开头的小写字母会被静默丢弃、大写不识别
                        if !tmp.is_empty() {
                            ret_vec.push(tmp.clone());
                            tmp.clear();
                        }
                        ret_vec.push(ch.to_ascii_lowercase().to_string());
                    }
                }
                if !tmp.is_empty() {
                    ret_vec.push(tmp);
                }
                ret_vec
            }

            // 取参数
            fn get_params(tokens: &[String]) -> HashMap<String, (Option<i64>, Option<i64>)> {
                let mut ret_map = HashMap::new();
                fn get_left(index: usize, tokens: &[String]) -> Option<i64> {
                    if index == 0 {
                        None
                    } else {
                        tokens[index - 1].parse::<i64>().ok()
                    }
                }
                fn get_right(index: usize, tokens: &[String]) -> Option<i64> {
                    if index == tokens.len() - 1 {
                        None
                    } else {
                        tokens[index + 1].parse::<i64>().ok()
                    }
                }
                for index in 0..tokens.len() {
                    let it = &tokens[index];
                    if it == "d"
                        || it == "k"
                        || it == "q"
                        || it == "p"
                        || it == "b"
                        || it == "m"
                        || it == "a"
                        || it == "c"
                        || it == "f"
                    {
                        ret_map.insert(
                            it.to_owned(),
                            (get_left(index, tokens), get_right(index, tokens)),
                        );
                    }
                }
                ret_map
            }
            let tokens = get_token(input);
            let mp = get_params(&tokens);

            fn get_tou(m: i64, des: &mut String) -> Result<i64, Box<dyn std::error::Error>> {
                if m <= 0 {
                    return Err("骰子面数必须大于0".into());
                }
                let ret = get_random()? % m as usize + 1;
                des.push_str(&format!("掷出面数为{}的骰子:{}\n", m, ret));
                Ok(ret as i64)
            }

            fn rand_faces(m: i64) -> Result<i64, Box<dyn std::error::Error>> {
                if m <= 0 {
                    return Err("骰子面数必须大于0".into());
                }
                Ok((get_random()? % m as usize + 1) as i64)
            }

            fn get_two_tou(des: &mut String) -> i64 {
                let ret1 = get_random().unwrap_or(0) % 10_usize + 1;
                let ret2 = get_random().unwrap_or(0) % 10_usize + 1;
                des.push_str(&format!("掷出两个面数为10的骰子:{:?}\n", vec![ret1, ret2]));
                let mut ret = (ret1 - 1) * 10 + ret2 - 1;
                if ret == 0 {
                    ret = 100;
                }
                des.push_str(&format!("组成一个面数位100的骰子:{}\n", ret));
                ret as i64
            }
            let mut des = String::new();
            if mp.contains_key("f") {
                // FATE掷骰池
                des += "检测到c参数,所以为`FATE掷骰池`\n";
                let f = mp["f"].0.unwrap_or(4);
                let mut tou_vec: Vec<i64> = vec![];
                for _i in 0..f {
                    tou_vec.push(((get_random().unwrap_or(0) % 3_usize) as i64) - 1);
                }
                des.push_str(&format!("掷出{f}个三面骰:{tou_vec:?}\n"));
                let mut sum: i64 = 0;
                for it in tou_vec {
                    sum += it;
                }
                des.push_str(&format!("{}个骰子的总和为:{}", f, sum));
            } else if mp.contains_key("d") {
                // 只可能是普通多面骰
                des += "检测到d参数,所以为`普通多面掷骰`\n";
                if mp.contains_key("a") {
                    // 应该转化为无限加骰池
                    des += "检测到a参数,转化为`无限加骰池`\n";
                    let a = mp["d"].0.unwrap_or(1);
                    let bb = mp["d"].1.ok_or("err1")?.checked_add(1).ok_or("err1")?;
                    let b = bb - 1;
                    let e = mp["a"].1.ok_or("err2")?;
                    let zhjg = format!("{a}a{bb}k{e}m{b}");
                    des.push_str(&format!("转化结果:{zhjg}\n"));
                    des += &tou(&zhjg)?;
                } else if mp.contains_key("k") {
                    // 选取最大
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut tou_vec: Vec<i64> = vec![];
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:", i + 1));
                        tou_vec.push(get_tou(ms, &mut des)?);
                    }
                    tou_vec.sort();
                    tou_vec.reverse();
                    des.push_str(&format!("从大到小排序后为:{:?}\n", tou_vec));
                    let k = mp["k"].1.unwrap_or(ts);
                    let tou_vec2 = tou_vec.get(0..k as usize).ok_or("err3")?;
                    des.push_str(&format!("选取最大的{}个骰子:{:?}\n", k, tou_vec2));
                    let mut sum = 0;
                    for it in tou_vec2 {
                        sum += it;
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}", k, sum));
                } else if mp.contains_key("q") {
                    // 选取最小
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut tou_vec: Vec<i64> = vec![];
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:", i + 1));
                        tou_vec.push(get_tou(ms, &mut des)?);
                    }
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n", tou_vec));
                    let k = mp["q"].1.unwrap_or(ts);
                    let tou_vec2 = tou_vec.get(0..k as usize).ok_or("err4")?;
                    des.push_str(&format!("选取最小的{}个骰子:{:?}\n", k, tou_vec2));
                    let mut sum = 0;
                    for it in tou_vec2 {
                        sum += it;
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}", k, sum));
                } else if mp.contains_key("p") {
                    // 追加惩罚
                    let p_num = mp["p"].1.unwrap_or(0);
                    let mut ret = get_two_tou(&mut des);
                    let mut p_vec: Vec<i64> = vec![];
                    for _i in 0..p_num {
                        p_vec.push((get_random().unwrap_or(0) % 10_usize + 1) as i64)
                    }
                    des.push_str(&format!("掷出{p_num}个惩罚骰:{p_vec:?}\n"));
                    p_vec.sort();
                    p_vec.reverse();
                    des.push_str(&format!("从大到小排序后为:{:?}\n", p_vec));
                    let sw = ret / 10;
                    if p_vec.first().ok_or("err5")? > &sw {
                        ret = ret % 10 + p_vec[0] * 10;
                    }
                    des.push_str(&format!("惩罚后的结果是:{ret}"));
                } else if mp.contains_key("b") {
                    // 追加奖励
                    let p_num = mp["b"].1.unwrap_or(0);
                    let mut ret = get_two_tou(&mut des);
                    let mut p_vec: Vec<i64> = vec![];
                    for _i in 0..p_num {
                        p_vec.push((get_random().unwrap_or(0) % 10_usize + 1) as i64)
                    }
                    des.push_str(&format!("掷出{p_num}个奖励骰:{p_vec:?}\n"));
                    p_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n", p_vec));
                    let sw = ret / 10;
                    if p_vec.first().ok_or("err6")? < &sw {
                        ret = ret % 10 + p_vec[0] * 10;
                    }
                    des.push_str(&format!("奖励后的结果是:{ret}"));
                } else {
                    // 普通掷骰
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut sum: i64 = 0;
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:", i + 1));
                        sum += get_tou(ms, &mut des)?;
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}", ts, sum));
                }
            } else if mp.contains_key("p") {
                // 惩罚
                des += "检测到p参数,所以为`惩罚骰`\n";
                let p_num = mp["p"].1.unwrap_or(0);
                let mut ret = get_two_tou(&mut des);
                let mut p_vec: Vec<i64> = vec![];
                for _i in 0..p_num {
                    p_vec.push((get_random().unwrap_or(0) % 10_usize + 1) as i64)
                }
                des.push_str(&format!("掷出{p_num}个惩罚骰:{p_vec:?}\n"));
                p_vec.sort();
                p_vec.reverse();
                des.push_str(&format!("从大到小排序后为:{:?}\n", p_vec));
                let sw = ret / 10;
                if p_vec.first().ok_or("err7")? > &sw {
                    ret = ret % 10 + p_vec[0] * 10;
                }
                des.push_str(&format!("惩罚后的结果是:{ret}"));
            } else if mp.contains_key("b") {
                // 奖励
                des += "检测到p参数,所以为`奖励骰`\n";
                let p_num = mp["b"].1.unwrap_or(0);
                let mut ret = get_two_tou(&mut des);
                let mut p_vec: Vec<i64> = vec![];
                for _i in 0..p_num {
                    p_vec.push((get_random().unwrap_or(0) % 10_usize + 1) as i64)
                }
                des.push_str(&format!("掷出{p_num}个奖励骰:{p_vec:?}\n"));
                p_vec.sort();
                des.push_str(&format!("从小到大排序后为:{:?}\n", p_vec));
                let sw = ret / 10;
                if p_vec.first().ok_or("err8")? < &sw {
                    ret = ret % 10 + p_vec[0] * 10;
                }
                des.push_str(&format!("奖励后的结果是:{ret}"));
            } else if mp.contains_key("a") {
                // 无限加骰池
                des += "检测到a参数,所以为`无限加骰池`\n";
                let mut a1 = mp["a"].0.unwrap_or(1);
                let a2 = mp["a"].1.ok_or("err9")?;
                let mut m = 10;
                if mp.contains_key("m") {
                    m = mp["m"].1.ok_or("骰子表达式错误:m参数缺少右值")?;
                }
                let mut cgx = 8;
                if mp.contains_key("k") {
                    cgx = mp["k"].1.ok_or("骰子表达式错误:k参数缺少右值")?;
                }
                let mut fxcgx = m;
                if mp.contains_key("q") {
                    fxcgx = mp["q"].1.ok_or("骰子表达式错误:q参数缺少右值")?;
                }
                let mut ls: i32 = 1;
                let mut ret = 0;
                // 阈值<=1 时每轮全部骰子都"成功"，循环永不退出且 des 无限增长，
                // 必须限制最大轮数
                const MAX_ROUNDS: i32 = 1000;
                loop {
                    if ls > MAX_ROUNDS {
                        des.push_str("(已达到最大轮数上限，强制结束)\n");
                        break;
                    }
                    let mut tou_vec: Vec<i64> = vec![];
                    for _i in 0..a1 {
                        tou_vec.push(rand_faces(m)?);
                    }
                    des.push_str(&format!("第{ls}轮掷骰:{tou_vec:?}\n"));
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n", tou_vec));
                    a1 = 0;
                    for it in &tou_vec {
                        if it >= &a2 {
                            a1 += 1;
                        }
                    }
                    des.push_str(&format!("下一轮掷骰数目:{a1}\n"));
                    for it in tou_vec {
                        if it >= cgx && it <= fxcgx {
                            ret += 1;
                        }
                    }
                    des.push_str(&format!("累计成功骰数:{ret}"));
                    ls += 1;
                    if a1 != 0 {
                        des.push('\n');
                    } else {
                        break;
                    }
                }
            } else if mp.contains_key("c") {
                // 双重十字加骰池
                des += "检测到c参数,所以为`双重十字加骰池`\n";
                let mut a = mp["c"].0.ok_or("骰子表达式错误:c参数缺少左值")?;
                let b = mp["c"].1.ok_or("骰子表达式错误:c参数缺少右值")?;
                let mut m = 10;
                if mp.contains_key("m") {
                    m = mp["m"].1.ok_or("骰子表达式错误:m参数缺少右值")?;
                }
                let mut ls = 1;
                let mut ret = 0;
                // 阈值<=1 时每轮全部骰子都"成功"，循环永不退出且 des 无限增长，
                // 必须限制最大轮数
                const MAX_ROUNDS: i32 = 1000;
                loop {
                    if ls > MAX_ROUNDS {
                        des.push_str("(已达到最大轮数上限，强制结束)\n");
                        break;
                    }
                    let mut tou_vec: Vec<i64> = vec![];
                    for _i in 0..a {
                        tou_vec.push(rand_faces(m)?);
                    }
                    des.push_str(&format!("第{ls}轮掷骰:{tou_vec:?}\n"));
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n", tou_vec));
                    a = 0;
                    for it in &tou_vec {
                        if it >= &b {
                            a += 1;
                        }
                    }
                    des.push_str(&format!("下一轮掷骰数目:{a}\n"));
                    if a != 0 {
                        ret += m;
                    } else {
                        ret += tou_vec.last().ok_or("err10")?;
                    }
                    des.push_str(&format!("累计成功骰数:{ret}\n"));
                    if a == 0 {
                        break;
                    } else {
                        des.push('\n');
                    }
                    ls += 1;
                }
            }
            Ok(des)
        }
        let text = self_t.get_param_text(params, 0)?;
        let ret = tou(&text)?;
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["补位"], |self_t, params| {
        let mut num_f64 = self_t.get_param_text(params, 0)?.parse::<f64>()?;
        let is_f;
        if num_f64 < 0.0 {
            num_f64 = -num_f64;
            is_f = true;
        } else {
            is_f = false;
        }
        let mut num = num_f64.to_string();
        let num_format: String = self_t.get_param_text(params, 1)?;
        // 得到整数部分和小数部分的位数
        let mut num_format_left_count;
        let mut num_format_right_count;
        if let Some(pos) = num_format.find('.') {
            num_format_left_count = pos;
            num_format_right_count = num_format.len() - pos - 1;
        } else {
            num_format_left_count = num_format.len();
            num_format_right_count = 0;
        }
        // 得到整数部分和小数部分的位数
        let mut num_left_count;
        let mut num_right_count;
        if let Some(pos) = num.find('.') {
            num_left_count = pos;
            num_right_count = num.len() - pos - 1;
        } else {
            num_left_count = num.len();
            num_right_count = 0;
        }
        // 处理四舍五入
        if num_right_count > num_format_right_count {
            let ch = num.as_bytes()[num_left_count + 1 + num_format_right_count];
            if ch - 48 >= 5 {
                num = (num_f64 + 1_f64 / 10_f64.powf(num_format_right_count as f64)).to_string();
            }
            if let Some(pos) = num_format.find('.') {
                num_format_left_count = pos;
                num_format_right_count = num_format.len() - pos - 1;
            } else {
                num_format_left_count = num_format.len();
                num_format_right_count = 0;
            }
            // 得到整数部分和小数部分的位数
            if let Some(pos) = num.find('.') {
                num_left_count = pos;
                num_right_count = num.len() - pos - 1;
            } else {
                num_left_count = num.len();
                num_right_count = 0;
            }
        }
        let mut out_str_num = String::new();
        // 处理负号
        if is_f {
            out_str_num.push('-');
        }
        // 处理整数部分
        if num_left_count < num_format_left_count {
            let num0 = num_format_left_count - num_left_count;
            for _i in 0..num0 {
                out_str_num.push('0');
            }
        }
        out_str_num.push_str(&num[0..num_left_count]);
        // 处理小数点
        if num_format_right_count != 0 {
            out_str_num.push('.');
        }
        // 处理小数部分
        if num_right_count <= num_format_right_count {
            let num0 = num_format_right_count - num_right_count;
            if num_right_count != 0 {
                out_str_num.push_str(&num[num_left_count + 1..]);
            }
            for _i in 0..num0 {
                out_str_num.push('0');
            }
        } else {
            out_str_num
                .push_str(&num[num_left_count + 1..num_left_count + num_format_right_count + 1]);
        }
        Ok(Some(rv_text(out_str_num)))
    });
    add_fun(vec!["排序"], |self_t, params| {
        let mut arr = self_t.get_param_array(params, 0)?;

        if arr.is_empty() {
            return Ok(Some(rv_array(vec![])));
        }

        if params.len() > 1 {
            let func_ast = params.get(1).ok_or("函数获取失败")?.clone();
            for i in 0..arr.len() - 1 {
                for j in i + 1..arr.len() {
                    let left_name = format!("__RED_SORT_LEFT_{}", get_random().unwrap_or(0));
                    let right_name = format!("__RED_SORT_RIGHT_{}", get_random().unwrap_or(0));
                    let var_vec_len = self_t.var_vec.len();
                    {
                        let mp = &mut self_t.var_vec[var_vec_len - 1];
                        mp.insert(left_name.clone(), Rc::new(RefCell::new(arr[i].clone())));
                        mp.insert(right_name.clone(), Rc::new(RefCell::new(arr[j].clone())));
                    }
                    let left_ast = astparser::parse_to_ast(&format!("【变量@{}】", left_name))
                        .map_err(|e| RedLang::make_err(&e))?;
                    let right_ast = astparser::parse_to_ast(&format!("【变量@{}】", right_name))
                        .map_err(|e| RedLang::make_err(&e))?;
                    let ret_str = self_t.call_fun(&[func_ast.clone(), left_ast, right_ast])?;
                    {
                        let mp = &mut self_t.var_vec[var_vec_len - 1];
                        mp.remove(&left_name);
                        mp.remove(&right_name);
                    }
                    if ret_str.text_or_empty() != "真" {
                        arr.swap(i, j);
                    }
                }
            }
        } else {
            for i in 0..arr.len() - 1 {
                for j in i + 1..arr.len() {
                    let f1 = arr[i].expect_text_value()?.parse::<f64>()?;
                    let f2 = arr[j].expect_text_value()?.parse::<f64>()?;
                    if f1 > f2 {
                        arr.swap(i, j);
                    }
                }
            }
        }
        Ok(Some(rv_array(arr)))
    });
    add_fun(vec!["翻转"], |self_t, params| {
        let to_rev = self_t.get_param(params, 0)?;
        if let Ok(mut arr) = to_rev.expect_array_value() {
            arr.reverse();
            Ok(Some(rv_array(arr)))
        } else if let Ok(text) = to_rev.expect_text_value() {
            let mut s = text.chars().collect::<Vec<char>>();
            s.reverse();
            Ok(Some(rv_text(String::from_iter(s))))
        } else if let Ok(mut bin) = to_rev.expect_bin_value() {
            bin.reverse();
            Ok(Some(rv_bin(bin)))
        } else {
            Err(RedLang::make_err(&format!(
                "不支持的翻转类型:{}",
                to_rev.get_type_name()
            )))
        }
    });
    add_fun(vec!["上传文件"], |self_t, params| {
        fn access(
            self_t: &mut RedLang,
            filename: &str,
            url: &str,
            file_data: &mut Vec<u8>,
        ) -> Result<String, Box<dyn std::error::Error>> {
            let proxy = self_t.get_coremap("代理");
            let mut timeout_str = self_t.get_coremap("访问超时");
            if timeout_str.is_empty() {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = coremap_value_to_text_map(self_t, "访问头")?;
            if !http_header.contains_key("User-Agent") {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let bound = uuid::Uuid::new_v4().to_string();

            {
                let ct = "multipart/form-data; boundary=".to_owned() + &bound;
                http_header.insert("Content-Type".to_string(), ct);
            }

            let mut data: Vec<u8> = vec![];
            data.append(&mut "--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            data.append(
                &mut "\r\nContent-Disposition: form-data; name=\"step\"\r\n\r\n0\r\n"
                    .as_bytes()
                    .to_owned(),
            );
            data.append(&mut "--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            let fname: String = url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
            data.append(&mut format!("\r\nContent-Disposition: form-data; name=\"file\";filename=\"{fname}\"\r\n\r\n").as_bytes().to_owned());
            data.append(file_data);
            data.append(&mut "\r\n--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            data.append(&mut "--\r\n".as_bytes().to_owned());

            let timeout = timeout_str.parse::<u64>()?;
            let content = runtime().block_on(async {
                let ret = tokio::select! {
                        val_rst = http_post(url,data,&http_header,&proxy,"POST") => {
                            if let Ok(val) = val_rst {
                                val
                            } else {
                                let _ = cq_add_log_w(&format!("{:?}",val_rst));
                                (vec![],BTreeMap::new())
                            }
                        },
                        _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                            let _ = cq_add_log_w(&format!("POST访问:`{}`超时",url));
                (vec![],BTreeMap::new())
                        }
                    };
                ret
            });
            let json: serde_json::Value = serde_json::from_slice(&content.0)?;
            let url = json["data"]
                .as_str()
                .ok_or(format!("can't upload file:{json:?}",))?;
            Ok(url.to_owned())
        }
        let filename = self_t.get_param_text(params, 1)?;
        let mut bin = self_t.get_param_bin(params, 0)?;
        let url = "https://uhsea.com/Frontend/upload";
        match access(self_t, &filename, url, &mut bin) {
            Ok(ret) => Ok(Some(rv_text(ret))),
            Err(err) => {
                let _ = cq_add_log_w(&format!("{:?}", err));
                Ok(Some(rv_empty()))
            }
        }
    });

}
