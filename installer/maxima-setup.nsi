; Maxima Installer Script (NSIS)
; A free and open-source replacement for the EA Desktop Launcher
; Can be compiled on macOS with: makensis installer/maxima-setup.nsi

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"

;---------- General ----------
!define PRODUCT_NAME "Maxima"
!define PRODUCT_PUBLISHER "Armchair Developers"
!define PRODUCT_WEB_SITE "https://github.com/ArmchairDevelopers/Maxima"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

; Where the cross-compiled binaries live
!define BIN_DIR "..\target\x86_64-pc-windows-gnu\release"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "MaximaSetup.exe"
InstallDir "$PROGRAMFILES64\Maxima"
InstallDirRegKey HKLM "Software\Maxima" "InstallPath"
RequestExecutionLevel admin
ShowInstDetails show
ShowUnInstDetails show

;---------- MUI Settings ----------
!define MUI_ABORTWARNING
!define MUI_ICON "..\maxima-resources\assets\logo.ico"
!define MUI_UNICON "..\maxima-resources\assets\logo.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP_NOSTRETCH
!define MUI_HEADERIMAGE

;---------- Pages ----------
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

;---------- Languages ----------
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Spanish"

;---------- Installer Sections ----------
Section "Maxima Core" SEC_CORE
    SectionIn RO ; Read-only, always installed

    SetOutPath "$INSTDIR"
    SetOverwrite on

    ; Install binaries
    File "${BIN_DIR}\maxima-bootstrap.exe"
    File "${BIN_DIR}\maxima-cli.exe"
    File "${BIN_DIR}\maxima-service.exe"

    ; Optional binaries (may not exist in all builds)
    File /nonfatal "${BIN_DIR}\maxima-tui.exe"
    File /nonfatal "${BIN_DIR}\maxima.exe"

    ; ---- Windows Registry Setup ----

    ; Origin compatibility: Point EA games to maxima-bootstrap.exe
    ; 64-bit registry view
    SetRegView 64
    WriteRegStr HKLM "SOFTWARE\WOW6432Node\Origin" "ClientPath" "$INSTDIR\maxima-bootstrap.exe"

    ; EA Desktop flag
    WriteRegStr HKLM "SOFTWARE\Electronic Arts\EA Desktop" "InstallSuccessful" "true"

    ; ---- Protocol Handlers ----

    ; qrc:// protocol (EA login redirection)
    WriteRegStr HKCR "qrc" "" "URL:Maxima Protocol"
    WriteRegStr HKCR "qrc" "URL Protocol" ""
    WriteRegStr HKCR "qrc\shell\open\command" "" '"$INSTDIR\maxima-bootstrap.exe" "%1"'

    ; link2ea:// protocol (Steam/Epic game launch)
    WriteRegStr HKCR "link2ea" "" "URL:Maxima Launcher"
    WriteRegStr HKCR "link2ea" "URL Protocol" ""
    WriteRegStr HKCR "link2ea\shell\open\command" "" '"$INSTDIR\maxima-bootstrap.exe" "%1"'

    ; origin2:// protocol (Legacy Origin game launch)
    WriteRegStr HKCR "origin2" "" "URL:Maxima Launcher"
    WriteRegStr HKCR "origin2" "URL Protocol" ""
    WriteRegStr HKCR "origin2\shell\open\command" "" '"$INSTDIR\maxima-bootstrap.exe" "%1"'

    ; ---- Windows Service ----

    ; Stop existing service if running (ignore errors)
    nsExec::ExecToLog 'sc stop MaximaBackgroundService'

    ; Remove existing service if present (ignore errors)
    nsExec::ExecToLog 'sc delete MaximaBackgroundService'

    ; Install and start the background service
    nsExec::ExecToLog 'sc create MaximaBackgroundService binPath= "$INSTDIR\maxima-service.exe" start= demand type= own DisplayName= "Maxima Background Service"'
    nsExec::ExecToLog 'sc description MaximaBackgroundService "Maxima Background Service - EA Launcher replacement"'
    nsExec::ExecToLog 'sc start MaximaBackgroundService'

    ; ---- Start Menu Shortcuts ----
    CreateDirectory "$SMPROGRAMS\Maxima"
    CreateShortcut "$SMPROGRAMS\Maxima\Maxima CLI.lnk" "$INSTDIR\maxima-cli.exe" "" "$INSTDIR\maxima-cli.exe" 0
    CreateShortcut "$SMPROGRAMS\Maxima\Uninstall Maxima.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\uninstall.exe" 0

    ; ---- Uninstaller ----
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Add/Remove Programs entry
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\maxima-cli.exe"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"

    ; Calculate installed size
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "EstimatedSize" "$0"

SectionEnd

;---------- Uninstaller Section ----------
Section "Uninstall"

    ; Stop and remove the Windows service
    nsExec::ExecToLog 'sc stop MaximaBackgroundService'
    ; Wait for service to stop
    Sleep 2000
    nsExec::ExecToLog 'sc delete MaximaBackgroundService'

    ; Remove protocol handlers
    DeleteRegKey HKCR "qrc"
    DeleteRegKey HKCR "link2ea"
    DeleteRegKey HKCR "origin2"

    ; Remove Origin compatibility registry keys
    SetRegView 64
    DeleteRegValue HKLM "SOFTWARE\WOW6432Node\Origin" "ClientPath"
    DeleteRegValue HKLM "SOFTWARE\Electronic Arts\EA Desktop" "InstallSuccessful"

    ; Remove Add/Remove Programs entry
    DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"

    ; Remove install path registry
    DeleteRegKey HKLM "Software\Maxima"

    ; Remove Start Menu shortcuts
    RMDir /r "$SMPROGRAMS\Maxima"

    ; Remove files
    Delete "$INSTDIR\maxima-bootstrap.exe"
    Delete "$INSTDIR\maxima-cli.exe"
    Delete "$INSTDIR\maxima-service.exe"
    Delete "$INSTDIR\maxima-tui.exe"
    Delete "$INSTDIR\maxima.exe"
    Delete "$INSTDIR\uninstall.exe"

    ; Remove install directory (only if empty)
    RMDir "$INSTDIR"

SectionEnd
