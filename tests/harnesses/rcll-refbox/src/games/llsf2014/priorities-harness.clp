; Harness for rcll-refbox/src/games/llsf2014/priorities.clp
; Detected constructs: defglobal: ?*PRIORITY_FIRST*, ?*PRIORITY_HIGHER*, ?*PRIORITY_HIGH*, ?*PRIORITY_CLEANUP*, ?*PRIORITY_LAST*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-413a749720b40bca6123479b90a0775968304c274cac05d72941030696357d47-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-413a749720b40bca6123479b90a0775968304c274cac05d72941030696357d47|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-413a749720b40bca6123479b90a0775968304c274cac05d72941030696357d47|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-413a749720b40bca6123479b90a0775968304c274cac05d72941030696357d47|COMPLETE" crlf))
