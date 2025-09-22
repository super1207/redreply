const globalOptions = {
    modules: {
        history: {
            delay: 500,
            maxStack: 100,
            userOnly: true
        },
        toolbar: "",
        keyboard: {
            bindings: {
                'list autofill': {
                    key: ' ',
                    prefix: /^-$/,
                    handler: () => true
                }
            }
        }
    },
    placeholder: '脚本内容',
    theme: 'snow'
};
VueQuill.QuillEditor.props.globalOptions.default = () => globalOptions;

const QuillMixin = {
    data() {
        return {
            composing: false,
            last_code: "",
            last_change_time: (new Date()).valueOf(),
            last_index: 0
        }
    },
    mounted() {
        let quill = this.$refs.child.getQuill();

        setInterval(() => {
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

        quill.root.addEventListener('paste', (evt) => {
            evt.preventDefault();
            const text = (evt.clipboardData || window.clipboardData).getData('text/plain');
            const range = quill.getSelection(false); // 不自动修正
            if (range) {
                if (range.length > 0) {
                    quill.deleteText(range.index, range.length, 'user');
                }
                quill.insertText(range.index, text, 'user');
                quill.setSelection(range.index + text.length, 0, 'user');
            }
        }, true); 

        let ele = document.getElementById("script_content")
        ele.oncopy = (e) => {
            quill = this.$refs.child.getQuill()
            range = quill.getSelection()
            e.clipboardData.setData('text/plain', quill.getText(range.index, range.length));
            e.preventDefault();
        }
        
		ele.oncut = (e) => {
            const quill = this.$refs.child.getQuill();
            const range = quill.getSelection();
            if (range && range.length > 0) {
                e.clipboardData.setData('text/plain', quill.getText(range.index, range.length));
                quill.deleteText(range.index, range.length);
            }
            e.preventDefault();
        };
        
        ele.addEventListener('compositionstart',(e) =>{
            this.composing = true
            console.log('compositionstart')
        })
        ele.addEventListener('compositionend',(e) =>{    
            this.composing = false
            console.log('compositionend')
            this.last_change_time = (new Date()).valueOf()
        })

        document.addEventListener('keydown', (e) => {
            if (!quill.root.contains(document.activeElement) && document.activeElement !== quill.root) {
                return;
            }
            
            if (e.key === 'Tab') {
                e.preventDefault();
                e.stopPropagation();
                e.stopImmediatePropagation();
                
                console.log('Tab key detected:', e.shiftKey ? 'Shift+Tab' : 'Tab');
                
                const range = quill.getSelection();
                
                if (!range) {
                    console.log('No selection range');
                    return;
                }
                
                console.log('Selection range:', range);
                
                if (e.shiftKey) {
                    console.log('Performing outdent');
                    if (range.length === 0) {
                        const text = quill.getText();
                        let deleteCount = 0;
                        for (let i = 1; i <= 4 && range.index - i >= 0; i++) {
                            if (text[range.index - i] === ' ') {
                                deleteCount++;
                            } else {
                                break;
                            }
                        }
                        if (deleteCount > 0) {
                            quill.deleteText(range.index - deleteCount, deleteCount, 'user');
                            quill.setSelection(range.index - deleteCount, 0, 'user');
                        }
                    } else {
                        const text = quill.getText();
                        
                        let startLineIndex = range.index;
                        while (startLineIndex > 0 && text[startLineIndex - 1] !== '\n') {
                            startLineIndex--;
                        }
                        
                        let endLineIndex = range.index + range.length;
                        if (endLineIndex < text.length && text[endLineIndex - 1] !== '\n') {
                            while (endLineIndex < text.length && text[endLineIndex] !== '\n') {
                                endLineIndex++;
                            }
                        }
                        
                        const fullSelectedText = text.substring(startLineIndex, endLineIndex);
                        const lines = fullSelectedText.split('\n');
                        
                        const spacesRemovedPerLine = [];
                        const outdentedLines = lines.map((line, index) => {
                            let spacesToRemove = 0;
                            for (let i = 0; i < Math.min(4, line.length); i++) {
                                if (line[i] === ' ') {
                                    spacesToRemove++;
                                } else {
                                    break;
                                }
                            }
                            spacesRemovedPerLine[index] = spacesToRemove;
                            return line.substring(spacesToRemove);
                        });
                        
                        const outdentedText = outdentedLines.join('\n');
                        
                        quill.deleteText(startLineIndex, endLineIndex - startLineIndex, 'user');
                        quill.insertText(startLineIndex, outdentedText, 'user');
                        
                        const originalSelectionStartOffset = range.index - startLineIndex;
                        const originalSelectionEndOffset = (range.index + range.length) - startLineIndex;
                        
                        const firstLineRemovedSpaces = spacesRemovedPerLine[0] || 0;
                        
                        const totalRemovedSpaces = spacesRemovedPerLine.reduce((sum, spaces) => sum + spaces, 0);
                        
                        let newSelectionStart = startLineIndex + originalSelectionStartOffset - firstLineRemovedSpaces;
                        let newSelectionEnd = startLineIndex + originalSelectionEndOffset - totalRemovedSpaces;
                        
                        newSelectionStart = Math.max(startLineIndex, newSelectionStart);
                        const maxIndex = startLineIndex + outdentedText.length;
                        newSelectionEnd = Math.min(newSelectionEnd, maxIndex);
                        
                        const newSelectionLength = Math.max(0, newSelectionEnd - newSelectionStart);
                        quill.setSelection(newSelectionStart, newSelectionLength, 'user');
                    }
                } else {
                    console.log('Performing indent');
                    if (range.length === 0) {
                        quill.insertText(range.index, '    ', 'user');
                        quill.setSelection(range.index + 4, 0, 'user');
                    } else {
                        const text = quill.getText();
                        
                        let startLineIndex = range.index;
                        while (startLineIndex > 0 && text[startLineIndex - 1] !== '\n') {
                            startLineIndex--;
                        }
                        
                        let endLineIndex = range.index + range.length;
                        if (endLineIndex < text.length && text[endLineIndex - 1] !== '\n') {
                            while (endLineIndex < text.length && text[endLineIndex] !== '\n') {
                                endLineIndex++;
                            }
                        }
                        
                        const fullSelectedText = text.substring(startLineIndex, endLineIndex);
                        const lines = fullSelectedText.split('\n');
                        
                        const indentedText = lines.map(line => {
                            return '    ' + line;
                        }).join('\n');
                        
                        quill.deleteText(startLineIndex, endLineIndex - startLineIndex, 'user');
                        quill.insertText(startLineIndex, indentedText, 'user');
                        
                        const originalSelectionStartOffset = range.index - startLineIndex;
                        const originalSelectionEndOffset = (range.index + range.length) - startLineIndex;
                        
                        let newSelectionStart = startLineIndex + originalSelectionStartOffset + 4; 
                        let newSelectionEnd = startLineIndex + originalSelectionEndOffset + (lines.length * 4); 
                        
                        const maxIndex = startLineIndex + indentedText.length;
                        newSelectionStart = Math.min(newSelectionStart, maxIndex);
                        newSelectionEnd = Math.min(newSelectionEnd, maxIndex);
                        
                        const newSelectionLength = newSelectionEnd - newSelectionStart;
                        quill.setSelection(newSelectionStart, newSelectionLength, 'user');
                    }
                }
            }
        }, true);

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
    },
    methods: {
        highlight() {
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

            quill = this.$refs.child.getQuill();
            
            let range = quill.getSelection();
            let curr_index = range ? range.index : 0;

            let code = quill.getText();
            
            quill.disable();
            
            quill.formatText(0, code.length, {
                color: false,
                background: false
            }, 'silent');

            let start = 0;
            current_color = 0;
            for (let i = 0; i < code.length; i++) {
                let length_to_apply = i - start;
                if (code[i] === '【') {
                    if (length_to_apply > 0) {
                        quill.formatText(start, length_to_apply, {color: colorList[current_color]}, 'silent');
                    }
                    next_color();
                    quill.formatText(i, 1, {color: colorList[current_color]}, 'silent');
                    start = i + 1;
                } else if (code[i] === '】') {
                    if (length_to_apply > 0) {
                        quill.formatText(start, length_to_apply, {color: colorList[current_color]}, 'silent');
                    }
                    quill.formatText(i, 1, {color: colorList[current_color]}, 'silent');
                    pre_color();
                    start = i + 1;
                } else if (code[i] === '\\') {
                    if (length_to_apply > 0) {
                        quill.formatText(start, length_to_apply, {color: colorList[current_color]}, 'silent');
                    }
                    i++; // assume next char exists
                    if (i < code.length) {
                        quill.formatText(i - 1, 2, {color: colorList[current_color]}, 'silent');
                        start = i + 1;
                    } else {
                        quill.formatText(i - 1, 1, {color: colorList[current_color]}, 'silent');
                        start = i;
                    }
                }
                // normal char, continue accumulating
            }
            // Apply last segment
            if (start < code.length) {
                quill.formatText(start, code.length - start, {color: colorList[current_color]}, 'silent');
            }

            // Handle cursor highlight
            if (curr_index >= 0 && curr_index < code.length) {
                let pos = curr_index;
                if (code[pos] !== '【' && code[pos] !== '】') {
                    if (pos > 0) pos--;
                }
                if (code[pos] === '【' || code[pos] === '】') {
                    // Find matching
                    let direction = code[pos] === '【' ? 1 : -1;
                    let bracket = code[pos] === '【' ? '【' : '】';
                    let matchBracket = bracket === '【' ? '】' : '【';
                    let count = 1;
                    let matchPos = pos + direction;
                    while (matchPos >= 0 && matchPos < code.length) {
                        if (code[matchPos] === '\\') {
                            matchPos += direction;
                        } else if (code[matchPos] === bracket) {
                            count++;
                        } else if (code[matchPos] === matchBracket) {
                            count--;
                            if (count === 0) {
                                break;
                            }
                        }
                        matchPos += direction;
                    }
                    if (count === 0 && matchPos >= 0 && matchPos < code.length) {
                        // Apply background to pos and matchPos
                        let colorAtPos = quill.getFormat(pos, 1).color || '#000000';
                        let bg = ColorReverse(colorAtPos);
                        quill.formatText(pos, 1, {background: bg}, 'silent');
                        quill.formatText(matchPos, 1, {background: bg}, 'silent');
                    }
                }
            }
            
            quill.enable();
            quill.setSelection(range);
        }
    }
};
