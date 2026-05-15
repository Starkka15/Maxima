; Maxima Installer Script (NSIS)
; A free and open-source replacement for the EA Desktop Launcher
; Can be compiled on macOS with: makensis installer/maxima-setup.nsi

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

;---------- Registry Backup / Restore ----------
;
; The installer takes ownership of several registry entries that EA Desktop /
; Origin / other launchers may also want to own (qrc://, link2ea://, origin2://
; protocol handlers, plus the WOW6432Node Origin\ClientPath entry). When the
; user uninstalls Maxima, we must restore whatever those entries pointed at
; before so EA Launcher works again without a reinstall.
;
; Strategy: before overwriting any of these entries, copy the current value
; (if any) into HKLM\Software\Maxima\Backup\<key>. The uninstaller reads that
; backup and either restores the previous value or deletes the entry outright
; if nothing was there before.
;
; "_existed" sentinel: "1" if there was a value to back up, "0" otherwise.

!define BACKUP_ROOT "Software\Maxima\Backup"

; Back up a single registry value at install time.
; Usage: !insertmacro BackupValue HKLM "SOFTWARE\WOW6432Node\Origin" "ClientPath" "Origin_ClientPath"
!macro BackupValue ROOT KEY VALUENAME BACKUPID
    ClearErrors
    ReadRegStr $0 ${ROOT} "${KEY}" "${VALUENAME}"
    ${If} ${Errors}
        WriteRegStr HKLM "${BACKUP_ROOT}\${BACKUPID}" "_existed" "0"
    ${Else}
        WriteRegStr HKLM "${BACKUP_ROOT}\${BACKUPID}" "_existed" "1"
        WriteRegStr HKLM "${BACKUP_ROOT}\${BACKUPID}" "value"    "$0"
    ${EndIf}
!macroend

; Restore (or delete) a backed-up value at uninstall time.
; Usage: !insertmacro RestoreValue HKLM "SOFTWARE\WOW6432Node\Origin" "ClientPath" "Origin_ClientPath"
!macro RestoreValue ROOT KEY VALUENAME BACKUPID
    ClearErrors
    ReadRegStr $0 HKLM "${BACKUP_ROOT}\${BACKUPID}" "_existed"
    ${If} $0 == "1"
        ReadRegStr $0 HKLM "${BACKUP_ROOT}\${BACKUPID}" "value"
        WriteRegStr ${ROOT} "${KEY}" "${VALUENAME}" "$0"
    ${Else}
        DeleteRegValue ${ROOT} "${KEY}" "${VALUENAME}"
    ${EndIf}
!macroend

; Back up a URL-protocol handler subtree at install time.
; Captures the three relevant fields: the default value, "URL Protocol", and
; shell\open\command's default value. Anything else (e.g. DefaultIcon) is not
; round-tripped, but EA Launcher's protocol entries don't rely on extras.
;
; Upgrade guard: if Maxima is already installed (detected via the
; HKLM\Software\Maxima\InstallPath reg key the prior install wrote), this
; is an upgrade. We skip the backup phase entirely so that:
;   - we don't overwrite a real EA-handler backup with Maxima's own values,
;   - we don't write a brand-new backup that points at a Maxima binary
;     (which the uninstaller would then "restore" to a deleted path).
; The first install captures the pre-Maxima state; subsequent upgrades
; leave that backup untouched.
;
; View safety: SetRegView default before HKCR reads so this macro is
; independent of caller state (the install section sets view 64 for HKLM
; ops, which would otherwise leak into HKCR).
;
; Usage: !insertmacro BackupProtocol "qrc"
!macro BackupProtocol PROTOCOL
    SetRegView default

    ClearErrors
    ReadRegStr $9 HKLM "Software\Maxima" "InstallPath"
    ${IfNot} ${Errors}
        DetailPrint "BackupProtocol(${PROTOCOL}): upgrade detected, preserving original backup"
        Goto skip_backup_${PROTOCOL}
    ${EndIf}

    ClearErrors
    ReadRegStr $0 HKCR "${PROTOCOL}" ""
    ${If} ${Errors}
        WriteRegStr HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "_existed" "0"
    ${Else}
        WriteRegStr HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "_existed" "1"
        WriteRegStr HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "default" "$0"

        ClearErrors
        ReadRegStr $0 HKCR "${PROTOCOL}" "URL Protocol"
        ${IfNot} ${Errors}
            WriteRegStr HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "URL Protocol" "$0"
        ${EndIf}

        ClearErrors
        ReadRegStr $0 HKCR "${PROTOCOL}\shell\open\command" ""
        ${IfNot} ${Errors}
            WriteRegStr HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "command" "$0"
        ${EndIf}
    ${EndIf}
    skip_backup_${PROTOCOL}:
