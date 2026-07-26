; Harness for telefonica-clips/branches/64x/test_suite/gnrcprc.clp
; Detected constructs: deffunction: testit/0; defgeneric: slot-replace, class-slots; defmethod: t1, t2, t3, slot-replace, class-slots
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-18f8f1a9c47aa12f0a4e708e3ff92acc67f44a750f3dc470c49ad2d03249d9b2-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-18f8f1a9c47aa12f0a4e708e3ff92acc67f44a750f3dc470c49ad2d03249d9b2|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-18f8f1a9c47aa12f0a4e708e3ff92acc67f44a750f3dc470c49ad2d03249d9b2|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-18f8f1a9c47aa12f0a4e708e3ff92acc67f44a750f3dc470c49ad2d03249d9b2|COMPLETE" crlf))
