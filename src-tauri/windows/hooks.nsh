!include LogicLib.nsh

; 安装、覆盖、卸载前先让流量记录自己停。nidong-meter 是提权进程，
; 安装程序没有管理员权限，taskkill 杀不掉它。写 stop 文件、结束计划任务，
; 它会在下一轮采样自己退出。不要因失败 Abort。
;
; 交互升级默认是「先卸载再安装」，会先跑旧版 uninstall.exe。旧版不会停
; meter，文件占用就会「无法卸载」。所以 GUI 一起来就先发停止信号，
; 等人点到安装页时进程已经退了。
!macro SignalNidongStop
  CreateDirectory "$APPDATA\edu.neu.nidong\meter"
  FileOpen $R9 "$APPDATA\edu.neu.nidong\meter\stop" w
  ${If} $R9 != ""
    FileWrite $R9 "stop$\r$\n"
    FileClose $R9
  ${EndIf}
  nsExec::Exec '"$SYSDIR\schtasks.exe" /End /TN "NidongMeter"'
  Pop $R9
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "nidong-meter.exe" /T'
  Pop $R9
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "nidong.exe" /T'
  Pop $R9
!macroend

!macro StopNidongProcesses
  !insertmacro SignalNidongStop
  Sleep 2500
!macroend

!ifndef MUI_CUSTOMFUNCTION_GUIINIT
  !define MUI_CUSTOMFUNCTION_GUIINIT StopNidongEarly
!endif
Function StopNidongEarly
  !insertmacro SignalNidongStop
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  !insertmacro StopNidongProcesses
!macroend

; 应用内更新走 /UPDATE，安装程序不会改快捷方式。注册表里的安装目录
; 和用户正在打开的副本可能不是同一个，桌面、开始菜单、开机自启会一直
; 打开旧的 nidong.exe。装完把这三个入口指到这次的安装目录。
!macro PointLaunchersAtInstall
  ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
  ${If} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nidong"
  ${If} $R8 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nidong" '"$INSTDIR\${MAINBINARYNAME}.exe"'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PointLaunchersAtInstall
  ; 覆盖安装时 stop 文件还在，新进程看见就会马上退出。先清掉再拉起来。
  Delete "$APPDATA\edu.neu.nidong\meter\stop"
  nsExec::Exec '"$SYSDIR\schtasks.exe" /Run /TN "NidongMeter"'
  Pop $R9
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopNidongProcesses
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 开机自启的值名是 Nidong，不是产品名。卸载登记表清不到它。
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nidong"
  ; 流量记录的开机任务是运行时登记的，卸载时清掉。删不掉也不要拦住卸载。
  nsExec::Exec '"$SYSDIR\schtasks.exe" /Delete /TN "NidongMeter" /F'
  Pop $R9
!macroend
