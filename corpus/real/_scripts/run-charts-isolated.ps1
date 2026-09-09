#requires -Version 7.0
[CmdletBinding()]
param([int]$TimeoutSeconds = 60)

$ErrorActionPreference = 'Stop'
$generator = Join-Path $PSScriptRoot 'create-charts.ps1'
$outputRoot = Split-Path -Parent $PSScriptRoot
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($generator, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw 'Generator has PowerShell parse errors.' }
$initialSpecs = $ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$specs' }, $true)[0]
$names = @($initialSpecs.FindAll({ param($node) $node -is [System.Management.Automation.Language.HashtableAst] }, $true) | ForEach-Object {
    foreach ($pair in $_.KeyValuePairs) {
        if ($pair.Item1.SafeGetValue() -eq 'Name') { $pair.Item2.SafeGetValue() }
    }
})
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$summaryPath = Join-Path $PSScriptRoot ('isolated-chart-batch-' + $stamp + '.json')
$summary = [System.Collections.Generic.List[object]]::new()
$runtime = (Get-Command pwsh).Source
foreach ($name in $names) {
    $documentPath = Join-Path $outputRoot ('chart/' + $name + '.docx')
    if (Test-Path -LiteralPath $documentPath) {
        $summary.Add(@{ Name = $name; Status = 'preexisting-skipped'; File = $documentPath })
        continue
    }
    $started = Get-Date
    $stdout = Join-Path $PSScriptRoot ($name + '-' + $stamp + '.stdout.log')
    $stderr = Join-Path $PSScriptRoot ($name + '-' + $stamp + '.stderr.log')
    $process = Start-Process -FilePath $runtime -ArgumentList @('-NoProfile', '-STA', '-File', ('"' + $generator + '"'), '-Only', $name, '-Visible') -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
    if ($timedOut) {
        $runFile = Get-ChildItem -LiteralPath $PSScriptRoot -Filter 'create-charts-log-*.json' | Where-Object { $_.CreationTime -ge $started } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($null -ne $runFile) {
            $run = Get-Content -LiteralPath $runFile.FullName -Raw | ConvertFrom-Json
            if ($run.WordProcessId -gt 0 -and @($run.Entries | Where-Object { $_.RequestedFile -eq ('chart/' + $name + '.docx') }).Count -gt 0) {
                $children = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $run.WordProcessId -and $_.Name -eq 'EXCEL.EXE' })
                foreach ($child in $children) { Stop-Process -Id $child.ProcessId -ErrorAction SilentlyContinue }
                Stop-Process -Id $run.WordProcessId -ErrorAction SilentlyContinue
                foreach ($entry in $run.Entries) {
                    if ($entry.Status -eq 'creating') {
                        $entry.Status = 'failed-timeout'
                        $entry.Error = @{ Message = 'Isolated COM operation exceeded timeout.'; Stage = $entry.Stage }
                    }
                }
                $run.FatalError = 'Terminated only this sample process and its recorded Word/Excel processes after timeout.'
                $run.CompletedUtc = [DateTime]::UtcNow.ToString('o')
                $run | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $runFile.FullName -Encoding UTF8
            }
        }
        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
        $status = 'timed-out'
    }
    else { $status = $(if ($process.ExitCode -eq 0) { 'generator-succeeded' } else { 'generator-failed' }) }
    $summary.Add(@{ Name = $name; Status = $status; File = $documentPath; Stdout = $stdout; Stderr = $stderr; ElapsedSeconds = [Math]::Round(((Get-Date) - $started).TotalSeconds, 1) })
    @($summary) | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
    Write-Host ($name + ': ' + $status)
    if (Test-Path -LiteralPath $stdout) { Get-Content -LiteralPath $stdout }
    if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr }
}
@($summary) | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
Write-Host ('Batch summary: ' + $summaryPath)
