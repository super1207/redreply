use super::*;

pub(super) fn register() {
    add_fun(vec!["执行频率"], |self_t, params| {
        self_t.set_coremap("剩余次数", &0.to_string())?;

        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        let name = self_t.get_param_text(params, 0)?;
        let time_str = self_t.get_param_text(params, 1)?;
        let times_str = self_t.get_param_text(params, 2)?;
        let time = time_str.parse::<i64>()?;
        let times = times_str.parse::<i64>()?;
        let key = format!("{name}245d51a6-5139-49ae-9d21-6d07065ee610{time}245d51a6-5139-49ae-9d21-6d07065ee610{times}");

        if times <= 0 {
            return Ok(Some(self_t.get_param(params, 3)?));
        }
        let times = times - 1;

        let can_run;
        {
            add_file_lock(&sql_file);

            let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
                del_file_lock(&sql_file);
            });

            let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
            let tm13 = tm.as_millis() as i64;

            let conn = rusqlite::Connection::open(sql_file)?;

            redlang::resource_manager::configure_conn_busy_timeout(&conn);
            // 先创建表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS RUN_FREQ_TABLE (KEY TEXT PRIMARY KEY,VALUE TEXT);",
                [],
            )?;
            // 再查询,VALUE格式:开始时间-剩余次数
            let ret_rst: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT VALUE FROM RUN_FREQ_TABLE WHERE KEY = ?",
                [key.to_owned()],
                |row| row.get(0),
            );
            if let Ok(ret) = ret_rst {
                // 查询到了,要判断是否过期
                let v = ret.split("-").collect::<Vec<&str>>();
                let start_time = v
                    .first()
                    .ok_or("执行频率记录格式错误(缺少开始时间)")?
                    .parse::<i64>()?;
                let r_times = v
                    .get(1)
                    .ok_or("执行频率记录格式错误(缺少剩余次数)")?
                    .parse::<i64>()?;
                if tm13 - start_time > time {
                    // 过期了
                    can_run = true;
                    let value = format!("{tm13}-{times}");
                    conn.execute(
                        "REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)",
                        [key, value],
                    )?;
                    self_t.set_coremap("剩余次数", &times.to_string())?;
                } else {
                    // 没过期
                    if r_times != 0 {
                        // 可以执行，剩余次数-1
                        can_run = true;
                        let r_times = r_times - 1;
                        let value = format!("{start_time}-{r_times}");
                        conn.execute(
                            "REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)",
                            [key, value],
                        )?;
                        self_t.set_coremap("剩余次数", &r_times.to_string())?;
                    } else {
                        // 不能执行
                        self_t.set_coremap("剩余次数", &0.to_string())?;
                        can_run = false;
                    }
                }
            } else {
                // 没查询到,等效于没过期
                can_run = true;
                let value = format!("{tm13}-{times}");
                conn.execute(
                    "REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)",
                    [key, value],
                )?;
                self_t.set_coremap("剩余次数", &times.to_string())?;
            }
        }
        if !can_run {
            // 不能执行
            return Ok(Some(self_t.get_param(params, 3)?));
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["剩余次数"], |self_t, _params| {
        let count = self_t.get_coremap("剩余次数");
        if count.is_empty() {
            return Ok(Some(rv_text("0".to_string())));
        }
        Ok(Some(rv_text(count)))
    });
    add_fun(vec!["设置延迟触发"], |self_t, params| {
        // 【设置延迟触发@关键词@时间@传递数据】
        let keyword = self_t.get_param_text(params, 0)?;
        let mut tm = self_t.get_param_text(params, 1)?.parse::<i64>()?;
        let subdata = self_t.get_param_text(params, 2)?;
        let now_time = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64;
        // checked_add 避免延迟值接近 i64::MAX 时溢出(debug panic/release 回绕成立即触发)
        tm = tm
            .checked_add(now_time)
            .ok_or_else(|| RedLang::make_err("设置延迟触发：延迟时间溢出"))?;
        crate::core::scheduler()
            .add_one_time_run(OneTimeRunStruct {
                run_time: tm,
                pkg_name: self_t.pkg_name.to_owned(),
                flag: keyword,
                sub_data: subdata,
                data: (*self_t.exmap).borrow().clone(),
            });
        Ok(Some(rv_empty()))
    });
    fn zhuhe(s_len: usize, n: usize) -> Vec<Vec<usize>> {
        if s_len == 0 || n == 0 || s_len < n {
            return vec![];
        }
        let mut stack_vec: Vec<usize> = Vec::new();
        let mut out_vec: Vec<Vec<usize>> = Vec::new();
        stack_vec.push(0);
        loop {
            if stack_vec.len() == n {
                out_vec.push(stack_vec.clone());
                let last_num = stack_vec[stack_vec.len() - 1];
                if last_num == (s_len - 1) {
                    stack_vec.pop();
                    if stack_vec.is_empty() {
                        break;
                    }
                    let last_num = stack_vec[stack_vec.len() - 1];
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                } else {
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                }
            } else {
                let last_num = stack_vec[stack_vec.len() - 1];
                if last_num == (s_len - 1) {
                    stack_vec.pop();
                    if stack_vec.is_empty() {
                        break;
                    }
                    let last_num = stack_vec[stack_vec.len() - 1];
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                } else {
                    stack_vec.push(last_num + 1)
                }
            }
        }
        out_vec
    }
    fn next_permutation(nums: &mut [usize]) -> bool {
        use std::cmp::Ordering;
        // or use feature(array_windows) on nightly
        let last_ascending = match nums.windows(2).rposition(|w| w[0] < w[1]) {
            Some(i) => i,
            None => {
                nums.reverse();
                return false;
            }
        };

        let swap_with = nums[last_ascending + 1..]
            .binary_search_by(|n| usize::cmp(&nums[last_ascending], n).then(Ordering::Less))
            .unwrap_err(); // cannot fail because the binary search will never succeed
        nums.swap(last_ascending, last_ascending + swap_with);
        nums[last_ascending + 1..].reverse();
        true
    }
    // 全排列
    fn quanpai(n: usize) -> Vec<Vec<usize>> {
        let mut out_vec: Vec<Vec<usize>> = Vec::new();
        let mut v = (0usize..n).collect::<Vec<usize>>();
        out_vec.push(v.clone());
        loop {
            let ret = next_permutation(&mut v);
            if !ret {
                break;
            }
            out_vec.push(v.clone());
        }
        out_vec
    }
    add_fun(vec!["组合"], |self_t, params| {
        let s = self_t.get_param_array(params, 0)?;
        let n = self_t.get_param_text(params, 1)?.parse::<usize>()?;
        let s_len = s.len();
        let out_vec = zhuhe(s_len, n);
        let mut out_vec_t: Vec<Rc<RedValue>> = Vec::new();
        for it in out_vec {
            let mut temp_vec = vec![];
            for it2 in it {
                temp_vec.push(s[it2].clone());
            }
            out_vec_t.push(rv_array(temp_vec));
        }
        Ok(Some(rv_array(out_vec_t)))
    });
    add_fun(vec!["排列"], |self_t, params| {
        let s = self_t.get_param_array(params, 0)?;
        let n = self_t.get_param_text(params, 1)?.parse::<usize>()?;
        // n! 全排列数量爆炸增长，无上限将导致挂起/内存耗尽
        if n > 9 {
            return Err(RedLang::make_err("排列参数n不能超过9"));
        }
        let s_len = s.len();
        let out_zhuhe = zhuhe(s_len, n);
        if out_zhuhe.is_empty() {
            return Ok(Some(rv_array(vec![])));
        }
        let quanpai_vec = quanpai(n);
        let mut out_vec_t: Vec<Rc<RedValue>> = Vec::new();
        for it3 in out_zhuhe {
            for it4 in &quanpai_vec {
                let mut temp_vec = vec![];
                for it5 in it4 {
                    temp_vec.push(s[it3[*it5]].clone());
                }
                out_vec_t.push(rv_array(temp_vec));
            }
        }
        Ok(Some(rv_array(out_vec_t)))
    });

    add_fun(vec!["HTML解析"], |self_t, params| {
        let html_rv = self_t.get_param(params, 0)?;
        let html = if let Ok(text) = html_rv.expect_text_value() {
            text
        } else if let Ok(bin) = html_rv.expect_bin_value() {
            String::from_utf8(bin)?
        } else {
            return Err(RedLang::make_err("HTML解析第1参数必须是文本或字节集"));
        };
        let css = self_t.get_param_text(params, 1)?;
        let document = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse(&css).map_err(|err| err.to_string())?;

        let mut retvec = vec![];

        for element in document.select(&selector) {
            let mut obj = BTreeMap::new();
            obj.insert("html".to_owned(), rv_text(element.html()));
            obj.insert("inner_html".to_owned(), rv_text(element.inner_html()));
            let text = element.text().collect::<Vec<&str>>();
            let mut text_str = String::new();
            for part in text {
                text_str.push_str(part);
            }
            obj.insert("value".to_owned(), rv_text(text_str));
            let atters_map = element.value().attrs();
            let mut atters_obj = BTreeMap::new();
            for (key, value) in atters_map {
                atters_obj.insert(key.to_string(), value.to_string());
            }
            obj.insert("attrs".to_owned(), text_map_to_red_object(atters_obj));
            retvec.push(rv_object(obj));
        }
        Ok(Some(rv_array(retvec)))
    });
    add_fun(vec!["计数"], |self_t, params| {
        let content_rv = self_t.get_param(params, 0)?;
        let ele_rv = self_t.get_param(params, 1)?;
        let count = if let Ok(content) = content_rv.expect_text_value() {
            let ele = ele_rv.expect_text_value()?;
            if ele.is_empty() {
                return Err(RedLang::make_err("计数的元素不能为空"));
            }
            let mut pos = 0;
            let mut text_count = 0;
            while let Some(found) = content[pos..].find(ele.as_str()) {
                text_count += 1;
                pos += found + ele.len();
            }
            text_count
        } else if let Ok(bin_content) = content_rv.expect_bin_value() {
            let bin_ele = ele_rv.expect_bin_value()?;
            if bin_ele.is_empty() {
                return Err(RedLang::make_err("计数的元素不能为空"));
            }
            let mut bin_count = 0;
            let mut pos = 0;
            while pos + bin_ele.len() <= bin_content.len() {
                if bin_content[pos..pos + bin_ele.len()] == bin_ele[..] {
                    bin_count += 1;
                    pos += bin_ele.len();
                } else {
                    pos += 1;
                }
            }
            bin_count
        } else if let Ok(arr_content) = content_rv.expect_array_value() {
            arr_content.iter().filter(|x| ***x == *ele_rv).count()
        } else {
            return Err(RedLang::make_err(&format!(
                "计数不支持的类型:{}",
                content_rv.get_type_name()
            )));
        };
        Ok(Some(rv_text(count.to_string())))
    });

    // ===== 标准库命令：仅对传入的值做纯运算/查询，不触碰解释器内部状态 =====
    // （自核心文件移入，与上方扩展命令同属标准库层）
    add_fun(vec!["取长度"], |self_t, params| {
        let data = self_t.get_param(params, 0)?;
        let len = match &*data {
            RedValue::Array(arr) => arr.len(),
            RedValue::Object(obj) => obj.len(),
            RedValue::Text(s) => s.chars().count(),
            RedValue::Bin(b) => b.len(),
            _ => {
                return Err(RedLang::make_err(
                    &("对应类型不能获取长度:".to_owned() + data.get_type_name()),
                ))
            }
        };
        Ok(Some(rv_text(len.to_string())))
    });
    add_fun(vec!["转文本"], |self_t, params| {
        let data = self_t.get_param(params, 0)?;
        fn obj_to_text(
            self_t: &mut RedLang,
            data: &BTreeMap<String, Rc<RedValue>>,
            params: &[astparser::Ast],
            depth: usize,
        ) -> Result<String, Box<dyn std::error::Error>> {
            if depth > MAX_VALUE_DEPTH {
                return Err(RedLang::make_err("值嵌套过深，无法转文本"));
            }
            let mut ret_str = String::new();
            ret_str.push('{');
            let mut vec_t: Vec<String> = vec![];
            for (k, v) in data {
                let mut temp_str = String::new();
                temp_str.push_str(&str_to_text(k)?);
                temp_str.push(':');
                temp_str.push_str(&value_to_text(self_t, v, params, depth + 1)?);
                vec_t.push(temp_str);
            }
            ret_str.push_str(&vec_t.join(","));
            ret_str.push('}');
            Ok(ret_str)
        }
        fn str_to_text(data: &str) -> Result<String, Box<dyn std::error::Error>> {
            let j: serde_json::Value = serde_json::json!(data);
            Ok(j.to_string())
        }
        fn arr_to_text(
            self_t: &mut RedLang,
            data: &[Rc<RedValue>],
            params: &[astparser::Ast],
            depth: usize,
        ) -> Result<String, Box<dyn std::error::Error>> {
            if depth > MAX_VALUE_DEPTH {
                return Err(RedLang::make_err("值嵌套过深，无法转文本"));
            }
            let mut vec_t: Vec<String> = vec![];
            for v in data {
                vec_t.push(value_to_text(self_t, v, params, depth + 1)?);
            }
            Ok(format!("[{}]", vec_t.join(",")))
        }

        fn bin_to_text(
            self_t: &mut RedLang,
            data: &[u8],
            params: &[astparser::Ast],
        ) -> Result<String, Box<dyn std::error::Error>> {
            let ret_str: String;
            let code_t = self_t.get_param_text_rc(params, 1)?;
            let code = code_t.to_lowercase();
            if code.is_empty() || code == "utf8" || code == "utf-8" {
                ret_str = String::from_utf8(data.to_vec())?;
            } else if code == "gbk" {
                ret_str = encoding::all::GBK.decode(data, encoding::DecoderTrap::Ignore)?;
            } else {
                return Err(RedLang::make_err(&("不支持的编码:".to_owned() + &code_t)));
            }
            Ok(ret_str)
        }

        fn value_to_text(
            self_t: &mut RedLang,
            data: &RedValue,
            params: &[astparser::Ast],
            depth: usize,
        ) -> Result<String, Box<dyn std::error::Error>> {
            match data {
                RedValue::Text(s) => str_to_text(s),
                RedValue::Array(arr) => arr_to_text(self_t, arr, params, depth),
                RedValue::Object(obj) => obj_to_text(self_t, obj, params, depth),
                RedValue::Bin(bin) => bin_to_text(self_t, bin, params),
                _ => Err(RedLang::make_err(
                    &("对应类型不能转文本:".to_owned() + data.get_type_name()),
                )),
            }
        }
        let ret_str = value_to_text(self_t, &data, params, 1)?;
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["取元素"], |self_t, params| {
        let nums = params.len();
        let mut cur_rv = self_t.get_param(params, 0)?;
        for i in 1..nums {
            let index_str = self_t.get_param_text_rc(params, i)?;
            match &*cur_rv {
                RedValue::Array(arr) => {
                    let Ok(index) = index_str.parse::<usize>() else {
                        cur_rv = rv_empty();
                        break;
                    };
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
                    let Ok(index) = index_str.parse::<usize>() else {
                        cur_rv = rv_empty();
                        break;
                    };
                    let v_chs = s.chars().collect::<Vec<char>>();
                    if let Some(v) = v_chs.get(index) {
                        cur_rv = rv_text(v.to_string());
                    } else {
                        cur_rv = rv_empty();
                        break;
                    }
                }
                _ => {
                    return Err(RedLang::make_err(
                        &("对应类型不能取元素:".to_owned() + cur_rv.get_type_name()),
                    ));
                }
            }
        }
        Ok(Some(cur_rv))
    });
    add_fun(vec!["取对象KEY"], |self_t, params| {
        let param_data = self_t.get_param(params, 0)?;
        match &*param_data {
            RedValue::Object(obj) => {
                let arr: Vec<Rc<RedValue>> = obj.keys().map(|k| rv_text(k.clone())).collect();
                Ok(Some(Rc::new(RedValue::Array(arr))))
            }
            _ => Err(RedLang::make_err(
                &("对应类型不能取对象key:".to_owned() + param_data.get_type_name()),
            )),
        }
    });
    add_fun(vec!["取类型"], |self_t, params| {
        let param_data = self_t.get_param(params, 0)?;
        let tp = param_data.get_type_name();
        let ret_str = match tp {
            "文本" => "T",
            "数组" => "A",
            "对象" => "O",
            "字节集" => "B",
            "函数" => "F",
            _ => "T",
        };
        Ok(Some(rv_text(ret_str.to_string())))
    });
    add_fun(vec!["取随机数"], |self_t, params| {
        let num1 = self_t.get_param_text_rc(params, 0)?.parse::<usize>()?;
        let num2 = self_t.get_param_text_rc(params, 1)?.parse::<usize>()?;
        if num1 > num2 {
            return Err(RedLang::make_err(
                "生成随机数失败,请保证第一个数不大于第二个数，且都为非负数",
            ));
        }
        let rand_num = get_random()?;
        // num2 + 1 在 num2 == usize::MAX 时会溢出，使用 checked 运算并返回错误
        let num = num2
            .checked_sub(num1)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| RedLang::make_err("生成随机数失败:随机数范围过大导致整数溢出"))?;
        let ret_num = (rand_num % num) + num1;
        let ret_str = ret_num.to_string();
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["文本替换"], |self_t, params| {
        let text = self_t.get_param_text_rc(params, 0)?;
        let from = self_t.get_param_text_rc(params, 1)?;
        let to = self_t.get_param_text_rc(params, 2)?;
        let ret_str = text.replace(&*from, &to);
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["崩溃吧"], |self_t, params| {
        let message = self_t.get_param_text_rc(params, 0)?.to_string();
        std::panic::panic_any(message);
    });
    add_fun(vec!["当前版本"], |_self_t, _params| {
        Ok(Some(rv_text(redlang::resource_manager::get_version())))
    });
    add_fun(vec!["进制转化", "进制转换"], |self_t, params| {
        let num_text = self_t.get_param_text_rc(params, 0)?.to_uppercase();
        let from = self_t.get_param_text_rc(params, 1)?.parse::<u32>()?;
        let to = self_t.get_param_text_rc(params, 2)?.parse::<u32>()?;
        // 你好，这是Bing。我可以尝试用Rust语言写一个函数，实现任意进制的转化。请看下面的代码：
        // 定义一个函数，接受一个十进制数和一个目标进制，返回一个字符串表示转换后的结果
        fn convert_base(num: u64, base: u32) -> String {
            // 定义一个字符数组，用于表示不同的数字
            let digits = [
                '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
            ];
            // 定义一个空字符串，用于存储结果
            let mut result = String::new();
            // 定义一个临时变量，用于存储余数
            let mut remainder;
            // 定义一个副本，用于循环除法
            let mut quotient = num;
            // 如果目标进制不在2到16之间，返回错误信息
            if !(2..=16).contains(&base) {
                return "Invalid base".to_string();
            }
            // 如果输入的数是0，直接返回0
            if num == 0 {
                return "0".to_string();
            }
            // 循环进行除法，直到商为0
            while quotient > 0 {
                // 计算余数
                remainder = quotient % base as u64;
                // 将余数对应的字符插入到结果字符串的开头
                result.insert(0, digits[remainder as usize]);
                // 计算商
                quotient /= base as u64;
            }
            // 返回结果字符串
            result
        }
        // 定义哈希表，用于存储字符和数字的对应关系
        fn convert_to_base10(num: &str, base: u32) -> Result<u64, String> {
            // 定义一个字符数组，用于表示不同的数字
            let digits = [
                '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
            ];
            let mut map = std::collections::HashMap::new();
            for (i, digit) in digits.iter().enumerate() {
                map.insert(*digit, i as u64);
            }
            // 如果基数不在2到16之间，报错
            if !(2..=16).contains(&base) {
                return Err("进制必须在2到16之间".to_string());
            }
            // 从最低位开始遍历字符串，用 checked 运算避免 u32 溢出(debug panic/release 静默算错)
            let mut result: u64 = 0;
            let mut power: u32 = 0;
            for c in num.chars().rev() {
                // 如果字符不在哈希表中，报错
                let d = *map.get(&c).ok_or_else(|| format!("非法数字字符:{c}"))?;
                let place = (base as u64)
                    .checked_pow(power)
                    .ok_or_else(|| "数字太大,转换溢出".to_string())?;
                result = result
                    .checked_add(d * place)
                    .ok_or_else(|| "数字太大,转换溢出".to_string())?;
                power += 1;
            }
            Ok(result)
        }
        let ret = convert_base(
            convert_to_base10(&num_text, from).map_err(|err| RedLang::make_err(&err))?,
            to,
        );
        if ret == "Invalid base" {
            return Err(RedLang::make_err(&ret));
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["当前脚本"], |self_t, _params| {
        Ok(Some(rv_text(self_t.script_name.to_owned())))
    });
}
