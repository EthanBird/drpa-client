# -*- coding: utf-8 -*-
Unicode true

!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR must point to the lightweight Core payload"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE must be provided"
!endif
!ifndef ICON_FILE
  !error "ICON_FILE must be provided"
!endif

!define APP_NAME "DRPA Next"
!define APP_VERSION "3.0.0"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$EXEDIR\DRPA Next"
RequestExecutionLevel user
BrandingText "DRPA Next | Offline-first | Registry-free"
Icon "${ICON_FILE}"
UninstallIcon "${ICON_FILE}"
SetCompressor lzma
ShowInstDetails show
ShowUninstDetails show

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\DRPA Component Installer.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--install-root $\"$INSTDIR$\" --scan $\"$EXEDIR$\""
!define MUI_FINISHPAGE_RUN_TEXT "打开组件安装向导"
!define MUI_DIRECTORYPAGE_TEXT_TOP "请选择非系统盘上的安装目录。应用数据将保存在该目录的 data 文件夹中。"
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE VerifyInstallDirectory

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Function TrimLineEnd
  Exch $R0
  Push $R1
trim_line_end_loop:
  StrCpy $R1 $R0 1 -1
  StrCmp $R1 "$\r" trim_line_end_remove
  StrCmp $R1 "$\n" trim_line_end_remove
  Goto trim_line_end_done
trim_line_end_remove:
  StrCpy $R0 $R0 -1
  Goto trim_line_end_loop
trim_line_end_done:
  Pop $R1
  Exch $R0
FunctionEnd

Function .onInit
  InitPluginsDir
  File /oname=$PLUGINSDIR\drpa.exe "${PAYLOAD_DIR}\drpa.exe"
  nsExec::ExecToStack '"$PLUGINSDIR\drpa.exe" install locate'
  Pop $0
  Pop $1
  ${If} $0 == 0
    Push $1
    Call TrimLineEnd
    Pop $1
    ${If} $1 != ""
      StrCpy $INSTDIR $1
    ${EndIf}
  ${EndIf}
FunctionEnd

Function VerifyInstallDirectory
  ReadEnvStr $0 "SystemDrive"
  StrCpy $1 "$INSTDIR" 2
  ${If} $1 == $0
    MessageBox MB_ICONEXCLAMATION|MB_OK "DRPA 工作区数据不能安装到 Windows 系统盘（$0）。请选择其他磁盘，例如 D:\DRPA Next。"
    Abort
  ${EndIf}
FunctionEnd

Section "DRPA Core（命令行、启动器与组件向导）" SEC_APP
  SectionIn RO
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD_DIR}\*"
  ExecWait '"$INSTDIR\drpa.exe" --install-root "$INSTDIR" install init "$INSTDIR"' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "DRPA Core 初始化失败（退出码 $0）。"
    Abort
  ${EndIf}
  ExecWait '"$INSTDIR\drpa.exe" --install-root "$INSTDIR" install reconcile-core "$INSTDIR\core-files.json"' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "旧版核心文件整理失败（退出码 $0）。"
    Abort
  ${EndIf}
  WriteUninstaller "$INSTDIR\卸载 DRPA Next.exe"
  CreateDirectory "$SMPROGRAMS\DRPA Next"
  CreateShortcut "$SMPROGRAMS\DRPA Next\DRPA Next.lnk" "$INSTDIR\DRPA Next.exe" "" "$INSTDIR\DRPA Next.exe"
  CreateShortcut "$SMPROGRAMS\DRPA Next\组件安装向导.lnk" "$INSTDIR\DRPA Component Installer.exe" '--install-root "$INSTDIR"' "$INSTDIR\DRPA Component Installer.exe"
  CreateShortcut "$SMPROGRAMS\DRPA Next\卸载 DRPA Next.lnk" "$INSTDIR\卸载 DRPA Next.exe"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\DRPA Next\DRPA Next.lnk"
  Delete "$SMPROGRAMS\DRPA Next\组件安装向导.lnk"
  Delete "$SMPROGRAMS\DRPA Next\卸载 DRPA Next.lnk"
  RMDir "$SMPROGRAMS\DRPA Next"
  Delete "$INSTDIR\DRPA Next.exe"
  Delete "$INSTDIR\DRPA Component Installer.exe"
  ExecWait '"$INSTDIR\drpa.exe" --install-root "$INSTDIR" install unregister'
  Delete "$INSTDIR\drpa.exe"
  Delete "$INSTDIR\.drpa-install.json"
  Delete "$INSTDIR\install-manifest.json"
  Delete "$INSTDIR\core-files.json"
  Delete "$INSTDIR\README.md"
  RMDir /r "$INSTDIR\runtime"
  RMDir /r "$INSTDIR\webview2"
  RMDir /r "$INSTDIR\examples"
  RMDir /r "$INSTDIR\jcode"
  RMDir /r "$INSTDIR\components"
  RMDir /r "$INSTDIR\component-packs"
  RMDir /r "$INSTDIR\state"
  Delete "$INSTDIR\卸载 DRPA Next.exe"
  RMDir "$INSTDIR"
  MessageBox MB_ICONINFORMATION|MB_OK "应用文件已卸载。为避免误删项目和运行产物，$INSTDIR\data 数据目录已保留，可手动备份或删除。"
SectionEnd
