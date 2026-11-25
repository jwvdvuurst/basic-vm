REM Sound demo. Sounds are queued and play in the background; the program
REM keeps running. Without an audio device (the default for the library and
REM for tests) the queue is timed on a virtual clock, so this runs instantly.

REM A melody in Music Macro Language: tempo 140, eighth notes by default.
PLAY "T140 O4 L8 E D C D E E E4 D D D4 E G G4"
PRINT "Melody queued: "; MUSICLENGTH("T140 O4 L8 E D C D E E E4 D D D4 E G G4"); " ms"

REM SOUND frequency, duration_ms [, volume]: a rising and falling siren.
WAVE SINE
VOLUME 40
FOR hz = 400 TO 800 STEP 100
    SOUND hz, 60
NEXT
FOR hz = 800 TO 400 STEP -100
    SOUND hz, 60
NEXT
SOUND 0, 100
WAVE "triangle"
SOUND NOTEFREQ(69), 250, 80
PRINT "Queued: "; SOUNDQUEUE(); " ms, playing: "; SOUNDPLAYING()

REM SOUNDWAIT blocks until everything queued has finished.
SOUNDWAIT
PRINT "Queued after SOUNDWAIT: "; SOUNDQUEUE(); " ms, playing: "; SOUNDPLAYING()

REM STOPSOUND silences everything at once.
WAVE SQUARE
BEEP
PLAY "O5 C D E F G"
STOPSOUND
PRINT "After STOPSOUND, playing: "; SOUNDPLAYING()
