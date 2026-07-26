; Harness for telefonica-clips/branches/63x/test_suite/attchtst4.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-32e24b9b444b872f9da8d9fd49a20ff0a7f6fb256e56a116125ce619f3f8a0f6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-32e24b9b444b872f9da8d9fd49a20ff0a7f6fb256e56a116125ce619f3f8a0f6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-32e24b9b444b872f9da8d9fd49a20ff0a7f6fb256e56a116125ce619f3f8a0f6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-32e24b9b444b872f9da8d9fd49a20ff0a7f6fb256e56a116125ce619f3f8a0f6|COMPLETE" crlf))
