use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::rc::Rc;
use std::sync::RwLock;

use serde_json::Value;

use super::{astparser, RedLang, RedValue, RedValueData};

pub type CommandFunction =
    fn(&mut RedLang, &[astparser::Ast]) -> Result<Option<Rc<RedValue>>, Box<dyn Error>>;
type TemporaryConstant = (RedValueData, u128);

const PACKAGE_COMMAND_MARKER: &str = "eb4d8f3e-1c82-653b-5b26-3be3abb007bc";

/// 脚本及包级运行时状态。
///
/// 这个 Store 只负责数据归属和锁，不负责脚本生命周期控制；生命周期
/// 由 `ControlPlane` 和 `lifecycle` 模块负责。
pub struct ScriptStore {
    scripts: RwLock<Value>,
    package_names: RwLock<HashSet<String>>,
    const_map: RwLock<HashMap<String, HashMap<String, RedValueData>>>,
    temp_const_map: RwLock<HashMap<String, HashMap<String, TemporaryConstant>>>,
    command_map: RwLock<HashMap<String, HashMap<String, String>>>,
    command_functions: RwLock<HashMap<String, CommandFunction>>,
    global_filter_code: RwLock<Option<String>>,
    global_init_code: RwLock<Option<String>>,
}

impl ScriptStore {
    pub fn new() -> Self {
        Self {
            scripts: RwLock::new(Value::Array(vec![])),
            package_names: RwLock::new(HashSet::new()),
            const_map: RwLock::new(HashMap::new()),
            temp_const_map: RwLock::new(HashMap::new()),
            command_map: RwLock::new(HashMap::new()),
            command_functions: RwLock::new(HashMap::new()),
            global_filter_code: RwLock::new(None),
            global_init_code: RwLock::new(None),
        }
    }

