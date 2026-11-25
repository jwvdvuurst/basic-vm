REM wavdemo.bas - play a WAV file and report its length.
REM
REM Usage:   basic-vm examples\wavdemo.bas <file.wav>
REM
REM The program makes no sound of its own. To get a file to play, render the
REM sound example to a WAV (nothing is played while it is rendered):
REM
REM   basic-vm --save-audio out.wav examples\sound.bas
REM   basic-vm examples\wavdemo.bas out.wav
REM
REM Without an argument it prints this usage and exits at once.
REM Supported files: PCM WAV, 8/16/24/32-bit, any number of channels (mixed
REM down to mono), any sample rate (resampled to 44100 Hz).

IF ARGC() < 1 THEN
  PRINT "usage: basic-vm examples\wavdemo.bas <file.wav>"
  PRINT "create one with: basic-vm --save-audio out.wav examples\sound.bas"
  END
END IF

file$ = ARG$(1)
PRINT "File:   "; file$
PRINT "Length: "; WAVLENGTH(file$); " ms"
VOLUME 30
PLAYWAV file$
PRINT "Queued: "; SOUNDQUEUE(); " ms"
SOUNDWAIT
PRINT "Done."
