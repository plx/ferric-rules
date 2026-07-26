; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AnimalFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8ac7b0ef47999945398e4549e442e55f9c688f714bf60c76d915870eaa437919-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8ac7b0ef47999945398e4549e442e55f9c688f714bf60c76d915870eaa437919|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8ac7b0ef47999945398e4549e442e55f9c688f714bf60c76d915870eaa437919|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8ac7b0ef47999945398e4549e442e55f9c688f714bf60c76d915870eaa437919|COMPLETE" crlf))
