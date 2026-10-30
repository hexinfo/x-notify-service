#!/usr/bin/env python3
"""Win7 兼容性闸门:解析 PE 导入表,禁止 Win8+/Win10+ 才有的 API 硬导入。

背景:Rust 1.78 起 std 面向 Win10,默认产物硬导入 GetSystemTimePreciseAsFileTime
(Win8+)、api-ms-win-core-synch-l1-2-0.dll 的 WaitOnAddress 族(Win8+)、
bcryptprimitives.dll 的 ProcessPrng(Win10+),在 Win7 上加载器直接报
"无法定位程序输入点"。本项目 Windows 产物改用 *-win7-windows-msvc 目标
(std 内部自动降级/延迟加载这些 API)并静态链接 CRT,本脚本验证这两点成立。

--fix-combase:dirs-sys 经 windows-sys 0.61 调 CoTaskMemFree,而该版本把
此函数绑定到 combase.dll(Win8+ 才有,Win7 报"丢失 combase.dll";ole32.dll
在所有 Windows 都导出它)。此模式将 combase.dll 导入项原地改名为
ole32.dll(更短、同缓冲区内改写,导入表 RVA 不动)。安全护栏:仅当 combase
导入符号集恰为 {CoTaskMemFree} 时执行——未来若混入真 WinRT 符号
(ole32 并不导出)则拒绝改写并报错,由闸门失败暴露。

用法: python3 check_win7_imports.py <exe> [--list] [--fix-combase]
退出码: 0 通过 / 1 发现违禁导入(产物无法在 Win7 加载)或无法安全修复
"""
import struct
import sys

# Windows runner 的 Python stdout 默认 cp1252,中文输出会 UnicodeEncodeError
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# Win7 的系统 DLL 中不存在的入口点(Win8+/Win10+ 才有)
FORBIDDEN_SYMBOLS = {
    "GetSystemTimePreciseAsFileTime",                    # Win8+  kernel32
    "WaitOnAddress", "WakeByAddressAll", "WakeByAddressSingle",  # Win8+  synch API set
    "ProcessPrng",                                        # Win10+ bcryptprimitives
    "SetThreadDescription", "GetThreadDescription",       # Win10 1607+
    "GetDpiForWindow", "SetProcessDpiAwarenessContext",   # Win10 1607+/1703+
    "AdjustWindowRectExForDpi", "GetSystemMetricsForDpi",
    "SetDefaultDllDirectories",                           # Win8+  kernel32
    # WinRT(Win8+,经 combase.dll / api-ms-win-core-winrt-*)
    "RoInitialize", "RoUninitialize", "RoActivateInstance",
    "RoGetActivationFactory", "WindowsCreateString",
    "WindowsDeleteString", "WindowsGetStringRawBuffer",
}

# 动态 CRT 依赖(crt-static 闸门):Win7 无补丁的机器上这些 DLL 可能不存在;
# combase.dll 为 Win8+(WinRT)——除非 --fix-combase 已把仅含 CoTaskMemFree
# 的导入项改名 ole32,否则在 Win7 报"丢失 combase.dll"
FORBIDDEN_DLL_EXACT = {
    "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll",
    "ucrtbase.dll", "api-ms-win-core-synch-l1-2-0.dll",
    "combase.dll", "api-ms-win-core-winrt-l1-1-0.dll",
}
FORBIDDEN_DLL_PREFIX = ("api-ms-win-crt-",)

# 允许经 --fix-combase 重定向到 ole32.dll 的唯一符号集
COMBASE_REDIRECTABLE = {"CoTaskMemFree"}


