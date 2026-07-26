; Harness for telefonica-clips/branches/64x/test_suite/attchtst2.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1593f582c56f45ffcef5a2e549c8f77331f06e45fa25ea0a1f66c3465bde52c2-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1593f582c56f45ffcef5a2e549c8f77331f06e45fa25ea0a1f66c3465bde52c2|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1593f582c56f45ffcef5a2e549c8f77331f06e45fa25ea0a1f66c3465bde52c2|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1593f582c56f45ffcef5a2e549c8f77331f06e45fa25ea0a1f66c3465bde52c2|COMPLETE" crlf))
