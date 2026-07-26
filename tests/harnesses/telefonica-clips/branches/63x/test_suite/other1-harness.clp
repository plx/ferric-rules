; Harness for telefonica-clips/branches/63x/test_suite/other1.clp
; Detected constructs: deffacts: wine-rules, initial-goal; deftemplate: rule
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-89490522416b8d8fbdf22672069c41c65107658593ffffa5dea9d0609598f093-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-89490522416b8d8fbdf22672069c41c65107658593ffffa5dea9d0609598f093|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-89490522416b8d8fbdf22672069c41c65107658593ffffa5dea9d0609598f093|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-89490522416b8d8fbdf22672069c41c65107658593ffffa5dea9d0609598f093|COMPLETE" crlf))
