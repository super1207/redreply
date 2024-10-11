
function validateFileName(fileName ){
	//var fileName = 'a.html';
	var reg = new RegExp('[\\\\/:*?\"<>|]');
	if (reg.test(fileName)) {
	    //"上传的文件名不能包含【\\\\/:*?\"<>|】这些非法字符,请修改后重新上传!";
	    return false;
	}
	return true;
}
function randomString(e) {    
    e = e || 32;
    var t = "123456789",
    a = t.length,
    n = "";
    for (i = 0; i < e; i++) n += t.charAt(Math.floor(Math.random() * a));
    return n
}

const { createApp } = Vue


last_text = "";

app = createApp({
    data() {
        return {
            // 用于显示
            script_name:"名字",
            script_description:"介绍",
            script_keyword:"关键词",
            script_ppfs:"匹配方式",
            script_cffs:"触发方式",
            select_name_index: -1,
            // 用于记录所有数据
            codes: "正在加载内容...",
            pkg_codes : {"默认包":[]},
            select_pkg_name:"默认包",
            version:"",
            new_pkg_name:"",
            rename_pkg_name:"",
            rename_pkg_process:[],
            can_emit_select:true,
            composing:false,
            last_code:"",
            last_change_time:(new Date()).valueOf(),
            last_index:0,
            playbkmusic:"播放背景音乐",
            is_load_success:false,
        }
    },
    mounted () {

        setInterval(() => {
            quill = this.$refs.child.getQuill();
            let sec = quill.getSelection()
            let curr_index = 0
            if(sec){
                curr_index = sec.index
            }
            
            let code = quill.getText();
            tm = (new Date()).valueOf()
            if ((code!= this.last_code || curr_index != this.last_index) && !this.composing && tm - this.last_change_time > 300) {
                this.highlight()
                this.last_code = code
                this.last_index = curr_index
            }
        }, 500)

        // 对复制进行hook，解决复制时多余的换行
        ele = document.getElementById("script_content")
        ele.oncopy = (e) => {
            quill = this.$refs.child.getQuill()
            range = quill.getSelection()
            e.clipboardData.setData('text/plain', quill.getText(range.index, range.length));
            e.preventDefault();
        }
        
        // 处理中文输入
        ele.addEventListener('compositionstart',(e) =>{
            this.composing = true
            console.log('compositionstart')
        })
        ele.addEventListener('compositionend',(e) =>{    
            this.composing = false
            console.log('compositionend')
            this.last_change_time = (new Date()).valueOf()
        })

        quill = this.$refs.child.getQuill()
        quill.on('text-change', (delta, oldDelta, source) => {
            if (source == 'user') {
                if(!this.composing){
                    this.last_change_time = (new Date()).valueOf()
                }
            }
        });

        quill.on('selection-change', (range, oldDelta, source) => {
            if (source == 'user') {
                if(!this.composing){
                    this.last_change_time = (new Date()).valueOf()
                }
            }
        });


        axios
        .get("/get_version")
        .then(
        res => {
            this.version = res.data["data"];
        })
        .catch(function (error) {
            console.log(error);
        });
        axios
        .get("/get_all_pkg_name")
        .then(
        res => {
            let ret = res.data["data"];
            // console.log(ret)
            this.pkg_codes["默认包"] = []
            for(let i = 0;i < ret.length;++i) {
                this.pkg_codes[ret[i]] = []
            }
            axios
            .get("/get_code")
            .then(
            res => {
                this.codes = res.data["data"]
                for(let i = 0;i < this.codes.length;++i) {
                    let pkg_name = this.codes[i]["pkg_name"]
                    if(pkg_name == undefined) {
                        pkg_name = "默认包"
                    }
                    if(this.pkg_codes[pkg_name] == undefined) {
                        this.pkg_codes[pkg_name] = []
                    }
                    this.pkg_codes[pkg_name].push(this.codes[i])
                }
                this.is_load_success = true
                // console.log(this.pkg_codes)
            })
            .catch(function (error) {
                console.log(error);
            });
        })
        .catch(function (error) {
            console.log(error);
        });
        
    },
    methods: {
        select_name_index_change(new_select) {
            // 切换新数据
            if(new_select != -1){
                this.script_name = this.pkg_codes[this.select_pkg_name][new_select]["name"]
                this.script_description = this.pkg_codes[this.select_pkg_name][new_select]["description"]
                this.script_keyword = this.pkg_codes[this.select_pkg_name][new_select]["content"]["关键词"]
                this.script_ppfs = this.pkg_codes[this.select_pkg_name][new_select]["content"]["匹配方式"]
                this.script_cffs = this.pkg_codes[this.select_pkg_name][new_select]["content"]["触发方式"]
                this.$refs.child.setText(this.pkg_codes[this.select_pkg_name][new_select]["content"]["code"])
                //this.highlight()
            }else{
                this.script_name = ""
                this.script_description = ""
                this.script_keyword = ""
                this.script_ppfs = ""
                this.script_cffs = ""
                this.$refs.child.setText("")
            }
        },
        // 缓存旧数据
        save_cache(old_select) {
            if(old_select != -1){
                this.pkg_codes[this.select_pkg_name][old_select]["pkg_name"] = this.select_pkg_name;
                this.pkg_codes[this.select_pkg_name][old_select]["name"] = this.script_name;
                this.pkg_codes[this.select_pkg_name][old_select]["description"] = this.script_description;
                this.pkg_codes[this.select_pkg_name][old_select]["content"]["关键词"] = this.script_keyword;
                this.pkg_codes[this.select_pkg_name][old_select]["content"]["匹配方式"] = this.script_ppfs;
                this.pkg_codes[this.select_pkg_name][old_select]["content"]["触发方式"] = this.script_cffs;
                let k = this.$refs.child.getText();
                this.pkg_codes[this.select_pkg_name][old_select]["content"]["code"] = k;
            }
        },
        save_code() {

            if(this.is_load_success == false)
            {
                alert("数据未加载成功，禁止保存，请稍后再试,或尝试刷新页面")
                return
            }

            this.save_cache(this.select_name_index);
            let code = []

            code.push(this.rename_pkg_process)

            let keys = []
            for(let k in this.pkg_codes) {
                if(k != "默认包") {
                    keys.push(k);
                }
            }
            code.push(keys)

            

            for(let k in this.pkg_codes) {
                
                if(k == "默认包"){
                    for(let it in this.pkg_codes[k])
                    {
                        let kk = JSON.parse(JSON.stringify(this.pkg_codes[k][it]))
                        delete kk["pkg_name"]
                        code.push(kk)
                    }
                }else
                {
                    for(let it in this.pkg_codes[k])
                    {
                        code.push(this.pkg_codes[k][it])
                    }
                }
                
            }
            axios
            .post("/set_code",code)
            .then((res) => {
                if(res.data['retcode'] == 0){
                    this.rename_pkg_process = []
                    alert("保存成功")
                }else {
                    alert("保存失败")
                }
            })
            .catch(function (error) {
                console.log(error);
                alert("保存失败")
            });
            
        },
        add_code() {
            this.save_cache(this.select_name_index);
            this.pkg_codes[this.select_pkg_name].push({"pkg_name":this.select_pkg_name,"name":"code_name","description":"code_description","content":{"关键词":"222","触发方式":"群聊触发","匹配方式":"完全匹配","code":"hello"}})
            this.select_name_index = this.pkg_codes[this.select_pkg_name].length - 1;
            this.select_name_index_change(this.select_name_index)
        },
        del_code() {
            if(this.select_name_index != -1){
                this.pkg_codes[this.select_pkg_name].splice(this.select_name_index,1);
                this.select_name_index = -1
            }
        },
        help_web() {
            window.open("/docs/index.html#/detailref/", "_blank");
        },
        watch_log() {
            window.open("/watchlog.html", "_blank");
        },
        debug_btn() {
            window.open("/debug.html", "_blank");
        },
        pluscenter_btn(){
            window.open("/pluscenter.html", "_blank");
        },
        playmusic_btn(){
            let audio = this.$refs.mybackmusic
            if(this.playbkmusic == "播放背景音乐"){
                audio.play()
                this.playbkmusic = "暂停播放背景音乐"
            }else{
                audio.pause()
                this.playbkmusic = "播放背景音乐"
            }
        },
        play_music(){
            window.open("/pluscenter.html", "_blank");
        },
        quit_redreply() {
            let is_quit = confirm("是否真的要退出强大的红色问答？")
            if(is_quit){
                setTimeout(function(){
                    location.reload();
                },1000);
                axios.get("/close")
            }
        },
        connect_ob() {
            window.open("/obconnect.html", "_blank");
        },
        gobal_filter_btn() {
            window.open("/gobal_filter.html","_blank");
        },
        select_up() {
            if(this.select_name_index <= 0){
                return;
            }
            this.save_cache(this.select_name_index);
            t = this.pkg_codes[this.select_pkg_name][this.select_name_index]
            this.pkg_codes[this.select_pkg_name][this.select_name_index] = this.pkg_codes[this.select_pkg_name][this.select_name_index - 1]
            this.pkg_codes[this.select_pkg_name][this.select_name_index - 1] = t
            this.select_name_index_change(this.select_name_index - 1);
            this.select_name_index = this.select_name_index - 1
        },
        select_down() {
            if(this.select_name_index < 0){
                return;
            }
            if(this.select_name_index == this.pkg_codes[this.select_pkg_name].length - 1){
                return;
            }
            this.save_cache(this.select_name_index);
            t = this.pkg_codes[this.select_pkg_name][this.select_name_index]
            this.pkg_codes[this.select_pkg_name][this.select_name_index] = this.pkg_codes[this.select_pkg_name][this.select_name_index + 1]
            this.pkg_codes[this.select_pkg_name][this.select_name_index + 1] = t
            this.select_name_index_change(this.select_name_index + 1);
            this.select_name_index = this.select_name_index + 1
        },
        the_other(event) {
            this.new_pkg_name = "包_"+randomString(4)
            this.rename_pkg_name = this.select_pkg_name
            document.getElementById('other_dlg').showModal();
        },
        other_close(event) {
            document.getElementById('other_dlg').close();
        },
        pkg_create(event) {
            if (this.new_pkg_name == "") {
                alert("失败，包名不能为空")
            }
            else if(validateFileName(this.new_pkg_name) == false) {
                alert("失败，包名不能包含【\\\\/:*?\"<>|】这些非法字符")
            }
            else if(this.pkg_codes.hasOwnProperty(this.new_pkg_name)) {
                alert("失败，包名已经存在")
            }
            else {
                this.pkg_codes[this.new_pkg_name] = []
                this.save_cache(this.select_name_index);
                this.select_name_index_change(-1);
                this.select_name_index=-1;
                this.select_pkg_name = this.new_pkg_name
                document.getElementById('other_dlg').close()
            }  
        },
        cron(){
            window.open("/crontool.html", "_blank");
        },
        pkg_rename(event) {
            if (this.rename_pkg_name == "") {
                alert("失败，包名不能为空")
            }
            else if (this.rename_pkg_name == this.select_pkg_name) {
                alert("失败，新旧包名一样")
            }
            else if(validateFileName(this.rename_pkg_name) == false) {
                alert("失败，包名不能包含【\\\\/:*?\"<>|】这些非法字符")
            }
            else if(this.pkg_codes.hasOwnProperty(this.rename_pkg_name)) {
                alert("失败，包名已经存在")
            }
            else if(this.select_pkg_name == "默认包") {
                alert("失败，不可以修改默认包")
            }
            else {
                this.rename_pkg_process.push([this.select_pkg_name,this.rename_pkg_name])
                this.pkg_codes[this.rename_pkg_name] = this.pkg_codes[this.select_pkg_name]
                delete this.pkg_codes[this.select_pkg_name]
                this.select_pkg_name = this.rename_pkg_name
                for(let it in this.pkg_codes[this.rename_pkg_name])
                {
                    this.pkg_codes[this.rename_pkg_name][it]['pkg_name'] = this.rename_pkg_name;
                }
                document.getElementById('other_dlg').close()
            }  
        },
        pkg_delete(event) {
            if(this.select_pkg_name == "默认包") {
                alert("失败，不可以删除默认包")
            }
            else if(this.pkg_codes.hasOwnProperty(this.select_pkg_name) == false)
            {
                alert("失败，没有这个包")
            }
            else
            {
                this.select_name_index=-1;
                delete this.pkg_codes[this.select_pkg_name]
                this.select_pkg_name = "默认包"
                document.getElementById('other_dlg').close()
            }
        },
        highlight()
        {
            var current_color = 0;
            function next_color(){
                current_color = (current_color + 1) % 4;
            }
            function pre_color(){
                current_color = (current_color + 3) % 4;
            }
            var colorList = ["#000000","#FF0000","#0000FF","#008000"]
            function ColorReverse(OldColorValue){
                var OldColorValue = "0x"+OldColorValue.replace(/#/g,"");
                var str="000000"+(0xFFFFFF-OldColorValue).toString(16);
                return '#' + str.substring(str.length-6,str.length);
            }
            function out_text(quill,index){
                colorList[current_color](quill,index)
            }

            quill = this.$refs.child.getQuill();
            
            select = quill.getSelection()
            let curr_index = 0
            if(select){
                curr_index = select.index
            }

            let code = quill.getText();
            content = []
            for(let i = 0;i<code.length;i++){
                if(code[i] == "【"){
                    next_color()
                    content.push({
                        "insert":code[i],attributes:{color:colorList[current_color]}
                    })
                }
                else if(code[i] == "】"){
                    content.push({
                        "insert":code[i],attributes:{color:colorList[current_color]}
                    })
                    pre_color()
                }
                else if(code[i] == "\\"){
                    content.push({
                        "insert":code[i],attributes:{color:colorList[current_color]}
                    })
                    i += 1
                    content.push({
                        "insert":code[i],attributes:{color:colorList[current_color]}
                    })
                }
                else{
                    if(code[i] == '\xa0'){
                        content.push({
                            "insert":" ",attributes:{color:colorList[current_color]}
                        })
                    }else {
                        content.push({
                            "insert":code[i],attributes:{color:colorList[current_color]}
                        })
                    }
                }
            }
            // 在这里处理光标
            if(curr_index >= 0 && curr_index < content.length){
                let t = 0;
                if(content[curr_index].insert == "【"){
                    t = 1;
                    content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                    while(curr_index < content.length){
                        curr_index += 1
                        if(curr_index >= content.length)break
                        if(content[curr_index].insert == "\\"){
                            curr_index += 1
                        }else if(content[curr_index].insert == "【"){
                            t += 1
                        }else if(content[curr_index].insert == "】"){
                            t -= 1
                        }
                        if(t == 0){
                            content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                            break
                        }
                    }
                }else if(content[curr_index].insert == "】") {
                    t = 1;
                    content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                    while(curr_index >= 0){
                        curr_index -= 1
                        if(curr_index < 0)break
                        if(curr_index - 1 > 0 && content[curr_index - 1].insert == "\\") {
                            curr_index -= 1
                        }else if(content[curr_index].insert == "【"){
                            t -= 1
                        }else if(content[curr_index].insert == "】"){
                            t += 1
                        }
                        if(t == 0){
                            content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                            break
                        }
                    }
                }else if(curr_index - 1 >=0){
                    curr_index -= 1;
                    if(content[curr_index].insert == "【"){
                        t = 1;
                        content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                        while(curr_index < content.length){
                            curr_index += 1
                            if(curr_index >= content.length)break
                            if(content[curr_index].insert == "\\"){
                                curr_index += 1
                            }else if(content[curr_index].insert == "【"){
                                t += 1
                            }else if(content[curr_index].insert == "】"){
                                t -= 1
                            }
                            if(t == 0){
                                content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                                break
                            }
                        }
                    }else if(content[curr_index].insert == "】") {
                        t = 1;
                        content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                        while(curr_index >= 0){
                            curr_index -= 1
                            if(curr_index < 0)break
                            if(curr_index - 1 > 0 && content[curr_index - 1].insert == "\\") {
                                curr_index -= 1
                            }else if(content[curr_index].insert == "【"){
                                t -= 1
                            }else if(content[curr_index].insert == "】"){
                                t += 1
                            }
                            if(t == 0){
                                content[curr_index].attributes.background = ColorReverse(content[curr_index].attributes.color)
                                break
                            }
                        }
                    }
                }
            }
            
            quill.setContents(content)
            quill.setSelection(select)
        },
        undo(){
            quill = this.$refs.child.getQuill();
            let history = quill.history;
            return history.undo();
        },
        redo(){
            quill = this.$refs.child.getQuill();
            let history = quill.history;
            return history.redo();
        }
    }
})

app.component('QuillEditor', VueQuill.QuillEditor);
app.mount('#app')