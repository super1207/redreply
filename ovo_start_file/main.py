# -*- encoding: utf-8 -*-
# by super1207 in 2022

import ctypes
import json
import os
import platform
import shutil
import subprocess
import threading
import time
import traceback
import uuid

import OlivOS
import requests
import base64

g_proc = None
g_bot_list = None
g_miraicq = None

def find_plus_file_name():
    files_vec = []
    path_str = os.path.dirname(os.path.abspath(__file__)) + os.sep + "MiraiCQ"+ os.sep +"app"
    for _, _, files in os.walk(path_str):
        for name in files:
            if name.lower().endswith(".dll"):
                return name[0:-4]
        break
    return ""

base64_t = str(base64.b64encode(bytes(find_plus_file_name(),encoding='utf-8')),encoding='utf-8')
client_uid = str(uuid.uuid4()).upper()
ser_uid = str(uuid.uuid4()).upper()
g_my_ipc_ser = None
with open(os.path.dirname(os.path.abspath(__file__)) + os.sep + "app.json",encoding='utf-8') as f:
    g_plus_name = json.load(f)['namespace']

# 这里主要是为了阻止namespace和opk插件文件名不同的插件被加载，使其规范化
__import__(g_plus_name)

def printLog(msg,level = 2):
    g_proc.log(level,g_plus_name + ":" + msg)

def call_cq_menu(fun_name):
    sendjs = json.dumps({'action':'call_menu','params':{'fun_name':fun_name}}).encode()
    g_my_ipc_ser.call_api(client_uid,sendjs,5000)

def server_api_thread_func_t(sender_uuid,recv_flag,msg):
    js = json.loads(msg.decode('utf-8'))
    node_ext = js['action']
    if node_ext == 'add_log':
        g_my_ipc_ser.reply_api(sender_uuid,recv_flag,'ok')
        params = js['params']
        level = params['level']
        category = params['category']
        dat = params['dat']
        if level == 'debug':
            printLog(category + ':' + dat,level=0)
        elif level == 'info':
            printLog(category + ':' + dat,level=2)
        else:
            printLog(category + ':' + dat,level=3)
    else:
        headers = {
            'Content-Type': 'application/json'
        }
        bot_info = g_bot_list[0].post_info
        send_url = bot_info.host + ':' + str(bot_info.port) + '/' + node_ext + '?access_token=' + bot_info.access_token
        try:
            json_str_tmp = js['params']
        except:
            json_str_tmp = {}
        printLog('send\n' + bot_info.host + ':' + str(bot_info.port) + '/' + node_ext + '\n' + str(json_str_tmp))
        msg_res = requests.post(send_url, headers = headers, json = json_str_tmp)
        printLog('recv\n' + msg_res.text)
        g_my_ipc_ser.reply_api(sender_uuid,recv_flag,msg_res.content)
            
def server_api_thread_func():
    while True:
        try:
            sender_uuid,recv_flag,msg = g_my_ipc_ser.api_recv()
            server_api_thread_func_t(sender_uuid,recv_flag,msg)
        except:
            g_my_ipc_ser.reply_api(sender_uuid,recv_flag,b'')
            printLog(traceback.format_exc(),level=3)


def copy3(src, dst):
    names = os.walk(src)
    for root, dirs, files in names:
        for i in files:
            srcname = os.path.join(root, i)
            dir = root.replace(src, '')
            dirname = dst + dir
            if os.path.exists(dirname):
                pass
            else:
                os.makedirs(dirname)
            dirfname = os.path.join(dirname, i)
            shutil.copy2(srcname, dirfname)


class IPCTool:
    def __init__(self,path:str,uid = "") -> None:
        self.dll = ctypes.CDLL(path)

        self.dll.IPC_Init.restype  = ctypes.c_int
        self.dll.IPC_ApiSend.restype  = ctypes.c_char_p
        self.dll.IPC_GetFlag.restype  = ctypes.c_char_p
        self.dll.IPC_ApiSend.IPC_GetEvent  = ctypes.c_char_p

        ret = self.dll.IPC_Init(uid.encode())
        self.uid = ctypes.c_char_p(self.dll.IPC_GetFlag()).value.decode()
        if ret != 0:
            raise Exception("IPC_Init Err")

    def get_uid(self):
        return self.uid

    def send_event(self,client_uid:str,msg:bytes):
        self.dll.IPC_SendEvent(client_uid.encode(),msg)
    
    def call_api(self,client_uid:str,msg:bytes,timeoutms:int) -> bytes:
        apiret = self.dll.IPC_ApiSend(client_uid.encode(),msg,timeoutms)
        return ctypes.c_char_p(apiret).value

    def api_recv(self):
        ret = []
        functype = ctypes.CFUNCTYPE(None,ctypes.c_char_p,ctypes.c_char_p,ctypes.c_char_p)
        def api_callback(sender_uuid,recv_flag,msg):
            ret.append(ctypes.c_char_p(sender_uuid).value.decode())
            ret.append(ctypes.c_char_p(recv_flag).value.decode())
            ret.append(ctypes.c_char_p(msg).value)
        c_callback_python = functype(api_callback)
        self.dll.IPC_ApiRecv(c_callback_python)  
        # sender_uuid:str recv_flag:str msg:bytes
        return ret[0],ret[1],ret[2]
    
    def reply_api(self,sender_uuid:str,recv_flag:str,msg:bytes):
        self.dll.IPC_ApiReply(sender_uuid.encode(),recv_flag.encode(),msg)

    def get_event(self) -> bytes:
        ret = self.dll.IPC_GetEvent(self.uid.encode())
        return ctypes.c_char_p(ret).value

