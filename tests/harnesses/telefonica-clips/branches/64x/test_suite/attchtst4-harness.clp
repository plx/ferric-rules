; Harness for telefonica-clips/branches/64x/test_suite/attchtst4.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-016a3e28c7ddc133d408b17499563583bf8f46108be3878c8d9648ce52eed7bd-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-016a3e28c7ddc133d408b17499563583bf8f46108be3878c8d9648ce52eed7bd|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-016a3e28c7ddc133d408b17499563583bf8f46108be3878c8d9648ce52eed7bd|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-016a3e28c7ddc133d408b17499563583bf8f46108be3878c8d9648ce52eed7bd|COMPLETE" crlf))
