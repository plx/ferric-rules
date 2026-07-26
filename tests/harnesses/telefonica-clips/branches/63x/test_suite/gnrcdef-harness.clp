; Harness for telefonica-clips/branches/63x/test_suite/gnrcdef.clp
; Detected constructs: defgeneric: foobar, foobar; defmethod: splunge
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-bc51f97a0085d1bd0930fc14881e312535d909222f4330654512a9226bccf312-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc51f97a0085d1bd0930fc14881e312535d909222f4330654512a9226bccf312|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc51f97a0085d1bd0930fc14881e312535d909222f4330654512a9226bccf312|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc51f97a0085d1bd0930fc14881e312535d909222f4330654512a9226bccf312|COMPLETE" crlf))
