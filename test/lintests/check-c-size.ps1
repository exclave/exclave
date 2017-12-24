$size = (Get-Volume C).SizeRemaining
if ($size -lt 10000000)
{
    Write-Host "Not enough free disk space: $size"
    exit 1
}
Write-Host "Disk size is okay ($size)"
exit 0