; Harness for telefonica-clips/branches/63x/clipsdotnet/AutoFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b9e0cf2683822dd0e96fd5612d5a822bc56d5480ea6477cafacb4248b3c2e75b-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b9e0cf2683822dd0e96fd5612d5a822bc56d5480ea6477cafacb4248b3c2e75b|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b9e0cf2683822dd0e96fd5612d5a822bc56d5480ea6477cafacb4248b3c2e75b|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b9e0cf2683822dd0e96fd5612d5a822bc56d5480ea6477cafacb4248b3c2e75b|COMPLETE" crlf))
