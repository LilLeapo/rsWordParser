param(
    [Parameter(Mandatory=$true)][object]$Word,
    [string]$OutputDir = (Join-Path (Split-Path -Parent $PSScriptRoot) '_trials/table-revision-probe')
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$results=[Collections.Generic.List[object]]::new()
foreach ($mode in 'direct','range','style','style-and-direct') {
    [string]$path=[string](Join-Path $OutputDir ($mode+'.docx'))
    if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite $path" }
    $doc=$null
    $result=[ordered]@{mode=$mode;path=$path}
    try {
        $doc=$Word.Documents.Add()
        $doc.Content.Text="before`rTABLE`rafter`r"
        $range=$doc.Paragraphs.Item(2).Range.Duplicate
        $range.End--
        $range.Text=''
        $table=$doc.Tables.Add($range,2,2)
        foreach ($row in 1,2) { foreach ($column in 1,2) { $table.Cell($row,$column).Range.Text="R$row C$column" } }
        if ($mode -like 'style*') {
            foreach ($color in @(@('Black',0),@('Red',255))) {
                [string]$name='Round2 Border '+$color[0]
                $style=$doc.Styles.Add($name,3)
                $style.BaseStyle=-106
                $style.Table.Borders.Enable=1
                foreach ($border in $style.Table.Borders) { $border.LineStyle=1; $border.Color=[int]$color[1] }
            }
            [string]$blackName='Round2 Border Black'
            $table.Style=$blackName
        } else {
            $table.Borders.Enable=1
            foreach ($border in $table.Borders) { $border.LineStyle=1; $border.Color=0 }
        }
        $doc.TrackFormatting=$true
        $doc.TrackRevisions=$true
        switch ($mode) {
            'direct' { foreach ($border in $table.Borders) { $border.Color=255 } }
            'range' { foreach ($border in $table.Range.Borders) { $border.Color=255 } }
            'style' { [string]$redName='Round2 Border Red'; $table.Style=$redName }
            'style-and-direct' {
                [string]$redName='Round2 Border Red'; $table.Style=$redName
                foreach ($border in $table.Borders) { $border.Color=255 }
            }
        }
        $result.revisions=@($doc.Revisions | ForEach-Object { @{type=[int]$_.Type;format=$_.FormatDescription} })
        $doc.SaveAs2($path,12)
        $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        $zip=$null
        try {
            $zip=[IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Read,$true)
            $reader=[IO.StreamReader]::new($zip.GetEntry('word/document.xml').Open())
            try { $xml=[xml]$reader.ReadToEnd() } finally { $reader.Dispose() }
            $ns=[Xml.XmlNamespaceManager]::new($xml.NameTable)
            $ns.AddNamespace('w','http://schemas.openxmlformats.org/wordprocessingml/2006/main')
            $result.tblPrChange=$xml.SelectNodes('//w:tblPrChange',$ns).Count
            $result.tcPrChange=$xml.SelectNodes('//w:tcPrChange',$ns).Count
            $result.tblPr=$xml.SelectSingleNode('//w:tblPr',$ns).OuterXml
        } finally { if ($null -ne $zip) {$zip.Dispose()}; $stream.Dispose() }
    } catch { $result.error=$_.Exception.Message; $result.trace=$_.ScriptStackTrace }
    finally { if ($null -ne $doc) { $doc.Close(0) }; $results.Add($result) }
}
$results | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $OutputDir 'results.json') -Encoding UTF8
$results | ConvertTo-Json -Depth 10
