<#
.SYNOPSIS
    Watch one or more folders and launch PowerShell scripts in independent threads as files arrive in the folder

.DESCRIPTION
    Watch one or more folders and launch PowerShell scripts in independent threads as files arrive in the folder

    
    The basic goal of this is to reduce resource consumption if many PowerShell.exe processes might be started at once, for example:
        1.) If you use a SCOM Command notification channel, an alert storm could bring up numerous PowerShell.exe processes and bring your server to a crawl.
        2.) If you run a number of PowerShell scheduled tasks, you could see excessive resource use depending on how the start of these scripts line up.

    Example implementations:
        For use case (1)
            Set up a SCOM command channel to create <alertid>.SCOM in a specific folder.
            Set up this daemon as a scheduled task to watch the folder(s) for .SCOM files
            Set up this daemon to launch a SCOM alert processing script that collects alert info, processes, and sends notification for each alert
        For use case (2)
            Configure scheduled tasks to add powershell scripts to a queue folder, rather than launching them in PowerShell.exe outright.
            Set up this daemon as a scheduled task to watch the folder(s)
            Launch the PS1 using the newFileAsScript parameter
            Caveat:  This would run with the privileges of the account running the daemon.  Configure necessary credential use in the individual ps1 scripts as necessary
        For a quick test, including verbose output, timing out after 1 minute
            Create a folder: New-Item -ItemType directory -path C:\queue | out-null
            Add a test script file to C:\queue.  For example:
                Set-Content C:\queue\testScript.ps1 "Set-Content C:\queue\success.txt 'SUCCESS!'"
            Run the daemon:  & "\\path\to\Invoke-PSDaemon.ps1" -queueDirectory C:\queue -workingDirectory C:\queue -launchertimeout 1 -verbose
            Wait one minutes for script to wrap up (launchertimeout), or press ctrl-c to exit early
            Confirm that the script ran by viewing the log files in C:\queue, or the success.txt file

.PARAMETER newFileAsScript
    Read in the content of each new file and execute it in PowerShell
    
    To reference the file that launched this script, use the following:
    $args[0] = file name
    $args[1] = full path to file

.PARAMETER scriptFile
    Execute a pre-determined script in PowerShell for each new file

    To reference the file that launched this script, use the following:
    $args[0] = file name
    $args[1] = full path to file

.PARAMETER QueueDirectory
    One or more directories to watch for new files

.PARAMETER gciFilter
    Filter when querying QueueDirectory for files.  For example, default is "*.PS1"

.PARAMETER workingDirectory
    Working directory.  In here, we create three folders and a log file:
        CompletedItems - files where processing has completed
        RunningItems - files where processing is currently running
        TimedoutItems - files where processing timed out based on the runspaceTimeout 
        0Processing.log - log file to track results

.PARAMETER logFile
    Path to log file to track results in.  By default, creates 0processing.log in $workingDirectory

.PARAMETER Throttle
    Runspacepool limited to this many threads

.PARAMETER SleepSeconds
    Seconds to sleep between checking for new files in QueueDirectory

.PARAMETER runspaceTimeout
    Minutes to wait until discarding an individual thread that has been running too long

    When a runspace times out, the file that kicked it off is moved to the workingDirectory/timedoutItems folder

.PARAMETER launcherTimeout
    Minutes to wait before running through remaining threads and gracefully exiting the daemon.  Default:  1 hour

    Be sure to set this parameter in tune with whatever you use to launch this daemon.
        For example,
            If I run a scheduled task once a day that launches this daemon, I would set launcherTimeout to 1440 minutes, or a few minutes less to allow for graceful exit
            If I run a scheduled task every 1 hour that launches this daemon, I would set launcherTimeout to 60 minutes, or a few minutes less to allow for graceful exit

    If you close a runspace prior to this graceful exit, files will be left in WorkingDirectory/runningItems

.PARAMETER SleepTimer
    Milliseconds to sleep between several steps - ctrl-f the source code to find where.

.PARAMETER LogError
    If specified, log error details to a filename.error file in CompletedItems folder

.NOTES
    Thanks to Tome Tanasovski for the idea:
        http://powertoe.wordpress.com/2012/06/27/how-to-execute-powershell-scripts-without-the-cpu-hit-to-start-powershell/

    Thanks to Boe Prox for many of the parallelization bits.
        This function uses a modified Invoke-Parallel from: http://gallery.technet.microsoft.com/scriptcenter/Run-Parallel-Parallel-377fd430
        Invoke-Parallel uses Boe's code from here: http://learn-powershell.net/2012/05/10/speedy-network-information-query-using-powershell/

