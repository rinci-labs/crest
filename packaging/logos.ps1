# Regenerate the package logos from the app icon drawing:
#   cargo test write_package_logos -- --ignored
#   powershell -File packaging\logos.ps1
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$sizes = @{ 'Square44x44Logo' = 44; 'StoreLogo' = 50; 'Square150x150Logo' = 150 }
foreach ($name in $sizes.Keys) {
    $size = $sizes[$name]
    $bytes = [System.IO.File]::ReadAllBytes("$root\target\logos\$name.raw")
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $data = $bitmap.LockBits([System.Drawing.Rectangle]::new(0, 0, $size, $size),
        [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bitmap.PixelFormat)
    for ($row = 0; $row -lt $size; $row++) {
        [System.Runtime.InteropServices.Marshal]::Copy($bytes, $row * $size * 4, [IntPtr]($data.Scan0.ToInt64() + $row * $data.Stride), $size * 4)
    }
    $bitmap.UnlockBits($data)
    $bitmap.Save("$root\packaging\package\assets\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}
