; Harness for telefonica-clips/branches/65x/test_suite/tempbug.clp
; Detected constructs: defglobal: ?*q*, ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-60ba128a310bc05eef9d461e7d30a639ae71860c26d706a03300c02dddc6dd41-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-60ba128a310bc05eef9d461e7d30a639ae71860c26d706a03300c02dddc6dd41|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-60ba128a310bc05eef9d461e7d30a639ae71860c26d706a03300c02dddc6dd41|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-60ba128a310bc05eef9d461e7d30a639ae71860c26d706a03300c02dddc6dd41|COMPLETE" crlf))
