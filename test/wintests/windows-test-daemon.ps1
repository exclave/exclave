Write-Output "daemon ready"
$loop_count = 0
while ($true) {
    Write-Output "Loop: $loop_count"
    $loop_count = $loop_count + 1
    Start-Sleep -Milliseconds 50
}