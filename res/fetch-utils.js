// 简单的fetch工具函数，用于替换axios
const fetchUtils = {
    // GET请求
    get: function(url) {
        return fetch(url, {
            method: 'GET',
            headers: {
                'Content-Type': 'application/json'
            }
        }).then(response => {
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            return response.json();
        }).then(data => {
            // 模拟axios的响应格式
            return { data: data };
        });
    },

    // POST请求
    post: function(url, data) {
        return fetch(url, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(data)
        }).then(response => {
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            return response.json();
        }).then(responseData => {
            // 模拟axios的响应格式
            return { data: responseData };
        });
    }
};

// 为了兼容现有代码，创建一个axios兼容对象
const axios = fetchUtils;