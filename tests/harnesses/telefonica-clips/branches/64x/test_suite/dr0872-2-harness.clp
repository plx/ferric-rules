; Harness for telefonica-clips/branches/64x/test_suite/dr0872-2.clp
; Detected constructs: defmethod: foo
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-728b3251bacc16d75970415ab122acbd69f2102549351c2131d8e56967b38a5f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-728b3251bacc16d75970415ab122acbd69f2102549351c2131d8e56967b38a5f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-728b3251bacc16d75970415ab122acbd69f2102549351c2131d8e56967b38a5f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-728b3251bacc16d75970415ab122acbd69f2102549351c2131d8e56967b38a5f|COMPLETE" crlf))
