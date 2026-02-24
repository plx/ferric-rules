; Harness for fawkes-robotics/src/plugins/clips-executive/clips/saliences.clp
; Detected constructs: defglobal: ?*SALIENCE-FIRST*, ?*SALIENCE-INIT*, ?*SALIENCE-INIT-LATE*, ?*SALIENCE-WM-IDKEY*, ?*SALIENCE-WM-SYNC-DEL*, ?*SALIENCE-WM-SYNC-ADD*, ?*SALIENCE-DOMAIN-GROUND*, ?*SALIENCE-DOMAIN-CHECK*, ?*SALIENCE-DOMAIN-APPLY*, ?*SALIENCE-HIGH*, ?*SALIENCE-MODERATE*, ?*SALIENCE-LOW*, ?*SALIENCE-LAST*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
