# RigDashboard — Installationsguide

En gaming-stats dashboard optimerad för stående sekundärskärm (450×1920).
Visar CPU, GPU (AMD RX 9070 XT), RAM, nätverk och disk i realtid.

Datornamn, CPU-modell och GPU-modell hämtas automatiskt från systemet vid start.
Skärmen förhindras från att gå i sleep mode så länge appen körs.

---

## Beroenden

| Paket | Version | Roll |
|---|---|---|
| `electron` | ^28 | App-ramverk (fönster, IPC, OS-åtkomst) |
| `electron-builder` | ^24 | Bygger installerbar .exe |
| `systeminformation` | ^5.22 | CPU/RAM/Disk/Nätverksdata |
| LibreHardwareMonitor | senaste | GPU-temperatur, GPU-load, fan, power (AMD) |

### LibreHardwareMonitor (krävs för GPU-data)
`systeminformation` kan inte hämta AMD GPU-sensorer direkt.
Istället körs LibreHardwareMonitor som en lokal webb-server på `http://localhost:8085`.

1. Ladda ner från: https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases
2. Packa upp till valfri mapp, t.ex. `C:\Tools\LibreHardwareMonitor\`
3. Starta `LibreHardwareMonitor.exe` som **Administratör**
4. Gå till **Options → Remote Web Server → Run** — aktivera och sätt port `8085`
5. Bocka i **Options → Run On Windows Startup** om du vill att det startar automatiskt

Om LHM inte körs visas `--` för GPU-sensorer, men resten av dashboarden fungerar normalt.

---

## Del 1 — Sätt upp projektet

### Steg 1: Förutsättningar
- **Windows 10/11** (x64)
- **Node.js LTS** — https://nodejs.org (välj "LTS", kör installern med standardval)

### Steg 2: Packa upp projektet
Packa upp ZIP-filen (eller klona repot) till valfri mapp, t.ex.:
```
C:\Users\DittNamn\rig-dashboard\
```

### Steg 3: Öppna Terminal i mappen
Högerklicka på mappen i Utforskaren → "Öppna i Terminal" (eller PowerShell).

### Steg 4: Installera beroenden
```powershell
npm install
```
Laddar ner Electron och systeminformation (~200 MB, tar 1–3 min).

### Steg 5: Starta appen
```powershell
npm start
```
Dashboarden öppnas. Om du har en 450×1920-skärm inkopplad placeras fönstret automatiskt på den.
Är den inte inkopplad används sekundärskärmen som fallback, eller primärskärmen om bara en finns.

---

## Del 2 — Bygga en installerbar .exe

```powershell
npm run build
```
Tar 3–5 minuter. Resultatet hamnar i mappen `dist\`:
```
dist\
  RigDashboard Setup 1.0.0.exe   ← Installerare (NSIS)
  RigDashboard-portable.exe      ← Kör utan installation
```

```powershell
# Bygga enbart portable:
npm run build-portable
```

Kör `RigDashboard Setup 1.0.0.exe` och följ guiden.
Standardinstallation hamnar i:
```
C:\Program Files\RigDashboard\
```

---

## Del 3 — Autostart med Windows

Aktivera autostart via Task Scheduler:

1. Sök efter "Schemaläggaren" i Start
2. Klicka "Skapa enkel uppgift..."
3. Utlösare: **Vid inloggning**
4. Åtgärd: **Starta ett program**
5. Program: `C:\Program Files\RigDashboard\RigDashboard.exe`
6. Klart — dashboarden startar automatiskt vid nästa inloggning

> Glöm inte att också aktivera autostart för **LibreHardwareMonitor** (se ovan).

---

## Filstruktur

```
rig-dashboard/
├── main.js          ← Electron main-process (fönster, IPC, LHM-parsing, sleep-blockering)
├── package.json     ← Projektfil + byggkonfiguration
├── src/
│   ├── index.html   ← Dashboard UI + JavaScript (renderer-process)
│   └── preload.js   ← Brygga mellan Electron och renderer (ej aktiv just nu)
├── assets/
│   └── icon.ico     ← App-ikon
└── dist/            ← Byggs av "npm run build" (skapas automatiskt)
```

---

## Vanliga frågor

**GPU-data visar "--" hela tiden**
Kontrollera att LibreHardwareMonitor körs som Administratör och att webb-servern är aktiverad på port 8085.
Testa i webbläsaren: `http://localhost:8085/data.json` — ska returnera JSON.

**Kan jag ändra vilken skärm den visas på?**
Ja. I `main.js`, funktionen `findDashboardDisplay()`, kan du justera logiken.
T.ex. byta `displays[1]` till `displays[2]` om du har tre skärmar.
Dashboarden letar automatiskt efter en skärm med exakt 450×1920 — byt dessa värden om din skärm har annan upplösning.

**Stöds Intel/NVIDIA?**
Delvis. `systeminformation` hanterar CPU oavsett tillverkare.
För NVIDIA-GPU fungerar LHM också — byt GPU-sökkriterierna i `parseLHM()` i `main.js` efter dina sensorsnamn.

**Hur uppdaterar jag UI:t utan att bygga om?**
Redigera `src/index.html` och kör `npm start` för att se ändringarna direkt.
Bygg ny `.exe` med `npm run build` när du är nöjd.

**Skärmen går fortfarande i sleep**
Appen blockerar display-sleep via Electrons `powerSaveBlocker` automatiskt vid start.
Om skärmen ändå sover kan det bero på att skärmen har egna sleep-inställningar (OSD-meny).
