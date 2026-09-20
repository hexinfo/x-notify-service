; x-notify-service Windows 安装器(NSIS 薄壳 + MUI2)
; 设计:per-user 免 UAC、可选安装目录、LZMA 压缩、完成页「启动服务」复选框;
;      注册/启动逻辑只在二进制内(x-notify-service.exe install),脚本不含第二套逻辑。
; 构建: makensis -DSTAGE=<staging> -DVERSION=<ver> scripts/pack-windows.nsi

Unicode true
SetCompressor /SOLID lzma
ManifestDPIAware true

!define APPNAME "x-notify-service"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
!define MUI_ICON "${STAGE}\x-notify-service.ico"
!define MUI_UNICON "${STAGE}\x-notify-service.ico"

Name "${APPNAME} ${VERSION}"
OutFile "dist\${APPNAME}-${VERSION}-windows-x86_64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel user
ShowUninstDetails show

!include "MUI2.nsh"


!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Section "安装"
    SetShellVarContext current
    SetOutPath "$INSTDIR"

    ; 升级场景:结束正在运行的旧实例(无状态服务,强杀安全,flock 自动释放)
    nsExec::Exec 'taskkill /F /IM ${APPNAME}.exe'

    File "${STAGE}\x-notify-service.exe"
    File /nonfatal "${STAGE}\config.toml"
    File "${STAGE}\sdk.js"
    File "${STAGE}\sdk.umd.js"
    File "${STAGE}\sdk-manual.md"

    WriteRegStr HKCU "Software\${APPNAME}" "InstallDir" "$INSTDIR"

    ; 控制面板卸载项(用户级)
    WriteUninstaller "$INSTDIR\uninstall.exe"
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "HexInfo"
    WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1

    ; 静默完成安装:注册自启动+协议并分离启动服务(install 立即返回,不阻塞安装器)
    Exec '"$INSTDIR\${APPNAME}.exe" install'
    DetailPrint "安装完成。SDK:安装目录内 sdk.js / sdk.umd.js / sdk-manual.md"
    DetailPrint "快速测试:浏览器打开 http://127.0.0.1:17320/"
SectionEnd

Section "Uninstall"
    SetShellVarContext current
    ; 注销注册项
    ExecWait '"$INSTDIR\${APPNAME}.exe" uninstall'
    ; 结束服务进程后清理文件
    nsExec::Exec 'taskkill /F /IM ${APPNAME}.exe'
    Sleep 500
    Delete "$INSTDIR\${APPNAME}.exe"
    Delete "$INSTDIR\sdk.js"
    Delete "$INSTDIR\sdk.umd.js"
    Delete "$INSTDIR\sdk-manual.md"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKCU "${UNINSTKEY}"
    DeleteRegKey HKCU "Software\${APPNAME}"
SectionEnd