def parse_imports(path):
    data = open(path, "rb").read()
    if data[:2] != b"MZ":
        sys.exit(f"❌ 不是 PE 文件: {path}")
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        sys.exit("❌ 缺少 PE 签名")
    coff = pe_off + 4
    nsec = struct.unpack_from("<H", data, coff + 2)[0]
    opt_size = struct.unpack_from("<H", data, coff + 16)[0]
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    if magic == 0x20B:  # PE32+
        dd_off = opt + 112
        thunk_sz = 8
    elif magic == 0x10B:  # PE32
        dd_off = opt + 96
        thunk_sz = 4
    else:
        sys.exit(f"❌ 未知 Optional Header magic: {magic:#x}")
    import_rva = struct.unpack_from("<I", data, dd_off + 1 * 8)[0]

    sections = []
    sec = opt + opt_size
    for _ in range(nsec):
        vsz, va, rsz, raw = struct.unpack_from("<IIII", data, sec + 8)
        sections.append((va, vsz, raw, rsz))
        sec += 40

    def rva2off(rva):
        for va, vsz, raw, rsz in sections:
            if va <= rva < va + max(vsz, rsz):
                return raw + (rva - va)
        return None

    def cstr(off):
        end = data.index(b"\0", off)
        return data[off:end].decode("ascii", "replace")

    imports = {}  # dll_name(lower) -> set(symbol)
    descriptors = []  # (dll_name(lower), dll_name字符串文件偏移, set(symbol))
    if import_rva == 0:
        return imports, descriptors
    desc = rva2off(import_rva)
    while True:
        oft, _ts, _fc, name_rva, ft = struct.unpack_from("<IIIII", data, desc)
        if oft == 0 and name_rva == 0 and ft == 0:
            break
        name_off = rva2off(name_rva)
        dll = cstr(name_off)
        thunk_rva = oft or ft
        off = rva2off(thunk_rva)
        syms = set()
        while True:
            (val,) = struct.unpack_from("<Q" if thunk_sz == 8 else "<I", data, off)
            if val == 0:
                break
            if not val & (1 << 63 if thunk_sz == 8 else 1 << 31):  # 按名导入
                syms.add(cstr(rva2off(val) + 2))  # 跳过 2 字节 Hint
            off += thunk_sz
        imports.setdefault(dll.lower(), set()).update(syms)
        descriptors.append((dll.lower(), name_off, syms))
        desc += 20
    return imports, descriptors


def fix_combase(path):
    """把仅含 CoTaskMemFree 的 combase.dll 导入项原地改名为 ole32.dll。

    ole32.dll 在所有 Windows(含 Win7)都导出 CoTaskMemFree,仅 Windows SDK
    metadata 把它记到了 combase.dll(Win8+)名下。新名更短,在原字符串
    缓冲区内改写即可,导入表结构不动。
    """
    data = bytearray(open(path, "rb").read())
    _, descriptors = parse_imports(path)
    targets = [(name_off, syms) for dll, name_off, syms in descriptors if dll == "combase.dll"]
    if not targets:
        print("combase.dll 导入: 无,无需改写")
        return
    for name_off, syms in targets:
        if not syms <= COMBASE_REDIRECTABLE:
            sys.exit(
                f"❌ combase.dll 导入含 CoTaskMemFree 之外的符号 {sorted(syms)},"
                "不能安全重定向到 ole32.dll(ole32 不导出它们),请人工分析依赖"
            )
        old = bytes(data[name_off : name_off + 12])
        if old != b"combase.dll\0":
            sys.exit(f"❌ 意外的导入名缓冲区内容: {old!r}")
        data[name_off : name_off + 12] = b"ole32.dll\0\0\0"
    open(path, "wb").write(data)
    print(f"✅ 已改写 combase.dll → ole32.dll ({', '.join(sorted(COMBASE_REDIRECTABLE))})")


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    if "--fix-combase" in sys.argv:
        fix_combase(path)
    imports, _ = parse_imports(path)

    if "--list" in sys.argv:
        for dll in sorted(imports):
            print(f"== {dll} ({len(imports[dll])}) ==")
            for s in sorted(imports[dll]):
                print("   ", s)
        return

    violations = []
    for dll, syms in sorted(imports.items()):
        if dll in FORBIDDEN_DLL_EXACT or dll.startswith(FORBIDDEN_DLL_PREFIX):
            violations.append(f"DLL {dll} (应为静态链接 CRT / 不该依赖此 API set)")
        for s in sorted(syms & FORBIDDEN_SYMBOLS):
            violations.append(f"{dll}: {s}")

    total = sum(len(s) for s in imports.values())
    print(f"导入表: {len(imports)} 个 DLL, {total} 个符号")
    if violations:
        print("❌ Win7 不兼容导入:")
        for v in violations:
            print("   -", v)
        sys.exit(1)
    print("✅ 无 Win8+/Win10+ 硬导入, 无动态 CRT 依赖 — Win7 兼容")


if __name__ == "__main__":
    main()