.EXAMPLE

    # Create a demo queue folder
    New-Item -ItemType directory -path C:\queue | out-null

    # Add a test script file to the queue folder.  For example:
    Set-Content C:\queue\testScript.ps1 "Set-Content C:\queue\success.txt 'SUCCESS!'" -force

    #Define the parameters we will use for Invoke-PSDaemon.
    $params = @{
        queueDirectory = "C:\queue"
        workingDirectory = "C:\queue"
        launchertimeout = 1
        verbose = $true
    }

    # Run the daemon.  Set it to close after 1 minute.
    # Show verbose info so you can watch what goes on
    & "\\path\to\Invoke-PSDaemon.ps1" @params

    # Wait one minute for script to wrap up (launchertimeout), or press ctrl-c to exit early
    # Confirm that the script ran by viewing the log files in C:\queue, or the success.txt file
#>
[cmdletbinding(DefaultParameterSetName='newFileAsScript')]
param(

    #run files in QueueDirectory as scripts, with the file name and path as arguments
    [Parameter(Position=0, Mandatory=$false, ParameterSetName='newFileAsScript')]
    [switch] $newFileAsScript,

    #Always run this specific script with the new file name and path as arguments
    [Parameter(Position=0, Mandatory=$false, ParameterSetName='runSpecificScript')]
    [string] $scriptFile = "C:\SCOM\SCOM-AlertProcessing.ps1",

    #directory to monitor for new scripts to run
    [Parameter(Position=1,Mandatory=$false)]
    [string[]] $QueueDirectory = @("C:\temp"),

    #filter for files to watch for
    [string] $gciFilter = "*.ps1",

    #build up working directories in this path
    [Parameter(Position=2,Mandatory=$false)]
    [ValidateScript({Test-Path $_ -PathType container})]
    [string] $workingDirectory = "C:\temp",

    #log file
    [Parameter(Mandatory=$false)]
    [ValidateScript({Test-Path $(split-path $_ -parent) -PathType container})]
    [string] $logFile = $(join-path $workingDirectory "0Processing.log"),

    #thread limit
    [int]$Throttle = 20,

    #seconds to sleep before checking for new script files
    [int]$SleepSeconds = 10,

    #minutes before individual threads time out
    [int]$runspaceTimeout = 10,

    #minutes to wait before gracefully closing daemon when no files or runspaces are active
    #note:  The actual run time could be $launcherTimeout + $sleepSeconds + $runspaceTimout (assuming you are under the throttle)
    [int]$launcherTimeout = 60,

    #ms to sleep between a few steps (e.g. while checking for max threads). ctrl+f for details!  Would recommend a minimum of 200, maximum of 500
    [int]$SleepTimer = 300,

    #log errors if they occur and this switch is specified ($WorkingDirectory/<filename>.error) 
    [switch]$logError
)

    #Get the initial launch timestamp
    $launchDate = get-date




