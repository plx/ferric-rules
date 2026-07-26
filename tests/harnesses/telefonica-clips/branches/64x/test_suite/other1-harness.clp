; Harness for telefonica-clips/branches/64x/test_suite/other1.clp
; Detected constructs: deffacts: wine-rules, initial-goal; deftemplate: rule
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8c65776db558665a740cdac13dbc955ac29e8cc0066d8908de373005d1efb0d1-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8c65776db558665a740cdac13dbc955ac29e8cc0066d8908de373005d1efb0d1|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8c65776db558665a740cdac13dbc955ac29e8cc0066d8908de373005d1efb0d1|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8c65776db558665a740cdac13dbc955ac29e8cc0066d8908de373005d1efb0d1|COMPLETE" crlf))
