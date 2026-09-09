param([Parameter(Mandatory=$true)][string]$Id,[string]$Script,[string]$Action='run',[switch]$Wait,[string]$Root='C:\word\real-word-round3-20260907')
$control=Join-Path $Root '_control'
if(Test-Path -LiteralPath (Join-Path $control 'request.json')){throw 'Pending job exists.'}
@{id=$Id;script=$Script;action=$Action} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $control 'request.tmp') -Encoding UTF8
Move-Item -LiteralPath (Join-Path $control 'request.tmp') -Destination (Join-Path $control 'request.json')
if($Wait){
    $response=Join-Path $control ($Id+'.json')
    $deadline=(Get-Date).AddSeconds(30)
    while(-not(Test-Path -LiteralPath $response)){if((Get-Date)-gt $deadline){throw "Job remains active: $Id"};Start-Sleep -Milliseconds 150}
    Get-Content -LiteralPath $response -Raw -Encoding UTF8
}