class Event(object):

    def init(plugin_event:OlivOS.API.Event, Proc:OlivOS.pluginAPI.shallow):
        if platform.system().lower() != 'windows':
            raise Exception(g_plus_name + '只可以在windows平台运行')

        if platform.architecture()[0] not in ['32bit','64bit']:
            raise Exception(g_plus_name + '只可以在32位或64位python上运行')
        
        # 获得插件解压路径
        plus_dir = os.path.dirname(os.path.abspath(__file__)) + os.sep
        # 复制IPC,MiraiCQ等文件到插件配置文件路径
        copy3(plus_dir + "IPCTool", "./plugin/data/"+ g_plus_name +"/IPCTool")
        copy3(plus_dir + "MiraiCQ", "./plugin/data/"+ g_plus_name +"/MiraiCQ")

    def init_after(plugin_event:OlivOS.API.Event, Proc:OlivOS.pluginAPI.shallow):

        global g_proc
        global g_bot_list
        global g_miraicq

        g_proc = Proc
        g_bot_list = [g_proc.Proc_data['bot_info_dict'][k] for k in g_proc.Proc_data['bot_info_dict'] if g_proc.Proc_data['bot_info_dict'][k].platform['sdk'] == 'onebot']

        if len(g_bot_list) == 0:
            raise Exception('没有找onebot账号,' + g_plus_name + '只可以在onebot平台运行')

        #  初始化IPC工具
        global g_my_ipc_ser
        if platform.architecture()[0] == '32bit':
            g_my_ipc_ser = IPCTool("./plugin/data/"+ g_plus_name +"/IPCTool/IPCTool.dll",ser_uid)
        elif platform.architecture()[0] == '64bit':
            g_my_ipc_ser = IPCTool("./plugin/data/"+ g_plus_name +"/IPCTool/IPCTool64.dll",ser_uid)

        # 开启API进程通讯服务
        threading.Thread(target=server_api_thread_func).start()

        # 启动MiraiCQ进程
        g_miraicq = subprocess.Popen('.\\plugin\\data\\'+ g_plus_name +'\\MiraiCQ\\MiraiCQ.exe OVO {} {} {}'.format(ser_uid,client_uid,base64_t),shell=False,stdout = subprocess.PIPE, stderr=subprocess.STDOUT,creationflags = subprocess.CREATE_NO_WINDOW)

        # 消耗掉子进程控制台输入，防止阻塞
        def ff(g_miraicq):
            while True:
                g_miraicq.stdout.readline()
        threading.Thread(target=ff,args=(g_miraicq,)).start()

        # 启动插件
        printLog("等待"+g_plus_name+"启动...")
        tm  = time.time()
        while True:
            if time.time() - tm > 5:
                raise Exception(g_plus_name+"启动失败")
            apiret = g_my_ipc_ser.call_api(client_uid,b'{"action":"is_load"}',500)
            if apiret == b'OK':
                break
        printLog(g_plus_name+"启动完成...")

        # 对event api进行hook
        plugin_event_router_t = Proc.plugin_event_router
        def mask_plugin_event_router(plugin_event, plugin_model, plugin_name):
            try:
                if plugin_name == g_plus_name and g_bot_list[0].hash == plugin_event.bot_info.hash:
                    try:
                        to_send = plugin_event.sdk_event.raw
                        g_my_ipc_ser.send_event(client_uid,to_send.encode('utf-8'))
                    except:
                        printLog(traceback.format_exc(),level=3)
            except:
                pass
            return plugin_event_router_t(plugin_event,plugin_model,plugin_name)
        Proc.plugin_event_router = mask_plugin_event_router
        
            
    def menu(plugin_event:OlivOS.API.Event, Proc:OlivOS.pluginAPI.shallow):
        if plugin_event.data.namespace == g_plus_name:
                call_cq_menu(plugin_event.data.event)

    # def save(plugin_event:OlivOS.API.Event, Proc:OlivOS.pluginAPI.shallow):
    #     printLog("结束进程。。。")
    #     # 发送cq的框架退出事件
    #     g_my_ipc_ser.send_event(client_uid,b'{"event_type":"exit"}')
    #     tm = time.time()
    #     while True:
    #         if time.time() - tm > 5: # 最多等待插件5秒钟
    #             printLog("强制结束进程。。。",level=3)
    #             break
    #         apiret = g_my_ipc_ser.call_api(client_uid,b'{"action":"is_load"}',500)
    #         # 返回''说明插件未启动
    #         if apiret == b'':
    #             printLog("进程已经结束。。。")
    #             break
    #         time.sleep(0.5)
    #     # 无论插件反馈如何，都杀死进程，即使没杀死也无所谓，MiraiCQ进程本身也会自己结束自己
    #     g_miraicq.terminate()



        

