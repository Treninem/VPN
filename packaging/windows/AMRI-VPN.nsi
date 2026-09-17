Unicode true
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "FileFunc.nsh"
!include "Sections.nsh"

!define PRODUCT_NAME "AMRI VPN"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "AMRI"

Name "${PRODUCT_NAME}"
OutFile "..\..\dist\AMRI-VPN-Windows-Setup.exe"
InstallDir "$PROGRAMFILES64\AMRI VPN"
InstallDirRegKey HKLM "Software\AMRI VPN" "InstallDir"

Page directory
Page components
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "AMRI VPN" SecMain
  SectionIn RO
  SetRegView 64
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "..\..\dist\windows\AMRI-VPN.exe"
  File "..\..\dist\windows\AMRI-VPN-Launcher.exe"
  File "..\..\dist\windows\sing-box.exe"
  File "..\..\dist\windows\wintun.dll"
  File "..\..\THIRD_PARTY_NOTICES.md"
  File /nonfatal "..\..\dist\windows\sing-box-LICENSE"
  File /nonfatal "..\..\dist\windows\wintun-LICENSE.txt"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\AMRI VPN" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN" "DisplayIcon" '"$INSTDIR\AMRI-VPN.exe",0'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN" "UninstallString" '"$INSTDIR\Uninstall.exe"'

  ; Remove compatibility overrides left by preview installers. AMRI now keeps the GUI asInvoker and
  ; requests elevation explicitly through its tiny launcher so startup remains deterministic.
  DeleteRegValue HKLM "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\AMRI-VPN.exe"

  CreateDirectory "$SMPROGRAMS\AMRI VPN"
  CreateShortcut "$SMPROGRAMS\AMRI VPN\AMRI VPN.lnk" "$INSTDIR\AMRI-VPN-Launcher.exe" "" "$INSTDIR\AMRI-VPN.exe" 0 SW_SHOWNORMAL "" "$INSTDIR"
  CreateShortcut "$DESKTOP\AMRI VPN.lnk" "$INSTDIR\AMRI-VPN-Launcher.exe" "" "$INSTDIR\AMRI-VPN.exe" 0 SW_SHOWNORMAL "" "$INSTDIR"
SectionEnd

Section /o "Start AMRI VPN with Windows" SecAutostart
  SetShellVarContext current
  CreateShortcut "$SMSTARTUP\AMRI VPN.lnk" "$INSTDIR\AMRI-VPN-Launcher.exe" "" "$INSTDIR\AMRI-VPN.exe" 0 SW_SHOWNORMAL "" "$INSTDIR"
SectionEnd

Function .onInit
  ; Interactive installs leave autostart unchecked. `/AUTOSTART` exists so unattended installs and
  ; CI can explicitly opt in and exercise the same optional component without changing the default.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/AUTOSTART" $R1
  IfErrors autostart_done
  SectionGetFlags ${SecAutostart} $R2
  IntOp $R2 $R2 | ${SF_SELECTED}
  SectionSetFlags ${SecAutostart} $R2

autostart_done:
FunctionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext current
  Delete "$SMSTARTUP\AMRI VPN.lnk"
  Delete "$DESKTOP\AMRI VPN.lnk"
  Delete "$SMPROGRAMS\AMRI VPN\AMRI VPN.lnk"
  RMDir "$SMPROGRAMS\AMRI VPN"
  DeleteRegValue HKLM "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\AMRI-VPN.exe"
  Delete "$INSTDIR\AMRI-VPN.exe"
  Delete "$INSTDIR\AMRI-VPN-Launcher.exe"
  Delete "$INSTDIR\sing-box.exe"
  Delete "$INSTDIR\wintun.dll"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\sing-box-LICENSE"
  Delete "$INSTDIR\wintun-LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMRI VPN"
  DeleteRegKey HKLM "Software\AMRI VPN"
SectionEnd
