; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8319d684de5ffa235667b1350963ba5a86efcb853ead812d9b0e7d5642d9b73d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8319d684de5ffa235667b1350963ba5a86efcb853ead812d9b0e7d5642d9b73d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8319d684de5ffa235667b1350963ba5a86efcb853ead812d9b0e7d5642d9b73d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8319d684de5ffa235667b1350963ba5a86efcb853ead812d9b0e7d5642d9b73d|COMPLETE" crlf))
