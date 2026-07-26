; Harness for rcll-refbox/src/games/rcll/priorities.clp
; Detected constructs: defglobal: ?*PRIORITY_FIRST*, ?*PRIORITY_HIGHER*, ?*PRIORITY_HIGH*, ?*PRIORITY_CLEANUP*, ?*PRIORITY_LAST*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-97ac39e45e71624ea6e6227bad84bf090b25dfa95bcf76d2c77442337bc92796-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-97ac39e45e71624ea6e6227bad84bf090b25dfa95bcf76d2c77442337bc92796|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-97ac39e45e71624ea6e6227bad84bf090b25dfa95bcf76d2c77442337bc92796|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-97ac39e45e71624ea6e6227bad84bf090b25dfa95bcf76d2c77442337bc92796|COMPLETE" crlf))
