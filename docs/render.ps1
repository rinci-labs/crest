# Builds the README images from the renderer's preview frames.
#   $env:CREST_PREVIEW_RAW = 1; cargo test preview_states
#   powershell -File docs\render.ps1
Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$preview = Join-Path $root 'target\preview'

function Load-Frame([string]$name) {
    $bytes = [System.IO.File]::ReadAllBytes((Join-Path $preview "$name.raw"))
    # Frame size comes from the matching BMP header (width at 18, negative height at 22).
    $header = [System.IO.File]::ReadAllBytes((Join-Path $preview "$name.bmp"))[0..25]
    $W = [BitConverter]::ToInt32($header, 18); $H = -[BitConverter]::ToInt32($header, 22)
    $bitmap = [System.Drawing.Bitmap]::new($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppPArgb)
    $data = $bitmap.LockBits([System.Drawing.Rectangle]::new(0, 0, $W, $H),
        [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bitmap.PixelFormat)
    for ($row = 0; $row -lt $H; $row++) {
        [System.Runtime.InteropServices.Marshal]::Copy($bytes, $row * $W * 4, [IntPtr]($data.Scan0.ToInt64() + $row * $data.Stride), $W * 4)
    }
    $bitmap.UnlockBits($data)
    # Trim to the drawn area (shape plus shadow).
    $left = $W; $right = 0; $bottom = 0
    for ($y = 0; $y -lt $H; $y++) {
        for ($x = 0; $x -lt $W; $x += 2) {
            if ($bytes[($y * $W + $x) * 4 + 3] -gt 6) {
                if ($x -lt $left) { $left = $x }; if ($x -gt $right) { $right = $x }; $bottom = $y
            }
        }
    }
    $crop = [System.Drawing.Rectangle]::new($left, 0, $right - $left + 2, $bottom + 1)
    $trimmed = $bitmap.Clone($crop, [System.Drawing.Imaging.PixelFormat]::Format32bppPArgb)
    $bitmap.Dispose()
    return $trimmed
}

function New-Canvas([int]$w, [int]$h, [string]$top, [string]$bottom) {
    $canvas = [System.Drawing.Bitmap]::new($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($canvas)
    $g.SmoothingMode = 'AntiAlias'; $g.TextRenderingHint = 'AntiAliasGridFit'; $g.InterpolationMode = 'HighQualityBicubic'
    $brush = [System.Drawing.Drawing2D.LinearGradientBrush]::new([System.Drawing.Point]::new(0, 0), [System.Drawing.Point]::new(0, $h),
        [System.Drawing.ColorTranslator]::FromHtml($top), [System.Drawing.ColorTranslator]::FromHtml($bottom))
    $g.FillRectangle($brush, 0, 0, $w, $h); $brush.Dispose()
    return @($canvas, $g)
}

function Draw-Text($g, [string]$text, [float]$size, [string]$style, [string]$color, [float]$x, [float]$y, [bool]$center) {
    $font = [System.Drawing.Font]::new('Segoe UI Variable Display', $size, [System.Drawing.FontStyle]$style, [System.Drawing.GraphicsUnit]::Pixel)
    if ($font.Name -ne 'Segoe UI Variable Display') { $font = [System.Drawing.Font]::new('Segoe UI', $size, [System.Drawing.FontStyle]$style, [System.Drawing.GraphicsUnit]::Pixel) }
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($color))
    $format = [System.Drawing.StringFormat]::new(); if ($center) { $format.Alignment = 'Center' }
    $g.DrawString($text, $font, $brush, $x, $y, $format)
    $font.Dispose(); $brush.Dispose()
}

function Save($canvas, $g, [string]$name) {
    $g.Dispose(); $canvas.Save((Join-Path $PSScriptRoot $name), [System.Drawing.Imaging.ImageFormat]::Png); $canvas.Dispose()
}

# 1. Hero: the player tab hanging from the top of a dark "screen", with the name below.
$canvas, $g = New-Canvas 1280 560 '#1B1830' '#0D0C14'
$g.FillRectangle([System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#2A2547')), 0, 0, 1280, 2)
$frame = Load-Frame 'player_lyrics'
$scale = 1.5
$g.DrawImage($frame, [int]((1280 - $frame.Width * $scale) / 2), 0, [int]($frame.Width * $scale), [int]($frame.Height * $scale)); $frame.Dispose()
Draw-Text $g 'Crest' 72 'Bold' '#FAFAFA' 640 330 $true
Draw-Text $g 'A quiet panel at the top of your Windows screen' 24 'Regular' '#A1A1AA' 640 420 $true
Draw-Text $g 'music  ·  notices  ·  coding agents  ·  focus  ·  shelf' 18 'Regular' '#8B7CFF' 640 464 $true
Save $canvas $g 'hero.png'

# 2. Pages: a 2 x 2 grid on a light backdrop, each with a caption.
$canvas, $g = New-Canvas 1280 600 '#F4F4F7' '#E6E6EE'
$cells = @(
    @('page_agents', 'Agents: live Claude Code, Codex, Cursor, and omp sessions'),
    @('page_system', 'System: CPU, GPU, RAM, disk, network'),
    @('theme_ocean', 'Focus timer in the Ocean theme'),
    @('page_shelf', 'Shelf: drop files to keep them handy')
)
for ($i = 0; $i -lt 4; $i++) {
    $frame = Load-Frame $cells[$i][0]
    $cx = 320 + ($i % 2) * 640; $top = 24 + [math]::Floor($i / 2) * 290
    $scale = [math]::Min(1.0, 600 / $frame.Width)
    $w = [int]($frame.Width * $scale); $h = [int]($frame.Height * $scale)
    $g.DrawImage($frame, [int]($cx - $w / 2), $top, $w, $h); $frame.Dispose()
    Draw-Text $g $cells[$i][1] 17 'Regular' '#3F3F46' $cx ($top + $h + 10) $true
}
Save $canvas $g 'pages.png'

# 3. Notices: compact pills and notice cards stacked on a soft backdrop.
$canvas, $g = New-Canvas 1280 420 '#EDEBFA' '#E2E0F2'
$rows = @('pill_music', 'pill_timer', 'hud_message', 'hud_bluetooth', 'hud_volume')
$y = 28
foreach ($name in $rows) {
    $frame = Load-Frame $name
    $g.DrawImage($frame, [int]((1280 - $frame.Width) / 2), $y, $frame.Width, $frame.Height)
    $y += $frame.Height + 14; $frame.Dispose()
}
Save $canvas $g 'notices.png'
