; Harness for telefonica-clips/branches/65x/test_suite/gnrcdef.clp
; Detected constructs: defgeneric: foobar, foobar; defmethod: splunge
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-941a6d65f058fa5de496d5250ff5602e8aaec24560599a4a5cd2f31c976f90e6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-941a6d65f058fa5de496d5250ff5602e8aaec24560599a4a5cd2f31c976f90e6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-941a6d65f058fa5de496d5250ff5602e8aaec24560599a4a5cd2f31c976f90e6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-941a6d65f058fa5de496d5250ff5602e8aaec24560599a4a5cd2f31c976f90e6|COMPLETE" crlf))
