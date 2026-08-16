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
;
; One installer, two install modes: MultiUser.nsh (bundled with NSIS) asks up front whether to
; install for just the current account — $LocalAppData, no UAC prompt, no admin rights needed —
; or for every account on the machine — $ProgramFiles, elevates via UAC. Launching the installer
; already elevated (or without admin rights at all) skips straight to the mode that's available.

Unicode true

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SRCEXE
  !define SRCEXE "..\target\release\app.exe"
!endif
; Directory holding the preview sidecars (pdfium.dll, ffmpeg.exe). Normally the same folder as
; SRCEXE, since scripts/fetch-binaries.sh puts them beside the executable. Both are optional.
!ifndef SRCDIR
  !define SRCDIR "..\target\release"
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

; --- Multi-user install mode -------------------------------------------------
; Must be defined, and MultiUser.nsh included, before MUI2.nsh.
!define MULTIUSER_EXECUTIONLEVEL Highest
!define MULTIUSER_MUI
!define MULTIUSER_INSTALLMODE_INSTDIR "${APPNAME}"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "Software\${APPNAME}"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "InstallDir"
!include "MultiUser.nsh"

Name "${APPNAME}"
OutFile "${OUTFILE}"
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ICON   "${ICONFILE}"
!define MUI_UNICON "${ICONFILE}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MULTIUSER_PAGE_INSTALLMODE
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

Function .onInit
  !insertmacro MULTIUSER_INIT
FunctionEnd

Function un.onInit
  !insertmacro MULTIUSER_UNINIT
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  File /oname=${EXENAME} "${SRCEXE}"

  ; Preview sidecars, taken from beside the built executable — see scripts/fetch-binaries.sh.
  ; PDFium is loaded dynamically and ffmpeg is run as a subprocess, and both tiers fall back to a
  ; type icon when their binary is missing, so /nonfatal is correct: an installer built without
  ; them still installs a working qrate that simply cannot preview PDFs or video.
  File /nonfatal "${SRCDIR}\pdfium.dll"
  File /nonfatal "${SRCDIR}\ffmpeg.exe"

  ; Start Menu + Desktop shortcuts. $SMPROGRAMS/$DESKTOP already resolve to the per-user or
  ; all-users locations MULTIUSER_INIT picked (it calls SetShellVarContext for us).
  CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXENAME}"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk"    "$INSTDIR\${EXENAME}"

  ; Uninstaller + Add/Remove Programs entry. SHCTX is HKLM for an all-users install, HKCU for a
  ; per-user one — set by MULTIUSER_INIT to match the mode picked above.
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr   SHCTX "Software\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr   SHCTX "${UNINSTKEY}" "DisplayName"     "${APPNAME}"
  WriteRegStr   SHCTX "${UNINSTKEY}" "DisplayVersion"  "${VERSION}"
  WriteRegStr   SHCTX "${UNINSTKEY}" "Publisher"       "${COMPANY}"
  WriteRegStr   SHCTX "${UNINSTKEY}" "DisplayIcon"     "$INSTDIR\${EXENAME}"
  WriteRegStr   SHCTX "${UNINSTKEY}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXENAME}"
  Delete "$INSTDIR\pdfium.dll"
  Delete "$INSTDIR\ffmpeg.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir  "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey SHCTX "${UNINSTKEY}"
  DeleteRegKey SHCTX "Software\${APPNAME}"
SectionEnd
