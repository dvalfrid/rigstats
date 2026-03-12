# RigDashboard — Installationsguide

En gaming-stats dashboard optimerad för stående sekundärskärm (450×1920).
Visar CPU, GPU (AMD RX 9070 XT), RAM, nätverk och disk i realtid.

---

## Förutsättningar

- **Windows 11** (x64)
- **Node.js LTS** — https://nodejs.org (välj "LTS", kör installern med standardval)
- **AMD Software: Adrenalin** installerat (krävs för GPU-data via WMI)
- Git (valfritt) — https://git-scm.com

---

## Del 1 — Sätt upp projektet

### Steg 1: Packa upp projektet
Packa upp ZIP-filen till valfri mapp, t.ex.:
```
C:\Users\DittNamn\rig-dashboard\
```

### Steg 2: Öppna Terminal i mappen
Högerklicka på mappen i Utforskaren → "Öppna i Terminal" (eller PowerShell).

### Steg 3: Installera beroenden
```powershell
npm install
```
Detta laddar ner Electron och systeminformation (~200 MB, tar 1–3 min).

### Steg 4: Testa att det fungerar
```powershell
npm start
```
Dashboarden öppnas. Om du har mini-skärmen inkopplad placeras fönstret automatiskt på den.
Är mini-skärmen inte inkopplad öppnas ett 450×1920-fönster på primärskärmen.

Tryck **Ctrl+W** eller stäng fönstret för att avsluta.

---

## Del 2 — Bygga en installerbar .exe

### Steg 5: Bygg installationsfil
```powershell
npm run build
```
Tar 3–5 minuter. Resultatet hamnar i mappen `dist\`:
```
dist\
  RigDashboard Setup 1.0.0.exe   ← Installerare (NSIS)
  RigDashboard-portable.exe      ← Kör utan installation
```

### Steg 6: Installera
Kör `RigDashboard Setup 1.0.0.exe` och följ guiden.
Väljer du standardsökväg hamnar den i:
```
C:\Program Files\RigDashboard\
```

---

## Del 3 — Autostart med Windows

I dashboarden finns ingen inbyggd UI för detta ännu, men du kan
aktivera autostart via Task Scheduler:

1. Sök efter "Schemaläggaren" i Start
2. Klicka "Skapa enkel uppgift..."
3. Utlösare: **Vid inloggning**
4. Åtgärd: **Starta ett program**
5. Program: `C:\Program Files\RigDashboard\RigDashboard.exe`
6. Klart — dashboarden startar automatiskt vid nästa inloggning

---

## Del 4 — AMD GPU-data

`systeminformation` hämtar GPU-data via **Windows Management Instrumentation (WMI)**.
AMD Adrenalin exponerar temperatur, VRAM och load via WMI automatiskt.

### Om GPU-data visar "--"
Det kan bero på att WMI-gränssnittet inte är tillgängligt. Prova:

1. Kontrollera att AMD Adrenalin är installerat och uppdaterat
2. Kör dashboarden som **Administratör** (högerklicka → Kör som administratör)
3. Kontrollera att WMI-tjänsten kör:
   ```powershell
   Get-Service winmgmt
   # Ska visa Status: Running
   ```
4. Om det ändå inte fungerar — öppna ett ärende på projektets GitHub

---

## Del 5 — Distribution till flera datorer

### Alternativ A: Portable EXE (enklast)
Kopiera `RigDashboard-portable.exe` till en USB-sticka eller nätverksmapp.
Kör direkt — ingen installation krävs.

Krav på mottagardatorn:
- Windows 10/11 x64
- AMD Adrenalin (för GPU-data)
- Dubbla skärmar (valfritt, fungerar i fönsterläge annars)

### Alternativ B: Installerare via nätverk
Dela `RigDashboard Setup 1.0.0.exe` på en nätverksmapp.
Mottagaren kör installern och får en ren installation med genvägar.

### Alternativ C: GitHub Releases (rekommenderat om ni är flera)
1. Skapa ett privat GitHub-repo
2. Lägg till `.exe`-filerna som en Release
3. Alla kan ladda ner senaste versionen direkt därifrån

---

## Filstruktur

```
rig-dashboard/
├── main.js          ← Electron main-process (fönster + IPC + systeminformation)
├── package.json     ← Projektfil + byggkonfiguration
├── src/
│   ├── index.html   ← Dashboard UI + JavaScript
│   └── preload.js   ← Säker brygga mellan Electron och webbsidan
├── assets/
│   └── icon.ico     ← App-ikon (lägg till en valfri .ico här)
└── dist/            ← Byggs av "npm run build" (skapas automatiskt)
```

---

## Vanliga frågor

**Kan jag ändra vilken skärm den visas på?**
Ja. I `main.js`, funktionen `findDashboardDisplay()`, kan du justera logiken.
T.ex. byta `displays[1]` till `displays[2]` om du har tre skärmar.

**Kan jag lägga till FPS-data på riktigt?**
Ja, via RivaTuner Statistics Server (RTSS) + MSI Afterburner som exporterar
FPS till en delad minnesfil. Det kräver ett extra npm-paket (`node-ipc` eller
läsning av RTSS shared memory). Hör av dig om du vill ha det.

**Stöds Intel/NVIDIA också?**
Ja. `systeminformation` hanterar båda automatiskt. NVIDIA-data är mer
komplett (nvidia-smi körs i bakgrunden). Byt ut AMD-logiken i `main.js`
mot `c.vendor.includes('nvidia')` för NVIDIA.

**Hur uppdaterar jag bara UI:t utan att bygga om?**
Redigera `src/index.html` direkt — kör sedan `npm start` för att se ändringarna.
Bygg en ny `.exe` med `npm run build` när du är nöjd.
