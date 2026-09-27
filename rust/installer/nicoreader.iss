; L'installer di NicoReader per Windows (Inno Setup 6.3 o piu' recente).
;
; Lo compila pacchetto.ps1 dopo lo zip, dalla stessa cartella dist\NicoReader:
;   ISCC /DVersione=0.1.0 /DArch=x64 /DSorgente=..\dist\NicoReader installer\nicoreader.iss
; e ne esce dist\NicoReader-<versione>-windows-<arch>-setup.exe.
;
; Cosa fa:
;   - installa per l'utente (in %LOCALAPPDATA%\Programs, senza chiedere di
;     essere amministratore); dal primo passo si puo' scegliere "per tutti"
;   - NicoReader nel menu Start, e sul desktop se lo si chiede
;   - CBZ, CBR, CB7 e CBT: li apre NicoReader con il doppio clic, ma solo se
;     nessun altro programma li apre gia'; in ogni caso NicoReader sta in "Apri
;     con", anche per i PDF (che non gli si prendono)
;   - il disinstallatore toglie tutto questo; i progressi di lettura e le
;     impostazioni restano (sono dell'utente, non del programma)

#ifndef Versione
  #error "manca /DVersione=..."
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef Sorgente
  #define Sorgente "..\dist\NicoReader"
#endif

#define Tipo "NicoReader.Volume"

[Setup]
; lo stesso per x64 e ARM64: e' lo stesso programma, un aggiornamento
; sostituisce l'altro
AppId={{DF75FA4D-CD40-435D-AEB6-379587D2F54F}
AppName=NicoReader
AppVersion={#Versione}
AppVerName=NicoReader {#Versione}
AppPublisherURL=https://github.com/nicgnello-cyber/NicoReader
DefaultDirName={autopf}\NicoReader
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
; anche su un PC ARM, dove Windows lo esegue emulato (meglio pero' la ARM64)
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=NicoReader-{#Versione}-windows-{#Arch}-setup
SetupIconFile=..\crates\fumetto\risorse\fumetto.ico
UninstallDisplayIcon={app}\NicoReader.exe
UninstallDisplayName=NicoReader
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
; aggiornando, NicoReader aperto si chiude da solo (e si riapre alla fine)
CloseApplications=yes

[Languages]
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktop"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Sorgente}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\NicoReader"; Filename: "{app}\NicoReader.exe"
Name: "{autodesktop}\NicoReader"; Filename: "{app}\NicoReader.exe"; Tasks: desktop

[Registry]
; il tipo "volume di NicoReader": nome, icona, come aprirlo
Root: HKA; Subkey: "Software\Classes\{#Tipo}"; ValueType: string; ValueName: ""; ValueData: "{cm:Volume}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#Tipo}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\NicoReader.exe,0"
Root: HKA; Subkey: "Software\Classes\{#Tipo}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\NicoReader.exe"" ""%1"""
; NicoReader in "Apri con" per ogni formato che legge
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "NicoReader"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\NicoReader.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\SupportedTypes"; ValueType: string; ValueName: ".cbz"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\SupportedTypes"; ValueType: string; ValueName: ".cbr"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\SupportedTypes"; ValueType: string; ValueName: ".cb7"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\SupportedTypes"; ValueType: string; ValueName: ".cbt"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\NicoReader.exe\SupportedTypes"; ValueType: string; ValueName: ".pdf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.cbz\OpenWithProgids"; ValueType: string; ValueName: "{#Tipo}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.cbr\OpenWithProgids"; ValueType: string; ValueName: "{#Tipo}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.cb7\OpenWithProgids"; ValueType: string; ValueName: "{#Tipo}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.cbt\OpenWithProgids"; ValueType: string; ValueName: "{#Tipo}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.pdf\OpenWithProgids"; ValueType: string; ValueName: "{#Tipo}"; ValueData: ""; Flags: uninsdeletevalue
; il doppio clic sui fumetti, solo dove nessun altro programma li apre gia'
; (li si toglie disinstallando, vedi Togli() sotto)
Root: HKA; Subkey: "Software\Classes\.cbz"; ValueType: string; ValueName: ""; ValueData: "{#Tipo}"; Check: Libero('.cbz')
Root: HKA; Subkey: "Software\Classes\.cbr"; ValueType: string; ValueName: ""; ValueData: "{#Tipo}"; Check: Libero('.cbr')
Root: HKA; Subkey: "Software\Classes\.cb7"; ValueType: string; ValueName: ""; ValueData: "{#Tipo}"; Check: Libero('.cb7')
Root: HKA; Subkey: "Software\Classes\.cbt"; ValueType: string; ValueName: ""; ValueData: "{#Tipo}"; Check: Libero('.cbt')

[Run]
Filename: "{app}\NicoReader.exe"; Description: "{cm:LaunchProgram,NicoReader}"; Flags: nowait postinstall skipifsilent

[CustomMessages]
it.Volume=NicoReader (volume)
en.Volume=NicoReader (volume)

[Code]
// Nessun programma apre gia' questi file (o li apre gia' NicoReader)? La vista
// unita HKCR vede sia le impostazioni dell'utente sia quelle per tutti.
function Libero(Estensione: String): Boolean;
var
  Chi: String;
begin
  Result := (not RegQueryStringValue(HKCR, Estensione, '', Chi)) or (Chi = '') or (Chi = '{#Tipo}');
end;

// Disinstallando, il doppio clic torna com'era: si toglie solo se e' ancora
// di NicoReader.
procedure Togli(Estensione: String);
var
  Chi: String;
begin
  if RegQueryStringValue(HKA, 'Software\Classes\' + Estensione, '', Chi) and (Chi = '{#Tipo}') then
    RegDeleteValue(HKA, 'Software\Classes\' + Estensione, '');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Togli('.cbz');
    Togli('.cbr');
    Togli('.cb7');
    Togli('.cbt');
  end;
end;