    pub fn read_scripts(&self) -> Value {
        self.scripts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn replace_scripts(&self, scripts: Value) {
        *self.scripts.write().unwrap_or_else(|e| e.into_inner()) = scripts;
    }

    pub fn package_names(&self) -> Vec<String> {
        self.package_names
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    pub fn replace_package_names<I>(&self, package_names: I)
    where
        I: IntoIterator<Item = String>,
    {
        let mut names = self
            .package_names
            .write()
            .unwrap_or_else(|e| e.into_inner());
        names.clear();
        names.extend(package_names);
    }

    pub fn add_package_name(&self, package_name: impl Into<String>) {
        self.package_names
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(package_name.into());
    }

    pub fn remove_package_name(&self, package_name: &str) {
        self.package_names
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(package_name);
    }

    pub fn contains_package_name(&self, package_name: &str) -> bool {
        self.package_names
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(package_name)
    }

    pub fn set_const(
        &self,
        pkg_name: &str,
        val_name: &str,
        val: RedValueData,
    ) -> Result<(), Box<dyn Error>> {
        let mut const_map = self.const_map.write().unwrap_or_else(|e| e.into_inner());
        const_map
            .entry(pkg_name.to_owned())
            .or_default()
            .insert(val_name.to_owned(), val);
        Ok(())
    }

    pub fn get_const(
        &self,
        pkg_name: &str,
        val_name: &str,
    ) -> Result<Option<RedValueData>, Box<dyn Error>> {
        Ok(self
            .const_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(pkg_name)
            .and_then(|values| values.get(val_name))
            .cloned())
    }

    pub fn clear_expired_temp_consts(&self) -> Result<(), Box<dyn Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let mut temp_const_map = self
            .temp_const_map
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mut expired = Vec::new();
        for (pkg_name, values) in &*temp_const_map {
            for (name, (_, expire_time)) in values {
                if *expire_time < now {
                    expired.push((pkg_name.clone(), name.clone()));
                }
            }
        }
        for (pkg_name, name) in expired {
            if let Some(values) = temp_const_map.get_mut(&pkg_name) {
                values.remove(&name);
            }
        }
        Ok(())
    }

    pub fn set_temp_const(
        &self,
        pkg_name: &str,
        val_name: &str,
        val: RedValueData,
        expire_time: u128,
    ) -> Result<(), Box<dyn Error>> {
        self.clear_expired_temp_consts()?;
        let mut temp_const_map = self
            .temp_const_map
            .write()
            .unwrap_or_else(|e| e.into_inner());
        temp_const_map
            .entry(pkg_name.to_owned())
            .or_default()
            .insert(val_name.to_owned(), (val, expire_time));
        Ok(())
    }

    pub fn get_temp_const(
        &self,
        pkg_name: &str,
        val_name: &str,
    ) -> Result<Option<RedValueData>, Box<dyn Error>> {
        let temp_const_map = self
            .temp_const_map
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let Some((value, expire_time)) = temp_const_map
            .get(pkg_name)
            .and_then(|values| values.get(val_name))
        else {
            return Ok(None);
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        if *expire_time < now {
            Ok(None)
        } else {
            Ok(Some(value.clone()))
        }
    }

    pub fn define_command(
        &self,
        pkg_name: &str,
        command_name: String,
        command_body: String,
    ) -> Result<(), Box<dyn Error>> {
        self.command_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(pkg_name.to_owned())
            .or_default()
            .insert(command_name, command_body);
        Ok(())
    }

    pub fn get_command(&self, pkg_name: &str, command_name: &str) -> Option<String> {
        self.command_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(pkg_name)
            .and_then(|commands| commands.get(command_name))
            .cloned()
    }

    pub fn add_command_function(&self, names: Vec<&str>, function: CommandFunction) {
        // 重复注册属于开发期错误，按 fail-fast 理念直接 panic（release 下 abort），
        // 让问题在启动时立刻暴露，而不是带着错误行为继续运行
        let mut functions = self
            .command_functions
            .write()
            .unwrap_or_else(|e| e.into_inner());
        for name in names {
            let normalized = name.to_uppercase();
            let normalized_full_width = super::text::str_to_ft(&normalized);
            if normalized == normalized_full_width {
                if functions.contains_key(&normalized) {
                    panic!("不可以重复添加命令:{}", normalized);
                }
                functions.insert(normalized, function);
            } else {
                if functions.contains_key(&normalized) {
                    panic!("不可以重复添加命令:{}", normalized);
                }
                functions.insert(normalized, function);
                if functions.contains_key(&normalized_full_width) {
                    panic!("不可以重复添加命令:{}", normalized_full_width);
                }
                functions.insert(normalized_full_width, function);
            }
        }
    }

    pub fn get_command_function(
        &self,
        pkg_name: &str,
        command_name: &str,
    ) -> Option<CommandFunction> {
        let functions = self
            .command_functions
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let package_command = format!("{pkg_name}{PACKAGE_COMMAND_MARKER}{command_name}");
        functions
            .get(&package_command)
            .copied()
            .or_else(|| functions.get(command_name).copied())
    }

    pub fn add_package_command_function(
        &self,
        pkg_name: &str,
        command_name: String,
        function: CommandFunction,
    ) {
        self.command_functions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                format!("{pkg_name}{PACKAGE_COMMAND_MARKER}{command_name}"),
                function,
            );
    }

    pub fn remove_package_memory(&self, pkg_name: &str) {
        self.const_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(pkg_name);
        self.temp_const_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(pkg_name);
        self.command_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(pkg_name);

        let prefix = format!("{pkg_name}{PACKAGE_COMMAND_MARKER}");
        self.command_functions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|name, _| !name.starts_with(&prefix));
    }

    pub fn global_filter_code(&self) -> Option<String> {
        self.global_filter_code
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_global_filter_code(&self, code: String) {
        *self
            .global_filter_code
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(code);
    }

    pub fn global_init_code(&self) -> Option<String> {
        self.global_init_code
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_global_init_code(&self, code: String) {
        *self
            .global_init_code
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(code);
    }
}

impl Default for ScriptStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptStore;

    #[test]
    fn script_store_tracks_package_names_and_scripts() {
        let store = ScriptStore::new();
        store.add_package_name("demo");
        assert!(store.contains_package_name("demo"));
        store.replace_scripts(serde_json::json!([{ "pkg_name": "demo" }]));
        assert_eq!(
            store.read_scripts(),
            serde_json::json!([{ "pkg_name": "demo" }])
        );
        store.remove_package_name("demo");
        assert!(!store.contains_package_name("demo"));
    }
}
