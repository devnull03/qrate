; NSIS installer for qrate (Windows).
;
; Compile with absolute paths supplied by CI, e.g.:
;   makensis /DVERSION=1.2.3 ^
;            /DSRCEXE=C:\path\qrate.exe ^
;            /DICONFILE=C:\path\app-icon.ico ^
;            /DOUTFILE=C:\path\qrate-1.2.3-setup.exe ^
;            scripts\installer.nsi
;
; The installer and the installed app use assets/icons/app-icon.ico — replace
; that icon (regenerate via scripts/gen-icons.ps1) to rebrand. UNSIGNED: users
; will see a Windows SmartScreen "unknown publisher" prompt (More info > Run anyway).

Unicode true

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SRCEXE
  !define SRCEXE "..\target\release\app.exe"
!endif
!ifndef ICONFILE
  !define ICONFILE "..\assets\icons\app-icon.ico"
!endif
!ifndef OUTFILE
  !define OUTFILE "qrate-${VERSION}-setup.exe"
!endif
; VIProductVersion needs a strict numeric X.X.X.X. CI derives this from VERSION
; (stripping any pre-release suffix); the fallback covers local/manual compiles.
!ifndef VIVERSION
  !define VIVERSION "0.0.0.0"
!endif

!define APPNAME   "qrate"
!define EXENAME   "qrate.exe"
!define COMPANY   "devnull03"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ICON   "${ICONFILE}"
!define MUI_UNICON "${ICONFILE}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName"     "${APPNAME}"
VIAddVersionKey "CompanyName"     "${COMPANY}"
VIAddVersionKey "FileDescription" "${APPNAME} Installer"
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "ProductVersion"  "${VERSION}"

Section "Install"
  SetOutPath "$INSTDIR"
  File /oname=${EXENAME} "${SRCEXE}"

  ; Start Menu + Desktop shortcuts
  CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXENAME}"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk"    "$INSTDIR\${EXENAME}"

  ; Uninstaller + Add/Remove Programs entry
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr   HKLM "Software\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayName"     "${APPNAME}"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayVersion"  "${VERSION}"
  WriteRegStr   HKLM "${UNINSTKEY}" "Publisher"       "${COMPANY}"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayIcon"     "$INSTDIR\${EXENAME}"
  WriteRegStr   HKLM "${UNINSTKEY}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXENAME}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir  "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey HKLM "${UNINSTKEY}"
  DeleteRegKey HKLM "Software\${APPNAME}"
SectionEnd
