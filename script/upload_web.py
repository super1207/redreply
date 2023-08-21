#!/usr/bin/python
# -*- coding: UTF-8 -*-

import requests
import zipfile
from io import BytesIO
from requests_toolbelt.multipart.encoder import MultipartEncoder
from ftplib import FTP

# 此脚本用于上传可执行文件到官网服务器
# 依赖安装:
# pip install requests requests_toolbelt

# 你需要填写以下数据
WORKFLOW_RUNID = "5917734512"  # github action workflow run id

# 你需要填写FTP用户信息
FTP_HOST = "127.0.0.1"
FTP_USER = "user_name"
FTP_PASSWORD= "password"

release_json = requests.get(f'https://api.github.com/repos/super1207/redreply/releases/latest').json()
tag_name = release_json["tag_name"]
version = tag_name[8:]
windows_i686_url = f"https://ghproxy.com/https://github.com/super1207/redreply/releases/download/{tag_name}/redlang.exe"
artifacts_json = requests.get(f"https://api.github.com/repos/super1207/redreply/actions/runs/{WORKFLOW_RUNID}/artifacts").json()
linux_i686_url_raw = ""
linux_aarch64_url_raw = ""
for it in artifacts_json['artifacts']:
    if it["name"] == "redlang_linux_i686":
        id = it["id"]
        linux_i686_url_raw = f"https://nightly.link/super1207/redreply/suites/{WORKFLOW_RUNID}/artifacts/{id}"
    elif it["name"] == "redlang_linux_aarch64":
        id = it["id"]
        linux_aarch64_url_raw = f"https://nightly.link/super1207/redreply/suites/{WORKFLOW_RUNID}/artifacts/{id}"

print("正在下载linux_i686_zip_data...")
linux_i686_zip_data = requests.get(linux_i686_url_raw).content
linux_i686_zip_file = zipfile.ZipFile(BytesIO(linux_i686_zip_data))
linux_i686_data = linux_i686_zip_file.open([file_info for file_info in linux_i686_zip_file.infolist()][0]).read()
print("正在下载linux_aarch64_zip_data...")
linux_aarch64_zip_data = requests.get(linux_aarch64_url_raw).content
linux_aarch64_zip_file = zipfile.ZipFile(BytesIO(linux_aarch64_zip_data))
linux_aarch64_data = linux_aarch64_zip_file.open([file_info for file_info in linux_aarch64_zip_file.infolist()][0]).read()
print("正在上传linux_i686_data...")
files = MultipartEncoder(fields=[('reqtype','fileupload'),('fileToUpload', ('linux_i686',linux_i686_data))])
linux_i686_url = requests.post("https://catbox.moe/user/api.php", data=files,headers={'Content-Type': files.content_type}).text
if not linux_i686_url.startswith("http"):
    raise Exception(f"Error:{linux_i686_url}")
print("正在上传linux_aarch64_data...")
files = MultipartEncoder(fields=[('reqtype','fileupload'),('fileToUpload', ('linux_aarch64',linux_aarch64_data))])
linux_aarch64_url = requests.post("https://catbox.moe/user/api.php", data=files,headers={'Content-Type': files.content_type}).text
if not linux_aarch64_url.startswith("http"):
    raise Exception(f"Error:{linux_aarch64_url}")


print("url准备完成:")
print("version",version)
print("windows_i686_url",windows_i686_url)
print("linux_i686_url",linux_i686_url)
print("linux_aarch64_url",linux_aarch64_url)

def create_302(url):
    return f"""<?php
Header("Location: {url}");
?>""".encode()

print("正在更新ftp远程文件...")
ftp = FTP(FTP_HOST)
ftp.login(FTP_USER,FTP_PASSWORD)
ftp.storbinary('STOR ' + 'download/latest_windows_i686.php',BytesIO(create_302(windows_i686_url)))
ftp.storbinary('STOR ' + 'download/latest_linux_i686.php',BytesIO(create_302(linux_i686_url)))
ftp.storbinary('STOR ' + 'download/latest_linux_aarch64.php',BytesIO(create_302(linux_aarch64_url)))
ftp.storbinary('STOR ' + 'version/latest_version.php',BytesIO(version.encode()))

print("everything is ok!")
