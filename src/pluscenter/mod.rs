use std::path::PathBuf;
use std::time::SystemTime;
use std::{error::Error, str::FromStr};

use tokio::sync::Mutex;

use serde_derive::Serialize;

use serde_derive::Deserialize;

use crate::cqapi::{cq_add_log, cq_add_log_w, cq_get_app_directory1, get_tmp_dir};
use crate::{initevent, G_PKG_NAME, G_SCRIPT};
use crate::mytool::{download_github, read_json_or_default};
use crate::mytool::{github_proxy, read_json_str};

#[derive(Serialize, Deserialize, Debug)]
pub struct PlusCenterPlusBase{
    pub repo:String,
    pub branch:String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlusCenterPlusInfo{
    pub repo:String,
    pub branch:String,
    pub name:String,
    pub description:String,
    pub author:String,
	pub version:String,
	pub need_python:bool
}

pub async fn get_proxy() -> Result<String,Box<dyn Error + Send + Sync>> {
    lazy_static! {
        static ref G_PROXY:Mutex<(Option<String>,u64)> = Mutex::new((None,0));
    }
    let mut lk = G_PROXY.lock().await;
    if lk.0.is_none() {
        let proxy_opt = github_proxy().await;
        if proxy_opt.is_none() {
            return Err("cann't connect to pluscenter".into());
        }else{
            let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            (*lk).0 = proxy_opt;
            (*lk).1 = tm;
            return Ok(lk.0.clone().unwrap());
        }
    }else{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        if lk.1 + 60 < tm {
            lk.0 = None;
            let proxy_opt = github_proxy().await;
            if proxy_opt.is_none() {
                return Err("cann't connect to pluscenter".into());
            }else{
                let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                (*lk).0 = proxy_opt;
                (*lk).1 = tm;
                return Ok(lk.0.clone().unwrap());
            }
        }else {
            return Ok(lk.0.clone().unwrap());
        }
    }
}

pub async fn get_plus_list() -> Result<Vec<PlusCenterPlusBase>,Box<dyn Error + Send + Sync>> {
    let info_url = "https://raw.githubusercontent.com/super1207/redreplyhub/main/plugins.json";
    let proxy_url = get_proxy().await?;
    let req_url = format!("{proxy_url}{info_url}");
    let resp = reqwest::get(&req_url).await?;
    let json_str = resp.text().await?;
    let json = serde_json::Value::from_str(&json_str)?;
    let json_arr = json.as_array().ok_or("plugins.json not array")?;
    let mut ret_vec = vec![];
    for it in json_arr {
        let t = PlusCenterPlusBase {
            repo: read_json_str(it, "repo"),
            branch: read_json_str(it, "branch"),
        };
        ret_vec.push(t);
        
    }
    Ok(ret_vec)
}


pub async fn get_plus_info(plus:&PlusCenterPlusBase) -> Result<PlusCenterPlusInfo,Box<dyn Error + Send + Sync>> {
    let proxy = get_proxy().await?;
    let req_url = format!("{}https://raw.githubusercontent.com/{}/{}/app.json",proxy,plus.repo,plus.branch);
    let resp = reqwest::get(&req_url).await?;
    let json_str = resp.text().await?;
    let json = serde_json::Value::from_str(&json_str)?;
    let need_python = read_json_or_default(&json, "need_python", &serde_json::Value::from(false)).as_bool().unwrap_or(false);
    Ok(PlusCenterPlusInfo {
        repo:plus.repo.to_owned(),
        branch:plus.branch.to_owned(),
        name: read_json_str(&json, "name"),
        description: read_json_str(&json, "description"),
        author: read_json_str(&json, "author"),
        version: read_json_str(&json, "version"),
        need_python
    })
}

fn extrat(from:&str,to:&str) -> Result<(),Box<dyn Error + Send + Sync>>{
    let file = std::fs::File::open(from)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => {
                // write by chatgpt4
                let deal_path = path;
                let components: Vec<_> = deal_path.components().collect();
                println!("components:{components:?}");
                if components.len() > 1 {
                    // 从第二个组件开始收集，直到倒数第二个（不包括最后一个组件）
                    let new_path = components[1..components.len()]
                        .iter()
                        .map(|c| c.as_os_str())
                        .collect::<PathBuf>();
                    PathBuf::from_str(to)?.join(new_path)
                } else {
                    continue;
                    //return Err("Path is too short to remove the last component".into());
                }
            },
            None => continue,
        };

        
        {
            let comment = file.comment();
            if !comment.is_empty() {
                cq_add_log(&format!("File {i} comment: {comment}")).unwrap();
            }
        }

