function Start-DCase {
    param([object]$Word,[ValidateSet('ink','comments')][string]$Case)
    . (Join-Path $PSScriptRoot 'task-a.ps1')
    if($Word.Documents.Count){throw 'Task D requires no other open task document.'}
    $Word.DisplayAlerts=0
    $Word.UserName=U '4F5C 8005 7532'
    $Word.UserInitials='A'
    $d=$Word.Documents.Add()
    $d.SetCompatibilityMode(15)
    $d.TrackRevisions=$false
    $d.Content.Font.Size=11
    if($Case -eq 'comments'){
        $d.Content.Text="before text`rFirst comment anchor.`rSecond comment anchor.`rThird comment anchor.`rafter text`r"
        foreach($n in 1,2,3){
            $text=@('First','Second','Third')[$n-1]+' comment anchor.'
            [void]$d.Comments.Add((FindA $d $text),('Root comment '+$n))
        }
        $d.ActiveWindow.View.ShowRevisionsAndComments=$true
    }else{
        $d.Content.Text="before text`r`r`r`r`r`r`r`r`r`r`rafter text`r"
    }
    $name=if($Case-eq'comments'){'comment-nesting-working.docx'}else{'ink-to-shape-working.docx'}
    $p=Join-Path 'C:\word\round3-work-20260907' $name
    if(Test-Path -LiteralPath $p){throw "Working path exists: $p"}
    Write-Host ('D_START_SAVE '+$p)
    $d.SaveAs2([string]$p,16)
    Write-Host ('D_START_SAVED '+$p)
    $d.Activate()
    $Word.DisplayAlerts=-1
    @{case=$Case;file=$p;compat=$d.CompatibilityMode;comments=$d.Comments.Count}|ConvertTo-Json
}
function Save-DCase {
    param([object]$Word,[ValidateSet('ink','comments')][string]$Case)
    . (Join-Path $PSScriptRoot 'task-a.ps1')
    $root=Split-Path $PSScriptRoot
    $rel=if($Case-eq'comments'){'comments2/comment-nesting.docx'}else{'ink2/ink-to-shape-2.docx'}
    $p=Join-Path $root $rel
    if(Test-Path -LiteralPath $p){throw "Output exists: $p"}
    [void][IO.Directory]::CreateDirectory((Split-Path $p))
    $d=$Word.ActiveDocument
    Write-Host ('D_FINAL_SAVE '+$p)
    $d.SaveAs2([string]$p,16)
    Write-Host ('D_FINAL_SAVED '+$p)
    $hash=SharedHash $p
    $r=@{file=$rel;sha256=$hash;compat=$d.CompatibilityMode;comments=@(foreach($c in $d.Comments){@{index=$c.Index;text=$c.Range.Text;done=$c.Done;ancestor=$(try{$c.Ancestor.Index}catch{$null});replies=$c.Replies.Count}});shapes=@(foreach($s in $d.Shapes){@{name=$s.Name;type=$s.Type;autoShapeType=$(try{$s.AutoShapeType}catch{$null})}})}
    $r|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $root ('_readouts/d-'+$Case+'.json')) -Encoding UTF8
    $Word.DisplayAlerts=0
    [string]$pdfPath=Join-Path $root ('_previews/d-'+$Case+'.pdf')
    $d.ExportAsFixedFormat($pdfPath,17)
    $d.Close(0)
    if((SharedHash $p)-ne$hash){throw 'D saved bytes changed after close.'}
    $r|ConvertTo-Json -Depth 8
}