!macroend

; Upgrade-safe wrapper around BackupValue with the same guard as
; BackupProtocol. Use for HKLM values that must be round-tripped on
; uninstall (Origin\ClientPath, EA Desktop\InstallSuccessful).
; Usage: !insertmacro BackupValueUpgradeSafe HKLM "Key" "Name" "BackupId"
!macro BackupValueUpgradeSafe ROOT KEY VALUENAME BACKUPID
    ClearErrors
    ReadRegStr $9 HKLM "Software\Maxima" "InstallPath"
    ${If} ${Errors}
        !insertmacro BackupValue ${ROOT} "${KEY}" "${VALUENAME}" "${BACKUPID}"
    ${Else}
        DetailPrint "BackupValueUpgradeSafe(${BACKUPID}): upgrade detected, preserving original backup"
    ${EndIf}
!macroend

; Restore (or delete) a URL-protocol handler subtree at uninstall time.
; Forces view default so HKCR reads/writes match the install-time store
; (BackupProtocol also runs under view default - they must agree or the
; uninstaller "restores" nothing and leaves dangling Maxima keys behind).
; Usage: !insertmacro RestoreProtocol "qrc"
!macro RestoreProtocol PROTOCOL
    SetRegView default
    ClearErrors
    ReadRegStr $0 HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "_existed"
    ${If} $0 == "1"
        ; Wipe Maxima's entries first so nothing stale lingers, then restore.
        DeleteRegKey HKCR "${PROTOCOL}"

        ReadRegStr $0 HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "default"
        WriteRegStr HKCR "${PROTOCOL}" "" "$0"

        ClearErrors
        ReadRegStr $0 HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "URL Protocol"
        ${IfNot} ${Errors}
            WriteRegStr HKCR "${PROTOCOL}" "URL Protocol" "$0"
        ${EndIf}

        ClearErrors
        ReadRegStr $0 HKLM "${BACKUP_ROOT}\Protocol_${PROTOCOL}" "command"
        ${IfNot} ${Errors}
            WriteRegStr HKCR "${PROTOCOL}\shell\open\command" "" "$0"
        ${EndIf}
    ${Else}
        DeleteRegKey HKCR "${PROTOCOL}"
    ${EndIf}
!macroend

;---------- General ----------
!define PRODUCT_NAME "Maxima"
!define PRODUCT_PUBLISHER "Armchair Developers"
!define PRODUCT_WEB_SITE "https://github.com/ArmchairDevelopers/Maxima"
!define PRODUCT_VERSION "0.4.0"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

; Binary directory - override at compile time with /DBIN_DIR="..."
; Default is the macOS cross-compilation path (installer/build.sh).
; CI Windows native builds pass /DBIN_DIR="..\target\release"
!ifndef BIN_DIR
  !define BIN_DIR "..\target\x86_64-pc-windows-gnu\release"
