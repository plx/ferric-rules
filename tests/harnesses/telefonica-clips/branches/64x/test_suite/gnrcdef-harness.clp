; Harness for telefonica-clips/branches/64x/test_suite/gnrcdef.clp
; Detected constructs: defgeneric: foobar, foobar; defmethod: splunge
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ce52e34fc9a82b9dc86f1b2b561fcb5edc4a57501d86c0c1abdb930ea45121df-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce52e34fc9a82b9dc86f1b2b561fcb5edc4a57501d86c0c1abdb930ea45121df|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce52e34fc9a82b9dc86f1b2b561fcb5edc4a57501d86c0c1abdb930ea45121df|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce52e34fc9a82b9dc86f1b2b561fcb5edc4a57501d86c0c1abdb930ea45121df|COMPLETE" crlf))
