# Records which Windows.Gaming.Input button / switch each control of the Cammus C5 rim
# (and base) reports. Walks through the numbered labels of the rim drawing in order;
# press the named control, or press Enter to skip a step that does not exist.
# Output: a table on screen and capture_buttons.csv next to this script.
#
# Run in Windows PowerShell 5.1 with FH6 closed:
#   powershell -NoProfile -ExecutionPolicy Bypass -File capture_buttons.ps1

$null = [Windows.Gaming.Input.RawGameController, Windows.Gaming.Input, ContentType = WindowsRuntime]

$controllers = @()
for ($t = 0; $t -lt 20 -and $controllers.Count -eq 0; $t++) {
    Start-Sleep -Milliseconds 500
    $controllers = @([Windows.Gaming.Input.RawGameController]::RawGameControllers)
}
if ($controllers.Count -eq 0) { Write-Host "No controllers found."; exit 1 }

# Per controller: reading buffers and the previous state.
$devs = foreach ($c in $controllers) {
    [pscustomobject]@{
        Name   = ("0x{0:x4}{1:x4}" -f $c.HardwareVendorId, $c.HardwareProductId)
        Ctrl   = $c
        B      = New-Object bool[] $c.ButtonCount
        S      = New-Object 'Windows.Gaming.Input.GameControllerSwitchPosition[]' $c.SwitchCount
        A      = New-Object double[] $c.AxisCount
        PrevB  = New-Object bool[] $c.ButtonCount
        PrevS  = @()
    }
}
foreach ($d in $devs) {
    $null = $d.Ctrl.GetCurrentReading($d.B, $d.S, $d.A)
    [Array]::Copy($d.B, $d.PrevB, $d.B.Count)
    $d.PrevS = @($d.S | ForEach-Object { "$_" })
    Write-Host ("found {0}: {1} buttons, {2} switches, {3} axes" -f $d.Name, $d.B.Count, $d.S.Count, $d.A.Count)
}

# Steps follow the drawing: number, position, what to do.
$steps = @(
    '1  | left paddle',
    '2  | right paddle',
    '3  | top left button',
    '7  | left button below 3',
    '4  | left D-pad: left',
    '5a | left D-pad: up',
    '5b | left D-pad: right',
    '6  | left D-pad: down',
    'L# | left D-pad: centre square (press)',
    '8  | centre top left',
    '11 | centre top right',
    '13 | centre bottom left',
    '14a| centre bottom right',
    '9  | left side, below D-pad',
    '10a| left side, bottom',
    '17 | big left knob/button: PRESS',
    '17L| big left knob: turn LEFT (one click)',
    '17R| big left knob: turn RIGHT (one click)',
    '18 | big bottom knob/button: PRESS',
    '18L| big bottom knob: turn LEFT (one click)',
    '18R| big bottom knob: turn RIGHT (one click)',
    '19 | big right knob/button: PRESS',
    '19L| big right knob: turn LEFT (one click)',
    '19R| big right knob: turn RIGHT (one click)',
    '20 | top right button',
    '10b| right button below 20',
    '21 | right D-pad: up',
    '22 | right D-pad: left',
    '23 | right D-pad: right',
    '14b| right D-pad: down',
    'R# | right D-pad: centre square (press)',
    '15 | right side, below D-pad',
    '16 | right side, bottom'
)

function Read-Events {
    $events = @()
    foreach ($d in $devs) {
        $null = $d.Ctrl.GetCurrentReading($d.B, $d.S, $d.A)
        for ($i = 0; $i -lt $d.B.Count; $i++) {
            if ($d.B[$i] -and -not $d.PrevB[$i]) { $events += "$($d.Name) button $i" }
        }
        $now = @($d.S | ForEach-Object { "$_" })
        for ($i = 0; $i -lt $now.Count; $i++) {
            if ($now[$i] -ne $d.PrevS[$i] -and $now[$i] -ne 'Center') { $events += "$($d.Name) switch $i $($now[$i])" }
        }
        [Array]::Copy($d.B, $d.PrevB, $d.B.Count)
        $d.PrevS = $now
    }
    $events
}

$results = @()
Write-Host ""
Write-Host "For each step: press the control once. Enter = skip. Ctrl+C = abort."
foreach ($step in $steps) {
    $label, $what = $step -split '\|', 2
    $label = $label.Trim(); $what = $what.Trim()
    Write-Host -NoNewline ("[{0,-3}] {1,-40} -> " -f $label, $what)
    $null = Read-Events   # discard anything pending
    $hit = $null
    while (-not $hit) {
        if ([Console]::KeyAvailable -and [Console]::ReadKey($true).Key -eq 'Enter') { $hit = 'skipped'; break }
        $ev = @(Read-Events)
        if ($ev.Count -gt 0) { $hit = $ev -join ' + ' }
        Start-Sleep -Milliseconds 15
    }
    Write-Host $hit
    $results += [pscustomobject]@{ Label = $label; Control = $what; Reported = $hit }
    # wait until everything is released again, so one press is not counted twice
    $deadline = (Get-Date).AddSeconds(3)
    do { Start-Sleep -Milliseconds 30; $null = Read-Events; $held = $devs | Where-Object { $_.B -contains $true } } while ($held -and (Get-Date) -lt $deadline)
}

$out = Join-Path $PSScriptRoot 'capture_buttons.csv'
$results | Export-Csv -NoTypeInformation -Encoding UTF8 $out
Write-Host ""
$results | Format-Table -AutoSize
Write-Host "Saved to $out"
