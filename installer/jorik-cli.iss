#define MyAppName "jorik-cli"
#ifndef MyAppVersion
  #if GetEnv("VERSION") != ""
    #define MyAppVersion GetEnv("VERSION")
  #else
    #define MyAppVersion "0.0.0"
  #endif
#endif
#ifndef MyTarget
  #if GetEnv("TARGET") != ""
    #define MyTarget GetEnv("TARGET")
  #else
    #define MyTarget "x86_64-pc-windows-msvc"
  #endif
#endif
#define MyExe "jorik.exe"

[Setup]
APPID={{BD6BB68C-5547-4FC4-A93D-7C322F0A1443}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppName}
DefaultDirName={userappdata}\{#MyAppName}
OutputDir=..\\dist
OutputBaseFilename={#MyAppName}-{#MyAppVersion}-{#MyTarget}-setup
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
Compression=lzma2
SolidCompression=yes
ChangesEnvironment=yes
WizardStyle=modern dynamic
PrivilegesRequired=lowest
UsePreviousAppDir=yes
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableWelcomePage=yes
DisableFinishedPage=no
UninstallDisplayIcon={app}\{#MyExe}
WizardImageAlphaFormat=defined
SetupIconFile=../installer/assets/icon.ico

[Files]
Source: "assets\logo.png"; DestDir: "{tmp}"; Flags: dontcopy
Source: "assets\logo.png"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\{#MyTarget}\release\{#MyExe}"; DestDir: "{app}"; Flags: ignoreversion

[Code]
const
  VC_REDIST_URL = 'https://aka.ms/vs/17/release/vc_redist.x64.exe';
  EnvironmentKey = 'Environment';

var
  PrePage: TWizardPage;
  LogoImage: TBitmapImage;
  CreditLabel, VersionLabel, PreTitle, PreDesc, InsTitle, FinTitle, FinDesc: TNewStaticText;
  PostPagePanel: TPanel;
  IsUpgradeMode: Boolean;

procedure StyleLabel(Lbl: TNewStaticText; FontSize: Integer; FontStyle: TFontStyles; FontColor: TColor);
begin
  Lbl.Font.Name := 'Segoe UI';
  Lbl.Font.Size := FontSize;
  Lbl.Font.Style := FontStyle;
  Lbl.Font.Color := FontColor;
end;

procedure RefreshLogoLayout(Parent: TWinControl; Image: TBitmapImage);
var
  ImgW, ImgH, TargetH, TargetW, MaxW, MaxH: Integer;
begin
  Image.Parent := Parent;
  // Dynamic scaling logic
  MaxW := Parent.ClientWidth - ScaleX(48);
  MaxH := ScaleY(120);
  TargetW := ScaleX(400);
  TargetH := MaxH;
  try
    ImgW := Image.PngImage.Width;
    ImgH := Image.PngImage.Height;
    if (ImgW > 0) and (ImgH > 0) then begin
      TargetH := MaxH;
      TargetW := (TargetH * ImgW) div ImgH;
      if TargetW > MaxW then begin
        TargetW := MaxW;
        TargetH := (TargetW * ImgH) div ImgW;
      end;
    end;
  except
  end;
  Image.Width := TargetW;
  Image.Height := TargetH;
  Image.Left := (Parent.ClientWidth - Image.Width) div 2;
  Image.Top := ScaleY(10);
end;

function IsUpgrade(): Boolean;
var
  sUnInstPath: String;
begin
  Result := False;
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{BD6BB68C-5547-4FC4-A93D-7C322F0A1443}_is1', 'UninstallString', sUnInstPath) then begin Result := True; Exit; end;
  if RegQueryStringValue(HKLM, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{BD6BB68C-5547-4FC4-A93D-7C322F0A1443}_is1', 'UninstallString', sUnInstPath) then begin Result := True; Exit; end;
  if DirExists(ExpandConstant('{userappdata}\{#MyAppName}')) then begin Result := True; Exit; end;
end;

function VCRedistNeedsInstall: Boolean;
var
  Installed: Cardinal;
begin
  if RegQueryDWordValue(HKEY_LOCAL_MACHINE, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) and (Installed = 1) then Result := False else Result := True;
end;

procedure EnvAddPath(Path: string);
var
  Paths: string; List: TStringList; i: Integer; Found: Boolean;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Paths) then Paths := '';
  List := TStringList.Create;
  try
    StringChange(Paths, ';', #13#10);
    List.Text := Paths;
    Found := False;
    for i := 0 to List.Count - 1 do begin if CompareText(List[i], Path) = 0 then begin Found := True; Break; end; end;
    if not Found then begin
      List.Add(Path);
      Paths := '';
      for i := 0 to List.Count - 1 do begin if List[i] <> '' then begin if Paths <> '' then Paths := Paths + ';'; Paths := Paths + List[i]; end; end;
      RegWriteStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Paths);
    end;
  finally List.Free; end;
end;

procedure EnvRemovePath(Path: string);
var
  Paths: string; List: TStringList; i: Integer; Changed: Boolean;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Paths) then exit;
  List := TStringList.Create;
  try
    StringChange(Paths, ';', #13#10);
    List.Text := Paths;
    Changed := False;
    for i := List.Count - 1 downto 0 do begin if CompareText(List[i], Path) = 0 then begin List.Delete(i); Changed := True; end; end;
    if Changed then begin
      Paths := '';
      for i := 0 to List.Count - 1 do begin if List[i] <> '' then begin if Paths <> '' then Paths := Paths + ';'; Paths := Paths + List[i]; end; end;
      RegWriteStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Paths);
    end;
  finally List.Free; end;
end;

procedure InitializeWizard();
begin
  WizardForm.Bevel.Visible := False;
  WizardForm.PageNameLabel.Visible := False;
  WizardForm.PageDescriptionLabel.Visible := False;
  WizardForm.WizardSmallBitmapImage.Visible := False;
  WizardForm.Font.Name := 'Segoe UI';
  WizardForm.Font.Size := 9;

  LogoImage := TBitmapImage.Create(WizardForm);
  LogoImage.Stretch := True;
  try ExtractTemporaryFile('logo.png'); LogoImage.PngImage.LoadFromFile(ExpandConstant('{tmp}\logo.png')); except end;

  CreditLabel := TNewStaticText.Create(WizardForm);
  CreditLabel.Caption := 'by xxanqw'; StyleLabel(CreditLabel, 8, [], clGrayText);
  CreditLabel.Parent := WizardForm;
  CreditLabel.Left := ScaleX(24);
  CreditLabel.Top := WizardForm.NextButton.Top + (WizardForm.NextButton.Height - CreditLabel.Height) div 2;

  VersionLabel := TNewStaticText.Create(WizardForm);
  VersionLabel.Caption := 'v{#MyAppVersion}'; StyleLabel(VersionLabel, 8, [], clGrayText);
  VersionLabel.Parent := WizardForm;
  VersionLabel.Left := CreditLabel.Left + CreditLabel.Width + ScaleX(10);
  VersionLabel.Top := CreditLabel.Top;

  PrePage := CreateCustomPage(wpWelcome, '', '');
  PreTitle := TNewStaticText.Create(PrePage); PreTitle.Parent := PrePage.Surface; StyleLabel(PreTitle, 14, [fsBold], clWindowText);
  PreDesc := TNewStaticText.Create(PrePage); PreDesc.Parent := PrePage.Surface; StyleLabel(PreDesc, 10, [], clWindowText);

  InsTitle := TNewStaticText.Create(WizardForm); InsTitle.Parent := WizardForm.InstallingPage; StyleLabel(InsTitle, 14, [fsBold], clWindowText);
  WizardForm.StatusLabel.Font.Color := clWindowText;
  WizardForm.FilenameLabel.Visible := True; WizardForm.FilenameLabel.Font.Color := clGrayText;

  PostPagePanel := TPanel.Create(WizardForm); PostPagePanel.Parent := WizardForm; PostPagePanel.Visible := False; PostPagePanel.BevelOuter := bvNone;
  FinTitle := TNewStaticText.Create(PostPagePanel); FinTitle.Parent := PostPagePanel; StyleLabel(FinTitle, 16, [fsBold], $228B22);
  FinDesc := TNewStaticText.Create(PostPagePanel); FinDesc.Parent := PostPagePanel; StyleLabel(FinDesc, 10, [], clWindowText);

  IsUpgradeMode := IsUpgrade();
end;

procedure LayoutPrePage();
var W, Margin: Integer;
begin
  W := PrePage.SurfaceWidth; Margin := ScaleX(24);
  RefreshLogoLayout(PrePage.Surface, LogoImage);
  PreTitle.Top := LogoImage.Top + LogoImage.Height + ScaleY(30); PreTitle.Left := Margin; PreTitle.Width := W - 2*Margin; PreTitle.AutoSize := True;
  PreDesc.Top := PreTitle.Top + PreTitle.Height + ScaleY(8); PreDesc.Left := Margin; PreDesc.Width := W - 2*Margin;
end;

procedure LayoutInstallingPage();
var W, Margin: Integer;
begin
  W := WizardForm.InstallingPage.Width; Margin := ScaleX(24);
  RefreshLogoLayout(WizardForm.InstallingPage, LogoImage);
  InsTitle.Top := LogoImage.Top + LogoImage.Height + ScaleY(30); InsTitle.Left := Margin;
  WizardForm.ProgressGauge.Left := Margin; WizardForm.ProgressGauge.Width := W - 2*Margin; WizardForm.ProgressGauge.Top := InsTitle.Top + InsTitle.Height + ScaleY(20);
  WizardForm.StatusLabel.Left := Margin; WizardForm.StatusLabel.Top := WizardForm.ProgressGauge.Top + WizardForm.ProgressGauge.Height + ScaleY(12); WizardForm.StatusLabel.Width := W - 2*Margin;
  WizardForm.FilenameLabel.Left := Margin; WizardForm.FilenameLabel.Top := WizardForm.StatusLabel.Top + WizardForm.StatusLabel.Height + ScaleY(4); WizardForm.FilenameLabel.Width := W - 2*Margin;
end;

procedure LayoutPostPage();
var Margin: Integer;
begin
  PostPagePanel.Left := WizardForm.OuterNotebook.Left + WizardForm.InnerNotebook.Left; PostPagePanel.Top := WizardForm.OuterNotebook.Top + WizardForm.InnerNotebook.Top;
  PostPagePanel.Width := WizardForm.InnerNotebook.Width; PostPagePanel.Height := WizardForm.InnerNotebook.Height;
  Margin := ScaleX(24);
  RefreshLogoLayout(PostPagePanel, LogoImage);
  FinTitle.Top := LogoImage.Top + LogoImage.Height + ScaleY(30); FinTitle.Left := Margin;
  FinDesc.Top := FinTitle.Top + FinTitle.Height + ScaleY(10); FinDesc.Left := Margin;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = PrePage.ID then begin
    if IsUpgradeMode then begin PreTitle.Caption := 'Update {#MyAppName}'; PreDesc.Caption := 'Click the button below to update the application to v{#MyAppVersion}.'; WizardForm.NextButton.Caption := 'Update'; end
    else begin PreTitle.Caption := 'Install {#MyAppName}'; PreDesc.Caption := 'Click the button below to install the application.'; WizardForm.NextButton.Caption := 'Install'; end;
    LayoutPrePage();
  end;
  if CurPageID = wpInstalling then begin
    if IsUpgradeMode then InsTitle.Caption := 'Updating...' else InsTitle.Caption := 'Installing...';
    LayoutInstallingPage();
  end;
  if CurPageID = wpFinished then begin
    WizardForm.OuterNotebook.Visible := False; PostPagePanel.Visible := True; PostPagePanel.BringToFront; CreditLabel.BringToFront; VersionLabel.BringToFront;
    if IsUpgradeMode then begin FinTitle.Caption := 'Update Completed'; FinDesc.Caption := '{#MyAppName} has been successfully updated.'; end
    else begin FinTitle.Caption := 'Installation Completed'; FinDesc.Caption := '{#MyAppName} has been successfully installed.'; end;
    LayoutPostPage();
    WizardForm.NextButton.Caption := 'Close'; WizardForm.CancelButton.Visible := False; WizardForm.BackButton.Visible := False;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var ResultCode: Integer; DownloadPath: String;
begin
  if (CurStep = ssPostInstall) then EnvAddPath(ExpandConstant('{app}'));
  if (CurStep = ssPostInstall) and VCRedistNeedsInstall then begin
    if not WizardSilent then if MsgBox('This application requires the Microsoft Visual C++ Redistributable (x64). Download and install it now?', mbConfirmation, MB_YESNO) = IDYES then begin
        DownloadPath := ExpandConstant('{tmp}\vc_redist.x64.exe');
        try DownloadTemporaryFile(VC_REDIST_URL, 'vc_redist.x64.exe', '', nil); Exec(DownloadPath, '/install /passive /norestart', '', SW_SHOW, ewWaitUntilTerminated, ResultCode);
        except MsgBox('Error downloading or installing Visual C++ Redistributable: ' + GetExceptionMessage, mbError, MB_OK); end;
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then EnvRemovePath(ExpandConstant('{app}'));
end;

function InitializeUninstall(): Boolean;
var
  WelcomeForm: TForm;
  Logo: TBitmapImage;
  Title, Desc: TNewStaticText;
  UninstallBtn, CancelBtn: TNewButton;
  Margin, W, ErrorCode: Integer;
begin
  if UninstallSilent then
  begin
    Result := True;
    Exit;
  end;

  Result := False;

  if FileExists(ExpandConstant('{app}\logo.png')) then
    CopyFile(ExpandConstant('{app}\logo.png'), ExpandConstant('{tmp}\logo_u.png'), False);

  WelcomeForm := TForm.Create(nil);
  WelcomeForm.BorderStyle := bsDialog;
  WelcomeForm.ClientWidth := ScaleX(500);
  WelcomeForm.ClientHeight := ScaleY(360);
  WelcomeForm.Caption := 'Uninstall {#MyAppName}';
  WelcomeForm.Position := poScreenCenter;
  WelcomeForm.Font.Name := 'Segoe UI';
  WelcomeForm.Font.Size := 9;

  Logo := TBitmapImage.Create(WelcomeForm);
  Logo.Parent := WelcomeForm;
  Logo.Stretch := True;
  if FileExists(ExpandConstant('{tmp}\logo_u.png')) then
    Logo.PngImage.LoadFromFile(ExpandConstant('{tmp}\logo_u.png'));
  RefreshLogoLayout(WelcomeForm, Logo);

  Margin := ScaleX(24);
  W := WelcomeForm.ClientWidth;

  Title := TNewStaticText.Create(WelcomeForm);
  Title.Parent := WelcomeForm;
  Title.Caption := 'Uninstall {#MyAppName}';
  StyleLabel(Title, 14, [fsBold], clWindowText);
  Title.Top := Logo.Top + Logo.Height + ScaleY(30);
  Title.Left := Margin;
  Title.AutoSize := True;

  Desc := TNewStaticText.Create(WelcomeForm);
  Desc.Parent := WelcomeForm;
  Desc.Caption := 'Click the button below to remove the application from your computer.';
  StyleLabel(Desc, 10, [], clWindowText);
  Desc.Top := Title.Top + Title.Height + ScaleY(8);
  Desc.Left := Margin;
  Desc.Width := W - 2*Margin;

  CancelBtn := TNewButton.Create(WelcomeForm);
  CancelBtn.Parent := WelcomeForm;
  CancelBtn.Caption := 'Cancel';
  CancelBtn.ModalResult := mrCancel;
  CancelBtn.Left := W - CancelBtn.Width - Margin;
  CancelBtn.Top := WelcomeForm.ClientHeight - CancelBtn.Height - ScaleY(12);
  
  UninstallBtn := TNewButton.Create(WelcomeForm);
  UninstallBtn.Parent := WelcomeForm;
  UninstallBtn.Caption := 'Uninstall';
  UninstallBtn.ModalResult := mrOk;
  UninstallBtn.Left := CancelBtn.Left - UninstallBtn.Width - ScaleX(10);
  UninstallBtn.Top := CancelBtn.Top;
  UninstallBtn.Default := True;

  if WelcomeForm.ShowModal() = mrOk then
  begin
    Exec(ExpandConstant('{uninstallexe}'), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART', '', SW_HIDE, ewNoWait, ErrorCode);
    Result := False;
  end;
end;