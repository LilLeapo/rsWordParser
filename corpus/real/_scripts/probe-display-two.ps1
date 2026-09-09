#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$word = New-Object -ComObject Word.Application
$word.Visible = $true
$word.DisplayAlerts = 0
$records = [Collections.Generic.List[object]]::new()
try {
    foreach ($variant in @('mso-end', 'mso-inside-end', 'mso-whole')) {
        $doc = $word.Documents.Add()
        try {
            $doc.Content.Text = "before 前文`r`rafter 后文`r"
            $r = $doc.Paragraphs.Item(2).Range.Duplicate
            $r.Collapse(1)
            $r.Text = 'x^2'
            $r.SetRange(10, 13)
            $mRange = $doc.OMaths.Add($r)
            $m = $mRange.OMaths.Item(1)
            $m.Type = if ($variant -eq 'inline-then-display') { 1 } else { 0 }
            $m.BuildUp()
            $doc.Activate()
            $m.Range.Select()
            $unicodeBefore = $word.CommandBars.GetPressedMso('EquationUnicodeFormat')
            $latexBefore = $word.CommandBars.GetPressedMso('EquationLaTexFormat')
            $word.CommandBars.ExecuteMso('EquationLaTexFormat')
            $latexAfter = $word.CommandBars.GetPressedMso('EquationLaTexFormat')
            $at = [int]$m.Range.End
            if ($variant -eq 'mso-inside-end') { $at-- }
            $insert = $doc.Range($at, $at)
            if ($variant -eq 'mso-whole') { $m.Range.Select() } else { $insert.Select() }
            $word.CommandBars.ExecuteMso('EquationInsertNew')
            $word.Selection.TypeText('E=mc^2')
            $word.CommandBars.ExecuteMso('EquationProfessionalOne')
            [xml]$xml = $doc.Content.WordOpenXML
            $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
            $ns.AddNamespace('m', 'http://schemas.openxmlformats.org/officeDocument/2006/math')
            $records.Add([pscustomobject]@{ variant = $variant; maths = $doc.OMaths.Count; mathParas = $xml.SelectNodes('//m:oMathPara', $ns).Count; desired = $xml.SelectNodes('//m:oMathPara[count(m:oMath)=2]', $ns).Count; unicodeBefore = $unicodeBefore; latexBefore = $latexBefore; latexAfter = $latexAfter; xml = $doc.Content.WordOpenXML })
        }
        catch { $records.Add([pscustomobject]@{ variant = $variant; error = $_.Exception.ToString(); position = $_.InvocationInfo.PositionMessage }) }
        finally { $doc.Close(0) }
    }
    $word.Templates.LoadBuildingBlocks()
    $names = @()
    for ($t = 1; $t -le $word.Templates.Count; $t++) {
        $template = $word.Templates.Item($t)
        for ($i = 1; $i -le $template.BuildingBlockEntries.Count; $i++) {
            $entry = $template.BuildingBlockEntries.Item($i)
            if ($entry.Name -match '傅|Four|泰|二次') { $names += [string]$entry.Name }
        }
    }
    $records | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'display-two-probe-results.json') -Encoding utf8
    $records | Select-Object variant,maths,mathParas,desired,unicodeBefore,latexBefore,latexAfter,error,position | ConvertTo-Json -Depth 4
    $names | ConvertTo-Json
}
finally { $word.Quit(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($word) }
