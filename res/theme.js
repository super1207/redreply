// 主题切换功能
class ThemeManager {
    constructor() {
        this.currentTheme = this.getStoredTheme() || 'light';
        this.init();
    }

    init() {
        // 应用当前主题
        this.applyTheme(this.currentTheme);
        
        // 创建主题切换按钮
        this.createThemeToggle();
        
        // 监听系统主题变化
        if (window.matchMedia) {
            window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
                if (!this.getStoredTheme()) {
                    this.setTheme(e.matches ? 'dark' : 'light');
                }
            });
        }
    }

    getStoredTheme() {
        try {
            return localStorage.getItem('redreply-theme');
        } catch (e) {
            return null;
        }
    }

    setStoredTheme(theme) {
        try {
            localStorage.setItem('redreply-theme', theme);
        } catch (e) {
            console.warn('无法保存主题设置');
        }
    }

    applyTheme(theme) {
        document.documentElement.setAttribute('data-theme', theme);
        this.currentTheme = theme;
        this.updateToggleIcon();
    }

    setTheme(theme) {
        this.applyTheme(theme);
        this.setStoredTheme(theme);
    }

    toggleTheme() {
        const newTheme = this.currentTheme === 'light' ? 'dark' : 'light';
        this.setTheme(newTheme);
    }

    createThemeToggle() {
        // 检查是否已存在切换按钮
        if (document.querySelector('.theme-toggle')) {
            return;
        }

        const toggle = document.createElement('button');
        toggle.className = 'theme-toggle';
        toggle.title = '切换主题';
        toggle.setAttribute('aria-label', '切换明暗主题');
        
        toggle.addEventListener('click', () => {
            this.toggleTheme();
        });

        // 添加到页面
        document.body.appendChild(toggle);
        
        this.toggleButton = toggle;
        this.updateToggleIcon();
    }

    updateToggleIcon() {
        if (this.toggleButton) {
            this.toggleButton.innerHTML = this.currentTheme === 'light' ? '🌙' : '☀️';
        }
    }

    // 获取当前主题
    getCurrentTheme() {
        return this.currentTheme;
    }

    // 检查是否为暗色主题
    isDarkTheme() {
        return this.currentTheme === 'dark';
    }
}

// 全局主题管理器实例
window.themeManager = new ThemeManager();

// 修复脚本编辑页面特定元素的主题适配
function fixScriptEditorTheme() {
    console.log('正在修复脚本编辑器主题...');
    
    // 查找所有可能的脚本栏容器
    const selectors = [
        'div[style*="background-color:#b8e7e4"]',
        'div[style*="background-color: #b8e7e4"]', 
        'div[style*="background-color:#B8E7E4"]',
        'div[style*="background-color: #B8E7E4"]',
        'div[style*="border-style:outset"][style*="background-color"]'
    ];
    
    let scriptBar = null;
    for (const selector of selectors) {
        scriptBar = document.querySelector(selector);
        if (scriptBar) {
            console.log('找到脚本栏:', selector);
            break;
        }
    }
    
    if (scriptBar) {
        const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
        console.log('当前主题:', isDark ? '夜间' : '日间');
        
        // 强制修复容器背景 - 使用setProperty确保优先级
        if (isDark) {
            scriptBar.style.setProperty('background-color', '#2d2d2d', 'important');
            scriptBar.style.setProperty('color', '#e0e0e0', 'important');
        } else {
            scriptBar.style.setProperty('background-color', '#b6cde4', 'important');
            scriptBar.style.setProperty('color', '#000', 'important');
        }
        
        // 修复内部所有div文字颜色
        const innerDivs = scriptBar.querySelectorAll('div');
        console.log('找到内部div数量:', innerDivs.length);
        innerDivs.forEach((div, index) => {
            console.log(`修复div ${index}:`, div.textContent.substring(0, 20));
            if (isDark) {
                div.style.setProperty('color', '#e0e0e0', 'important');
                div.style.setProperty('background-color', 'transparent', 'important');
            } else {
                div.style.setProperty('color', '#000', 'important');
                div.style.setProperty('background-color', 'transparent', 'important');
            }
        });
        
        // 修复按钮样式
        const buttons = scriptBar.querySelectorAll('button');
        console.log('找到按钮数量:', buttons.length);
        buttons.forEach((button, index) => {
            if (isDark) {
                if (button.classList.contains('name_active')) {
                    button.style.setProperty('background-color', '#ff7b6b', 'important');
                    button.style.setProperty('color', 'white', 'important');
                } else {
                    button.style.setProperty('background-color', '#404040', 'important');
                    button.style.setProperty('color', '#e0e0e0', 'important');
                    button.style.setProperty('border-color', '#555', 'important');
                }
            } else {
                if (button.classList.contains('name_active')) {
                    button.style.setProperty('background-color', '#e55743', 'important');
                    button.style.setProperty('color', 'white', 'important');
                } else {
                    button.style.setProperty('background-color', '#e1ebe7', 'important');
                    button.style.setProperty('color', '#000', 'important');
                    button.style.setProperty('border-color', '#999', 'important');
                }
            }
        });
        
        console.log('脚本栏主题修复完成');
    } else {
        console.log('未找到脚本栏容器');
    }
}

// 监听主题变化并应用修复
const observer = new MutationObserver((mutations) => {
    mutations.forEach((mutation) => {
        if (mutation.type === 'attributes' && mutation.attributeName === 'data-theme') {
            setTimeout(fixScriptEditorTheme, 50);
        }
    });
});

// 开始观察主题变化
if (document.documentElement) {
    observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['data-theme']
    });
}

// 页面加载完成后立即应用修复
document.addEventListener('DOMContentLoaded', () => {
    setTimeout(fixScriptEditorTheme, 100);
});

// 定期检查并修复（作为备用方案）
setInterval(fixScriptEditorTheme, 500);

// 立即执行一次修复
setTimeout(fixScriptEditorTheme, 100);

// 监听页面变化，Vue应用加载后再次修复
const checkVueApp = setInterval(() => {
    if (window.Vue && document.querySelector('#app')) {
        console.log('检测到Vue应用，执行脚本栏修复');
        setTimeout(fixScriptEditorTheme, 200);
        clearInterval(checkVueApp);
    }
}, 100);

// 导出供其他脚本使用
if (typeof module !== 'undefined' && module.exports) {
    module.exports = ThemeManager;
}