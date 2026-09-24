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


!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Section "安装"
    SetShellVarContext current
    StrCpy $R0 "$LOCALAPPDATA\Programs\${APPNAME}"

    ; 升级场景:结束正在运行的旧实例(无状态服务,强杀安全,flock 自动释放)
    nsExec::Exec 'taskkill /F /IM ${APPNAME}.exe'

    ; 只迁移精确的旧默认安装目录。目录整体移动可保留未知用户文件。
    ; 新目录已存在或移动失败时保留旧目录，避免覆盖同名文件。
    StrCmp $INSTDIR "$LOCALAPPDATA\Programs\Hexinfo\${APPNAME}" 0 migrationDone
    IfFileExists "$R0" 0 migrationDone
    IfFileExists "$INSTDIR" migrationConflict 0
    CreateDirectory "$LOCALAPPDATA\Programs\Hexinfo"
    ClearErrors
    Rename "$R0" "$INSTDIR"
    IfErrors migrationConflict migrationDone
migrationConflict:
    DetailPrint "旧默认安装目录未整体迁移，已保留: $R0；请检查新旧目录中的同名文件。"
    IfSilent migrationDone 0
    MessageBox MB_ICONEXCLAMATION|MB_OK "旧默认安装目录未整体迁移，已保留: $R0。请检查新旧目录中的同名文件。"
migrationDone:
    SetOutPath "$INSTDIR"

    ClearErrors
    File "${STAGE}\x-notify-service.exe"
    IfErrors 0 executableReady
    Abort "新程序写入失败；旧安装位置注册表已保留"
executableReady:
    ; 首次从旧默认目录升级时保留用户配置;已有新目录配置优先于旧配置。
    StrCmp $INSTDIR $R0 configReady 0
    IfFileExists "$INSTDIR\config.toml" configReady 0
    IfFileExists "$R0\config.toml" 0 configReady
    ClearErrors
    CopyFiles /SILENT "$R0\config.toml" "$INSTDIR\config.toml"
    IfErrors 0 configReady
    Abort "旧配置复制失败；旧默认安装目录已保留"
configReady:
    IfFileExists "$INSTDIR\config.toml" configDone 0
    File /nonfatal "${STAGE}\config.toml"
configDone:
    ClearErrors
    File "${STAGE}\sdk.js"
    File "${STAGE}\sdk.umd.js"
    File "${STAGE}\sdk-manual.md"
    IfErrors 0 filesReady
    Abort "SDK 写入失败；旧安装位置注册表已保留"
filesReady:

    ; 控制面板卸载项(用户级)
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; 等待新版程序完成自启动、协议及用户数据迁移，再更新安装位置注册表。
    ClearErrors
    ExecWait '"$INSTDIR\${APPNAME}.exe" install' $R1
    IfErrors installFailed 0
    StrCmp $R1 "0" installSucceeded installFailed
installFailed:
    Abort "新程序安装失败；旧目录及旧安装位置注册表已保留"
installSucceeded:
    ClearErrors
    WriteRegStr HKCU "${APPKEY}" "InstallDir" "$INSTDIR"
    IfErrors 0 registryReady
    Abort "新安装位置注册失败；旧安装位置注册表已保留"
registryReady:
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
    WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "HexInfo"
    WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
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
