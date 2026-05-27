# Run with admin privileges to read LeagueClientUx command line
$procs = Get-CimInstance Win32_Process -Filter "Name='LeagueClientUx.exe'" -ErrorAction Stop
foreach ($p in $procs) {
    $cmd = $p.CommandLine
    if ($cmd -and $cmd -match 'app-port') {
        $portMatch = $cmd -match '--app-port=([0-9]+)'
        $tokenMatch = $cmd -match '--remoting-auth-token=(.+?)(?=\s--|\s")'
        if ($portMatch -and $tokenMatch) {
            $port = $Matches[1]
            $token = $Matches[2]
            $outPath = 'F:\tft-bot\artifacts\lcu-auth.json'
            @{port=[int]$port; token=$token; pid=$p.ProcessId} | ConvertTo-Json | Set-Content $outPath
            Write-Output "OK: port=$port token=$($token.Substring(0,8))... -> $outPath"
            exit 0
        }
    }
}
Write-Output "FAILED: no auth info found"
exit 1
