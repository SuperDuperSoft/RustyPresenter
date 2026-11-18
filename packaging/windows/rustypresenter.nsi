; RustyPresenter's Windows installer, made on a PC by make-installer.cmd (packaging/windows/make-installer.sh).
; Plain NSIS 3: only its own headers and its nsDialogs plugin. The CI release build uses cargo-packager's template (installer.nsi) instead; both
; install to the same folder under the same uninstall key, so either one upgrades the other.
;
;   makensis -DVERSION=1.0.0 -DVIVERSION=1.0.0.0 -DFILES_NSH=<files.nsh> -DUNFILES_NSH=<unfiles.nsh>
;            -DOUTFILE=<setup.exe> -DICON=<icon.ico> rustypresenter.nsi
;
; FILES_NSH holds the File lines for everything staged, UNFILES_NSH the matching Delete/RMDir lines, so the
; uninstaller removes only what was installed, never a whole folder.
;
; - Per user, no admin rights: %LOCALAPPDATA%\Programs\RustyPresenter.
; - Start menu and desktop shortcuts, an uninstaller, an entry in Settings > Apps.
; - The data folder %LOCALAPPDATA%\RustyPresenter is deleted only when the box on the uninstall page is ticked.
; - Running it again upgrades in place: same folder, same uninstall entry.

!ifndef VERSION
  !error "Pass -DVERSION=x.y.z"
!endif
!ifndef VIVERSION
  !error "Pass -DVIVERSION=x.y.z.w"
!endif
!ifndef FILES_NSH
  !error "Pass -DFILES_NSH=<file list>"
!endif
!ifndef UNFILES_NSH
  !error "Pass -DUNFILES_NSH=<uninstall list>"
!endif
!ifndef OUTFILE
  !error "Pass -DOUTFILE=<setup exe>"
!endif
!ifndef ICON
  !error "Pass -DICON=<icon.ico>"
!endif

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!define PRODUCT "RustyPresenter"
!define PUBLISHER "RustyPresenter contributors"
!define EXE "rustypresenter.exe"
; The same key cargo-packager's template uses (Software\...\Uninstall\<productName>).
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
; The app's data folder (src/server.rs data_dir). Never the program folder.
!define DATADIR "$LOCALAPPDATA\${PRODUCT}"

!include MUI2.nsh
!include nsDialogs.nsh
!include LogicLib.nsh
!include FileFunc.nsh

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
; An upgrade goes where the last install went.
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "${PRODUCT} ${VERSION}"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

Var DeleteDataBox
Var DeleteData

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "This installs ${PRODUCT} ${VERSION} for you on this computer. It needs no administrator password.$\r$\n$\r$\nIf ${PRODUCT} is already installed, it is updated. Your songs, services and settings stay as they are.$\r$\n$\r$\nClose ${PRODUCT} before you go on."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${PRODUCT} now"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
UninstPage custom un.DataPage un.DataPageLeave
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; The exe cannot be deleted while it runs, so a failed delete means the app is open.
!macro CLOSE_APP_FIRST UN
Function ${UN}CloseAppFirst
  retry:
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    ClearErrors
    Delete "$INSTDIR\${EXE}"
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${PRODUCT} is still open. Close it (and its room and stage screens), then click Retry." /SD IDCANCEL IDRETRY retry
      Abort "${PRODUCT} is still open."
    ${EndIf}
  ${EndIf}
FunctionEnd
!macroend
!insertmacro CLOSE_APP_FIRST ""
!insertmacro CLOSE_APP_FIRST "un."

Function .onInit
  SetShellVarContext current
FunctionEnd

; The program must never go into the data folder: the uninstaller would then share it.
Function .onVerifyInstDir
  ${If} "$INSTDIR" == "${DATADIR}"
    Abort
  ${EndIf}
FunctionEnd

Section "${PRODUCT}" SecMain
  SectionIn RO
  Call CloseAppFirst
  !include "${FILES_NSH}"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  CreateShortCut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"
SectionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

; Unticked unless the user ticks it; a silent uninstall (/S) never shows it, so it keeps the data.
Function un.DataPage
  !insertmacro MUI_HEADER_TEXT "Your songs and services" "They are kept unless you tick the box."
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 40u "${PRODUCT} keeps your songs, services, pictures, videos, Bibles, settings and backups in this folder:$\r$\n${DATADIR}$\r$\n$\r$\nThey are kept, so a new install finds them again."
  Pop $0
  ${NSD_CreateCheckbox} 0 52u 100% 12u "Also &delete that folder and everything in it (this cannot be undone)"
  Pop $DeleteDataBox
  ${NSD_SetState} $DeleteDataBox ${BST_UNCHECKED}
  nsDialogs::Show
FunctionEnd

Function un.DataPageLeave
  ${NSD_GetState} $DeleteDataBox $DeleteData
FunctionEnd

Section "Uninstall"
  Call un.CloseAppFirst
  !include "${UNFILES_NSH}"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"
  Delete "$INSTDIR\uninstall.exe"
  ; Only when empty: anything else in the folder was not ours.
  RMDir "$INSTDIR"
  ${If} $DeleteData == ${BST_CHECKED}
    DetailPrint "Deleting ${DATADIR}"
    RMDir /r "${DATADIR}"
  ${EndIf}
SectionEnd
