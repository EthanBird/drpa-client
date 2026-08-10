# -*- coding: utf-8 -*-
Unicode true

!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR must point to the complete offline application payload"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE must be provided"
!endif
!ifndef ICON_FILE
  !error "ICON_FILE must be provided"
!endif

!define APP_NAME "DRPA Next"
!define APP_VERSION "2.1.0"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$EXEDIR\DRPA Next"
RequestExecutionLevel user
BrandingText "DRPA Next | Offline-first | Registry-free"
Icon "${ICON_FILE}"
UninstallIcon "${ICON_FILE}"
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\DRPA Next.exe"
!define MUI_FINISHPAGE_RUN_TEXT "启动 DRPA Next"
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

Function VerifyInstallDirectory
  ReadEnvStr $0 "SystemDrive"
  StrCpy $1 "$INSTDIR" 2
  ${If} $1 == $0
    MessageBox MB_ICONEXCLAMATION|MB_OK "DRPA 工作区数据不能安装到 Windows 系统盘（$0）。请选择其他磁盘，例如 D:\DRPA Next。"
    Abort
  ${EndIf}
FunctionEnd

Section "DRPA Next" SEC_APP
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD_DIR}\*"
  WriteUninstaller "$INSTDIR\卸载 DRPA Next.exe"
  CreateDirectory "$SMPROGRAMS\DRPA Next"
  CreateShortcut "$SMPROGRAMS\DRPA Next\DRPA Next.lnk" "$INSTDIR\DRPA Next.exe" "" "$INSTDIR\DRPA Next.exe"
  CreateShortcut "$SMPROGRAMS\DRPA Next\卸载 DRPA Next.lnk" "$INSTDIR\卸载 DRPA Next.exe"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\DRPA Next\DRPA Next.lnk"
  Delete "$SMPROGRAMS\DRPA Next\卸载 DRPA Next.lnk"
  RMDir "$SMPROGRAMS\DRPA Next"
  Delete "$INSTDIR\DRPA Next.exe"
  Delete "$INSTDIR\install-manifest.json"
  Delete "$INSTDIR\README.md"
  RMDir /r "$INSTDIR\runtime"
  RMDir /r "$INSTDIR\webview2"
  RMDir /r "$INSTDIR\examples"
  RMDir /r "$INSTDIR\jcode"
  Delete "$INSTDIR\卸载 DRPA Next.exe"
  RMDir "$INSTDIR"
  MessageBox MB_ICONINFORMATION|MB_OK "应用文件已卸载。为避免误删项目和运行产物，$INSTDIR\data 数据目录已保留，可手动备份或删除。"
SectionEnd
