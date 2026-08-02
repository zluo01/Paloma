function Emit($o) {
    $json = ConvertTo-Json -InputObject $o -Depth 5 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes([string]$json)
    $stdout = [Console]::OpenStandardOutput()
    $stdout.Write($bytes, 0, $bytes.Length)
    $stdout.Flush()
}
function Fail { Emit @{ ok = $false }; exit 0 }
$ErrorActionPreference = 'Stop'
try {
    $stdin = [Console]::OpenStandardInput()
    $buffer = New-Object System.IO.MemoryStream
    $stdin.CopyTo($buffer)
    $src = [System.Text.Encoding]::UTF8.GetString($buffer.ToArray())
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($src, [ref]$tokens, [ref]$errors)
    if ($errors.Count -gt 0) { Fail }
    $allowed = 'ScriptBlockAst', 'NamedBlockAst', 'PipelineAst', 'CommandAst',
        'StringConstantExpressionAst', 'CommandParameterAst', 'ConstantExpressionAst'
    foreach ($node in $ast.FindAll({ $true }, $true)) {
        if ($allowed -notcontains $node.GetType().Name) { Fail }
    }
    if ($ast.ParamBlock -or $ast.BeginBlock -or $ast.ProcessBlock -or -not $ast.EndBlock) { Fail }
    $commands = @()
    foreach ($stmt in $ast.EndBlock.Statements) {
        if ($stmt.GetType().Name -ne 'PipelineAst') { Fail }
        foreach ($cmd in $stmt.PipelineElements) {
            if ($cmd.GetType().Name -ne 'CommandAst') { Fail }
            if ($cmd.InvocationOperator -ne 'Unknown') { Fail }
            if ($cmd.Redirections.Count -gt 0) { Fail }
            $words = @()
            foreach ($element in $cmd.CommandElements) {
                if ($element.Extent.Text -ceq '--%') { Fail }
                switch ($element.GetType().Name) {
                    'StringConstantExpressionAst' { $words += [string]$element.Value }
                    # Numeric literal: emit the source spelling, not the evaluated
                    # value — 0x10/007/1e3/10kb must survive verbatim.
                    'ConstantExpressionAst' { $words += $element.Extent.Text }
                    'CommandParameterAst' {
                        if ($null -ne $element.Argument) { Fail }
                        $words += $element.Extent.Text
                    }
                    default { Fail }
                }
            }
            if ($words.Count -eq 0) { Fail }
            $commands += , $words
        }
    }
    if ($commands.Count -eq 0) { Fail }
    Emit @{ ok = $true; commands = $commands }
} catch {
    Emit @{ ok = $false }
}
