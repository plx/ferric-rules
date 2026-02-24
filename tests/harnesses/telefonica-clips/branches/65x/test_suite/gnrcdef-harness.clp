; Harness for telefonica-clips/branches/65x/test_suite/gnrcdef.clp
; Detected constructs: defgeneric: foobar, foobar; defmethod: splunge
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
