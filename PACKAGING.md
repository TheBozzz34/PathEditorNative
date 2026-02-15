# Packaging

## Build portable zip + installer

Run from project root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1
```

Outputs:
- `dist\PathEditorNative-<version>-win64.zip`
- `dist\PathEditorNative-<version>-win64.zip.sha256.txt`
- `dist\PathEditorNative-Setup-<version>.exe` (if Inno Setup is available)

## Build only the portable zip

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1 -SkipInstaller
```
