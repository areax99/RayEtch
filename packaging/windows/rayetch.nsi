Unicode true
RequestExecutionLevel user
ManifestSupportedOS all
!include "MUI2.nsh"
!include "x64.nsh"

Name "RayEtch ${APP_VERSION}"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\RayEtch"
SetCompressor /SOLID lzma
AllowSkipFiles off
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\RayEtch"
!define PROJECT_KEY "Software\Classes\RayEtch.Project"
!define MUI_ICON "${ROOT}/assets/icon.ico"
!define MUI_UNICON "${ROOT}/assets/icon.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_LINK "$(RuntimeLink)"
!define MUI_FINISHPAGE_LINK_LOCATION "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Italian"
LangString RuntimeLink ${LANG_ENGLISH} "Microsoft Visual C++ runtime downloads"
LangString RuntimeLink ${LANG_ITALIAN} "Scarica il runtime Microsoft Visual C++"

Function .onInit
  !if "${ARCH}" == "x86_64"
    ${IfNot} ${IsNativeAMD64}
      StrCpy $0 0
      System::Call 'kernel32::IsWow64GuestMachineSupported(i 0x8664, *i .r0) i.r1'
      ${If} $1 != 0
      ${OrIf} $0 == 0
        MessageBox MB_OK|MB_ICONSTOP "This release of RayEtch requires x64 support." /SD IDOK
        Abort
      ${EndIf}
    ${EndIf}
  !else if "${ARCH}" == "aarch64"
    ${IfNot} ${IsNativeARM64}
      MessageBox MB_OK|MB_ICONSTOP "RayEtch (${ARCH}) requires ARM64 Windows." /SD IDOK
      Abort
    ${EndIf}
  !else if "${ARCH}" != "i686"
    !error "Unsupported Windows architecture"
  !endif
FunctionEnd

Section
  SetOutPath "$INSTDIR"
  File "/oname=rayetch.exe" "${APP_BINARY}"
  File "${DOCUMENTS}/README.md"
  File "${DOCUMENTS}/LICENSE"
  File "${DOCUMENTS}/NOTICE"
  SetOutPath "$INSTDIR\vendor\zint"
  File "${ROOT}/vendor/zint/LICENSE"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\RayEtch.lnk" "$INSTDIR\rayetch.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "RayEtch"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "servimmagine"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\rayetch.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\rayetch.exe"
  ${If} ${FileExists} "$INSTDIR\rayetch.exe"
    MessageBox MB_OK|MB_ICONSTOP "Close RayEtch and run the uninstaller again." /SD IDOK
    Abort
  ${EndIf}
  ; RayEtch registers its file association on launch. A portable copy may own it now.
  ReadRegStr $0 HKCU "${PROJECT_KEY}\shell\open\command" ""
  ${If} $0 == '"$INSTDIR\rayetch.exe" "%1"'
    DeleteRegKey HKCU "${PROJECT_KEY}"
    ReadRegStr $0 HKCU "Software\Classes\.rayetch" ""
    ${If} $0 == "RayEtch.Project"
      DeleteRegValue HKCU "Software\Classes\.rayetch" ""
      DeleteRegKey /ifempty HKCU "Software\Classes\.rayetch"
    ${EndIf}
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  ${EndIf}
  Delete "$SMPROGRAMS\RayEtch.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  Delete "$INSTDIR\vendor\zint\LICENSE"
  RMDir "$INSTDIR\vendor\zint"
  RMDir "$INSTDIR\vendor"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
