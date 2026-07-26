; Harness for fawkes-robotics/src/plugins/clips-executive/clips/saliences.clp
; Detected constructs: defglobal: ?*SALIENCE-FIRST*, ?*SALIENCE-INIT*, ?*SALIENCE-INIT-LATE*, ?*SALIENCE-WM-IDKEY*, ?*SALIENCE-WM-SYNC-DEL*, ?*SALIENCE-WM-SYNC-ADD*, ?*SALIENCE-DOMAIN-GROUND*, ?*SALIENCE-DOMAIN-CHECK*, ?*SALIENCE-DOMAIN-APPLY*, ?*SALIENCE-HIGH*, ?*SALIENCE-MODERATE*, ?*SALIENCE-LOW*, ?*SALIENCE-LAST*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-0c022210918c8c13af33b93c37928ecc7959eef2eacf18fbd486d01397bbe68a-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c022210918c8c13af33b93c37928ecc7959eef2eacf18fbd486d01397bbe68a|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c022210918c8c13af33b93c37928ecc7959eef2eacf18fbd486d01397bbe68a|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c022210918c8c13af33b93c37928ecc7959eef2eacf18fbd486d01397bbe68a|COMPLETE" crlf))
