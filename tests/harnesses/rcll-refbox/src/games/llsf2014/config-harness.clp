; Harness for rcll-refbox/src/games/llsf2014/config.clp
; Detected constructs: deftemplate: confval
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-5027c7bfcd86190bb50f15acd35e59e085e6424ff823f25a719f28d1533a7076-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-5027c7bfcd86190bb50f15acd35e59e085e6424ff823f25a719f28d1533a7076|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5027c7bfcd86190bb50f15acd35e59e085e6424ff823f25a719f28d1533a7076|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5027c7bfcd86190bb50f15acd35e59e085e6424ff823f25a719f28d1533a7076|COMPLETE" crlf))
