FROM ubuntu
RUN apt-get update -y
RUN apt install cargo git libdbus-glib-1-dev libfontconfig1-dev libxcb* wget -y
RUN git clone https://github.com/super1207/redreply.git \
	&&cd redreply \
	&&cargo build --release
# 下载宋体，这样可以在红色问答中使用宋体
RUN wget -O /usr/share/fonts/simsun.ttf https://pfh-file-store.oss-cn-hangzhou.aliyuncs.com/simsun.ttf
ADD "https://www.random.org/cgi-bin/randbyte?nbytes=10&format=h" skipcache 
RUN cd redreply && git pull
# 两次构建，可以充分利用缓存
RUN cd redreply && cargo build --release \
	&&cp /redreply/target/release/redlang /redlang

# 构建可执行文件的运行环境，只安装必须的库即可
FROM ubuntu
RUN apt-get update  -y \
	&& apt install dbus fontconfig xcb tzdata -y \
	&& apt-get clean
COPY --from=0 redlang /
COPY --from=0 /usr/share/fonts/simsun.ttf /usr/share/fonts/
EXPOSE 1207
CMD if [ ! -f "/plus_dir/config.json" ]; then echo '{"web_port":1207,"web_host":"0.0.0.0","ws_urls":[],"not_open_browser":true}' > /plus_dir/config.json; fi && ./redlang