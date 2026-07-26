; Harness for telefonica-clips/branches/63x/test_suite/gnrcovl.clp
; Detected constructs: defglobal: ?*success*; deffunction: alt-str-cat/1, print-result/2, testit/0; defmethod: sym-cat
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-26eb48b635b4ca1d07caf0a785fe7428980e273e2291fd40032763e3182a0b9e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-26eb48b635b4ca1d07caf0a785fe7428980e273e2291fd40032763e3182a0b9e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-26eb48b635b4ca1d07caf0a785fe7428980e273e2291fd40032763e3182a0b9e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-26eb48b635b4ca1d07caf0a785fe7428980e273e2291fd40032763e3182a0b9e|COMPLETE" crlf))
