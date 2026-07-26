; Harness for clips-official/test_suite/tempbug.clp
; Detected constructs: defglobal: ?*q*, ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-f435de4568e117ebe72333e9d168f84a52010b536fe1ca515f0295a6439d4c4d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-f435de4568e117ebe72333e9d168f84a52010b536fe1ca515f0295a6439d4c4d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f435de4568e117ebe72333e9d168f84a52010b536fe1ca515f0295a6439d4c4d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f435de4568e117ebe72333e9d168f84a52010b536fe1ca515f0295a6439d4c4d|COMPLETE" crlf))
