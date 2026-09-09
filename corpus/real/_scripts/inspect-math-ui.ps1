param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Names = @('math-display-two-2')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$copyRoot = Join-Path $OutputRoot ('_checks/math-ui-offline/' + [DateTime]::Now.ToString('yyyyMMdd-HHmmss-fff'))
[void][IO.Directory]::CreateDirectory($copyRoot)
$records = @()
foreach ($name in $Names) {
    $relative = 'math/' + $name + '.docx'
    $path = Join-Path $OutputRoot $relative
    $hashBefore = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $copy = Join-Path $copyRoot ($name + '.zip')
    Copy-Item -LiteralPath $path -Destination $copy
    $zip = [IO.Compression.ZipFile]::OpenRead($copy)
    try {
        $stream = $zip.GetEntry('word/document.xml').Open()
        try { $xml = [xml]::new(); $xml.Load($stream) } finally { $stream.Dispose() }
        $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
        $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
        $ns.AddNamespace('m', 'http://schemas.openxmlformats.org/officeDocument/2006/math')
        $records += [ordered]@{
            file = $relative
            copy = $copy
            sourceSha256 = $hashBefore
            originalUnchanged = $hashBefore -eq (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
            oMathParaCount = $xml.SelectNodes('//m:oMathPara', $ns).Count
            oMathCount = $xml.SelectNodes('//m:oMath', $ns).Count
            equationsPerMathParagraph = @($xml.SelectNodes('//m:oMathPara', $ns) | ForEach-Object { $_.SelectNodes('./m:oMath', $ns).Count })
            equations = @($xml.SelectNodes('//m:oMath', $ns) | ForEach-Object { [ordered]@{ text = ($_.SelectNodes('.//m:t', $ns) | ForEach-Object InnerText) -join ''; lineBreaks = $_.SelectNodes('.//w:br', $ns).Count } })
            paragraphs = @($xml.SelectNodes('/w:document/w:body/w:p', $ns) | ForEach-Object { [ordered]@{ text = ($_.SelectNodes('.//w:t | .//m:t', $ns) | ForEach-Object InnerText) -join ''; mathParagraphCount = $_.SelectNodes('./m:oMathPara', $ns).Count } })
            runFontSizesHalfPoints = @($xml.SelectNodes('//m:r/w:rPr/w:sz/@w:val', $ns) | ForEach-Object Value | Sort-Object -Unique)
            mathRuns = @($xml.SelectNodes('//m:r', $ns) | ForEach-Object OuterXml)
            mathParagraphXml = @($xml.SelectNodes('//m:oMathPara', $ns) | ForEach-Object OuterXml)
        }
    } finally { $zip.Dispose() }
}
$destination = Join-Path $PSScriptRoot 'math-ui-offline-review.json'
$records | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $destination -Encoding utf8
$records | ConvertTo-Json -Depth 12
