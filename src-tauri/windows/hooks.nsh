; The setup for one user puts the command on that user's PATH, with the command's own code for it, and takes it off again.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\libnima.exe" --add-to-path'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$INSTDIR\libnima.exe" --remove-from-path'
  Pop $0
!macroend
