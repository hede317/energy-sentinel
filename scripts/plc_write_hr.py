#!/usr/bin/env python3
"""OpenPLC 沙盘 Modbus TCP 命令写入器（纯标准库，零依赖）
用法:
  python3 plc_write_hr.py --read 10 8              # 读 HR10 起 8 个寄存器
  python3 plc_write_hr.py 60 1                     # 写单个寄存器（FC06）：HR60=1 触发过载演练
  python3 plc_write_hr.py 56 420 57 150            # 多寄存器（FC16）：SP=0.42MPa, demand=150%
寄存器地址 = ST 里的 %QW 地址（feeder-im 命令字 HR60，pump 命令 HR56/57）。
"""
import socket
import struct
import sys

HOST, PORT, UNIT = "127.0.0.1", 5020, 1


def frame(tid, unit, pdu):
    return struct.pack(">HHHB", tid, 0, len(pdu) + 1, unit) + pdu


def parse(ip):
    tid, _pid, _l, uid, fc = struct.unpack(">HHHBB", ip[:8])
    return tid, uid, fc, ip[8:]


def read_regs(sock, addr, qty):
    pdu = struct.pack(">BHH", 3, addr, qty)
    sock.sendall(frame(next(tids), UNIT, pdu))
    ip = sock.recv(512)
    _, _, fc, data = parse(ip)
    return struct.unpack(f">{qty}H", data[1:1 + qty * 2])


def write_single(sock, addr, val):
    pdu = struct.pack(">BHH", 6, addr, val)
    sock.sendall(frame(next(tids), UNIT, pdu))
    parse(sock.recv(512))


def write_multi(sock, addr, vals):
    pdu = struct.pack(">BHHB", 16, addr, len(vals), len(vals) * 2)
    pdu += struct.pack(f">{len(vals)}H", *vals)
    sock.sendall(frame(next(tids), UNIT, pdu))
    parse(sock.recv(512))


tids = iter(range(1, 1000))

if __name__ == "__main__":
    a = sys.argv[1:]
    s = socket.create_connection((HOST, PORT), timeout=5)
    if a[0] == "--read":
        regs = read_regs(s, int(a[1]), int(a[2]))
        print(f"HR{a[1]}..{int(a[1]) + int(a[2]) - 1} =", list(regs))
    elif len(a) == 2:
        write_single(s, int(a[0]), int(a[1]))
        print(f"HR{a[0]} = {a[1]} (FC06)")
    else:
        pairs = [(int(a[i]), int(a[i + 1])) for i in range(0, len(a), 2)]
        base = pairs[0][0]
        vals = [v for _, v in pairs]
        write_multi(s, base, vals)
        print(f"HR{base}.. = {vals} (FC16)")
    s.close()