#region functions
    
    #Function that will be used to process runspace jobs
        Function Get-RunspaceData {
            [cmdletbinding()]
            param( [switch]$Wait )

            #loop through runspaces
            #if $wait is specified, keep looping until all complete
            Do {

                #set more to false for tracking completion
                $more = $false

                #give verbose status
                if($PSBoundParameters['Wait']){ Write-verbose ("Looping through {0} runspaces until complete" -f $runspaces.count) }
                else{ Write-verbose ("Looping through {0} runspaces one time" -f $runspaces.count) }

                #run through each runspace.           
                Foreach($runspace in $runspaces) {
                    
                    #get the duration - inaccurate
                    $currentdate = get-date
                    $runtime = $currentdate - $runspace.startTime
                    
                    #set up log object
                    $log = "" | select Date, Action, Runtime, Status, Details
                    $log.Action = $runspace.File
                    $log.Date = $currentdate
                    $log.Runtime = "$([math]::Round($runtime.totalminutes, 2)) minutes"

                    #If runspace completed, end invoke, dispose, recycle, counter++
                    If ($runspace.Runspace.isCompleted) {
                        
                        #check if there were errors
                        $runspaceErrors = $runspace.powershell.HadErrors

                        if($runspaceErrors) {
                            
                            #set the logging info and move the file to completed
                            $log.status = "CompletedWithErrors"
                            if(test-path $runspace.file) { move-item $runspace.file $completedDirectory -force }
                            Write-Verbose ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1]

                            #only log errors if specified
                            if($logError){

                                #create error log file path
                                $errorLogFile = join-path $completedDirectory "$($file.BaseName).error"
                                new-item -path $errorLogFile -ItemType file -force | out-null

                                #gather error details and write them to the log
                                $errs = @( $runspace.powershell.streams | select -ExpandProperty error )
                                $count = 1
                                $errCount = $errs.count
                                $errorString = ""
                                foreach($err in $errs){
                                    "#########      ERROR $count of $errCount      #########"
                                    $count++
                                    $errorString += $err | Format-List * -Force | out-string
                                    $errorString += "-" * 100
                                    $errorString += $err.InvocationInfo | Format-List * | out-string
                                    $errorString += ("-" * 100) + "`n"
                                    $exception = $err.Exception
                                    for ($depth = 0; $Exception -ne $null; $depth++){
                                        $errorString += "$depth" * 100
                                        $errorString += $Exception | Format-List -Force * | out-string 
                                        $Exception = $Exception.InnerException
                                    }
                                    $errorString += ("#" * 100) + "`n"
                                }
                                $errorString | out-file -Append -FilePath $errorLogFile -force
                            }
                        }
                        else {
                            #add logging details and cleanup
                            $log.status = "Completed"
                            if(test-path $runspace.file) { move-item $runspace.file $completedDirectory -force }
                            Write-Verbose ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1]
                        }

                        #everything is logged, clean up the runspace
                        $runspace.powershell.EndInvoke($runspace.Runspace)
                        $runspace.powershell.dispose()
                        $runspace.Runspace = $null
                        $runspace.powershell = $null

                    }

                    #If runtime exceeds max, dispose the runspace
                    ElseIf ( $runspaceTimeout -ne 0 -and $runtime.totalMinutes -gt $runspaceTimeout) {
                        $runspace.powershell.dispose()
                        $runspace.Runspace = $null
                        $runspace.powershell = $null

                        
                        #add logging details and cleanup
                        $log.status = "TimedOut"
                        if(test-path $runspace.file) { move-item $runspace.file $timeoutDirectory -force }
                        Write-verbose ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1]

                    }
                   
                    #If runspace isn't null set more to true  
                    ElseIf ($runspace.Runspace -ne $null) {
                        $log = $null
                        $more = $true
                    }

                    #log the results if a log file was indicated
                    if($logFile -and $log){
                        ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1] | out-file $logFile -append
                    }

                }

                #Clean out unused runspace jobs
                $temphash = $runspaces.clone()
                $temphash | Where { $_.runspace -eq $Null } | ForEach {
                    Write-Verbose ("Removing runspace for {0}" -f $_.file)
                    $Runspaces.remove($_)
                }

                #sleep for a bit if we will loop again
                if($PSBoundParameters['Wait']){ start-sleep -milliseconds $SleepTimer }

            #Loop again only if -wait parameter and there are more runspaces to process
            } while ($more -and $PSBoundParameters['Wait'])
            #End of runspace function
        }

#endregion functions




#region build runspace pool

    #Create runspace pool with specified throttle
    Write-Verbose ("Creating runspace pool and session states")
    $sessionstate = [system.management.automation.runspaces.initialsessionstate]::CreateDefault()
    $runspacepool = [runspacefactory]::CreateRunspacePool(1, $Throttle, $sessionstate, $Host)
    $runspacepool.Open() 
     
#endregion build runspacepool




#region init
    
    Write-Verbose "Creating empty collection to hold runspace jobs"
    $Script:runspaces = New-Object System.Collections.ArrayList   

     <#
     Calculate a maximum number of runspaces to allow queued up

         ($Throttle * 2) + 1 ensures the pool always has at least double the throttle + 1
             This means $throttle + 1 items will be queued up and their actual start time will drift from the startTime we create
             For better timeout accuracy or performance, pick an appropriate maxQueue below or define it to fit your needs
     #>
 
         #PERFORMANCE - don't worry about timeout accuracy, throw 10x the throttle in the queue
         #$maxQueue = $throttle * 10
         
         #ACCURACY - sacrifice performance for an accurate timeout.  Don't keep anything in the pool beyond the throttle
         $maxQueue = $throttle
         
         #BALANCE - performance and reasonable timeout accuracy
         #$maxQueue = ($Throttle * 2) + 1

    #Set up log file if specified
    if( $logFile -and -not (test-path $logFile) ){
        new-item -ItemType file -path $logFile -force | out-null
        ("" | Select Date, Action, Runtime, Status, Details | ConvertTo-Csv -NoTypeInformation -Delimiter ";")[0] | out-file $logFile -append
    }

    #write initial log entry
    $log = "" | Select Date, Action, Runtime, Status, Details
        $log.Date = $launchDate
        $log.Action = "Batch processing started"
        $log.Runtime = $null
        $log.Status = "Started"
        $log.Details = $null
        ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1] | out-file $logFile -append


    #set up completed, timedout and running folders
    try{
        $completedDirectory = join-path $workingDirectory "completedItems"
        $timeoutDirectory = join-path $workingDirectory "timedoutItems"
        $runningDirectory = join-path $workingDirectory "runningItems"
        if(-not (test-path $completedDirectory)){
            new-item -ItemType directory -path $completedDirectory -ErrorAction stop | out-null
        }
        if(-not (test-path $timeoutDirectory)){
            new-item -ItemType directory -path $timeoutDirectory -ErrorAction stop | out-null
        }
        if(-not (test-path $runningDirectory)){
            new-item -ItemType directory -path $runningDirectory -ErrorAction stop | out-null
        }
    }
    catch{
        Throw "Could not set up working directories in $queueDirectory.  Details: $($_ | out-string)"
        Break
    }

