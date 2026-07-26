; Harness for clips-official/test_suite/gnrcprc.clp
; Detected constructs: deffunction: testit/0; defgeneric: mv-slot-replace, class-slots; defmethod: t1, t2, t3, mv-slot-replace, class-slots
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ca3a02b74e0a1a3dc21c6d07b0accfb879a6d82c7cc920c92a430ef4734bc52d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3a02b74e0a1a3dc21c6d07b0accfb879a6d82c7cc920c92a430ef4734bc52d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3a02b74e0a1a3dc21c6d07b0accfb879a6d82c7cc920c92a430ef4734bc52d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3a02b74e0a1a3dc21c6d07b0accfb879a6d82c7cc920c92a430ef4734bc52d|COMPLETE" crlf))
