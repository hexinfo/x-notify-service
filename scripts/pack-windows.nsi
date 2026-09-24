; x-notify-service Windows 安装器(NSIS 薄壳 + MUI2)
; 设计:per-user 免 UAC、可选安装目录、LZMA 压缩、完成页「启动服务」复选框;
;      注册/启动逻辑只在二进制内(x-notify-service.exe install),脚本不含第二套逻辑。
; 构建: makensis -DSTAGE=<staging> -DVERSION=<ver> scripts/pack-windows.nsi

Unicode true
SetCompressor /SOLID lzma
ManifestDPIAware true

!define APPNAME "x-notify-service"
!define APPKEY "Software\Hexinfo\${APPNAME}"
!define OLDAPPKEY "Software\${APPNAME}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
!define MUI_ICON "${STAGE}\x-notify-service.ico"
!define MUI_UNICON "${STAGE}\x-notify-service.ico"

Name "${APPNAME} ${VERSION}"
OutFile "dist\${APPNAME}-${VERSION}-windows-x86_64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Hexinfo\${APPNAME}"
InstallDirRegKey HKCU "${APPKEY}" "InstallDir"
RequestExecutionLevel user
ShowUninstDetails show

!include "MUI2.nsh"
!include "LogicLib.nsh"


!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Var OldInstDir

Section "安装"
    SetShellVarContext current

    ; 仅在旧版程序确实存在时卸载旧位置。旧版卸载器会清理自启动和协议注册。
    ReadRegStr $OldInstDir HKCU "${OLDAPPKEY}" "InstallDir"
    ${If} $OldInstDir == ""
        ReadRegStr $OldInstDir HKCU "${UNINSTKEY}" "InstallLocation"
    ${EndIf}
    ${If} $OldInstDir == ""
        StrCpy $OldInstDir "$LOCALAPPDATA\Programs\${APPNAME}"
    ${EndIf}
    StrCmp $OldInstDir "$INSTDIR" old_done
        IfFileExists "$OldInstDir\uninstall.exe" 0 old_exe
        ExecWait '"$OldInstDir\uninstall.exe" /S _?=$OldInstDir' $0
        ${If} $0 != 0
            Abort "旧版卸载失败($0)，安装已停止。"
        ${EndIf}
        Goto old_done
        old_exe:
        IfFileExists "$OldInstDir\${APPNAME}.exe" 0 old_done
        ExecWait '"$OldInstDir\${APPNAME}.exe" uninstall' $0
        ${If} $0 != 0
            Abort "旧版服务注销失败($0)，安装已停止。"
        ${EndIf}
        old_done:

    ; 升级场景:结束正在运行的旧实例(无状态服务,强杀安全,flock 自动释放)
    nsExec::Exec 'taskkill /F /IM ${APPNAME}.exe'

    SetOutPath "$INSTDIR"
    File "${STAGE}\x-notify-service.exe"
    SetOverwrite off
    File /nonfatal "${STAGE}\config.toml"
    SetOverwrite on
    File "${STAGE}\sdk.js"
    File "${STAGE}\sdk.umd.js"
    File "${STAGE}\sdk-manual.md"

    WriteRegStr HKCU "${APPKEY}" "InstallDir" "$INSTDIR"

    ; 控制面板卸载项(用户级)
    WriteUninstaller "$INSTDIR\uninstall.exe"
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "HexInfo"
    WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1

    ; 注册自启动和协议；install 会分离服务进程并立即返回。
    ExecWait '"$INSTDIR\${APPNAME}.exe" install' $0
    ${If} $0 != 0
        Abort "新版服务注册失败($0)，安装已停止。"
    ${EndIf}
    ; 新版完成后只清理两个固定的旧目录，绝不按注册表路径递归删除自定义目录。
    StrCmp $INSTDIR "$LOCALAPPDATA\Programs\${APPNAME}" skip_old_program_cleanup
    RMDir /r "$LOCALAPPDATA\Programs\${APPNAME}"
    skip_old_program_cleanup:
    RMDir /r "$LOCALAPPDATA\${APPNAME}"
    DeleteRegKey HKCU "${OLDAPPKEY}"
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
    DeleteRegKey HKCU "${APPKEY}"
SectionEnd
