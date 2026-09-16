Unicode true
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!define PRODUCT_NAME "AMRI VPN"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "AMRI"

Name "${PRODUCT_NAME}"
OutFile "..\..\dist\AMRI-VPN-Windows-Setup.exe"
Icon "..\..\dist\windows\amri-vpn.ico"
UninstallIcon "..\..\dist\windows\amri-vpn.ico"
InstallDir "$PROGRAMFILES64\AMRI VPN"
InstallDirRegKey HKLM "Software\AMRI VPN" "InstallDir"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "AMRI VPN" SecMain
  SetRegView 64
  SetOutPath "$INSTDIR"
  File "..\..\dist\windows\AMRI-VPN.exe"
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

  ; Older preview installers used the AppCompat RUNASADMIN compatibility layer. The executable now
  ; carries a proper requireAdministrator manifest, so remove that legacy override on upgrade.
  DeleteRegValue HKLM "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\AMRI-VPN.exe"

  CreateDirectory "$SMPROGRAMS\AMRI VPN"
  CreateShortcut "$SMPROGRAMS\AMRI VPN\AMRI VPN.lnk" "$INSTDIR\AMRI-VPN.exe" "" "$INSTDIR\AMRI-VPN.exe" 0 SW_SHOWNORMAL "" "$INSTDIR"
  CreateShortcut "$DESKTOP\AMRI VPN.lnk" "$INSTDIR\AMRI-VPN.exe" "" "$INSTDIR\AMRI-VPN.exe" 0 SW_SHOWNORMAL "" "$INSTDIR"
SectionEnd

Section "Uninstall"
  SetRegView 64
  Delete "$DESKTOP\AMRI VPN.lnk"
  Delete "$SMPROGRAMS\AMRI VPN\AMRI VPN.lnk"
  RMDir "$SMPROGRAMS\AMRI VPN"
  DeleteRegValue HKLM "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\AMRI-VPN.exe"
  Delete "$INSTDIR\AMRI-VPN.exe"
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
