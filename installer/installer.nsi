; Plex (Rust) v0.4.0 -- current-user installer (no admin, no UAC).
; Built with NSIS 3 (makensis installer/installer.nsi, CWD = repo root).
; Installs the egui dashboard + the CLI/MCP server side by side.

!include "MUI2.nsh"

Name "Plex (Rust) 0.4.0"
OutFile "..\target\release\bundle\Plex-Rust-0.4.0-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\rust-plex-mcp"
RequestExecutionLevel user

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Binaries" SecBinaries
  SetOutPath "$INSTDIR"
  File "..\target\release\plex-app.exe"
  File "..\target\release\rust-plex-mcp.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp" \
    "DisplayName" "Plex (Rust) 0.4.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp" \
    "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp" \
    "DisplayIcon" "$INSTDIR\plex-app.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp" \
    "DisplayVersion" "0.4.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp" \
    "Publisher" "sandraschi"
SectionEnd

Section "Start Menu shortcuts" SecShortcuts
  CreateDirectory "$SMPROGRAMS\Plex (Rust)"
  CreateShortcut "$SMPROGRAMS\Plex (Rust)\Plex (Rust).lnk" "$INSTDIR\plex-app.exe"
  CreateShortcut "$SMPROGRAMS\Plex (Rust)\Uninstall.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "uninstall"
  Delete "$INSTDIR\plex-app.exe"
  Delete "$INSTDIR\rust-plex-mcp.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Plex (Rust)\Plex (Rust).lnk"
  Delete "$SMPROGRAMS\Plex (Rust)\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Plex (Rust)"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-plex-mcp"
SectionEnd
