@echo off
:: Self-elevating LCU token extractor
:: Checks if running as admin, if not re-launches itself elevated

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting admin privileges...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs -Wait"
    goto :check_result
)

:: We are admin now - extract the token
echo Running as admin, extracting LCU auth...

set "OUTFILE=F:\tft-bot\artifacts\lcu-auth.json"

for /f "tokens=*" %%i in ('wmic process where name^="LeagueClientUx.exe" get commandline /format:list 2^>nul ^| findstr /i "CommandLine"') do (
    set "CMDLINE=%%i"
)

if not defined CMDLINE (
    echo FAILED: Could not read LeagueClientUx command line
    exit /b 1
)

:: Extract port
for /f "tokens=2 delims==" %%a in ('echo %CMDLINE% ^| findstr /o "app-port="') do set "PORT_RAW=%%a"
:: This won't work well with for loops, use PowerShell for parsing

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$raw = '%CMDLINE%'; " ^
  "$pm = [regex]::Match($raw, '--app-port=([0-9]+)'); " ^
  "$tm = [regex]::Match($raw, '--remoting-auth-token=(.+?)(?=\s--|\s*[\"])'); " ^
  "if ($pm.Success -and $tm.Success) { " ^
  "  $port = $pm.Groups[1].Value; $token = $tm.Groups[1].Value; " ^
  "  @{port=[int]$port; token=$token} | ConvertTo-Json | Set-Content '%OUTFILE%' -Encoding UTF8; " ^
  "  Write-Output ('OK port=' + $port + ' token=' + $token.Substring(0,[Math]::Min(8,$token.Length)) + '...'); " ^
  "} else { Write-Output 'FAILED: regex no match'; exit 1 }"

goto :eof

:check_result
if exist "%OUTFILE%" (
    echo.
    echo SUCCESS! Auth file created:
    type "%OUTFILE%"
) else (
    echo.
    echo FAILED: Auth file not created. UAC may have been denied.
)
