Option Explicit

Dim shell, fso, root, exePath, modelPath, tokensPath, melPath, hotwordsPath
Dim logDir, logPath, logFile, missing, commandLine, wrappedCommand, exitCode

Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

root = fso.GetParentFolderName(WScript.ScriptFullName)
exePath = fso.BuildPath(root, "Shenava.exe")
modelPath = fso.BuildPath(root, "Models\ShenavaStreaming\koochik_hd.nnef.tgz")
If Not fso.FileExists(modelPath) Then
  modelPath = fso.BuildPath(root, "Models\ShenavaStreaming\koochik_hd.onnx")
End If
tokensPath = fso.BuildPath(root, "Models\ShenavaStreaming\tokens.txt")
melPath = fso.BuildPath(root, "engine\mel_filters_slaney_80x257.json")
hotwordsPath = fso.BuildPath(root, "Models\ShenavaStreaming\hotwords_fa.txt")

logDir = fso.BuildPath(shell.ExpandEnvironmentStrings("%LOCALAPPDATA%"), "Shenava")
If Not fso.FolderExists(logDir) Then
  fso.CreateFolder logDir
End If
logPath = fso.BuildPath(logDir, "launcher.log")

Set logFile = fso.OpenTextFile(logPath, 2, True)
logFile.WriteLine Now & " Starting Shenava"
logFile.Close

missing = ""
AppendMissing exePath, missing
AppendMissing modelPath, missing
AppendMissing tokensPath, missing
AppendMissing melPath, missing
AppendMissing hotwordsPath, missing
If Len(missing) > 0 Then
  MsgBox "Shenava is missing required files:" & vbCrLf & missing & vbCrLf & _
    "Reinstall the application.", vbCritical, "Shenava"
  WScript.Quit 2
End If

commandLine = Quote(exePath) & " --control koochik_hd " & Quote(modelPath) & " " & _
  Quote(tokensPath) & " " & Quote(melPath) & " --hotwords " & Quote(hotwordsPath)
wrappedCommand = Quote(shell.ExpandEnvironmentStrings("%ComSpec%")) & " /d /s /c " & _
  Quote(commandLine & " 1>>" & Quote(logPath) & " 2>&1")

On Error Resume Next
exitCode = shell.Run(wrappedCommand, 0, True)
If Err.Number <> 0 Then
  MsgBox "Shenava could not start." & vbCrLf & Err.Description & vbCrLf & _
    "Log: " & logPath, vbCritical, "Shenava"
  WScript.Quit 3
End If
On Error GoTo 0

If exitCode <> 0 Then
  MsgBox "Shenava stopped with exit code " & exitCode & "." & vbCrLf & _
    "Log: " & logPath, vbCritical, "Shenava"
End If
WScript.Quit exitCode

Sub AppendMissing(path, ByRef result)
  If Not fso.FileExists(path) Then
    result = result & vbCrLf & path
  End If
End Sub

Function Quote(value)
  Quote = Chr(34) & CStr(value) & Chr(34)
End Function
