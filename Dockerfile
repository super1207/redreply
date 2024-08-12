# for ubuntu 24.04
FROM ubuntu
RUN sed -i 's/security.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list.d/ubuntu.sources
RUN sed -i 's/archive.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list.d/ubuntu.sources
RUN apt-get update  -y
RUN echo "Asia\nShanghai" | apt install -y tzdata
RUN apt-get install unzip wget -y
RUN apt-get install python3 -y
RUN apt-get install python3-pip -y
RUN apt-get install python3-venv -y
RUN wget https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb
RUN apt-get install -y ./google-chrome-stable_current_amd64.deb
RUN wget -O /usr/share/fonts/simsun.ttf https://pfh-file-store.oss-cn-hangzhou.aliyuncs.com/simsun.ttf
ADD "https://red.super1207.top/version/latest_nightly_version.php" skipcache
RUN wget -O radlang.zip https://red.super1207.top/download/latest_nightly_linux_x86_64.php
RUN unzip radlang.zip
RUN chmod +x redlang_linux_x86_64
EXPOSE 1207
CMD if [ ! -f "/plus_dir/config.json" ]; then echo '{"web_port":1207,"web_host":"0.0.0.0","ws_urls":[],"not_open_browser":true}' > /plus_dir/config.json; fi && ./redlang_linux_x86_64

# 构建镜像：
#    docker build  -t super1207/redreply .
# 创建并运行容器：
#    docker run --rm -p 1207:1207 -v ${pwd}:/plus_dir super1207/redreply

