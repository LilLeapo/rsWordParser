# Definitions only: invoke New-RevisionStage with the existing task-owned Word instance.
function U([string]$Hex){-join @($Hex.Split(' ') | ForEach-Object{[char][Convert]::ToInt32($_,16)})}
function SharedHash([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $sha=[Security.Cryptography.SHA256]::Create()
    try{[BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','')}finally{$sha.Dispose();$stream.Dispose()}
}
function FindA($Document,[string]$Text){
    $range=$Document.Content.Duplicate
    $find=$range.Find
    $find.ClearFormatting();$find.MatchCase=$true;$find.MatchWholeWord=$false;$find.MatchWildcards=$false;$find.MatchSoundsLike=$false;$find.MatchAllWordForms=$false;$find.Forward=$true;$find.Wrap=0;$find.Format=$false
    if(-not $find.Execute($Text)){throw "Text not found: $Text"}
    return $range
}
function ReadA($Document){
    $revs=@(foreach($rev in $Document.Revisions){[ordered]@{type=[int]$rev.Type;author=[string]$rev.Author;text=[string]$rev.Range.Text}})
    $sections=@(foreach($section in $Document.Sections){[ordered]@{index=$section.Index;start=$section.Range.Start;end=$section.Range.End;orientation=$section.PageSetup.Orientation;sectionStart=$section.PageSetup.SectionStart}})
    $shapes=@(foreach($shape in $Document.Shapes){[ordered]@{name=$shape.Name;left=$shape.Left;top=$shape.Top;width=$shape.Width;height=$shape.Height;relativeHorizontal=$shape.RelativeHorizontalPosition;relativeVertical=$shape.RelativeVerticalPosition;zOrder=$shape.ZOrderPosition;wrap=$shape.WrapFormat.Type}})
    $paragraphs=@(foreach($paragraph in $Document.Paragraphs){[ordered]@{text=$paragraph.Range.Text;alignment=$paragraph.Alignment;firstLineIndent=$paragraph.FirstLineIndent;characterFirstLineIndent=$paragraph.CharacterUnitFirstLineIndent}})
    return [ordered]@{text=[string]$Document.Content.Text;paragraphs=$paragraphs;revisions=$revs;revisionCount=$Document.Revisions.Count;sections=$sections;shapes=$shapes;pages=$Document.ComputeStatistics(2);compatibility=$Document.CompatibilityMode;trackMoves=$Document.TrackMoves;trackFormatting=$Document.TrackFormatting;trackRevisions=$Document.TrackRevisions}
}
function Close-AReviewed($Word,[string]$Root='C:\word\real-word-round3-20260907'){
    if($Word.Documents.Count -eq 0){return}
    $doc=$Word.ActiveDocument
    $path=[string]$doc.FullName
    if(-not $path.StartsWith($Root+'\revfix\',[StringComparison]::OrdinalIgnoreCase)){throw "Unexpected active document: $path"}
    $hash=SharedHash $path
    $doc.Close(0)
    if((SharedHash $path)-ne $hash){throw 'Closing changed saved DOCX.'}
}
function New-RevisionStage {
    param([Parameter(Mandatory=$true)][object]$Word,[Parameter(Mandatory=$true)][string]$Case,[Parameter(Mandatory=$true)][ValidateSet('base','tracked','accepted','rejected','before','after')][string]$Stage,[string]$Root='C:\word\real-word-round3-20260907')
    $ErrorActionPreference='Stop'
    Close-AReviewed $Word $Root
    [string]$path=Join-Path $Root ('revfix/'+$Case+'/'+$Stage+'.docx')
    if(Test-Path -LiteralPath $path){throw "Refusing to overwrite saved document: $path"}
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $path))
    [string]$before='before '+(U '524D 6587');[string]$after='after '+(U '540E 6587')
    [string]$first=U '7B2C 4E00 53E5 539F 6587 3002';[string]$second=U '7B2C 4E8C 53E5 539F 6587 3002';[string]$third=U '7B2C 4E09 53E5 539F 6587 3002';[string]$added=U '65B0 589E 7684 4E00 53E5 3002'
    [string]$paraA=U '7532 6BB5 843D 7684 6587 5B57 3002';[string]$paraB=U '4E59 6BB5 843D 7684 6587 5B57 3002';[string]$move=U '53EF 79FB 52A8 6BB5 843D 3002';[string]$original=U '539F 59CB 6B63 6587 3002'
    [string]$sectOne=U '7B2C 4E00 8282 6B63 6587 3002';[string]$sectTwo=U '7B2C 4E8C 8282 6B63 6587 3002'
    $Word.UserName=[string](U '4F5C 8005 7532');$Word.UserInitials='A';$Word.DisplayAlerts=0
    $operations=[Collections.Generic.List[string]]::new()
    $sourcePath=$null;$sourceHash=$null
    if($Stage -in 'base','before'){
        $doc=$Word.Documents.Add();$doc.SetCompatibilityMode(15);$doc.TrackRevisions=$false;$doc.Content.Font.Size=11
        switch($Case){
            'run-edits'{$doc.Content.Text="$before`r$first$second$third`r$after`r"}
            'para-split-merge'{$doc.Content.Text="$before`r$paraA`r$paraB`r$after`r"}
            'table-and-move'{
                $doc.Content.Text="$before`rTABLE`r$move`r$after`r"
                $r=FindA $doc 'TABLE';$r.Text='';$table=$doc.Tables.Add($r,3,2);$table.Borders.Enable=1
                foreach($row in 1,2,3){$table.Cell($row,1).Range.Text='A'+$row;$table.Cell($row,2).Range.Text='B'+$row}
            }
            'tracked-two-authors'{$doc.Content.Text="$before`r$original`r$after`r"}
            {$_ -in 'sect-insert','sect-delete'}{
                $doc.Content.Text="$before`r$sectOne`r$sectTwo`r$after`r"
                if($Case -eq 'sect-delete'){$r=FindA $doc $sectTwo;$r.Collapse(1);$r.InsertBreak(2);$doc.Sections.Item(2).PageSetup.Orientation=1}
            }
            {$_ -in 'z-order','move-resize'}{
                $doc.Content.Text="$before`rPicture anchor.`r$after`r"
                $r=FindA $doc 'Picture anchor.';$r.Collapse(1)
                [string]$asset=Join-Path $Root '_scripts/task-a-picture.png'
                if(-not(Test-Path -LiteralPath $asset)){
                    Add-Type -AssemblyName System.Drawing
                    $bitmap=[Drawing.Bitmap]::new(160,80);$graphics=[Drawing.Graphics]::FromImage($bitmap)
                    try{$graphics.Clear([Drawing.Color]::Teal);$graphics.FillRectangle([Drawing.Brushes]::Gold,80,0,80,80);$graphics.DrawRectangle([Drawing.Pens]::Black,1,1,157,77);$bitmap.Save($asset,[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
                }
                $count=if($Case -eq 'z-order'){3}else{1}
                foreach($index in 1..$count){
                    $shape=$doc.Shapes.AddPicture($asset,$false,$true,0,0,120,60,$r)
                    $shape.Name='Round3 Picture '+$index;$shape.RelativeHorizontalPosition=0;$shape.RelativeVerticalPosition=2;$shape.Left=($index-1)*38;$shape.Top=25+($index-1)*25;$shape.WrapFormat.Type=0;$shape.LockAnchor=-1
                }
                $r.ParagraphFormat.SpaceAfter=210
            }
            default{throw "Unknown case: $Case"}
        }
        $operations.Add('Created required baseline with TrackRevisions=false; saved once.')
    }else{
        $sourceName=if($Stage -in 'accepted','rejected'){'tracked'}elseif($Stage-eq'tracked'){'base'}else{'before'}
        [string]$sourcePath=Join-Path $Root ('revfix/'+$Case+'/'+$sourceName+'.docx');$sourceHash=SharedHash $sourcePath
        $doc=$Word.Documents.Open($sourcePath,$false,$false,$false)
        $operations.Add('Opened unchanged '+$sourceName+'.docx independently.')
        if($Stage -in 'accepted','rejected'){
            if($Stage -eq 'accepted'){$doc.AcceptAllRevisions();$operations.Add('Word Document.AcceptAllRevisions; not Undo-based.')}
            else{$doc.RejectAllRevisions();$operations.Add('Word Document.RejectAllRevisions from the same tracked file; not Undo-based.')}
        }elseif($Stage -eq 'tracked'){
            $doc.TrackFormatting=$true;$doc.TrackMoves=$true;$doc.TrackRevisions=$true
            switch($Case){
                'run-edits'{
                    (FindA $doc $first).InsertAfter($added);$operations.Add('Inserted requested sentence after first sentence.')
                    [void](FindA $doc $second).Delete();$operations.Add('Deleted second original sentence.')
                    $r=FindA $doc (U '7B2C 4E09 53E5');$r.Font.Bold=-1;$r.Font.Color=255;$operations.Add('Set three characters of third sentence bold and red.')
                }
                'para-split-merge'{
                    $r=FindA $doc $paraA;$p=$r.Start+2;$doc.Range($p,$p).InsertBefore("`r");$operations.Add('Inserted paragraph break after first two characters of paragraph A.')
                    $r=(FindA $doc $paraB).Paragraphs.Item(1).Range;$doc.Range($r.End-1,$r.End).Delete() | Out-Null;$operations.Add('Deleted paragraph mark after paragraph B, merging it with after paragraph.')
                    $r=FindA $doc (U '843D 7684 6587 5B57 3002');$r.ParagraphFormat.Alignment=1;$r.ParagraphFormat.CharacterUnitFirstLineIndent=2;$operations.Add('Centered second split paragraph with 2-character first-line indent.')
                }
                'table-and-move'{
                    $table=$doc.Tables.Item(1);$row=$table.Rows.Add($table.Rows.Item(2));$row.Cells.Item(1).Range.Text='Inserted A';$row.Cells.Item(2).Range.Text='Inserted B';$operations.Add('Inserted row before original row 2.')
                    (FindA $doc 'A3').Rows.Item(1).Delete();$operations.Add('Deleted original final row.')
                    $table.Cell(1,1).Merge($table.Cell(1,2));$operations.Add('Merged original first row cells.')
                    $r=(FindA $doc $move).Paragraphs.Item(1).Range.Duplicate;$r.Cut();$target=FindA $doc $after;$target.Collapse(1);$target.Paste();$operations.Add('TrackMoves=true; cut whole moving paragraph and pasted immediately before after, its literal original location. Package check determines whether Word recorded a move.')
                }
                'tracked-two-authors'{
                    (FindA $doc $original).InsertAfter((U '4F5C 8005 7532 63D2 5165 7684 53E5 5B50 3002'));$operations.Add('Author A inserted a sentence.')
                    $Word.UserName=[string](U '4F5C 8005 4E59');$Word.UserInitials='B'
                    (FindA $doc $original).InsertBefore((U '4F5C 8005 4E59 63D2 5165 7684 53E5 5B50 3002'));[void](FindA $doc (U '6B63 6587 3002')).Delete();$operations.Add('Author B inserted a separate sentence before original text and deleted its latter part (body text and punctuation).')
                }
            }
        }else{
            $doc.TrackRevisions=$false
            switch($Case){
                'sect-insert'{$r=FindA $doc $sectTwo;$r.Collapse(1);$r.InsertBreak(2);$doc.Sections.Item(2).PageSetup.Orientation=1;$operations.Add('Inserted next-page section break before second section text; made new second section landscape.')}
                'sect-delete'{$end=$doc.Sections.Item(1).Range.End;$doc.Range($end-1,$end).Delete() | Out-Null;$operations.Add('Deleted the section-break character; Word determines inherited page setup.')}
                'z-order'{$shapes=@(foreach($s in $doc.Shapes){$s});$bottom=$shapes | Sort-Object ZOrderPosition | Select-Object -First 1;$name=$bottom.Name;$bottom.ZOrder(0);$operations.Add('Brought lowest floating picture to front: '+$name)}
                'move-resize'{$shape=$doc.Shapes.Item(1);$shape.Left=[single]([double]$shape.Left+56.692913);$shape.Top=[single]([double]$shape.Top+56.692913);$shape.LockAspectRatio=-1;$shape.Width=[single]([double]$shape.Width/2);$operations.Add('Moved native floating picture 2cm right/down and halved width with aspect ratio locked through Word shape properties; precise COM equivalent, not a mouse drag.')}
            }
        }
    }
    $doc.Repaginate();$doc.Activate();$doc.ActiveWindow.View.Type=3;$doc.ActiveWindow.View.Zoom.Percentage=90
    $doc.ActiveWindow.View.ShowRevisionsAndComments=$true;$doc.ActiveWindow.View.RevisionsView=0
    $doc.Range(0,0).Select();$Word.ScreenRefresh()
    $readout=ReadA $doc
    $doc.SaveAs2($path,12);$hash=SharedHash $path
    [string]$pdf=Join-Path $Root ('_previews/revfix/'+$Case+'/'+$Stage+'.pdf');[void][IO.Directory]::CreateDirectory((Split-Path -Parent $pdf))
    $doc.ExportAsFixedFormat($pdf,17,$false)
    if((SharedHash $path)-ne$hash){throw 'DOCX changed after first save.'}
    if($sourcePath -and (SharedHash $sourcePath)-ne$sourceHash){throw 'Source DOCX was changed.'}
    $entry=[ordered]@{case=$Case;stage=$Stage;path=$path;sha256=$hash;sourcePath=$sourcePath;sourceSha256=$sourceHash;operations=@($operations.ToArray());readout=$readout;pdf=$pdf;date=(Get-Date).ToString('o');method='Native Windows Word COM; DOCX saved once at this path; PDF export and current-document UI inspection afterward without save back.';uiPending=$true}
    [string]$result=Join-Path $Root ('_readouts/a-'+$Case+'-'+$Stage+'.json')
    $entry | ConvertTo-Json -Depth 35 | Set-Content -LiteralPath $result -Encoding UTF8
    $Word.ScreenRefresh()
    @{case=$Case;stage=$Stage;path=$path;sha256=$hash;revisions=$readout.revisionCount;text=$readout.text;pages=$readout.pages} | ConvertTo-Json -Depth 6
}
