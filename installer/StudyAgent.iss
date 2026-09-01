; ---------------------------------------------------------------------------
; StudyAgent 安装程序（Inno Setup 6）
;
; 构建方式（推荐用 scripts 里的 build.ps1，它会传入版本号与暂存目录）：
;   ISCC.exe StudyAgent.iss /DMyAppVersion=0.6.0 /DStagingDir=staging /O..\src-tauri\target\release\bundle\inno
;
; 约定：
;   - Tauri 侧 bundle.active=false，tauri build 只产出 studyagent-desktop.exe
;   - build.ps1 把待打包文件放进 staging/，本脚本整体打包 staging/ 下的内容
;   - 学习数据默认写在 {app}\data，卸载时默认保留（由用户选择是否删除）
; ---------------------------------------------------------------------------

#ifndef MyAppVersion
  #define MyAppVersion "0.6.0"
#endif

; VersionInfoVersion / ProductVersion 只接受纯数字点分格式（如 0.6.1）。
; 对 0.6.1-indev 等带预发布后缀的版本，剥离 "-indev"/"-rc.*"/"-beta" 等后缀。
#if defined(Pos) && Pos("-", MyAppVersion) > 0
  #define MyAppVersionInfoVersion Copy(MyAppVersion, 1, Pos("-", MyAppVersion) - 1)
#else
  #define MyAppVersionInfoVersion MyAppVersion
#endif

#ifndef StagingDir
  #define StagingDir "staging"
#endif

