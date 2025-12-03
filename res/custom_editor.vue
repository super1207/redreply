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
		maskComments(text) {
			let arr = text.split('');
			for (let i = 0; i < arr.length; i++) {
				if (arr[i] === '\\' && arr[i+1] === '\\') {
					arr[i] = ' '; arr[i+1] = ' ';
					i++; 
					continue;
				}
				if (arr[i] === '\\') {
					const next = arr[i+1];
					if (next === '【' || next === '】') {
						arr[i] = ' '; arr[i+1] = ' ';
						i++; continue;
					}
					if (next === '#' && arr[i+2] === '#') {
						arr[i] = ' '; arr[i+1] = ' '; arr[i+2] = ' ';
						i += 2; continue;
					}
				}
				if (arr[i] === '#' && arr[i+1] === '#') {
					while(i < arr.length && arr[i] !== '\n') {
						arr[i] = ' ';
						i++;
					}
				}
			}
			return arr.join('');
		},
		findMatchIndices(text, cursorIndex) {
			const matches = new Set();
			let targetIndex = -1;
			let char = '';
			if (cursorIndex < text.length) {
				const c = text[cursorIndex];
				if (c === '【' || c === '】') {
					targetIndex = cursorIndex;
					char = c;
				}
			}
			if (targetIndex === -1 && cursorIndex > 0) {
				const c = text[cursorIndex - 1];
				if (c === '【' || c === '】') {
					targetIndex = cursorIndex - 1;
					char = c;
				}
			}
			if (targetIndex === -1) return matches;
			matches.add(targetIndex);
			if (char === '【') {
				let depth = 0;
				for (let i = targetIndex + 1; i < text.length; i++) {
					if (text[i] === '【') depth++;
					else if (text[i] === '】') {
						if (depth === 0) { matches.add(i); break; }
						depth--;
					}
				}
			} else {
				let depth = 0;
				for (let i = targetIndex - 1; i >= 0; i--) {
					if (text[i] === '】') depth++;
					else if (text[i] === '【') {
						if (depth === 0) { matches.add(i); break; }
						depth--;
					}
				}
			}
			return matches;
		},
		render() {
			const inputEl = this.$refs.inputLayer;
			const highlightEl = this.$refs.highlightLayer;
			if (!inputEl || !highlightEl) return;
			const text = inputEl.value;
			const cursorIndex = inputEl.selectionStart;
			const cleanText = this.maskComments(text);
			const activeIndices = this.findMatchIndices(cleanText, cursorIndex);
			let html = '';
			let depth = 1;
			const getColorClass = (d) => {
				if (d <= 0) return 'lvl-1'; 
				const colorIdx = (d - 1) % this.MAX_COLORS + 1;
				return `lvl-${colorIdx}`;
			};
			const openSpan = (d) => `<span class="${getColorClass(d)}">`;
			const closeSpan = (d) => `</span>`;
			html += openSpan(1); 
			for (let i = 0; i < text.length; i++) {
				if (text[i] === '\\' && text[i+1] === '\\') {
					html += this.escapeHtml('\\\\');
					i++; continue;
				}
				if (text[i] === '\\' && text[i+1] === '#' && text[i+2] === '#') {
					html += this.escapeHtml('\\##'); 
					i += 2; continue;
				}
				if (text[i] === '\\' && (text[i+1] === '【' || text[i+1] === '】')) {
					html += this.escapeHtml(text[i] + text[i+1]);
					i++; continue;
				}
				if (text[i] === '#' && text[i+1] === '#') {
					let lineEnd = text.indexOf('\n', i);
					if (lineEnd === -1) lineEnd = text.length;
					const commentContent = text.substring(i, lineEnd);
					html += `<span class="comment-text">${this.escapeHtml(commentContent)}</span>`;
					i = lineEnd - 1; 
					continue; 
				}
				const char = text[i];
				const isActive = activeIndices.has(i);
				const activeClass = isActive ? ' active-pair' : '';
				if (char === '【') {
					html += closeSpan(depth);
					depth++;
					html += openSpan(depth);
					html += `<span class="${activeClass}">【</span>`; 
				} 
				else if (char === '】') {
					if (depth > 1) {
						html += `<span class="${activeClass}">】</span>`;
						html += closeSpan(depth);
						depth--;
						html += openSpan(depth);
					} else {
						html += `<span class="error-bracket${activeClass}">】</span>`;
					}
				} 
				else {
					html += this.escapeHtml(char);
				}
			}
			html += closeSpan(depth);
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
				if (end > start && value[end-1] === '\n') {
				}
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
