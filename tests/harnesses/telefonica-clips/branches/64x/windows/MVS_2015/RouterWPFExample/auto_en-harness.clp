; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-097822a49473e8ab5fae1447eee82bddcbf7987ae06267483c80b370974222c3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-097822a49473e8ab5fae1447eee82bddcbf7987ae06267483c80b370974222c3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-097822a49473e8ab5fae1447eee82bddcbf7987ae06267483c80b370974222c3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-097822a49473e8ab5fae1447eee82bddcbf7987ae06267483c80b370974222c3|COMPLETE" crlf))