        if (*file.name()).ends_with('/') {
            cq_add_log(&format!("File {} extracted to \"{}\"", i, outpath.display())).unwrap();
            std::fs::create_dir_all(&outpath)?;
        } else {
            cq_add_log(&format!(
                "File {} extracted to \"{}\" ({} bytes)",
                i,
                outpath.display(),
                file.size()
            )).unwrap();
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p)?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}


pub async fn install_plus(repo:&str,name:&str,version:&str) -> Result<(),Box<dyn Error + Send + Sync>> {
    let req_url = format!("https://github.com/{}/archive/refs/tags/{}.zip",repo,version);
    println!("req_url {req_url}");
    let plus_dir_str = cq_get_app_directory1()?;
    let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
    std::fs::create_dir_all(&pkg_dir)?;
    let tmp_dir = get_tmp_dir()?;
    let tmp_file = PathBuf::from_str(&tmp_dir)?.join(uuid::Uuid::new_v4().to_string());
    download_github(&req_url,&tmp_file.to_string_lossy()).await?;
    let ret_name = pkg_dir.join(&name);
    let (tx, rx) =  tokio::sync::oneshot::channel();
    let tmp_file_t = tmp_file.clone();
    let ret_name_t = ret_name.clone();
    tokio::task::spawn_blocking(move ||{
        let rst = extrat(&tmp_file_t.to_string_lossy(),&ret_name_t.to_string_lossy());  
        if rst.is_ok() {
            tx.send("".to_owned()).unwrap();
        }else{
            tx.send(format!("err:{:?}",rst.err().unwrap())).unwrap();
        }
    });
    // 删除临时文件
    let _ = tokio::fs::remove_file(tmp_file).await;
    let ret = rx.await.unwrap();
    if ret != "" {
        return Err(ret.into());
    }

    // 更新内存中的脚本

    let mut new_script = vec![];
    // 读取已有脚本
    {
        let wk = G_SCRIPT.read().unwrap();
        for it in wk.as_array().ok_or("read G_SCRIPT err")? {
            let it_name = read_json_str(it, "pkg_name");
            if it_name != name {
                new_script.push(it.to_owned());
            }
        }
    }
    // 更新新增脚本
    let ret_scripts = ret_name.join("script.json");
    let scripts_str = tokio::fs::read_to_string(ret_scripts).await?;
    let mut scripts:serde_json::Value = serde_json::from_str(&scripts_str)?;
    for it in scripts.as_array_mut().ok_or("script.json not array")? {
        let obj_mut = it.as_object_mut().ok_or("script obj not object")?;
        obj_mut.insert("pkg_name".to_owned(), serde_json::json!(name));
        new_script.push(it.to_owned());
    }
    {
        let mut wk = G_SCRIPT.write().unwrap();
        (*wk) = serde_json::Value::Array(new_script);
    }
    // 添加新脚本名
    G_PKG_NAME.write().unwrap().insert(name.to_owned());

    // 执行初始化脚本，不用等待
    let name_t = name.to_owned();
    tokio::task::spawn_blocking(move ||{
        if let Err(err) = initevent::do_init_event(Some(&name_t)){
            cq_add_log_w(&err.to_string()).unwrap();
        }
    });
    
    Ok(())
}