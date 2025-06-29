# for ubuntu 24.04
FROM ubuntu
RUN sed -i 's/security.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list.d/ubuntu.sources
RUN sed -i 's/archive.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list.d/ubuntu.sources
RUN apt-get update  -y \
    && echo "Asia\nShanghai" | apt install -y tzdata \
    && apt-get install unzip wget python3 python3-pip python3-venv -y \
    && wget https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
    && apt-get install -y ./google-chrome-stable_current_amd64.deb \ 
    && rm google-chrome-stable_current_amd64.deb \
    && wget -O /usr/share/fonts/simsun.ttf https://file.uhsea.com/2506/cb8b989d3a5bd9e836fd27c475cadf0dU6.ttf \
    && apt-get clean
ADD "https://red.super1207.top/version/latest_nightly_version.php" skipcache
RUN wget -O radlang.zip https://red.super1207.top/download/latest_nightly_linux_x86_64.php \
    && unzip radlang.zip \
    && rm radlang.zip \
    && chmod +x redlang_linux_x86_64
EXPOSE 1207
CMD if [ ! -f "/plus_dir/config.json" ]; \
    then echo '{"web_port":1207,"web_host":"0.0.0.0","ws_urls":[],"not_open_browser":true}' > /plus_dir/config.json; fi \
    && ./redlang_linux_x86_64

# 构建镜像：
#    docker build  -t super1207/redreply .
# 创建并运行容器：
#    docker run --rm -p 1207:1207 -v ${pwd}:/plus_dir super1207/redreply

