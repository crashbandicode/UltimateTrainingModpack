# Host before/after renderer benchmark. Does not build the product crate.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

$Repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$Rustc = '/home/intpa/eden-gnw/tools/rustup/toolchains/nightly-2026-02-14-x86_64-unknown-linux-gnu/bin/rustc'
$Work = '/tmp/input-display-bench'
$Build = Join-Path $Work 'build'
$Bench = Join-Path $Repo 'scripts/input-display-bench'
$Baseline = Join-Path $Bench 'fixtures/baseline-input_log.rs'
$CandidateSrc = Join-Path $Repo 'src/training/ui/input_log.rs'
$Candidate = Join-Path $Work 'candidate-input_log.rs'
$ExpectedBlob = '9c1bcc221da0be1faf9d429ceeec4db090de0f09'

New-Item -ItemType Directory -Force -Path $Build | Out-Null
if (-not (Test-Path -LiteralPath $Rustc)) {
    throw "Pinned rustc is missing: $Rustc"
}

& /usr/bin/python3 -c @"
import hashlib, pathlib, sys
data = pathlib.Path('$Baseline').read_bytes()
digest = hashlib.sha1(b'blob %d\0' % len(data) + data).hexdigest()
print('baseline_bytes', len(data))
print('baseline_blob', digest)
if digest != '$ExpectedBlob':
    sys.exit(1)
"@
if ($LASTEXITCODE -ne 0) {
    throw 'Baseline fixture does not match the frozen git blob'
}

$Shim = Join-Path $Bench 'rust/shims'
& $Rustc --edition 2018 --crate-type rlib --crate-name training_mod_sync -C opt-level=3 -C debuginfo=0 -o (Join-Path $Build 'libtraining_mod_sync.rlib') (Join-Path $Shim 'training_mod_sync.rs')
if ($LASTEXITCODE -ne 0) { throw 'training_mod_sync shim failed' }
& $Rustc --edition 2018 --crate-type rlib --crate-name training_mod_consts --extern "training_mod_sync=$(Join-Path $Build 'libtraining_mod_sync.rlib')" -C opt-level=3 -C debuginfo=0 -o (Join-Path $Build 'libtraining_mod_consts.rlib') (Join-Path $Shim 'training_mod_consts.rs')
if ($LASTEXITCODE -ne 0) { throw 'training_mod_consts shim failed' }
& $Rustc --edition 2018 --crate-type rlib --crate-name skyline -C opt-level=3 -C debuginfo=0 -o (Join-Path $Build 'libskyline.rlib') (Join-Path $Shim 'skyline.rs')
if ($LASTEXITCODE -ne 0) { throw 'skyline shim failed' }
& $Rustc --edition 2018 --crate-type rlib --crate-name smash --extern "skyline=$(Join-Path $Build 'libskyline.rlib')" -C opt-level=3 -C debuginfo=0 -o (Join-Path $Build 'libsmash.rlib') (Join-Path $Shim 'smash.rs')
if ($LASTEXITCODE -ne 0) { throw 'smash shim failed' }

function Build-Bench([string]$Renderer, [string]$Label, [string]$Cfg, [string]$Output, [string]$Log) {
    $env:INPUT_DISPLAY_RENDERER = $Renderer
    $env:INPUT_DISPLAY_SOURCE_LABEL = $Label
    $RustcArgs = @(
        '--edition', '2018',
        '--crate-type', 'bin',
        '--crate-name', 'input_display_bench',
        '--cfg', $Cfg,
        '-L', "dependency=$Build",
        '--extern', "skyline=$(Join-Path $Build 'libskyline.rlib')",
        '--extern', "smash=$(Join-Path $Build 'libsmash.rlib')",
        '--extern', "training_mod_sync=$(Join-Path $Build 'libtraining_mod_sync.rlib')",
        '--extern', "training_mod_consts=$(Join-Path $Build 'libtraining_mod_consts.rlib')",
        '-C', 'opt-level=3',
        '-C', 'debuginfo=0',
        '-A', 'unused',
        '-o', $Output,
        (Join-Path $Bench 'rust/harness.rs')
    )
    [IO.File]::WriteAllLines((Join-Path $Work 'last-rustc-args.txt'), $RustcArgs)
    if ($Log) {
        & $Rustc @RustcArgs 2>&1 | Out-File -FilePath $Log -Encoding utf8NoBOM
    } else {
        & $Rustc @RustcArgs
    }
    return $LASTEXITCODE
}

$BaselineBin = Join-Path $Build 'bench-baseline'
$BaselineCode = Build-Bench $Baseline 'baseline-7.1.2' 'bench_baseline' $BaselineBin
if ($BaselineCode -ne 0) { throw 'baseline harness failed to compile' }

[IO.File]::Copy($CandidateSrc, $Candidate, $true)
$CandidateBin = Join-Path $Build 'bench-candidate'
$CandidateLog = Join-Path $Work 'candidate-build.log'
$CandidateCode = Build-Bench $Candidate 'candidate-worktree' 'bench_candidate' $CandidateBin $CandidateLog

& $BaselineBin | Out-File -FilePath (Join-Path $Work 'baseline.txt') -Encoding utf8NoBOM
if ($LASTEXITCODE -ne 0) { throw 'baseline bench failed' }
& $BaselineBin | Out-File -FilePath (Join-Path $Work 'baseline-repeat.txt') -Encoding utf8NoBOM
if ($LASTEXITCODE -ne 0) { throw 'baseline repeat failed' }

$CandidateOut = Join-Path $Work 'candidate.txt'
if ($CandidateCode -eq 0) {
    & $CandidateBin | Out-File -FilePath $CandidateOut -Encoding utf8NoBOM
    if ($LASTEXITCODE -ne 0) {
        'candidate run failed' | Out-File -FilePath $CandidateOut -Encoding utf8NoBOM
    }
} else {
    'candidate compile failed' | Out-File -FilePath $CandidateOut -Encoding utf8NoBOM
}

& /usr/bin/python3 (Join-Path $Bench 'compare.py') `
    (Join-Path $Work 'baseline.txt') `
    (Join-Path $Work 'baseline-repeat.txt') `
    $CandidateOut `
    (Join-Path $Work 'report.md')
exit $LASTEXITCODE