#endregion init




#region infinite loop

    while ($true) {
    
        #list files in folder, fifo
        $files = foreach($directory in $QueueDirectory){
            dir $directory -file -Filter $gciFilter | sort LastWriteTime
        }
    
        #if files exist...
        if ($files) {

            write-verbose "Found $($files.count) files, creating runspaces"
            
            #loop through files
            foreach ($file in $files) {
            
                Write-Verbose ('Reading new file {0}' -f $file.fullname)
                
                #Create script content from specified single script file, or from incoming ps1 file
                if($PSCmdlet.ParameterSetName -eq "runSpecificScript"){ $content = Get-Content $scriptFile | out-string }
                else { $content =  [System.IO.File]::OpenText($file.fullname).ReadToEnd() }
                $scriptBlock = [scriptblock]::Create($content)
        
                #region add scripts to runspace pool

                    #Create the powershell instance and supply the scriptblock with the other parameters
                    $powershell = [powershell]::Create().AddScript($ScriptBlock).AddArgument($file.name).addargument($file.fullname)
    
                    #Add the runspace into the powershell instance
                    $powershell.RunspacePool = $runspacepool
    
                    #Create a temporary collection for each runspace
                    $temp = "" | Select-Object PowerShell, StartTime, file, Runspace
                    $temp.PowerShell = $powershell
                    $temp.StartTime = get-date
                    $temp.file = $file.fullname
    
                    #Save the handle output when calling BeginInvoke() that will be used later to end the runspace
                    $temp.Runspace = $powershell.BeginInvoke()



                    #move item to running directory and update the script.  Loop until file move is confirmed
                    do{
                        
                        #sleep for a bit before trying to move the file.  Otherwise it may fail
                        start-sleep -milliseconds $sleeptimer

                        #try to move the file, if not, keep looping!
                        try{
                            $newFilePath = join-path $runningDirectory $file.name
                            move-item $file.fullname -Destination $newFilePath -force -erroraction stop
                            $temp.file = join-path $runningDirectory $file.name
                        }
                        catch{
                            Write-Verbose "Couldn't move $($file.fullname) to $runningDirectory"
                        }
                    } until(test-path $newFilePath)


                    #Add the temp tracking info to $runspaces collection
                    Write-Verbose ( "Adding {0} to collection at {1}" -f $temp.file, $temp.starttime.tostring() )
                    $runspaces.Add($temp) | Out-Null
                    
                    #loop through existing runspaces one time
                    Get-RunspaceData

                #endregion add scripts to runspace pool

                #If we have more running than max queue (used to control timeout accuracy)
                while ($runspaces.count -ge $maxQueue) {
                    
                    #run get-runspace data and sleep for a short while
                    Write-Verbose "'$($runspaces.count)' items running - exceeded '$maxQueue' limit."
                    Get-RunspaceData
                    Start-Sleep -milliseconds $sleepTimer

                }
            }
        }
        else{

            $endDate = get-date
            $totalRunTime = [math]::Round( ($endDate - $launchDate).totalminutes , 2 )

            #if there are no files, check if we are running any scripts, break out of launcher script if timeout is exceeded
            if($totalRunTime -gt $launcherTimeout){
                
                write-verbose "Launcher started at $launchDate, ran longer than $launcherTimeout.  Wrapping up"
                Get-RunspaceData -wait
                
                #get items that made it into the running directory during final processing.  If any exist here, they won't run.  Move to queue folder
                $running = gci $runningDirectory | select -ExpandProperty fullname
                $log = "" | Select Date, Action, Runtime, Status, Details
                    $log.Date = $endDate
                    $log.Action = "Batch processing timed outlauncherTimeOut '$launcherTimeout'"
                    $log.Runtime = "$totalRunTime minutes"
                    $log.Status = "Started"
                    $log.Details = "launcherTimeOut '$launcherTimeout' minutes exceeded.  '$running' items to re-process"
                    ($log | convertto-csv -Delimiter ";" -NoTypeInformation)[1] | out-file $logFile -append
                $running | %{copy-item -path $_ -Destination $QueueDirectory[0] -force}
                start-sleep -seconds 2

                exit
            }

            #check for completed scripts
            Get-RunspaceData

        }

        #collect garbage
        [gc]::Collect()

        #delete the script
        write-verbose "Waiting for scripts in $QueueDirectory..."
        start-sleep -seconds $SleepSeconds
    }

#endregion infinite loop