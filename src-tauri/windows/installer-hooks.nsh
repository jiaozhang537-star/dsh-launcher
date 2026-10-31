; DSH 启动器 · NSIS 安装器钩子（P7 W1）
;
; PREUNINSTALL = 卸载数据警示卡：列出两处数据目录与当前量级，由用户决定
; 是否一并删除。口径与 README「数据位置与卸载说明」互认：
;   1) 托管根         %LOCALAPPDATA%\dsh-launcher   （实例 / Node / 凭据 / 缓存）
;   2) WebView2 数据  %APPDATA%\com.mltz.dshlauncher（应用标识推导，卸载项由 NSIS 自清）
; 两处目录是 src-tauri/src/paths.rs 硬编码锚点（ROOT_DIR_NAME）与 identifier
; 的字面量——**不随 productName 变化**，故此处不用宏推导、直接写死。
; 文件须保持 UTF-8（含 BOM）：makensis 对无 BOM 文件按系统代码页解读，中文会乱码。

!include "FileFunc.nsh"
!include "LogicLib.nsh"

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $R0 "$LOCALAPPDATA\dsh-launcher"
  StrCpy $R1 "$APPDATA\com.mltz.dshlauncher"

  StrCpy $R8 ""
  ${If} ${FileExists} "$R0\*.*"
    ${GetSize} "$R0" "/S=0M" $R2 $R3 $R4
    StrCpy $R8 "$R8$\n　· $R0$\n　　（实例、Node 运行时、凭据库、缓存）—— 约 $R2 MB"
  ${EndIf}
  ${If} ${FileExists} "$R1\*.*"
    ${GetSize} "$R1" "/S=0M" $R5 $R6 $R7
    StrCpy $R8 "$R8$\n　· $R1$\n　　（WebView2 界面数据）—— 约 $R5 MB"
  ${EndIf}
  ${If} $R8 == ""
    StrCpy $R8 "$\n　（未发现数据目录，无需选择）"
  ${EndIf}

  MessageBox MB_YESNO|MB_ICONQUESTION "即将卸载 DSH 启动器。$\n$\n以下数据目录默认保留：$R8$\n$\n是否一并删除全部数据？$\n$\n「是」= 彻底删除（重装后为全新状态）；$\n「否」= 保留数据（重装后实例与配置仍在）。$\n$\n提示：若实例正在运行，请先在启动器中停止，否则删除可能不完整。" IDYES dsh_purge_data IDNO dsh_keep_data

  dsh_purge_data:
    DetailPrint "按用户选择：一并删除数据目录"
    RMDir /r "$R0"
    RMDir /r "$R1"
    Goto dsh_data_done
  dsh_keep_data:
    DetailPrint "按用户选择：保留数据目录"
  dsh_data_done:
!macroend
