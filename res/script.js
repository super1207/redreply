const { createApp } = Vue
            createApp({
                data() {
                    return {
                        // 用于显示
                        script_name:"名字",
                        script_description:"介绍",
                        script_keyword:"关键词",
                        script_ppfs:"匹配方式",
                        script_cffs:"触发方式",
                        script_content:"脚本内容",
                        select_name_index: -1,
                        // 用于记录所有数据
                        codes: "正在加载内容..."
                    }
                },
                mounted () {
                    axios
                    .get("/get_code")
                    .then(res => (this.codes = res.data["data"]))
                    .catch(function (error) {
                        console.log(error);
                    });
                },
                methods: {
                    select_name_index_change(new_select) {
                        // 切换新数据
                        if(new_select != -1){
                            this.script_name = this.codes[new_select]["name"]
                            this.script_description = this.codes[new_select]["description"]
                            this.script_keyword = this.codes[new_select]["content"]["关键词"]
                            this.script_ppfs = this.codes[new_select]["content"]["匹配方式"]
                            this.script_cffs = this.codes[new_select]["content"]["触发方式"]
                            this.script_content = this.codes[new_select]["content"]["code"]
                        }
                    },
                    // 缓存旧数据
                    save_cache(old_select) {
                        if(old_select != -1){
                            this.codes[old_select]["name"] = this.script_name;
                            this.codes[old_select]["description"] = this.script_description;
                            this.codes[old_select]["content"]["关键词"] = this.script_keyword;
                            this.codes[old_select]["content"]["匹配方式"] = this.script_ppfs;
                            this.codes[old_select]["content"]["触发方式"] = this.script_cffs;
                            this.codes[old_select]["content"]["code"] = this.script_content;
                        }
                    },
                    save_code() {
                        this.save_cache(this.select_name_index);
                        axios
                        .post("/set_code",this.codes)
                        .then(function (res){
                            alert("保存成功")
                        })
                        .catch(function (error) {
                            console.log(error);
                            alert("保存失败")
                        });
                        
                    },
                    add_code() {
                        this.save_cache(this.select_name_index);
                        this.codes.push({"name":"code_name","description":"code_description","content":{"关键词":"222","触发方式":"群聊触发","匹配方式":"完全匹配","code":"hello"}})
                        this.select_name_index = this.codes.length - 1;
                        this.select_name_index_change(this.select_name_index)
                    },
                    del_code() {
                        if(this.select_name_index != -1){
                            this.codes.splice(this.select_name_index,1);
                            this.select_name_index = -1
                        }
                    },
                    help_web() {
                        window.open("/readme.html", "_blank");
                    },
                    watch_log() {
                        window.open("/watchlog.html", "_blank");
                    },
                    quit_redreply() {
                        axios.get("/close")
                        location.reload()
                    }
                }
            }).mount('#app')