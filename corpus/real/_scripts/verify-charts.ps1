#requires -Version 5.1
[CmdletBinding()]
param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))

$ErrorActionPreference = 'Stop'
$tokens = $null
$errors = $null
$generator = Join-Path $PSScriptRoot 'create-charts.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($generator, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw 'Generator has PowerShell parse errors.' }
# Load only read-only package inspection helpers; never execute the COM generator.
foreach ($function in $ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -in @('Read-ZipXml', 'Test-ChartPackage') }, $true)) {
    . ([scriptblock]::Create($function.Extent.Text))
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$results = @(
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $OutputRoot 'chart') -Filter '*.docx' | Where-Object { $_.Name -notlike 'chart-pasted-*' }) {
        $name = [IO.Path]::GetFileNameWithoutExtension($file.Name)
        $spec = @{ ChartEx = $name.StartsWith('chartex-'); SingleSeries = $name -in @('chart-pie', 'chart-point-color') }
        $check = Test-ChartPackage $file.FullName $spec
        [ordered]@{ File = 'chart/' + $file.Name; Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash; Check = $check }
    }
)
$report = Join-Path $PSScriptRoot ('chart-package-verification-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.json')
$results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $report -Encoding UTF8
$results | ForEach-Object { [pscustomobject]@{ File = $_.File; Passed = $_.Check.Passed; Failures = $_.Check.Failures -join '; ' } } | Format-Table -AutoSize
Write-Host ('Verification report: ' + $report)
if (@($results | Where-Object { -not $_.Check.Passed }).Count -gt 0) { exit 1 }
