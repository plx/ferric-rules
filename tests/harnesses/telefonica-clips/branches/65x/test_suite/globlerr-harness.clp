; Harness for telefonica-clips/branches/65x/test_suite/globlerr.clp
; Detected constructs: defglobal: ?*x*, ?*r*, ?*y*, ?*z*, ?*w*, ?*q*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7bb531d2f309d5a799eb4c06bbc09809aa0fc266f77c1d3af224f4932f34ddac-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7bb531d2f309d5a799eb4c06bbc09809aa0fc266f77c1d3af224f4932f34ddac|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7bb531d2f309d5a799eb4c06bbc09809aa0fc266f77c1d3af224f4932f34ddac|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7bb531d2f309d5a799eb4c06bbc09809aa0fc266f77c1d3af224f4932f34ddac|COMPLETE" crlf))