#define MyAppName "StudyAgent"
#define MyAppPublisher "StudyAgent"
#define MyAppURL "https://github.com/Wenjiugugugu/StudyAgent-client"
#define MyAppExeName "studyagent-desktop.exe"
#define MyAppDataDirName "data"
; AppId 一经发布就不要再改动，否则升级时无法复用旧的安装目录/卸载入口
#define MyAppId "{{2E1F7C4B-9A3D-4C58-B6E0-7D14AF38C295}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersionInfoVersion}
VersionInfoProductVersion={#MyAppVersionInfoVersion}
VersionInfoProductName={#MyAppName}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} 安装程序
DefaultDirName={localappdata}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=yes
UsePreviousAppDir=yes
UsePreviousGroup=yes
UninstallDisplayName={#MyAppName} {#MyAppVersion}
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile=..\src-tauri\icons\icon.ico
WizardStyle=modern
WizardImageFile=assets\wizard.bmp
; Inno 6 实际不渲染 WizardSmallImageFile 的 32-bit BMP alpha 通道
; （把 BGRA 当 BGR 解析，透明区直接变黑底）。空值=不显示右上角小图。
WizardSmallImageFile=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 与旧版 NSIS 一致：默认当前用户安装，不请求管理员权限
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=yes
RestartApplications=yes
Compression=lzma2/ultra64
SolidCompression=yes
AllowNoIcons=yes
OutputDir=dist
OutputBaseFilename=StudyAgent_{#MyAppVersion}_x64-setup

[Languages]
; 简体中文语言文件随仓库提供（Inno Setup 官方安装包不含中文）
Name: "chinesesimp"; MessagesFile: "assets\languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startmenu"; Description: "创建开始菜单快捷方式"; GroupDescription: "附加任务："; Flags: checkedonce
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："; Flags: unchecked

[Files]
Source: "{#StagingDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: startmenu
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "立即启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  WEBVIEW2_GUID = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WEBVIEW2_DOWNLOAD_URL = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';
  // 说明：Inno 自身生成的卸载键名是 AppId_is1，即
  //       Software\...\Uninstall\2E1F7C4B-9A3D-4C58-B6E0-7D14AF38C295_is1，
  //       且本安装器为 lowest（用户级）权限，卸载信息写在 HKCU 下。
  //       下方 UNINSTALL_KEY 沿用旧 NSIS 时代的键名（StudyAgent），
  //       仅用于 InitializeSetup 检测旧版 NSIS 残留，不要用它读取
  //       Inno 自身版本的卸载入口。
  UNINSTALL_KEY = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#MyAppName}';
  AUTOSTART_RUN_KEY = 'Software\Microsoft\Windows\CurrentVersion\Run';

var
  ResultCode: Integer;
  NSISUninstallString: String;   // 旧 NSIS 版的卸载命令；为空表示未检测到
  NSISInstallDir: String;        // 旧 NSIS 版的安装目录；为空表示未检测到
  AutoStartCommand: String;      // 卸载前读到的开机自启命令原文
  RestoreAutoStart: Boolean;     // 卸载前是否开着开机自启

function RegHasWebView2(const RootKey: Integer; const SubKey: String): Boolean;
var
  Version: String;
begin
  if RegQueryStringValue(RootKey, SubKey, 'pv', Version) then
    Result := Version <> ''
  else
    Result := False;
end;

function IsWebView2Installed(): Boolean;
var
  ClientKey: String;
begin
  Result := False;
  ClientKey := 'SOFTWARE\Microsoft\EdgeUpdate\Clients\' + WEBVIEW2_GUID;
  if IsWin64 then
    Result := RegHasWebView2(HKLM, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\' + WEBVIEW2_GUID);
  if not Result then
    Result := RegHasWebView2(HKLM, ClientKey);
  if not Result then
    Result := RegHasWebView2(HKCU, ClientKey);
end;

function InitializeSetup(): Boolean;
var
  UninstallString: String;
  InstallLocation: String;
begin
  Result := True;

  // 记录旧版（NSIS）的卸载命令，真正的卸载推迟到 PrepareToInstall 执行。
  // 顺序是硬约束：NSIS 卸载脚本会无条件 Delete "$INSTDIR\主程序.exe"，
  // 必须在 Inno 复制新文件之前跑完，否则会把刚装好的新版主程序删掉。
  if RegQueryStringValue(HKCU, UNINSTALL_KEY, 'UninstallString', UninstallString) or
     RegQueryStringValue(HKLM32, UNINSTALL_KEY, 'UninstallString', UninstallString) or
     RegQueryStringValue(HKLM64, UNINSTALL_KEY, 'UninstallString', UninstallString) then
  begin
    // unins000.exe 是 Inno 自己的卸载程序；匹配到它说明装的是 Inno 旧版，
    // 同 AppId 直接覆盖升级即可，不需要卸载。
    if (UninstallString <> '') and (Pos('unins000.exe', Lowercase(UninstallString)) = 0) then
    begin
      NSISUninstallString := UninstallString;
      Log('检测到旧版 NSIS 安装，将在复制文件前静默卸载：' + UninstallString);

      // 沿用旧版的安装目录。学习数据就在 {app}\data 下，
      // UsePreviousAppDir 只认 Inno 自己装的旧版，对 NSIS 版无效；
      // 不沿用就会装到默认目录，用户看到一份全新的空数据，以为数据丢了。
      if RegQueryStringValue(HKCU, UNINSTALL_KEY, 'InstallLocation', InstallLocation) then
      begin
        InstallLocation := RemoveQuotes(InstallLocation);
        if (InstallLocation <> '') and DirExists(InstallLocation) then
        begin
          NSISInstallDir := InstallLocation;
          Log('沿用旧版安装目录：' + InstallLocation);
        end;
      end;
    end;
  end;

  // 静默安装（应用内更新）时先结束正在运行的应用，避免主程序文件被占用
  if WizardSilent() then
    Exec('taskkill.exe', '/IM "{#MyAppExeName}" /F /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure InitializeWizard();
begin
  // 把目录页默认值设成旧 NSIS 版的安装目录。
  // InitializeWizard 在 Inno 处理完 UsePreviousAppDir 之后才调用，
  // 且 Inno 旧版覆盖升级时 NSISInstallDir 为空，不会误覆盖。
  // 交互模式和静默模式（应用内更新）都会从 DirEdit.Text 取 {app}。
  if NSISInstallDir <> '' then
    WizardForm.DirEdit.Text := NSISInstallDir;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    // 恢复开机自启：旧版 NSIS 卸载会清掉 HKCU Run 下的该项，
    // 这里重新指向新安装的主程序，保证用户升级后自启不丢。
    if RestoreAutoStart then
      RegWriteStringValue(HKCU, AUTOSTART_RUN_KEY, '{#MyAppName}',
                          '"' + ExpandConstant('{app}\{#MyAppExeName}') + '"');

    // 静默安装只用于「应用内更新」：装完直接拉起新版本，
    // 避免用户更新后还要手动去开始菜单启动。
    if WizardSilent() then
      Exec(ExpandConstant('{app}\{#MyAppExeName}'), '', '', SW_SHOW, ewNoWait, ResultCode);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';

  // 卸载旧版 NSIS 安装。此处是唯一安全的时间窗：
  // 用户已确认安装、但 Inno 还没复制任何文件。
  // ssInstall / ssPostInstall 时新文件已落地，再唤 NSIS 卸载会删掉新版主程序。
  if NSISUninstallString <> '' then
  begin
    // NSIS 卸载会清掉 HKCU Run 下的开机自启，先记下来，装完再写回
    RestoreAutoStart := RegQueryStringValue(HKCU, AUTOSTART_RUN_KEY, '{#MyAppName}', AutoStartCommand)
                        and (AutoStartCommand <> '');

    // /S = NSIS 静默卸载。此时「删除应用数据」复选框为未勾选状态，
    // 学习数据（{app}\data）不会被删除。
    if not Exec(RemoveQuotes(NSISUninstallString), '/S', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    begin
      Result := '未能自动卸载旧版本 ' + '{#MyAppName}' + '。' + #13#10#13#10 +
                '请先在「添加或删除程序」中手动卸载旧版本，然后重新运行本安装程序。';
      Exit;
    end;
    Log('旧版 NSIS 卸载程序已执行，退出码 ' + IntToStr(ResultCode));
    NSISUninstallString := '';   // 已处理，避免重复执行
  end;

  if not IsWebView2Installed() then
  begin
    if not WizardSilent() then
    begin
      if MsgBox('StudyAgent 需要 Microsoft Edge WebView2 运行时，但本机未检测到。' + #13#10#13#10 +
                '是否立即打开下载页面？', mbConfirmation, MB_YESNO) = IDYES then
        ShellExec('open', WEBVIEW2_DOWNLOAD_URL, '', '', SW_SHOWNORMAL, ewNoWait, ResultCode);
    end;
    Result := '未检测到 Microsoft Edge WebView2 运行时，安装已中止。请先安装 WebView2 运行时后重试。';
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
  Message: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    // 清理开机自启项（tauri-plugin-autostart 写在 HKCU Run 下）
    RegDeleteValue(HKCU, AUTOSTART_RUN_KEY, '{#MyAppName}');

    // 学习数据默认保留，只有用户明确确认才删除
    DataDir := ExpandConstant('{app}\{#MyAppDataDirName}');
    if DirExists(DataDir) and (not UninstallSilent()) then
    begin
      Message := '是否删除 StudyAgent 的学习数据？' + #13#10 +
                 DataDir + #13#10#13#10 +
                 '选择「否」将保留数据，重新安装后可继续使用。';
      if MsgBox(Message, mbConfirmation, MB_YESNO) = IDYES then
        DelTree(DataDir, True, True, True);
    end;
  end;
end;
