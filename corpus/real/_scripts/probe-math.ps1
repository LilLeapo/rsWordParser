#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
$results = [Collections.Generic.List[object]]::new()
try {
    foreach ($inputText in @('(a+b)/2+x^2', '(a+b)/2+x^2 ', '\frac{a+b}{2}+x^2', '∫_0^1 x^2 ⅆx', '[■(a&b@c&d)]')) {
        $doc = $word.Documents.Add()
        try {
            $doc.Content.Text = "before 前文`r`rafter 后文`r"
            $r = $doc.Paragraphs.Item(2).Range.Duplicate
            $r.Collapse(1)
            $start = $r.Start
            $r.Text = $inputText
            $r.SetRange($start, $start + $inputText.Length)
            $mathRange = $doc.OMaths.Add($r)
            $math = $mathRange.OMaths.Item(1)
            $math.Type = 0
            $math.BuildUp()
            $types = @()
            for ($i = 1; $i -le $math.Functions.Count; $i++) { $types += [int]$math.Functions.Item($i).Type }
            $results.Add([pscustomobject]@{ input = $inputText; output = $math.Range.Text; types = $types; equations = $doc.OMaths.Count; xml = $math.Range.WordOpenXML })
        }
        finally { $doc.Close(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) }
    }
    $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'math-probe-results.json') -Encoding utf8
    $results | Select-Object input,output,types,equations | ConvertTo-Json -Depth 4
}
finally { $word.Quit(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($word) }
