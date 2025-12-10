<template>
	<div class="custom-editor-wrapper">
		<pre ref="highlightLayer" class="editor-layer highlight-layer" aria-hidden="true"></pre>
		<textarea 
			ref="inputLayer" 
			class="editor-layer input-layer" 
			spellcheck="false" 
			placeholder="请输入脚本内容..."
			@input="handleInput"
			@scroll="syncScroll"
			@keydown="handleKeyDown"
			@click="render"
			@keyup="render"
			@focus="render"
		></textarea>
	</div>
</template>

<script>
export default {
	name: 'CustomEditor',
	data() {
		return {
			MAX_COLORS: 4
		};
	},
	mounted() {
		window.addEventListener('resize', this.syncScroll);
	},
	beforeUnmount() {
		window.removeEventListener('resize', this.syncScroll);
	},
	methods: {
		getText() {
			return this.$refs.inputLayer.value;
		},
		setText(val) {
			this.$refs.inputLayer.value = val || '';
			this.$nextTick(() => {
				this.render(); 
				this.syncScroll(); 
			});
		},
		escapeHtml(str) {
			return str.replace(/&/g, "&amp;")
					  .replace(/</g, "&lt;")
					  .replace(/>/g, "&gt;")
					  .replace(/"/g, "&quot;")
					  .replace(/'/g, "&#039;");
		},
		// 核心解析逻辑：计算括号匹配
		analyzeMatches(text, cursorIndex) {
			const matches = new Set();
			const pairs = {}; 
			const stack = []; 
			let i = 0;
			
			while (i < text.length) {
				// 1. 优先检测注释
				if (text.startsWith('##', i)) {
					const lineEnd = text.indexOf('\n', i);
					i = lineEnd === -1 ? text.length : lineEnd;
					continue;
				}

				// 2. 检测原始字符串 【@
				if (text.startsWith('【@', i)) {
					const startIndex = i;
					stack.push({ index: i, type: 'raw' });
					i += 2; // 跳过 【@
					
					let innerDepth = 0;
					// 进入原始字符串内部循环
					while (i < text.length) {
						// 内部依然检测 ## 注释，如果遇到，直接跳出内部循环，交给外层处理
						if (text.startsWith('##', i)) {
							break; 
						}

						const char = text[i];
						if (char === '【') {
							innerDepth++;
							i++;
						} else if (char === '】') {
							if (innerDepth > 0) {
								innerDepth--;
								i++;
							} else {
								// 找到结束符
								const startInfo = stack.pop();
								if (startInfo) {
									pairs[startInfo.index] = i;
									pairs[i] = startInfo.index;
								}
								i++; // 消费这个 】
								break; // 退出内部循环
							}
						} else {
							i++;
						}
					}
					continue;
				}

				// 3. 普通模式下的转义
				if (text[i] === '\\') {
					i += 2; 
					continue;
				}

				// 4. 普通左括号
				if (text[i] === '【') {
					stack.push({ index: i, type: 'normal' });
					i++;
					continue;
				}

				// 5. 普通右括号
				if (text[i] === '】') {
					if (stack.length > 0 && stack[stack.length - 1].type === 'normal') {
						const startInfo = stack.pop();
						pairs[startInfo.index] = i;
						pairs[i] = startInfo.index;
					}
					i++;
					continue;
				}

				i++;
			}

			// 计算高亮
			let targetIndex = -1;
			if (pairs[cursorIndex] !== undefined) targetIndex = cursorIndex;
			else if (cursorIndex > 0 && pairs[cursorIndex - 1] !== undefined) targetIndex = cursorIndex - 1;

			if (targetIndex !== -1) {
				matches.add(targetIndex);
				matches.add(pairs[targetIndex]);
			}
			return matches;
		},
		render() {
			const inputEl = this.$refs.inputLayer;
			const highlightEl = this.$refs.highlightLayer;
			if (!inputEl || !highlightEl) return;

			const text = inputEl.value;
			const cursorIndex = inputEl.selectionStart;
			const activeIndices = this.analyzeMatches(text, cursorIndex);

			let html = '';
			let depth = 1; 
			let i = 0;

			const getColorClass = (d) => {
				if (d <= 0) return 'lvl-1';
				const colorIdx = (d - 1) % this.MAX_COLORS + 1;
				return `lvl-${colorIdx}`;
			};
			const openSpan = (d) => `<span class="${getColorClass(d)}">`;
			const closeSpan = () => `</span>`;

			html += openSpan(depth);

			while (i < text.length) {
				// 1. 注释渲染 (优先级最高)
				if (text.startsWith('##', i)) {
					let lineEnd = text.indexOf('\n', i);
					if (lineEnd === -1) lineEnd = text.length;
					const comment = text.substring(i, lineEnd);
					html += `<span class="comment-text">${this.escapeHtml(comment)}</span>`;
					i = lineEnd;
					continue;
				}

				// 2. 原始字符串渲染 【@
				if (text.startsWith('【@', i)) {
					const isActive = activeIndices.has(i);
					const activeClass = isActive ? ' active-pair' : '';

					html += closeSpan();
					depth++;
					html += openSpan(depth);
					
					// 渲染 【 (带高亮类)
					html += `<span class="${activeClass}">【</span>`;
					// 渲染 @ 
					html += '@'; 
					
					i += 2; // 跳过 【@
					
					let innerDepth = 0;
					while (i < text.length) {
						if (text.startsWith('##', i)) {
							// 遇到注释，不做任何处理，直接 break
							// i 指向 ## 的开始，外层循环会接手处理
							break; 
						}

						const char = text[i];
						if (char === '【') {
							innerDepth++;
							html += this.escapeHtml(char);
							i++;
						} else if (char === '】') {
							if (innerDepth > 0) {
								innerDepth--;
								html += this.escapeHtml(char);
								i++;
							} else {
								// 找到结束符
								const isEndActive = activeIndices.has(i);
								const endActiveClass = isEndActive ? ' active-pair' : '';
								
								html += `<span class="${endActiveClass}">】</span>`;
								
								// 恢复层级
								html += closeSpan();
								depth--;
								html += openSpan(depth);
								
								i++; // 消费这个 】
								break; // 退出内部循环
							}
						} else {
							html += this.escapeHtml(char);
							i++;
						}
					}
					continue;
				}

				// 3. 普通转义
				if (text[i] === '\\') {
					if (i + 1 < text.length) {
						html += this.escapeHtml(text.substring(i, i + 2));
						i += 2;
					} else {
						html += this.escapeHtml('\\');
						i++;
					}
					continue;
				}

				// 4. 普通左括号
				if (text[i] === '【') {
					const isActive = activeIndices.has(i);
					const activeClass = isActive ? ' active-pair' : '';
					html += closeSpan();
					depth++;
					html += openSpan(depth);
					html += `<span class="${activeClass}">【</span>`;
					i++;
					continue;
				}

				// 5. 普通右括号
				if (text[i] === '】') {
					const isActive = activeIndices.has(i);
					const activeClass = isActive ? ' active-pair' : '';
					if (depth > 1) {
						html += `<span class="${activeClass}">】</span>`;
						html += closeSpan();
						depth--;
						html += openSpan(depth);
					} else {
						html += `<span class="error-bracket${activeClass}">】</span>`;
					}
					i++;
					continue;
				}

				// 6. 其他字符
				html += this.escapeHtml(text[i]);
				i++;
			}

			html += closeSpan();
			if (text.endsWith('\n')) {
				html += '<br>&nbsp;';
			}
			highlightEl.innerHTML = html;
		},
		syncScroll() {
			if (this.$refs.inputLayer && this.$refs.highlightLayer) {
				this.$refs.highlightLayer.scrollTop = this.$refs.inputLayer.scrollTop;
				this.$refs.highlightLayer.scrollLeft = this.$refs.inputLayer.scrollLeft;
			}
		},
		handleInput() {
			this.render();
			this.syncScroll();
		},
		handleKeyDown(e) {
			const inputEl = this.$refs.inputLayer;
			if (e.ctrlKey && e.key === '/') {
				e.preventDefault();
				const start = inputEl.selectionStart;
				const end = inputEl.selectionEnd;
				const value = inputEl.value;
				let startLineIndex = value.lastIndexOf('\n', start - 1) + 1; 
				let endLineIndex = value.indexOf('\n', end);
				if (endLineIndex === -1) endLineIndex = value.length;
				const blockStart = startLineIndex;
				const blockEnd = endLineIndex;
				const selectedText = value.substring(blockStart, blockEnd);
				const lines = selectedText.split('\n');
				const validLines = lines.filter(line => line.trim().length > 0);
				const allCommented = validLines.length > 0 && validLines.every(line => /^\s*##/.test(line));
				let newLines;
				if (allCommented) {
					newLines = lines.map(line => line.replace(/^(\s*)## ?/, '$1'));
				} else {
					newLines = lines.map(line => {
						if (line.trim().length === 0) return line;
						return '## ' + line;
					});
				}
				const newText = newLines.join('\n');
				inputEl.setRangeText(newText, blockStart, blockEnd, 'select');
				this.handleInput();
				return;
			}
			if (e.key === 'Tab') {
				e.preventDefault();
				const start = inputEl.selectionStart;
				const end = inputEl.selectionEnd;
				const value = inputEl.value;
				const hasSelection = start !== end;
				let startLineIndex = value.lastIndexOf('\n', start - 1) + 1; 
				let endLineIndex = value.indexOf('\n', end);
				if (endLineIndex === -1) endLineIndex = value.length;
				const blockStart = startLineIndex;
				const blockEnd = endLineIndex;
				const selectedText = value.substring(blockStart, blockEnd);
				const lines = selectedText.split('\n');
				let newText = '';
				if (e.shiftKey) {
					const modifiedLines = lines.map(line => {
						let removeCount = 0;
						if (line.startsWith('    ')) removeCount = 4;
						else if (line.startsWith('   ')) removeCount = 3;
						else if (line.startsWith('  ')) removeCount = 2;
						else if (line.startsWith(' ')) removeCount = 1;
						return line.substring(removeCount);
					});
					newText = modifiedLines.join('\n');
					inputEl.setRangeText(newText, blockStart, blockEnd, 'select');
				} else {
					if (!hasSelection) {
						document.execCommand('insertText', false, '    ');
					} else {
						const modifiedLines = lines.map(line => '    ' + line);
						newText = modifiedLines.join('\n');
						inputEl.setRangeText(newText, blockStart, blockEnd, 'select');
					}
				}
				this.handleInput(); 
			}
		}
	}
}
</script>