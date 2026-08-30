# Build paloma-ffi and generate the C# bindings the Windows app compiles
# against. Runs standalone or as a pre-build step.
#
#   scripts/build-cs-bindings.ps1
#
# Outputs:
#   target/cs/    generated C# bindings next to the paloma_ffi.dll they load
Set-Location (Join-Path $PSScriptRoot '..') -ErrorAction Stop

$lib = 'target/release/paloma_ffi.dll'
$out = 'target/cs'

cargo build -p paloma-ffi --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (Test-Path $out) { Remove-Item -Recurse -Force $out -ErrorAction Stop }
New-Item -ItemType Directory -Force $out -ErrorAction Stop | Out-Null

cargo run --quiet -p paloma-uniffi-bindgen-cs --bin uniffi-bindgen-cs -- --library $lib --out-dir $out
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# The generated `Action` record shadows System.Action in the generator's own
# helper; its rename pass breaks type references, so qualify it here instead.
$bindings = Get-ChildItem $out -Filter *.cs -ErrorAction Stop | Select-Object -First 1
(Get-Content $bindings.FullName -Raw) -replace 'InvokeCallbackOnce\(Action invoke\)', 'InvokeCallbackOnce(System.Action invoke)' |
    Set-Content $bindings.FullName -Encoding utf8 -NoNewline -ErrorAction Stop

Copy-Item $lib $out -ErrorAction Stop

"bindings:  $out/"
Get-ChildItem $out -Filter *.cs | ForEach-Object { "           $($_.Name)" }
"library:   $lib"
