; Harness for clips-official/test_suite/other1.clp
; Detected constructs: deffacts: wine-rules, initial-goal; deftemplate: rule
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-792127503cf33a351ea66971c40775a673c7a974e419f2dd10873a1ab2036fe4-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-792127503cf33a351ea66971c40775a673c7a974e419f2dd10873a1ab2036fe4|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-792127503cf33a351ea66971c40775a673c7a974e419f2dd10873a1ab2036fe4|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-792127503cf33a351ea66971c40775a673c7a974e419f2dd10873a1ab2036fe4|COMPLETE" crlf))
