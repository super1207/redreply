FROM ubuntu
RUN apt-get update -y
RUN apt install git wget curl gcc -y
RUN curl https://sh.rustup.rs -sSf | \
    sh -s -- --default-toolchain stable -y
ENV PATH=/root/.cargo/bin:$PATH
RUN git clone https://github.com/super1207/redreply.git \
	&&cd redreply \
	&&cargo build --release
# 下载宋体，这样可以在红色问答中使用宋体
ADD "https://www.random.org/cgi-bin/randbyte?nbytes=10&format=h" skipcache 
RUN cd redreply && git pull
# 两次构建，可以充分利用缓存
RUN cd redreply \
	&&cargo build --release \
	&&cp /redreply/target/release/redlang /redlang

# 构建可执行文件的运行环境，只安装必须的库即可
FROM ubuntu
RUN apt-get update  -y \
	&& apt install tzdata -y \
	&& apt-get clean
COPY --from=0 redlang /
EXPOSE 1207
CMD if [ ! -f "/plus_dir/config.json" ]; then echo '{"web_port":1207,"web_host":"0.0.0.0","ws_urls":[],"not_open_browser":true}' > /plus_dir/config.json; fi && ./redlang