!endif

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

    ; Back up any pre-existing values BEFORE we overwrite them, so the
    ; uninstaller can restore the user's previous EA Launcher / Origin setup.
    ;
    ; HKLM\SOFTWARE\WOW6432Node\* and HKLM\SOFTWARE\Electronic Arts\* are
    ; backed up under the 64-bit view so the values match where EA Desktop
    ; / Origin writes them on a real 64-bit Windows install.
    SetRegView 64
    !insertmacro BackupValueUpgradeSafe HKLM "SOFTWARE\WOW6432Node\Origin"             "ClientPath"         "Origin_ClientPath"
    !insertmacro BackupValueUpgradeSafe HKLM "SOFTWARE\Electronic Arts\EA Desktop"     "InstallSuccessful"  "EADesktop_InstallSuccessful"

    ; HKCR protocol handlers are SHARED keys on Windows (not WOW64-redirected
    ; for URL protocol associations), but NSIS still resolves HKCR through
    ; whichever view is currently active. Force back to default so the
    ; backups and the subsequent writes target the same store the OS
    ; serves to both 32-bit and 64-bit URL-resolving processes.
    ;
    ; This is the critical fix for the v0.2.0 -> v0.2.1 upgrade regression
    ; where view-64 leaked into HKCR writes and left 32-bit consumers
    ; (Titanfall2.exe, Origin emitting link2ea://) looking at stale or
    ; missing handlers.
    SetRegView default
    !insertmacro BackupProtocol "qrc"
    !insertmacro BackupProtocol "link2ea"
    !insertmacro BackupProtocol "origin2"

    ; Origin compatibility: Point EA games to maxima-bootstrap.exe
    ; (back under view 64 - same store as the BackupValue above).
    SetRegView 64
    WriteRegStr HKLM "SOFTWARE\WOW6432Node\Origin" "ClientPath" "$INSTDIR\maxima-bootstrap.exe"

    ; EA Desktop flag
    WriteRegStr HKLM "SOFTWARE\Electronic Arts\EA Desktop" "InstallSuccessful" "true"

    ; ---- Protocol Handlers ----
    ; Reset view for HKCR writes - see comment above the BackupProtocol calls.
    SetRegView default

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

    ; The graphical UI (maxima.exe) and terminal UI (maxima-tui.exe) are
    ; built but optional - create their shortcuts only when the files were
    ; actually packaged. Matches the `File /nonfatal` semantics above so a
    ; CLI-only build doesn't leave dangling links.
    IfFileExists "$INSTDIR\maxima.exe" make_ui_shortcut skip_ui_shortcut
    make_ui_shortcut:
        CreateShortcut "$SMPROGRAMS\Maxima\Maxima.lnk" "$INSTDIR\maxima.exe" "" "$INSTDIR\maxima.exe" 0
    skip_ui_shortcut:

    IfFileExists "$INSTDIR\maxima-tui.exe" make_tui_shortcut skip_tui_shortcut
    make_tui_shortcut:
        CreateShortcut "$SMPROGRAMS\Maxima\Maxima TUI.lnk" "$INSTDIR\maxima-tui.exe" "" "$INSTDIR\maxima-tui.exe" 0
    skip_tui_shortcut:

    CreateShortcut "$SMPROGRAMS\Maxima\Uninstall Maxima.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\uninstall.exe" 0

    ; ---- Uninstaller ----
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Add/Remove Programs entry
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall.exe"
    ; Prefer the GUI's icon for Add/Remove Programs when available - falls back
    ; to the CLI icon for CLI-only installs.
    IfFileExists "$INSTDIR\maxima.exe" use_ui_icon use_cli_icon
    use_ui_icon:
        WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\maxima.exe"
        Goto icon_done
    use_cli_icon:
        WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\maxima-cli.exe"
    icon_done:
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

    ; Restore protocol handlers to whatever they pointed at before the install
    ; (typically EA Desktop / Origin Launcher). If they didn't exist pre-install
    ; the macro just deletes the key, which leaves the system in a clean state
    ; ready for EA Launcher to register itself on its next run.
    ;
    ; RestoreProtocol forces SetRegView default internally so HKCR
    ; operations target the same store the installer's BackupProtocol used.
    !insertmacro RestoreProtocol "qrc"
    !insertmacro RestoreProtocol "link2ea"
    !insertmacro RestoreProtocol "origin2"

    ; Restore Origin compatibility values (view 64 - matches install-time)
    SetRegView 64
    !insertmacro RestoreValue HKLM "SOFTWARE\WOW6432Node\Origin"         "ClientPath"        "Origin_ClientPath"
    !insertmacro RestoreValue HKLM "SOFTWARE\Electronic Arts\EA Desktop" "InstallSuccessful" "EADesktop_InstallSuccessful"

    ; Remove Add/Remove Programs entry
    DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"

    ; Remove install path registry AND the backup subtree (which lived under
    ; HKLM\Software\Maxima\Backup). DeleteRegKey is recursive so this also
    ; cleans up the backup we just consumed.
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
