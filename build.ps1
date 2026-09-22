<#
.SYNOPSIS
    The one thing to run, from PowerShell: exactly what build.sh does, for a
    Windows where Docker Desktop is the only thing installed. Everything is
    built inside a container; what is built goes to output\, what is finished
    goes to release\.

.EXAMPLE
    .\build.ps1                             the two packs, in release\
    .\build.ps1 pack-switch                 one of them (or pack-emulator)
    .\build.ps1 plugins                     the example plugins
    .\build.ps1 -Romfs D:\romfs mod         the EnemyHp mod, in release\mods\
    .\build.ps1 test                        the tests of the crates
    .\build.ps1 doctor                      what the image has
    .\build.ps1 shell                       a shell inside the container
    .\build.ps1 clean                       empties output\ (-All: release\ too)
    .\build.ps1 -RebuildImage               build the image again
    .\build.ps1 -Run 'ls release'           run that in the container instead
#>
# PositionalBinding is off on purpose: without it, `.\build.ps1 clean` binds
# "clean" to the first declared parameter (-Romfs) instead of to the targets.
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Romfs,
    [switch]$Zip,
    [string]$TitleId,
    [switch]$RebuildImage,
    [switch]$NoDotnet,
    [switch]$All,
    [string]$Run,
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Targets
)

$ErrorActionPreference = 'Stop'
# docker's exit codes are read here, one by one; PowerShell 7.4 would otherwise
# turn the first non-zero one into an exception (the image not being there yet).
$PSNativeCommandUseErrorActionPreference = $false

$root = $PSScriptRoot
$image = if ($env:TOTK_IMAGE) { $env:TOTK_IMAGE } else { 'totk-various-poc:latest' }
$buildDir = 'output/build'
$first = if ($Targets -and $Targets.Count -gt 0) { $Targets[0] } else { '' }

# Emptying a directory needs no container.
if ($first -eq 'clean') {
    Remove-Item -Recurse -Force (Join-Path $root 'output') -ErrorAction SilentlyContinue
    Write-Host "output\ emptied"
    if ($All) {
        Remove-Item -Recurse -Force (Join-Path $root 'release') -ErrorAction SilentlyContinue
        Write-Host "release\ emptied"
    }
    exit 0
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker not found. Install Docker Desktop, then run this again."
}

# Forgetting `git submodule update --init` fails much further on, inside CMake,
# with a message about borealis that says nothing about submodules.
$borealis = Join-Path $root 'totk-mod-manager\library\borealis\library\CMakeLists.txt'
if (-not (Test-Path $borealis) -and (Test-Path (Join-Path $root '.git'))) {
    Write-Host "== the submodules are empty: git submodule update --init"
    git -C $root submodule update --init
}

docker image inspect $image *> $null
if ($RebuildImage -or $LASTEXITCODE -ne 0) {
    Write-Host "== building the image $image (once; a few minutes)"
    $withDotnet = if ($NoDotnet) { '0' } else { '1' }
    docker build --build-arg "WITH_DOTNET=$withDotnet" -t $image `
        -f (Join-Path $root 'Dockerfile') $root
    if ($LASTEXITCODE -ne 0) { throw "the image could not be built" }
}

# The romfs stays where it is and is mounted read-only: it is tens of
# gigabytes of game files, and the build only reads a handful of them.
$mounts = @('-v', "${root}:/work")
$setupOptions = @()
if ($Romfs) {
    if (-not (Test-Path -PathType Container $Romfs)) {
        throw "-Romfs $Romfs is not a directory."
    }
    $romfsPath = (Resolve-Path $Romfs).Path
    $mounts += @('-v', "${romfsPath}:/romfs:ro")
    $setupOptions += '-Dtotk_romfs=/romfs'
}
if ($Zip) { $setupOptions += '-Dpack_zip=true' }
if ($TitleId) { $setupOptions += "-Dtitle_id=$TitleId" }

# `meson setup` on a directory that is already configured exits 0 and ignores
# the options it was given, so past the first run the options go to
# `meson configure` instead.
$options = $setupOptions -join ' '
$configure = "if [ -e $buildDir/meson-info/meson-info.json ]; " +
             "then meson configure $options $buildDir >/dev/null; " +
             "else meson setup $options $buildDir >/dev/null; fi"

if ($Run) {
    $command = @('bash', '-lc', $Run)
} elseif ($first -eq 'shell') {
    $command = @('bash', '-l')
} elseif ($first -eq 'doctor') {
    # Straight to the script: it is about the image, and it has to work even
    # when meson does not.
    $command = @('bash', '-lc', 'bash utils/doctor.sh')
} elseif ($first -eq 'test') {
    $command = @('bash', '-lc', "$configure; meson test -C $buildDir --print-errorlogs")
} else {
    if (-not $Targets -or $Targets.Count -eq 0) { $Targets = @('packs') }
    $command = @('bash', '-lc', "$configure; meson compile -C $buildDir $($Targets -join ' ')")
}

# A terminal when there is one (a shell needs it), plain output otherwise.
$terminal = if ([Console]::IsInputRedirected -or [Console]::IsOutputRedirected) { '-i' } else { '-it' }

& docker run --rm $terminal @mounts -w /work $image @command
exit $LASTEXITCODE
