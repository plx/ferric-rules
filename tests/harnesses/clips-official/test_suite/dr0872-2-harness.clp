; Harness for clips-official/test_suite/dr0872-2.clp
; Detected constructs: defmethod: foo
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-eb4590373a4c3bf7af12a035f65f54523e2aabafbfedb38cf2cf772f5fc9137c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb4590373a4c3bf7af12a035f65f54523e2aabafbfedb38cf2cf772f5fc9137c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb4590373a4c3bf7af12a035f65f54523e2aabafbfedb38cf2cf772f5fc9137c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-eb4590373a4c3bf7af12a035f65f54523e2aabafbfedb38cf2cf772f5fc9137c|COMPLETE" crlf))
