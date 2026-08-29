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
VersionInfoVersion={#MyAppVersion}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoProductName={#MyAppName}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} 安装程序
DefaultDirName={localappdata}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=auto
UsePreviousAppDir=yes
UsePreviousGroup=yes
UninstallDisplayName={#MyAppName} {#MyAppVersion}
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile=..\src-tauri\icons\icon.ico
WizardStyle=modern
WizardImageFile=assets\wizard.bmp
WizardSmallImageFile=assets\wizard-small.bmp
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
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
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
  UNINSTALL_KEY = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#MyAppName}';
  AUTOSTART_RUN_KEY = 'Software\Microsoft\Windows\CurrentVersion\Run';

var
  ResultCode: Integer;

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
  Message: String;
begin
  Result := True;

  // 旧版（NSIS）安装的卸载程序不是 Inno 生成的 unins000.exe，
  // 直接覆盖安装会残留旧的卸载入口，这里先提示用户。
  if RegQueryStringValue(HKCU, UNINSTALL_KEY, 'UninstallString', UninstallString) or
     RegQueryStringValue(HKLM32, UNINSTALL_KEY, 'UninstallString', UninstallString) or
     RegQueryStringValue(HKLM64, UNINSTALL_KEY, 'UninstallString', UninstallString) then
  begin
    if (UninstallString <> '') and (Pos('unins000.exe', Lowercase(UninstallString)) = 0) then
    begin
      if WizardSilent() then
        Log('检测到旧版（非 Inno Setup）安装，继续静默安装：' + UninstallString)
      else
      begin
        Message := '检测到本机已安装旧版安装程序打包的 ' + '{#MyAppName}' + '。' + #13#10 +
                   '建议先在「添加或删除程序」中卸载旧版本，再运行本安装程序，' + #13#10 +
                   '否则旧的卸载入口会残留在系统中。' + #13#10#13#10 +
                   '仍要继续安装吗？';
        if MsgBox(Message, mbConfirmation, MB_YESNO) <> IDYES then
        begin
          Result := False;
          Exit;
        end;
      end;
    end;
  end;

  // 静默安装（应用内更新）时先结束正在运行的应用，避免主程序文件被占用
  if WizardSilent() then
    Exec('taskkill.exe', '/IM "{#MyAppExeName}" /F /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  // 静默安装只用于「应用内更新」：装完直接拉起新版本，
  // 避免用户更新后还要手动去开始菜单启动。
  if (CurStep = ssPostInstall) and WizardSilent() then
    Exec(ExpandConstant('{app}\{#MyAppExeName}'), '', '', SW_SHOW, ewNoWait, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
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
