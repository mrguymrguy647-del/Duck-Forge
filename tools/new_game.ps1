# The text edits for tools\new_game.bat (Windows): batch files can't
# safely replace whole words in files, but PowerShell, built into every
# Windows, can. It does what tools/new_game.sh does between copying the
# starter and verifying the new game. Run it through new_game.bat.

param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$Kind,
    [Parameter(Mandatory = $true)][string]$Starter,
    [Parameter(Mandatory = $true)][string]$Type,
    [string]$Title = ""
)
$ErrorActionPreference = "Stop"
$root = (Get-Location).Path

$folder = Join-Path $root "examples\$Name"
if (Test-Path (Join-Path $folder "main.rs")) {
    $files = Get-ChildItem -Path $folder -Filter *.rs | ForEach-Object { $_.FullName }
    $main = Join-Path $folder "main.rs"
} else {
    $main = Join-Path $root "examples\$Name.rs"
    $files = @($main)
}

# bee_maze -> BeeMaze (BeeMazeGame if the starter already uses that word).
$newType = -join ($Name.Split("_") | Where-Object { $_ } | ForEach-Object { $_.Substring(0, 1).ToUpper() + $_.Substring(1) })
$word = { param($w) "(?<![A-Za-z0-9_])" + [regex]::Escape($w) + "(?![A-Za-z0-9_])" }
foreach ($file in $files) {
    if ([IO.File]::ReadAllText($file) -cmatch (& $word $newType)) { $newType = "${newType}Game"; break }
}
$pretty = $Name.Replace("_", " ")
$pretty = $pretty.Substring(0, 1).ToUpper() + $pretty.Substring(1)

foreach ($file in $files) {
    $text = [IO.File]::ReadAllText($file)
    # The title first: it can contain the type's name ("Template - duckforge").
    if ($Title) { $text = $text.Replace("""$Title""", """$pretty - duckforge""") }
    $text = $text -creplace (& $word $Type), $newType
    $text = $text -creplace (& $word $Starter), $Name
    if ($file -eq $main) {
        $text = "//! ${pretty}: started from the $Kind starter with tools/new_game.`n//!`n" + $text
    }
    # UTF-8 without a byte order mark, like the files it came from.
    [IO.File]::WriteAllText($file, $text)
}

[IO.File]::AppendAllText((Join-Path $root "Cargo.toml"), "`n[[example]]`nname = ""$Name""`ntest = true`n")
Write-Output "new_game: made examples\$Name from the $Kind starter ($Starter), with the type $newType"
